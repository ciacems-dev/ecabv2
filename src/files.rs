//! Fichiers joints : stockage hors du dossier web et téléchargement contrôlé.
//!
//! Avant : `public/uploads/…` servi sans authentification, noms devinables, aucun contrôle de type.
//! Maintenant : chaque téléchargement passe par `/fichiers/{categorie}/{nom}`, qui retrouve la ligne
//! en base et vérifie que l'utilisateur a le droit de la voir.

use std::path::PathBuf;

use axum::Extension;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, Request, header};
use axum::response::{IntoResponse, Response};
use rand::RngCore;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult};
use crate::forms::Upload;
use crate::state::AppState;

/// Sous-dossiers d'origine (`config/services.yaml`), conservés pour reprendre les 23 Go tels quels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Categorie {
    CourrierArrive,
    CourrierSorti,
    Imputation,
}

impl Categorie {
    pub fn dir(self) -> &'static str {
        match self {
            Categorie::CourrierArrive => "courrier_arrives",
            Categorie::CourrierSorti => "courrier_sortis",
            Categorie::Imputation => "imputations",
        }
    }

    fn from_dir(s: &str) -> Option<Self> {
        Some(match s {
            "courrier_arrives" => Categorie::CourrierArrive,
            "courrier_sortis" => Categorie::CourrierSorti,
            "imputations" => Categorie::Imputation,
            _ => return None,
        })
    }
}

/// Types acceptés. Le courrier arrivé n'accepte que le PDF, comme dans l'original.
pub const PDF_ONLY: &[&str] = &["pdf"];
pub const DOCUMENTS: &[&str] = &["pdf", "doc", "docx", "xls", "xlsx", "odt", "ods", "jpg", "jpeg", "png"];

/// Taille maximale d'un fichier (l'original acceptait 1 Go).
pub const MAX_UPLOAD: usize = 50 * 1024 * 1024;

fn extension(filename: &str) -> Option<String> {
    let ext = std::path::Path::new(filename).extension()?.to_str()?.to_ascii_lowercase();
    Some(ext)
}

/// Vérifie que le contenu correspond à l'extension (signature du fichier).
fn signature_ok(ext: &str, bytes: &[u8]) -> bool {
    match ext {
        "pdf" => bytes.starts_with(b"%PDF"),
        "png" => bytes.starts_with(b"\x89PNG"),
        "jpg" | "jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "docx" | "xlsx" | "odt" | "ods" => bytes.starts_with(b"PK\x03\x04"),
        "doc" | "xls" => bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]),
        _ => false,
    }
}

/// Valide le fichier puis l'enregistre sous un nom aléatoire. Renvoie le nom à stocker en base.
pub async fn save(state: &AppState, cat: Categorie, upload: &Upload, allowed: &[&str]) -> Result<String, String> {
    let ext = extension(&upload.filename).unwrap_or_default();
    if !allowed.contains(&ext.as_str()) {
        return Err(format!("Type de fichier non autorisé. Formats acceptés : {}.", allowed.join(", ")));
    }
    if upload.bytes.len() > MAX_UPLOAD {
        return Err(format!("Fichier trop volumineux (maximum {} Mo).", MAX_UPLOAD / 1024 / 1024));
    }
    if !signature_ok(&ext, &upload.bytes) {
        return Err("Le contenu du fichier ne correspond pas à son extension.".into());
    }
    let mut rnd = [0u8; 16];
    rand::rng().fill_bytes(&mut rnd);
    let name = format!("{}.{ext}", hex::encode(rnd));
    let dir = PathBuf::from(&state.cfg.uploads_dir).join(cat.dir());
    let result = async {
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(dir.join(&name), &upload.bytes).await
    }
    .await;
    result.map_err(|e| {
        tracing::error!(error = %e, "écriture du fichier");
        "Le fichier n'a pas pu être enregistré.".to_string()
    })?;
    Ok(name)
}

/// URL de téléchargement utilisée par les templates (`file_url('courrier_arrives', nom)`).
pub fn url(dir: &str, name: &str) -> String {
    format!("/fichiers/{dir}/{}", urlencode(name))
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Nom proposé au téléchargement (au lieu du nom aléatoire de stockage).
async fn authorize(state: &AppState, user: &CurrentUser, cat: Categorie, name: &str) -> AppResult<Option<String>> {
    let my_dest = user.destinataire_id();
    match cat {
        Categorie::CourrierArrive => {
            let row: Option<(i32, Option<i32>, Option<bool>, bool)> = sqlx::query_as(
                "SELECT c.id, c.numero, c.confi,
                        EXISTS (SELECT 1 FROM imputation i WHERE i.courrier_id = c.id AND i.destinataire_id = $2)
                 FROM carrive c WHERE c.courrier = $1 LIMIT 1",
            )
            .bind(name)
            .bind(my_dest)
            .fetch_optional(&state.db)
            .await?;
            let Some((_, numero, confi, imputed_to_me)) = row else { return Err(AppError::NotFound) };
            let allowed = if confi == Some(true) {
                user.is_super_admin()
            } else {
                user.can_manage_imputations() || imputed_to_me
            };
            crate::auth::ensure(allowed)?;
            Ok(numero.map(|n| format!("{n}.pdf")))
        }
        Categorie::Imputation => {
            let row: Option<(Option<i32>,)> = sqlx::query_as(
                "SELECT destinataire_id FROM imputation WHERE doctoure = $1 OR docfinal = $1 LIMIT 1",
            )
            .bind(name)
            .fetch_optional(&state.db)
            .await?;
            let Some((dest,)) = row else { return Err(AppError::NotFound) };
            crate::auth::ensure(user.can_manage_imputations() || user.owns(dest))?;
            Ok(None)
        }
        Categorie::CourrierSorti => {
            let row: Option<(Option<i32>,)> = sqlx::query_as(
                "SELECT initiateur_id FROM csortir WHERE courrier = $1 OR decharge = $1 LIMIT 1",
            )
            .bind(name)
            .fetch_optional(&state.db)
            .await?;
            let Some((initiateur,)) = row else { return Err(AppError::NotFound) };
            crate::auth::ensure(user.can_manage_courrier() || user.owns(initiateur))?;
            Ok(None)
        }
    }
}

pub async fn download(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path((dir, name)): Path<(String, String)>,
    req: Request<Body>,
) -> AppResult<Response> {
    let cat = Categorie::from_dir(&dir).ok_or(AppError::NotFound)?;
    // Le nom vient de la base : on refuse tout ce qui pourrait sortir du dossier.
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(AppError::NotFound);
    }
    let download_name = authorize(&state, &user, cat, &name).await?;

    let path = PathBuf::from(&state.cfg.uploads_dir).join(cat.dir()).join(&name);
    if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
        tracing::warn!(fichier = %path.display(), "fichier référencé en base mais absent du disque");
        return Err(AppError::NotFound);
    }
    let mut res = ServeFile::new(&path).oneshot(req).await.map_err(|e| AppError::Internal(e.into()))?.into_response();

    // PDF et images s'ouvrent dans le navigateur, le reste se télécharge.
    let ext = extension(&name).unwrap_or_default();
    let inline = matches!(ext.as_str(), "pdf" | "png" | "jpg" | "jpeg");
    let shown = download_name.unwrap_or(name).replace('"', "");
    let disposition = format!("{}; filename=\"{shown}\"", if inline { "inline" } else { "attachment" });
    if let Ok(v) = HeaderValue::from_str(&disposition) {
        res.headers_mut().insert(header::CONTENT_DISPOSITION, v);
    }
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok(res)
}
