//! Référentiels : types de départ (TypedepartController) et services (DestinataireController,
//! hors espace service qui est dans `destinataire.rs`).

use axum::Extension;
use axum::extract::{Multipart, Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Serialize;
use sqlx::FromRow;
use tower_sessions::Session;

use super::common::redirect_back;
use crate::auth::{CurrentUser, Profil, ensure};
use crate::error::AppResult;
use crate::forms::FormData;
use crate::layout;
use crate::session::flash;
use crate::state::AppState;

// =============================================================================================
// Types de départ
// =============================================================================================

#[derive(Serialize, FromRow)]
struct Typedepart {
    id: i32,
    libelle: String,
    statut: bool,
}

const TYPES: &str = "/admin/types-depart";

pub async fn types_index(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
) -> AppResult<Response> {
    ensure(user.can_manage_courrier())?;
    let typedeparts: Vec<Typedepart> =
        sqlx::query_as("SELECT id, libelle, statut FROM typedepart ORDER BY id").fetch_all(&state.db).await?;
    Ok(layout::render(&state, &session, &user, "admin/typedepart/index.html", ("3", "2"), minijinja::context! { typedeparts })
        .await?
        .into_response())
}

async fn type_form(
    state: &AppState,
    session: &Session,
    user: &CurrentUser,
    titre: &str,
    libelle: Option<String>,
    statut: bool,
    errors: Vec<String>,
) -> AppResult<Response> {
    Ok(layout::render(
        state,
        session,
        user,
        "admin/typedepart/form.html",
        ("3", "2"),
        minijinja::context! { titre, libelle, statut, errors },
    )
    .await?
    .into_response())
}

pub async fn type_new(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    ensure(u.can_manage_courrier())?;
    type_form(&s, &se, &u, "Enregistrement Types départs", None, true, vec![]).await
}

pub async fn type_edit(
    State(s): State<AppState>,
    se: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    ensure(u.can_manage_courrier())?;
    let t: Typedepart = sqlx::query_as("SELECT id, libelle, statut FROM typedepart WHERE id = $1").bind(id).fetch_one(&s.db).await?;
    type_form(&s, &se, &u, "Modification de Type départ", Some(t.libelle), t.statut, vec![]).await
}

async fn type_save(state: AppState, session: Session, user: CurrentUser, id: Option<i32>, mp: Multipart) -> AppResult<Response> {
    ensure(user.can_manage_courrier())?;
    let form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let libelle = form.required("libelle", "Libellé", &mut errors);
    let statut = form.flag("statut");
    if !errors.is_empty() {
        let titre = if id.is_some() { "Modification de Type départ" } else { "Enregistrement Types départs" };
        return type_form(&state, &session, &user, titre, libelle, statut, errors).await;
    }
    match id {
        Some(id) => sqlx::query("UPDATE typedepart SET libelle = $1, statut = $2 WHERE id = $3").bind(&libelle).bind(statut).bind(id),
        None => sqlx::query("INSERT INTO typedepart (libelle, statut) VALUES ($1, $2)").bind(&libelle).bind(statut),
    }
    .execute(&state.db)
    .await?;
    Ok(Redirect::to(TYPES).into_response())
}

pub async fn type_create(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, mp: Multipart) -> AppResult<Response> {
    type_save(s, se, u, None, mp).await
}

pub async fn type_update(
    State(s): State<AppState>,
    se: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
    mp: Multipart,
) -> AppResult<Response> {
    type_save(s, se, u, Some(id), mp).await
}

pub async fn type_delete(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    ensure(user.can_manage_courrier())?;
    let used: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM csortir WHERE type_id = $1)").bind(id).fetch_one(&state.db).await?;
    if used {
        flash(&session, "warning", "Ce type est utilisé par des courriers départ : désactivez-le plutôt que de le supprimer.").await?;
    } else {
        sqlx::query("DELETE FROM typedepart WHERE id = $1").bind(id).execute(&state.db).await?;
        flash(&session, "success", "Type de départ supprimé").await?;
    }
    Ok(Redirect::to(TYPES).into_response())
}

// =============================================================================================
// Services (destinataires)
// =============================================================================================

/// Ligne des pages « Suivi des imputations » et « Unités Administratives ».
#[derive(Serialize, FromRow)]
struct Suivi {
    id: i32,
    libelle: String,
    statut: bool,
    /// Libellé du profil (« Service courrier »…), vide pour un service ordinaire.
    profil: Option<String>,
    /// Imputations directes (sans `responsable`).
    cr: i64,
    /// … dont traitées.
    crt: i64,
    /// … dont non traitées.
    crnt: i64,
}

