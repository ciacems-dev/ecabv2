//! Courrier arrivé (CarriveController).

use axum::Extension;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tower_sessions::Session;

use super::common::{self, Libelle, Nom, redirect_back};
use crate::auth::{CurrentUser, ensure};
use crate::error::AppResult;
use crate::files::{self, Categorie};
use crate::forms::{FormData, parse_range};
use crate::layout;
use crate::session::flash;
use crate::state::AppState;

// ---------------------------------------------------------------------------------------------
// Données
// ---------------------------------------------------------------------------------------------

#[derive(FromRow)]
struct CarriveRow {
    id: i32,
    numero: Option<i32>,
    date: Option<NaiveDate>,
    datearrive: Option<NaiveDateTime>,
    expeditaire: Option<String>,
    objet: Option<String>,
    courrier: Option<String>,
    archive: Option<bool>,
    confi: Option<bool>,
    deposant: Option<String>,
    numcour: Option<i32>,
    destinataire_id: Option<i32>,
    destinataire_libelle: Option<String>,
    imputations: Vec<String>,
}

/// Forme attendue par les templates (`carrive.destinataire.libelle`, `carrive.imputations`).
#[derive(Serialize)]
pub struct Carrive {
    id: i32,
    numero: Option<i32>,
    date: Option<NaiveDate>,
    datearrive: Option<NaiveDateTime>,
    expeditaire: Option<String>,
    objet: Option<String>,
    courrier: Option<String>,
    archive: bool,
    confi: bool,
    deposant: Option<String>,
    numcour: Option<i32>,
    destinataire: Option<Libelle>,
    /// Libellés des services auxquels le courrier est imputé (ordre de création).
    imputations: Vec<String>,
}

impl From<CarriveRow> for Carrive {
    fn from(r: CarriveRow) -> Self {
        Carrive {
            id: r.id,
            numero: r.numero,
            date: r.date,
            datearrive: r.datearrive,
            expeditaire: r.expeditaire,
            objet: r.objet,
            courrier: r.courrier,
            archive: r.archive.unwrap_or(false),
            confi: r.confi.unwrap_or(false),
            deposant: r.deposant,
            numcour: r.numcour,
            destinataire: r.destinataire_id.zip(r.destinataire_libelle).map(|(id, libelle)| Libelle { id, libelle }),
            imputations: r.imputations,
        }
    }
}

const CARRIVE_SELECT: &str = "SELECT c.id, c.numero, c.date, c.datearrive, c.expeditaire, c.objet, c.courrier,
        c.archive, c.confi, c.deposant, c.numcour, c.destinataire_id, d.libelle AS destinataire_libelle,
        COALESCE((SELECT array_agg(d2.libelle ORDER BY i.id) FROM imputation i
                  JOIN destinataire d2 ON d2.id = i.destinataire_id WHERE i.courrier_id = c.id), '{}') AS imputations
     FROM src c LEFT JOIN destinataire d ON d.id = c.destinataire_id";

/// Les quatre listes d'origine, avec leurs filtres (voir docs/regles-courrier-arrive.md).
#[derive(Clone, Copy)]
enum Liste {
    /// Service courrier : non archivés, non confidentiels, année en cours, parmi les 200 derniers.
    Arrivees,
    /// Directeur de cabinet : non archivés, année en cours, confidentiels compris, parmi les 200 derniers.
    Dircab,
    /// Archivages : non confidentiels de l'année précédente.
    AnneePrecedente,
    /// Courriers classés : archivés.
    Classes,
}

