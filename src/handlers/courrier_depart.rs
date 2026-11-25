//! Courrier départ (CsortirController).

use axum::Extension;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tower_sessions::Session;

use super::common::{self, Choix, Libelle, redirect_back};
use crate::auth::{CurrentUser, ensure};
use crate::error::AppResult;
use crate::files::{self, Categorie};
use crate::forms::{FormData, parse_range};
use crate::layout;
use crate::session::flash;
use crate::state::AppState;

const LISTE: &str = "/admin/courriers/departs";

#[derive(FromRow)]
struct Row {
    id: i32,
    date: Option<NaiveDate>,
    datesortir: Option<NaiveDateTime>,
    numsortir: Option<i32>,
    objet: Option<String>,
    destinataire: Option<String>,
    agent: Option<String>,
    courrier: Option<String>,
    decharge: Option<String>,
    statut: Option<bool>,
    archive: Option<bool>,
    type_id: Option<i32>,
    type_libelle: Option<String>,
    auteur_id: Option<i32>,
    auteur_libelle: Option<String>,
    initiateur_id: Option<i32>,
    initiateur_libelle: Option<String>,
}

/// Forme attendue par les templates (`csortir.type.libelle`, `csortir.auteur.libelle`…).
#[derive(Serialize)]
pub struct Csortir {
    id: i32,
    date: Option<NaiveDate>,
    datesortir: Option<NaiveDateTime>,
    numsortir: Option<i32>,
    objet: Option<String>,
    destinataire: Option<String>,
    agent: Option<String>,
    courrier: Option<String>,
    decharge: Option<String>,
    /// Envoyé (décharge jointe).
    statut: bool,
    archive: bool,
    r#type: Option<Libelle>,
    auteur: Option<Libelle>,
    initiateur: Option<Libelle>,
}

fn lib(id: Option<i32>, libelle: Option<String>) -> Option<Libelle> {
    id.zip(libelle).map(|(id, libelle)| Libelle { id, libelle })
}

impl From<Row> for Csortir {
    fn from(r: Row) -> Self {
        Csortir {
            id: r.id,
            date: r.date,
            datesortir: r.datesortir,
            numsortir: r.numsortir,
            objet: r.objet,
            destinataire: r.destinataire,
            agent: r.agent,
            courrier: r.courrier,
            decharge: r.decharge,
            statut: r.statut.unwrap_or(false),
            archive: r.archive.unwrap_or(false),
            r#type: lib(r.type_id, r.type_libelle),
            auteur: lib(r.auteur_id, r.auteur_libelle),
            initiateur: lib(r.initiateur_id, r.initiateur_libelle),
        }
    }
}

const SELECT: &str = "SELECT s.id, s.date, s.datesortir, s.numsortir, s.objet, s.destinataire, s.agent, s.courrier,
        s.decharge, s.statut, s.archive, s.type_id, t.libelle AS type_libelle,
        s.auteur_id, a.libelle AS auteur_libelle, s.initiateur_id, i.libelle AS initiateur_libelle
     FROM csortir s
     LEFT JOIN typedepart t ON t.id = s.type_id
     LEFT JOIN destinataire a ON a.id = s.auteur_id
     LEFT JOIN destinataire i ON i.id = s.initiateur_id";

async fn find(state: &AppState, id: i32) -> AppResult<Csortir> {
    Ok(sqlx::query_as::<_, Row>(&format!("{SELECT} WHERE s.id = $1")).bind(id).fetch_one(&state.db).await?.into())
}

// ---------------------------------------------------------------------------------------------
// Listes
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListQuery {
    dates: Option<String>,
}

