//! Audiences (AudienceController) et rendez-vous (RendvousController).

use axum::Extension;
use axum::extract::{Multipart, Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{NaiveDate, NaiveTime};
use serde::Serialize;
use sqlx::FromRow;
use tower_sessions::Session;

use super::common::{self, Libelle, redirect_back};
use crate::auth::{CurrentUser, ensure};
use crate::error::AppResult;
use crate::forms::FormData;
use crate::layout;
use crate::session::flash;
use crate::state::AppState;

const AUDIENCES: &str = "/admin/audiences";
const RDV: &str = "/admin/rendez-vous";

async fn render(s: &AppState, se: &Session, u: &CurrentUser, t: &str, menu: (&str, &str), ctx: minijinja::Value) -> AppResult<Response> {
    Ok(layout::render(s, se, u, t, menu, ctx).await?.into_response())
}

// =============================================================================================
// Audiences
// =============================================================================================

#[derive(FromRow)]
struct Row {
    id: i32,
    date: NaiveDate,
    heure: Option<NaiveTime>,
    nom: Option<String>,
    service: Option<String>,
    telephone: Option<String>,
    email: Option<String>,
    qualite: Option<String>,
    rdv: Option<bool>,
    objet: Option<String>,
    statut: Option<bool>,
    commentaire: Option<String>,
    destinataire_id: Option<i32>,
    d_libelle: Option<String>,
}

/// Forme attendue par les templates (`audience.destinataire.libelle`, `audience.statut`).
#[derive(Serialize)]
pub struct Audience {
    id: i32,
    date: NaiveDate,
    heure: Option<NaiveTime>,
    nom: Option<String>,
    service: Option<String>,
    telephone: Option<String>,
    email: Option<String>,
    qualite: Option<String>,
    rdv: bool,
    objet: Option<String>,
    /// Acceptée.
    statut: bool,
    commentaire: Option<String>,
    destinataire: Option<Libelle>,
}

impl From<Row> for Audience {
    fn from(r: Row) -> Self {
        Audience {
            id: r.id,
            date: r.date,
            heure: r.heure,
            nom: r.nom,
            service: r.service,
            telephone: r.telephone,
            email: r.email,
            qualite: r.qualite,
            rdv: r.rdv.unwrap_or(false),
            objet: r.objet,
            statut: r.statut.unwrap_or(false),
            commentaire: r.commentaire,
            destinataire: r.destinataire_id.zip(r.d_libelle).map(|(id, libelle)| Libelle { id, libelle }),
        }
    }
}

pub async fn audiences(state: &AppState, where_: &str) -> AppResult<Vec<Audience>> {
    Ok(sqlx::query_as::<_, Row>(&format!(
        "SELECT a.id, a.date, a.heure, a.nom, a.service, a.telephone, a.email, a.qualite, a.rdv, a.objet, a.statut,
                a.commentaire, a.destinataire_id, d.libelle AS d_libelle
         FROM audience a LEFT JOIN destinataire d ON d.id = a.destinataire_id
         WHERE {where_} ORDER BY a.id DESC"
    ))
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(Audience::from)
    .collect())
}

async fn find(s: &AppState, id: i32) -> AppResult<Audience> {
    audiences(s, &format!("a.id = {id}")).await?.into_iter().next().ok_or(crate::error::AppError::NotFound)
}

pub async fn index(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    ensure(u.can_manage_audiences())?;
    let list = audiences(&s, "true").await?;
    render(&s, &se, &u, "admin/audience/index.html", ("4", "4"), minijinja::context! { audiences => list }).await
}

pub async fn show(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    ensure(u.can_manage_audiences())?;
    let audience = find(&s, id).await?;
    render(&s, &se, &u, "admin/audience/show.html", ("4", "4"), minijinja::context! { audience }).await
}

#[derive(Serialize, Default)]
struct Valeurs {
    destinataire: Option<i32>,
    date: Option<String>,
    heure: Option<String>,
    nom: Option<String>,
    telephone: Option<String>,
    email: Option<String>,
    service: Option<String>,
    qualite: Option<String>,
    rdv: bool,
    objet: Option<String>,
}

async fn form_page(s: &AppState, se: &Session, u: &CurrentUser, edition: bool, v: Valeurs, errors: Vec<String>) -> AppResult<Response> {
    let titre = if edition { "Editer la demande" } else { "Ajouter une demande d'audience" };
    let destinataires = common::destinataires(s).await?;
    render(s, se, u, "admin/audience/form.html", ("4", "4"), minijinja::context! { titre, v, errors, destinataires }).await
}

pub async fn new(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    ensure(u.can_manage_audiences())?;
    form_page(&s, &se, &u, false, Valeurs { rdv: true, ..Default::default() }, vec![]).await
}

pub async fn edit(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    ensure(u.can_manage_audiences())?;
    let a = find(&s, id).await?;
    let v = Valeurs {
        destinataire: a.destinataire.as_ref().map(|d| d.id),
        date: Some(a.date.to_string()),
        heure: a.heure.map(|h| h.format("%H:%M").to_string()),
        nom: a.nom,
        telephone: a.telephone,
        email: a.email,
        service: a.service,
        qualite: a.qualite,
        rdv: a.rdv,
        objet: a.objet,
    };
    form_page(&s, &se, &u, true, v, vec![]).await
}

fn heure(form: &FormData, name: &str, label: &str, errors: &mut Vec<String>) -> Option<NaiveTime> {
    let v = form.text(name)?;
    NaiveTime::parse_from_str(&v, "%H:%M").or_else(|_| NaiveTime::parse_from_str(&v, "%H:%M:%S")).ok().or_else(|| {
        errors.push(format!("Le champ « {label} » n'est pas une heure valide."));
        None
    })
}

async fn save(state: AppState, session: Session, user: CurrentUser, id: Option<i32>, mp: Multipart) -> AppResult<Response> {
    ensure(user.can_manage_audiences())?;
    if let Some(id) = id {
        find(&state, id).await?;
    }
    let form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let destinataire = form.int("destinataire", "Destinataire", &mut errors);
    if destinataire.is_none() {
        errors.push("Le champ « Destinataire » est obligatoire.".into());
    }
    // Facultative dans AudienceType mais obligatoire en base : l'original plantait si elle était vide.
    let date = form.date("date", "Date", &mut errors);
    if form.text("date").is_none() {
        errors.push("Le champ « Date » est obligatoire.".into());
    }
    let h = heure(&form, "heure", "Heure", &mut errors);
    let v = Valeurs {
        destinataire,
        date: form.text("date"),
        heure: form.text("heure"),
        nom: form.text("nom"),
        telephone: form.text("telephone"),
        email: form.text("email"),
        service: form.text("service"),
        qualite: form.text("qualite"),
        rdv: form.flag("rdv"),
        objet: form.text("objet"),
    };
    if !errors.is_empty() {
        return form_page(&state, &session, &user, id.is_some(), v, errors).await;
    }
    match id {
        Some(id) => sqlx::query(
            "UPDATE audience SET destinataire_id = $1, date = $2, heure = $3, nom = $4, telephone = $5, email = $6,
                    service = $7, qualite = $8, rdv = $9, objet = $10 WHERE id = $11",
        )
        .bind(destinataire).bind(date).bind(h).bind(&v.nom).bind(&v.telephone).bind(&v.email)
        .bind(&v.service).bind(&v.qualite).bind(v.rdv).bind(&v.objet).bind(id),
        None => sqlx::query(
            "INSERT INTO audience (destinataire_id, date, heure, nom, telephone, email, service, qualite, rdv, objet)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(destinataire).bind(date).bind(h).bind(&v.nom).bind(&v.telephone).bind(&v.email)
        .bind(&v.service).bind(&v.qualite).bind(v.rdv).bind(&v.objet),
    }
    .execute(&state.db)
    .await?;
    // Le profil accueil revient à sa page d'accueil, qui liste les audiences.
    let target = if user.is_super_admin() { AUDIENCES } else { "/admin" };
    Ok(Redirect::to(target).into_response())
}

pub async fn create(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, mp: Multipart) -> AppResult<Response> {
    save(s, se, u, None, mp).await
}
pub async fn update(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, mp: Multipart) -> AppResult<Response> {
    save(s, se, u, Some(id), mp).await
}

async fn set_statut(s: &AppState, u: &CurrentUser, id: i32, value: bool) -> AppResult<()> {
    ensure(u.is_super_admin())?;
    sqlx::query("UPDATE audience SET statut = $1 WHERE id = $2").bind(value).bind(id).execute(&s.db).await?;
    Ok(())
}

pub async fn accepter(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, h: HeaderMap) -> AppResult<Response> {
    set_statut(&s, &u, id, true).await?;
    flash(&se, "success", "Audience acceptée").await?;
    Ok(redirect_back(&h, AUDIENCES))
}
pub async fn refuser(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, h: HeaderMap) -> AppResult<Response> {
    set_statut(&s, &u, id, false).await?;
    flash(&se, "warning", "Audience refusée").await?;
    Ok(redirect_back(&h, AUDIENCES))
}

pub async fn delete(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, h: HeaderMap) -> AppResult<Response> {
    ensure(u.is_super_admin())?;
    let mut tx = s.db.begin().await?;
    sqlx::query("DELETE FROM rendvous WHERE audience_id = $1").bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM audience WHERE id = $1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    flash(&se, "success", "Audience supprimée").await?;
    Ok(redirect_back(&h, AUDIENCES))
}

// =============================================================================================
// Rendez-vous
// =============================================================================================

#[derive(Serialize, FromRow)]
struct Rendvous {
    id: i32,
    date: Option<NaiveDate>,
    datefin: Option<NaiveDate>,
    heured: Option<NaiveTime>,
    heuref: Option<NaiveTime>,
    lieu: Option<String>,
    message: Option<String>,
}

pub async fn rdv_index(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    ensure(u.can_manage_audiences())?;
    let rendvouses: Vec<Rendvous> =
        sqlx::query_as("SELECT id, date, datefin, heured, heuref, lieu, message FROM rendvous ORDER BY id DESC").fetch_all(&s.db).await?;
    render(&s, &se, &u, "admin/rendvous/index.html", ("4", "4"), minijinja::context! { rendvouses }).await
}

async fn rdv_form(s: &AppState, se: &Session, u: &CurrentUser, edition: bool, v: minijinja::Value, errors: Vec<String>) -> AppResult<Response> {
    let titre = if edition { "Modifier le rendez-vous" } else { "Nouveau rendez-vous" };
    render(s, se, u, "admin/rendvous/form.html", ("4", "4"), minijinja::context! { titre, v, errors }).await
}

pub async fn rdv_new(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    ensure(u.can_manage_audiences())?;
    rdv_form(&s, &se, &u, false, minijinja::context! {}, vec![]).await
}

pub async fn rdv_edit(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    ensure(u.can_manage_audiences())?;
    let r: Rendvous =
        sqlx::query_as("SELECT id, date, datefin, heured, heuref, lieu, message FROM rendvous WHERE id = $1").bind(id).fetch_one(&s.db).await?;
    let v = minijinja::context! {
        date => r.date.map(|d| d.to_string()), datefin => r.datefin.map(|d| d.to_string()),
        heured => r.heured.map(|h| h.format("%H:%M").to_string()), heuref => r.heuref.map(|h| h.format("%H:%M").to_string()),
        lieu => r.lieu, message => r.message,
    };
    rdv_form(&s, &se, &u, true, v, vec![]).await
}

async fn rdv_save(state: AppState, session: Session, user: CurrentUser, id: Option<i32>, mp: Multipart) -> AppResult<Response> {
    ensure(user.can_manage_audiences())?;
    let form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let date = form.date("date", "Date debut", &mut errors);
    let datefin = form.date("datefin", "Date fin", &mut errors);
    let heured = heure(&form, "heured", "Heure debut", &mut errors);
    let heuref = heure(&form, "heuref", "Heure Fin", &mut errors);
    if !errors.is_empty() {
        let v = minijinja::context! {
            date => form.text("date"), datefin => form.text("datefin"), heured => form.text("heured"),
            heuref => form.text("heuref"), lieu => form.text("lieu"), message => form.text("message"),
        };
        return rdv_form(&state, &session, &user, id.is_some(), v, errors).await;
    }
    match id {
        Some(id) => sqlx::query("UPDATE rendvous SET date = $1, datefin = $2, heured = $3, heuref = $4, lieu = $5, message = $6 WHERE id = $7")
            .bind(date).bind(datefin).bind(heured).bind(heuref).bind(form.text("lieu")).bind(form.text("message")).bind(id),
        None => sqlx::query("INSERT INTO rendvous (date, datefin, heured, heuref, lieu, message) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(date).bind(datefin).bind(heured).bind(heuref).bind(form.text("lieu")).bind(form.text("message")),
    }
    .execute(&state.db)
    .await?;
    Ok(Redirect::to(RDV).into_response())
}

pub async fn rdv_create(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, mp: Multipart) -> AppResult<Response> {
    rdv_save(s, se, u, None, mp).await
}
pub async fn rdv_update(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, mp: Multipart) -> AppResult<Response> {
    rdv_save(s, se, u, Some(id), mp).await
}

pub async fn rdv_delete(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    ensure(u.can_manage_audiences())?;
    sqlx::query("DELETE FROM rendvous WHERE id = $1").bind(id).execute(&s.db).await?;
    flash(&se, "success", "Rendez-vous supprimé").await?;
    Ok(Redirect::to(RDV).into_response())
}