async fn list(state: &AppState, liste: Liste, range: Option<(NaiveDate, NaiveDate)>) -> AppResult<Vec<Carrive>> {
    // Source : les 200 derniers (CourrieArrive) ou toute la base, ou la période recherchée (recherarrive).
    let source = match (range, liste) {
        (Some(_), _) => "SELECT * FROM carrive WHERE date >= $1 AND date <= $2",
        (None, Liste::Arrivees | Liste::Dircab) => "SELECT * FROM carrive ORDER BY id DESC LIMIT 200",
        (None, _) => "SELECT * FROM carrive",
    };
    let annee = "left(c.numero::text, 4) = EXTRACT(YEAR FROM CURRENT_DATE)::int::text";
    let (filtre, tri) = match liste {
        Liste::Arrivees => (
            format!("COALESCE(c.archive, false) = false AND COALESCE(c.confi, false) = false AND {annee}"),
            "c.numero DESC NULLS LAST",
        ),
        Liste::Dircab => (format!("COALESCE(c.archive, false) = false AND {annee}"), "c.id DESC"),
        Liste::AnneePrecedente => (
            "COALESCE(c.confi, false) = false
             AND left(c.numero::text, 4) = (EXTRACT(YEAR FROM CURRENT_DATE)::int - 1)::text"
                .to_string(),
            "c.date DESC NULLS LAST, c.id DESC",
        ),
        Liste::Classes => ("c.archive = true".to_string(), "c.id DESC"),
    };
    let sql = format!("WITH src AS ({source}) {CARRIVE_SELECT} WHERE {filtre} ORDER BY {tri}");
    let mut q = sqlx::query_as::<_, CarriveRow>(&sql);
    if let Some((from, to)) = range {
        q = q.bind(from).bind(to);
    }
    Ok(q.fetch_all(&state.db).await?.into_iter().map(Carrive::from).collect())
}

async fn find(state: &AppState, id: i32) -> AppResult<Carrive> {
    let sql = format!("WITH src AS (SELECT * FROM carrive WHERE id = $1) {CARRIVE_SELECT}");
    Ok(sqlx::query_as::<_, CarriveRow>(&sql).bind(id).fetch_one(&state.db).await?.into())
}

/// Accès à un courrier précis : gestionnaires, et super admin seulement pour un confidentiel.
fn ensure_can_see(user: &CurrentUser, c: &Carrive) -> AppResult<()> {
    ensure(user.can_manage_courrier() && (!c.confi || user.is_super_admin()))
}

/// Imputation telle qu'affichée sur la fiche et la page du courrier.
#[derive(Serialize, FromRow)]
struct ImputationLigne {
    id: i32,
    date: Option<NaiveDate>,
    reponse: Option<String>,
    commentaire: Option<String>,
    etat: Option<String>,
    effet: bool,
    destinataire_libelle: Option<String>,
    responsable_id: Option<i32>,
    responsable_nom: Option<String>,
}

#[derive(Serialize)]
struct ImputationVue {
    id: i32,
    date: Option<NaiveDate>,
    reponse: Option<String>,
    commentaire: Option<String>,
    etat: Option<String>,
    effet: bool,
    destinataire: Option<Libelle>,
    responsable: Option<Nom>,
}

async fn imputations_of(state: &AppState, courrier_id: i32) -> AppResult<Vec<ImputationVue>> {
    let rows: Vec<ImputationLigne> = sqlx::query_as(
        "SELECT i.id, i.date, i.reponse, i.commentaire, i.etat, COALESCE(trim(i.effet) = '1', false) AS effet,
                d.libelle AS destinataire_libelle, u.id AS responsable_id, u.nom AS responsable_nom
         FROM imputation i
         LEFT JOIN destinataire d ON d.id = i.destinataire_id
         LEFT JOIN users u ON u.id = i.responsable_id
         WHERE i.courrier_id = $1 ORDER BY i.id",
    )
    .bind(courrier_id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ImputationVue {
            id: r.id,
            date: r.date,
            reponse: r.reponse,
            commentaire: r.commentaire,
            etat: r.etat,
            effet: r.effet,
            destinataire: r.destinataire_libelle.map(|libelle| Libelle { id: 0, libelle }),
            responsable: r.responsable_id.map(|id| Nom { id, nom: r.responsable_nom }),
        })
        .collect())
}

// ---------------------------------------------------------------------------------------------
// Listes
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListQuery {
    dates: Option<String>,
}