/// `decalage` : 0 = année en cours (Départs), 1 = année précédente (Archivages).
async fn render_list(
    state: AppState,
    session: Session,
    user: CurrentUser,
    q: ListQuery,
    decalage: i32,
    template: &str,
    menu: (&str, &str),
) -> AppResult<Response> {
    ensure(user.can_manage_courrier())?;
    let range = parse_range(q.dates.as_deref());
    // `csortir.archive == 0` (nul compris) et année de `date` ; un `date` vide valait l'année en cours en Twig.
    let sql = format!(
        "{SELECT} WHERE COALESCE(s.archive, false) = false
           AND EXTRACT(YEAR FROM COALESCE(s.date, CURRENT_DATE)) = EXTRACT(YEAR FROM CURRENT_DATE) - $1
           AND ($2::date IS NULL OR s.date >= $2) AND ($3::date IS NULL OR s.date <= $3)
         ORDER BY s.date DESC NULLS LAST, s.id DESC"
    );
    let (from, to) = range.map(|(a, b)| (Some(a), Some(b))).unwrap_or((None, None));
    let csortirs: Vec<Csortir> = sqlx::query_as::<_, Row>(&sql)
        .bind(decalage)
        .bind(from)
        .bind(to)
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(Csortir::from)
        .collect();
    Ok(layout::render(&state, &session, &user, template, menu, minijinja::context! { csortirs, dates => q.dates })
        .await?
        .into_response())
}

pub async fn index(
    State(s): State<AppState>,
    se: Session,
    Extension(u): Extension<CurrentUser>,
    Query(q): Query<ListQuery>,
) -> AppResult<Response> {
    render_list(s, se, u, q, 0, "admin/csortir/index.html", ("3", "2")).await
}

pub async fn archives(
    State(s): State<AppState>,
    se: Session,
    Extension(u): Extension<CurrentUser>,
    Query(q): Query<ListQuery>,
) -> AppResult<Response> {
    render_list(s, se, u, q, 1, "admin/csortir/archive.html", ("7", "2")).await
}

pub async fn show(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    let csortir = find(&state, id).await?;
    ensure(user.can_manage_courrier() || user.owns(csortir.initiateur.as_ref().map(|i| i.id)))?;
    Ok(layout::render(&state, &session, &user, "admin/csortir/show.html", ("3", "2"), minijinja::context! { csortir })
        .await?
        .into_response())
}

// ---------------------------------------------------------------------------------------------
// Saisie, modification, décharge
// ---------------------------------------------------------------------------------------------

#[derive(Serialize, Default)]
struct Valeurs {
    r#type: Option<i32>,
    date: Option<String>,
    numsortir: Option<String>,
    objet: Option<String>,
    destinataire: Option<String>,
    auteur: Option<i32>,
    initiateur: Option<i32>,
    agent: Option<String>,
    courrier: Option<String>,
    decharge: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Nouveau,
    Edition,
}

async fn types(state: &AppState) -> sqlx::Result<Vec<Choix>> {
    sqlx::query_as("SELECT id, libelle FROM typedepart ORDER BY id").fetch_all(&state.db).await
}

async fn render_form(
    state: &AppState,
    session: &Session,
    user: &CurrentUser,
    mode: Mode,
    v: Valeurs,
    errors: Vec<String>,
) -> AppResult<Response> {
    let titre = match mode {
        Mode::Nouveau => "Enregistrement de courrier départ",
        Mode::Edition => "Modification de courrier départ",
    };
    let destinataires = common::destinataires(state).await?;
    let types = types(state).await?;
    let edition = mode == Mode::Edition;
    Ok(layout::render(
        state,
        session,
        user,
        "admin/csortir/form.html",
        ("3", "2"),
        minijinja::context! { titre, v, errors, destinataires, types, edition },
    )
    .await?
    .into_response())
}

pub async fn new(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    ensure(u.can_manage_courrier())?;
    render_form(&s, &se, &u, Mode::Nouveau, Valeurs::default(), vec![]).await
}

pub async fn edit(
    State(s): State<AppState>,
    se: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    ensure(u.can_manage_courrier())?;
    let c = find(&s, id).await?;
    let v = Valeurs {
        r#type: c.r#type.as_ref().map(|t| t.id),
        date: c.date.map(|d| d.to_string()),
        numsortir: c.numsortir.map(|n| n.to_string()),
        objet: c.objet,
        destinataire: c.destinataire,
        auteur: c.auteur.as_ref().map(|a| a.id),
        initiateur: c.initiateur.as_ref().map(|i| i.id),
        agent: c.agent,
        courrier: c.courrier,
        decharge: c.decharge,
    };
    render_form(&s, &se, &u, Mode::Edition, v, vec![]).await
}