async fn suivi(state: &AppState) -> sqlx::Result<Vec<Suivi>> {
    sqlx::query_as(
        "SELECT d.id, d.libelle, COALESCE(d.statut, false) AS statut,
                CASE d.profil WHEN 'courrier' THEN 'Service courrier' WHEN 'dircab' THEN 'Directeur de cabinet'
                              WHEN 'etudes' THEN 'Chargé d''études' WHEN 'accueil' THEN 'Accueil' END AS profil,
                COUNT(i.id) AS cr,
                COUNT(i.id) FILTER (WHERE i.statut = true) AS crt,
                COUNT(i.id) FILTER (WHERE COALESCE(i.statut, false) = false) AS crnt
         FROM destinataire d
         LEFT JOIN imputation i ON i.destinataire_id = d.id AND i.responsable_id IS NULL
         GROUP BY d.id ORDER BY d.id",
    )
    .fetch_all(&state.db)
    .await
}

/// « Cabinet du Ministre › Suivi des imputations » (sans actions).
pub async fn suivi_imputations(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
) -> AppResult<Response> {
    ensure(user.is_super_admin())?;
    let destinataires = suivi(&state).await?;
    Ok(layout::render(&state, &session, &user, "admin/destinataire/index.html", ("4", "5"), minijinja::context! { destinataires })
        .await?
        .into_response())
}

/// « Comptes › DESTINATAIRES » (avec actions).
pub async fn services_index(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
) -> AppResult<Response> {
    ensure(user.is_super_admin())?;
    let destinataires = suivi(&state).await?;
    Ok(layout::render(&state, &session, &user, "admin/destinataire/indexDES.html", ("4", "5"), minijinja::context! { destinataires })
        .await?
        .into_response())
}

async fn service_form(
    state: &AppState,
    session: &Session,
    user: &CurrentUser,
    titre: &str,
    libelle: Option<String>,
    profil: Option<String>,
    errors: Vec<String>,
) -> AppResult<Response> {
    let profils: Vec<_> =
        Profil::TOUS.iter().map(|p| minijinja::context! { code => p.code(), libelle => p.libelle() }).collect();
    Ok(layout::render(
        state,
        session,
        user,
        "admin/destinataire/form.html",
        ("2", "0"),
        minijinja::context! { titre, libelle, profil, profils, errors },
    )
    .await?
    .into_response())
}

pub async fn service_new(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    ensure(u.is_super_admin())?;
    service_form(&s, &se, &u, "Ajouter un nouveau destinataire", None, None, vec![]).await
}

pub async fn service_edit(
    State(s): State<AppState>,
    se: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    ensure(u.is_super_admin())?;
    let (libelle, profil): (String, Option<String>) =
        sqlx::query_as("SELECT libelle, profil FROM destinataire WHERE id = $1").bind(id).fetch_one(&s.db).await?;
    service_form(&s, &se, &u, "Editer le destinataire", Some(libelle), profil, vec![]).await
}

async fn service_save(state: AppState, session: Session, user: CurrentUser, id: Option<i32>, mp: Multipart) -> AppResult<Response> {
    ensure(user.is_super_admin())?;
    let form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let libelle = form.required("libelle", "Libelle", &mut errors);
    // Profil : vide = service ordinaire ; sinon un code connu (contrôlé aussi par la base).
    let profil = form.text("profil");
    if let Some(code) = &profil
        && Profil::from_code(code).is_none()
    {
        errors.push("Profil inconnu.".into());
    }
    if !errors.is_empty() {
        let titre = if id.is_some() { "Editer le destinataire" } else { "Ajouter un nouveau destinataire" };
        return service_form(&state, &session, &user, titre, libelle, profil, errors).await;
    }
    match id {
        Some(id) => sqlx::query("UPDATE destinataire SET libelle = $1, profil = $2 WHERE id = $3")
            .bind(&libelle)
            .bind(&profil)
            .bind(id),
        None => sqlx::query("INSERT INTO destinataire (libelle, profil) VALUES ($1, $2)").bind(&libelle).bind(&profil),
    }
    .execute(&state.db)
    .await?;
    Ok(Redirect::to("/admin/services").into_response())
}

pub async fn service_create(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, mp: Multipart) -> AppResult<Response> {
    service_save(s, se, u, None, mp).await
}

pub async fn service_update(
    State(s): State<AppState>,
    se: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
    mp: Multipart,
) -> AppResult<Response> {
    service_save(s, se, u, Some(id), mp).await
}

async fn service_statut(state: &AppState, user: &CurrentUser, id: i32, value: bool) -> AppResult<()> {
    ensure(user.is_super_admin())?;
    sqlx::query("UPDATE destinataire SET statut = $1 WHERE id = $2").bind(value).bind(id).execute(&state.db).await?;
    Ok(())
}

pub async fn service_activer(
    State(s): State<AppState>,
    se: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
    h: HeaderMap,
) -> AppResult<Response> {
    service_statut(&s, &u, id, true).await?;
    flash(&se, "success", "Destinataire activé").await?;
    Ok(redirect_back(&h, "/admin/services"))
}

pub async fn service_desactiver(
    State(s): State<AppState>,
    se: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
    h: HeaderMap,
) -> AppResult<Response> {
    service_statut(&s, &u, id, false).await?;
    flash(&se, "warning", "Destinataire désactivé").await?;
    Ok(redirect_back(&h, "/admin/services"))
}