async fn render_list(
    state: AppState,
    session: Session,
    user: CurrentUser,
    q: ListQuery,
    liste: Liste,
    template: &str,
    menu: (&str, &str),
) -> AppResult<Response> {
    let range = parse_range(q.dates.as_deref());
    let carrives = list(&state, liste, range).await?;
    Ok(layout::render(&state, &session, &user, template, menu, minijinja::context! { carrives, dates => q.dates })
        .await?
        .into_response())
}

pub async fn index(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<ListQuery>,
) -> AppResult<Response> {
    ensure(user.can_manage_courrier())?;
    render_list(state, session, user, q, Liste::Arrivees, "admin/carrive/index.html", ("3", "1")).await
}

pub async fn index_dircab(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<ListQuery>,
) -> AppResult<Response> {
    ensure(user.is_super_admin())?;
    render_list(state, session, user, q, Liste::Dircab, "admin/carrive/indexDIRCAB.html", ("3", "1")).await
}

pub async fn index_annee_precedente(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<ListQuery>,
) -> AppResult<Response> {
    ensure(user.can_manage_courrier())?;
    render_list(state, session, user, q, Liste::AnneePrecedente, "admin/carrive/indexarchive.html", ("7", "1")).await
}

pub async fn index_classes(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<ListQuery>,
) -> AppResult<Response> {
    ensure(user.can_manage_courrier())?;
    render_list(state, session, user, q, Liste::Classes, "admin/carrive/archive.html", ("6", "1")).await
}

// ---------------------------------------------------------------------------------------------
// Fiche du courrier
// ---------------------------------------------------------------------------------------------

pub async fn show(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    let carrive = find(&state, id).await?;
    ensure_can_see(&user, &carrive)?;
    let imputations = imputations_of(&state, id).await?;
    Ok(layout::render(
        &state,
        &session,
        &user,
        "admin/carrive/show.html",
        ("3", "1"),
        minijinja::context! { carrive, imputations },
    )
    .await?
    .into_response())
}