/// Lecture et validation (CsortirType / EditersortirType : objet et agent obligatoires en modification).
fn lire(form: &FormData, mode: Mode, errors: &mut Vec<String>) -> (Valeurs, Option<NaiveDate>, Option<i32>) {
    let mut req_int = |name: &str, label: &str| {
        let v = form.int(name, label, errors);
        if form.text(name).is_none() {
            errors.push(format!("Le champ « {label} » est obligatoire."));
        }
        v
    };
    let r#type = req_int("type", "Type");
    let auteur = req_int("auteur", "Signataire");
    let initiateur = req_int("initiateur", "Initiateur");
    let numsortir = req_int("numsortir", "Numéro enregistrement");
    let date = form.date("date", "Date enregistrement", errors);
    if form.text("date").is_none() {
        errors.push("Le champ « Date enregistrement » est obligatoire.".into());
    }
    let destinataire = form.required("destinataire", "Destinataire", errors);
    let (objet, agent) = match mode {
        Mode::Nouveau => (form.text("objet"), form.text("agent")),
        Mode::Edition => (form.required("objet", "Objet", errors), form.required("agent", "Agent de dépot", errors)),
    };
    let v = Valeurs {
        r#type,
        date: form.text("date"),
        numsortir: form.text("numsortir"),
        objet,
        destinataire,
        auteur,
        initiateur,
        agent,
        courrier: None,
        decharge: None,
    };
    (v, date, numsortir)
}

/// Enregistre les fichiers envoyés. Renvoie (courrier, décharge) ; une décharge passe le courrier à « envoyé ».
async fn fichiers(state: &AppState, form: &mut FormData, errors: &mut Vec<String>) -> (Option<String>, Option<String>) {
    let mut out = (None, None);
    for (champ, slot) in [("courrier", &mut out.0), ("decharge", &mut out.1)] {
        if let Some(up) = form.file(champ) {
            match files::save(state, Categorie::CourrierSorti, &up, files::DOCUMENTS).await {
                Ok(name) => *slot = Some(name),
                Err(e) => errors.push(format!("{champ} : {e}")),
            }
        }
    }
    out
}

pub async fn create(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    mp: Multipart,
) -> AppResult<Response> {
    ensure(user.can_manage_courrier())?;
    let mut form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let (v, date, numsortir) = lire(&form, Mode::Nouveau, &mut errors);
    let (courrier, decharge) = if errors.is_empty() { fichiers(&state, &mut form, &mut errors).await } else { (None, None) };
    if !errors.is_empty() {
        return render_form(&state, &session, &user, Mode::Nouveau, v, errors).await;
    }
    sqlx::query(
        "INSERT INTO csortir (type_id, date, numsortir, objet, destinataire, auteur_id, initiateur_id, agent,
                              courrier, decharge, statut, datesortir)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, (now() AT TIME ZONE 'UTC'))",
    )
    .bind(v.r#type)
    .bind(date)
    .bind(numsortir)
    .bind(&v.objet)
    .bind(&v.destinataire)
    .bind(v.auteur)
    .bind(v.initiateur)
    .bind(&v.agent)
    .bind(courrier)
    .bind(&decharge)
    .bind(decharge.is_some())
    .execute(&state.db)
    .await?;
    Ok(Redirect::to(LISTE).into_response())
}