/// Fiche de suivi PDF (dompdf → Chromium). Même nom de fichier que l'original : `{timestamp}Fiche.pdf`.
pub async fn fiche(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    let carrive = find(&state, id).await?;
    ensure_can_see(&user, &carrive)?;
    let imputations = imputations_of(&state, id).await?;
    let html = state
        .tpl
        .get_template("admin/carrive/ficherecp.html")?
        .render(minijinja::context! { carrive, imputations })?;
    let pdf = crate::pdf::html_to_pdf(&state.cfg.chrome_bin, &html).await?;
    let name = format!("{}Fiche.pdf", chrono::Utc::now().timestamp());
    let mut res = pdf.into_response();
    let headers = res.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/pdf"));
    if let Ok(v) = HeaderValue::from_str(&format!("inline; filename=\"{name}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(res)
}

// ---------------------------------------------------------------------------------------------
// Saisie et modification
// ---------------------------------------------------------------------------------------------

/// Valeurs du formulaire, réaffichées en cas d'erreur.
#[derive(Serialize, Default)]
struct Valeurs {
    destinataire: Option<i32>,
    confi: bool,
    date: Option<String>,
    numero: Option<String>,
    deposant: Option<String>,
    numcour: Option<String>,
    expeditaire: Option<String>,
    objet: Option<String>,
    courrier: Option<String>,
}

#[derive(Clone, Copy)]
enum Variante {
    /// Saisie depuis « Arrivées » (service courrier) : retour à la liste « Arrivées ».
    Courrier,
    /// Saisie depuis la vue « Dircab » : retour à cette vue.
    Dircab,
}

impl Variante {
    fn liste(self) -> &'static str {
        match self {
            Variante::Courrier => "/admin/courriers/arrives",
            Variante::Dircab => "/admin/courriers/arrives/dircab",
        }
    }
    fn allowed(self, user: &CurrentUser) -> bool {
        match self {
            Variante::Courrier => user.can_manage_courrier(),
            Variante::Dircab => user.is_super_admin(),
        }
    }
}

async fn render_form(
    state: &AppState,
    session: &Session,
    user: &CurrentUser,
    titre: &str,
    valeurs: Valeurs,
    errors: Vec<String>,
) -> AppResult<Response> {
    let destinataires = common::destinataires(state).await?;
    Ok(layout::render(
        state,
        session,
        user,
        "admin/carrive/form.html",
        ("3", "1"),
        minijinja::context! { titre, v => valeurs, errors, destinataires },
    )
    .await?
    .into_response())
}

const TITRE_NOUVEAU: &str = "Ajouter un courrier arrivé";
const TITRE_EDITION: &str = "Editer un courrier arrivé";

async fn new_form(state: AppState, session: Session, user: CurrentUser, v: Variante) -> AppResult<Response> {
    ensure(v.allowed(&user))?;
    // ConfCarriveType propose « NON » en premier, CarriveType « OUI » : le choix par défaut diffère.
    let valeurs = Valeurs { confi: matches!(v, Variante::Courrier), ..Default::default() };
    render_form(&state, &session, &user, TITRE_NOUVEAU, valeurs, vec![]).await
}

pub async fn new(State(s): State<AppState>, session: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    new_form(s, session, u, Variante::Courrier).await
}

pub async fn new_confidentiel(
    State(s): State<AppState>,
    session: Session,
    Extension(u): Extension<CurrentUser>,
) -> AppResult<Response> {
    new_form(s, session, u, Variante::Dircab).await
}

struct Saisie {
    destinataire: Option<i32>,
    confi: bool,
    date: Option<NaiveDate>,
    sequence: Option<i32>,
    deposant: Option<String>,
    numcour: Option<i32>,
    expeditaire: Option<String>,
    objet: Option<String>,
}

fn lire_saisie(form: &FormData, errors: &mut Vec<String>) -> (Saisie, Valeurs) {
    let saisie = Saisie {
        destinataire: form.int("destinataire", "Destinataire", errors),
        confi: form.flag("confi"),
        date: form.date("date", "Date Arrivée", errors),
        sequence: form.int("numero", "Numéro Courrier", errors),
        deposant: form.text("deposant"),
        numcour: form.int("numcour", "téléphone du déposant", errors),
        expeditaire: form.required("expeditaire", "Expéditeur", errors),
        objet: form.text("objet"),
    };
    if saisie.destinataire.is_none() {
        errors.push("Le champ « Destinataire » est obligatoire.".into());
    }
    if form.text("date").is_none() {
        errors.push("Le champ « Date Arrivée » est obligatoire.".into());
    }
    if form.text("numero").is_none() {
        errors.push("Le champ « Numéro Courrier » est obligatoire.".into());
    }
    let valeurs = Valeurs {
        destinataire: saisie.destinataire,
        confi: saisie.confi,
        date: form.text("date"),
        numero: form.text("numero"),
        deposant: form.text("deposant"),
        numcour: form.text("numcour"),
        expeditaire: form.text("expeditaire"),
        objet: form.text("objet"),
        courrier: None,
    };
    (saisie, valeurs)
}

/// Numéro complet = année + numéro d'ordre (Carrive::setNumero). À la modification, l'année
/// d'origine est conservée (l'original y remettait l'année en cours).
fn numero_complet(annee: i32, sequence: i32) -> Option<i32> {
    format!("{annee}{sequence}").parse().ok()
}

async fn numero_pris(state: &AppState, numero: i32, sauf: Option<i32>) -> AppResult<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM carrive WHERE numero = $1 AND id IS DISTINCT FROM $2)")
        .bind(numero)
        .bind(sauf)
        .fetch_one(&state.db)
        .await?)
}

async fn create(state: AppState, session: Session, user: CurrentUser, v: Variante, mp: Multipart) -> AppResult<Response> {
    ensure(v.allowed(&user))?;
    let mut form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let (saisie, valeurs) = lire_saisie(&form, &mut errors);
    let annee = chrono::Utc::now().year();
    let numero = saisie.sequence.and_then(|s| numero_complet(annee, s));
    if saisie.sequence.is_some() && numero.is_none() {
        errors.push("Numéro Courrier invalide.".into());
    }
    if let Some(n) = numero
        && numero_pris(&state, n, None).await? {
            errors.push("Le numéro est déja utilisé".into());
        }
    let mut fichier = None;
    if errors.is_empty()
        && let Some(up) = form.file("courrier") {
            match files::save(&state, Categorie::CourrierArrive, &up, files::PDF_ONLY).await {
                Ok(name) => fichier = Some(name),
                Err(e) => errors.push(e),
            }
        }
    if !errors.is_empty() {
        return render_form(&state, &session, &user, TITRE_NOUVEAU, valeurs, errors).await;
    }
    sqlx::query(
        "INSERT INTO carrive (destinataire_id, confi, date, numero, deposant, numcour, expeditaire, objet, courrier,
                              create_at, datearrive, action)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, (now() AT TIME ZONE 'UTC'), (now() AT TIME ZONE 'UTC'), false)",
    )
    .bind(saisie.destinataire)
    .bind(saisie.confi)
    .bind(saisie.date)
    .bind(numero)
    .bind(saisie.deposant)
    .bind(saisie.numcour)
    .bind(saisie.expeditaire)
    .bind(saisie.objet)
    .bind(fichier)
    .execute(&state.db)
    .await?;
    Ok(Redirect::to(v.liste()).into_response())
}

pub async fn create_courrier(
    State(s): State<AppState>,
    session: Session,
    Extension(u): Extension<CurrentUser>,
    mp: Multipart,
) -> AppResult<Response> {
    create(s, session, u, Variante::Courrier, mp).await
}

pub async fn create_confidentiel(
    State(s): State<AppState>,
    session: Session,
    Extension(u): Extension<CurrentUser>,
    mp: Multipart,
) -> AppResult<Response> {
    create(s, session, u, Variante::Dircab, mp).await
}

/// Sépare le numéro complet en (année, numéro d'ordre) : `20262479` → (2026, 2479).
fn decouper(numero: Option<i32>) -> (Option<i32>, Option<String>) {
    match numero.map(|n| n.to_string()) {
        Some(s) if s.len() > 4 => (s[..4].parse().ok(), Some(s[4..].to_string())),
        Some(s) => (None, Some(s)),
        None => (None, None),
    }
}

async fn edit_form(state: AppState, session: Session, user: CurrentUser, id: i32, v: Variante) -> AppResult<Response> {
    ensure(v.allowed(&user))?;
    let c = find(&state, id).await?;
    ensure_can_see(&user, &c)?;
    let valeurs = Valeurs {
        destinataire: c.destinataire.as_ref().map(|d| d.id),
        confi: c.confi,
        date: c.date.map(|d| d.to_string()),
        numero: decouper(c.numero).1,
        deposant: c.deposant,
        numcour: c.numcour.map(|n| n.to_string()),
        expeditaire: c.expeditaire,
        objet: c.objet,
        courrier: c.courrier,
    };
    render_form(&state, &session, &user, TITRE_EDITION, valeurs, vec![]).await
}

pub async fn edit(
    State(s): State<AppState>,
    session: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    edit_form(s, session, u, id, Variante::Courrier).await
}

pub async fn edit_cab(
    State(s): State<AppState>,
    session: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    edit_form(s, session, u, id, Variante::Dircab).await
}