pub async fn update(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
    mp: Multipart,
) -> AppResult<Response> {
    ensure(user.can_manage_courrier())?;
    let existant = find(&state, id).await?;
    let mut form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let (mut v, date, numsortir) = lire(&form, Mode::Edition, &mut errors);
    let (courrier, decharge) = if errors.is_empty() { fichiers(&state, &mut form, &mut errors).await } else { (None, None) };
    if !errors.is_empty() {
        v.courrier = existant.courrier;
        v.decharge = existant.decharge;
        return render_form(&state, &session, &user, Mode::Edition, v, errors).await;
    }
    sqlx::query(
        "UPDATE csortir SET type_id = $1, date = $2, numsortir = $3, objet = $4, destinataire = $5, auteur_id = $6,
                initiateur_id = $7, agent = $8, courrier = COALESCE($9, courrier), decharge = COALESCE($10, decharge),
                statut = CASE WHEN $10::text IS NULL THEN statut ELSE true END
         WHERE id = $11",
    )
    .bind(v.r#type)
    .bind(date)
    .bind(numsortir)
    .bind(&v.objet)
    .bind(&v.destinataire)
    .bind(v.auteur)
    .bind(v.initiateur)
    .bind(&v.agent)
    .bind(courrier)
    .bind(decharge)
    .bind(id)
    .execute(&state.db)
    .await?;
    Ok(Redirect::to(LISTE).into_response())
}

pub async fn decharge_form(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    ensure(user.can_manage_courrier())?;
    let csortir = find(&state, id).await?;
    Ok(layout::render(
        &state,
        &session,
        &user,
        "admin/csortir/decharge.html",
        ("3", "2"),
        minijinja::context! { csortir, errors => Vec::<String>::new() },
    )
    .await?
    .into_response())
}

pub async fn decharge_submit(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
    mp: Multipart,
) -> AppResult<Response> {
    ensure(user.can_manage_courrier())?;
    let csortir = find(&state, id).await?;
    let mut form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let mut fichier = None;
    if let Some(up) = form.file("decharge") {
        match files::save(&state, Categorie::CourrierSorti, &up, files::DOCUMENTS).await {
            Ok(name) => fichier = Some(name),
            Err(e) => errors.push(e),
        }
    }
    if !errors.is_empty() {
        return Ok(layout::render(
            &state,
            &session,
            &user,
            "admin/csortir/decharge.html",
            ("3", "2"),
            minijinja::context! { csortir, errors },
        )
        .await?
        .into_response());
    }
    sqlx::query(
        "UPDATE csortir SET agent = $1, decharge = COALESCE($2, decharge),
                statut = CASE WHEN $2::text IS NULL THEN statut ELSE true END
         WHERE id = $3",
    )
    .bind(form.text("agent"))
    .bind(fichier)
    .bind(id)
    .execute(&state.db)
    .await?;
    Ok(Redirect::to(LISTE).into_response())
}

// ---------------------------------------------------------------------------------------------
// Actions (POST + CSRF)
// ---------------------------------------------------------------------------------------------

pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
    headers: HeaderMap,
) -> AppResult<Response> {
    ensure(user.can_manage_courrier())?;
    sqlx::query("DELETE FROM csortir WHERE id = $1").bind(id).execute(&state.db).await?;
    flash(&session, "success", "Courrier supprimé avec succès").await?;
    Ok(redirect_back(&headers, LISTE))
}

async fn set_archive(state: &AppState, user: &CurrentUser, id: i32, value: bool) -> AppResult<()> {
    ensure(user.can_manage_courrier())?;
    sqlx::query("UPDATE csortir SET archive = $1 WHERE id = $2").bind(value).bind(id).execute(&state.db).await?;
    Ok(())
}

pub async fn archive(
    State(s): State<AppState>,
    se: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
    h: HeaderMap,
) -> AppResult<Response> {
    set_archive(&s, &u, id, true).await?;
    flash(&se, "success", "Dossier archivé avec succès").await?;
    Ok(redirect_back(&h, LISTE))
}

pub async fn unarchive(
    State(s): State<AppState>,
    se: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
    h: HeaderMap,
) -> AppResult<Response> {
    set_archive(&s, &u, id, false).await?;
    flash(&se, "success", "Dossier retourné avec succès").await?;
    Ok(redirect_back(&h, "/admin/courriers/departs/archives"))
}