async fn update(
    state: AppState,
    session: Session,
    user: CurrentUser,
    id: i32,
    v: Variante,
    mp: Multipart,
) -> AppResult<Response> {
    ensure(v.allowed(&user))?;
    let existant = find(&state, id).await?;
    ensure_can_see(&user, &existant)?;
    let mut form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let (saisie, mut valeurs) = lire_saisie(&form, &mut errors);
    valeurs.courrier = existant.courrier.clone();
    let annee = decouper(existant.numero).0.unwrap_or_else(|| chrono::Utc::now().year());
    let numero = saisie.sequence.and_then(|s| numero_complet(annee, s));
    if saisie.sequence.is_some() && numero.is_none() {
        errors.push("Numéro Courrier invalide.".into());
    }
    if let Some(n) = numero
        && numero_pris(&state, n, Some(id)).await? {
            errors.push("Le numéro est déja utilisé".into());
        }
    let mut fichier = existant.courrier.clone();
    if errors.is_empty()
        && let Some(up) = form.file("courrier") {
            match files::save(&state, Categorie::CourrierArrive, &up, files::PDF_ONLY).await {
                Ok(name) => fichier = Some(name),
                Err(e) => errors.push(e),
            }
        }
    if !errors.is_empty() {
        return render_form(&state, &session, &user, TITRE_EDITION, valeurs, errors).await;
    }
    sqlx::query(
        "UPDATE carrive SET destinataire_id = $1, confi = $2, date = $3, numero = $4, deposant = $5, numcour = $6,
                expeditaire = $7, objet = $8, courrier = $9
         WHERE id = $10",
    )
    .bind(saisie.destinataire)
    .bind(saisie.confi)
    .bind(saisie.date)
    .bind(numero)
    .bind(saisie.deposant)
    .bind(saisie.numcour)
    .bind(saisie.expeditaire)
    .bind(saisie.objet)
    .bind(fichier)
    .bind(id)
    .execute(&state.db)
    .await?;
    Ok(Redirect::to(v.liste()).into_response())
}

pub async fn update_courrier(
    State(s): State<AppState>,
    session: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
    mp: Multipart,
) -> AppResult<Response> {
    update(s, session, u, id, Variante::Courrier, mp).await
}

pub async fn update_cab(
    State(s): State<AppState>,
    session: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
    mp: Multipart,
) -> AppResult<Response> {
    update(s, session, u, id, Variante::Dircab, mp).await
}

// ---------------------------------------------------------------------------------------------
// Actions (POST + CSRF, vérifié par le middleware des actions)
// ---------------------------------------------------------------------------------------------

async fn supprimer(state: &AppState, session: &Session, user: &CurrentUser, id: i32) -> AppResult<()> {
    let c = find(state, id).await?;
    ensure_can_see(user, &c)?;
    if !c.imputations.is_empty() {
        flash(session, "warning", "Ce courrier a des imputations : supprimez-les d'abord.").await?;
        return Ok(());
    }
    sqlx::query("DELETE FROM document WHERE arrive_id = $1").bind(id).execute(&state.db).await?;
    sqlx::query("DELETE FROM carrive WHERE id = $1").bind(id).execute(&state.db).await?;
    flash(session, "success", "Courrier supprimé avec succès").await
}

pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
    headers: HeaderMap,
) -> AppResult<Response> {
    ensure(user.can_manage_courrier())?;
    supprimer(&state, &session, &user, id).await?;
    Ok(redirect_back(&headers, "/admin/courriers/arrives"))
}

pub async fn delete_cab(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    ensure(user.is_super_admin())?;
    supprimer(&state, &session, &user, id).await?;
    Ok(Redirect::to("/admin/courriers/arrives/dircab").into_response())
}

async fn set_archive(state: &AppState, user: &CurrentUser, id: i32, value: bool) -> AppResult<()> {
    let c = find(state, id).await?;
    ensure_can_see(user, &c)?;
    sqlx::query("UPDATE carrive SET archive = $1 WHERE id = $2").bind(value).bind(id).execute(&state.db).await?;
    Ok(())
}

pub async fn archive(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
    headers: HeaderMap,
) -> AppResult<Response> {
    set_archive(&state, &user, id, true).await?;
    flash(&session, "success", "Dossier archivé avec succès").await?;
    Ok(redirect_back(&headers, "/admin/courriers/arrives"))
}

pub async fn unarchive(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
    headers: HeaderMap,
) -> AppResult<Response> {
    set_archive(&state, &user, id, false).await?;
    flash(&session, "success", "Dossier retourné avec succès").await?;
    Ok(redirect_back(&headers, "/admin/courriers/arrives/classes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numero() {
        assert_eq!(numero_complet(2026, 2479), Some(20262479));
        assert_eq!(decouper(Some(20262479)), (Some(2026), Some("2479".into())));
        assert_eq!(decouper(Some(2025794)), (Some(2025), Some("794".into())));
    }
}
