//! Cabinet du Ministre : comptes rendus (CompterenduController) et diligences (DiligenceController).

use axum::Extension;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tower_sessions::Session;

use super::common::redirect_back;
use crate::auth::{CurrentUser, ensure};
use crate::error::AppResult;
use crate::files::{self, Categorie};
use crate::forms::{FormData, parse_range};
use crate::layout;
use crate::session::flash;
use crate::state::AppState;

const CRS: &str = "/admin/comptes-rendus";
const DILIGENCES: &str = "/admin/diligences";

// =============================================================================================
// Données
// =============================================================================================

#[derive(Serialize, FromRow, Clone)]
pub struct DiligenceRow {
    pub id: i32,
    cr_id: Option<i32>,
    titre: Option<String>,
    libelle: Option<String>,
    ressource: Option<String>,
    debut: Option<NaiveDate>,
    delai: Option<NaiveDate>,
    #[serde(skip_serializing)]
    etat_brut: Option<bool>,
    observation: Option<String>,
    taux: Option<i32>,
    #[serde(skip_serializing)]
    archive_brut: Option<bool>,
    #[serde(skip_serializing)]
    cr_date: Option<NaiveDate>,
}

#[derive(Serialize, Clone)]
struct CrRef {
    id: i32,
    date: NaiveDate,
}

/// Forme attendue par les templates (`diligence.cr.date`, `diligence.etat`).
#[derive(Serialize, Clone)]
pub struct Diligence {
    #[serde(flatten)]
    d: DiligenceRow,
    /// Réalisée.
    etat: bool,
    archive: bool,
    cr: Option<CrRef>,
}

impl From<DiligenceRow> for Diligence {
    fn from(d: DiligenceRow) -> Self {
        Diligence {
            etat: d.etat_brut.unwrap_or(false),
            archive: d.archive_brut.unwrap_or(false),
            cr: d.cr_id.zip(d.cr_date).map(|(id, date)| CrRef { id, date }),
            d,
        }
    }
}

const DILIGENCE_SELECT: &str = "SELECT g.id, g.cr_id, g.titre, g.libelle, g.ressource, g.debut, g.delai,
        g.etat AS etat_brut, g.observation, g.taux, g.archive AS archive_brut, c.date AS cr_date
     FROM diligence g LEFT JOIN compterendu c ON c.id = g.cr_id";

pub async fn diligences(state: &AppState, where_: &str, order: &str, range: Option<(NaiveDate, NaiveDate)>) -> AppResult<Vec<Diligence>> {
    let (from, to) = range.map(|(a, b)| (Some(a), Some(b))).unwrap_or((None, None));
    let sql = format!(
        "{DILIGENCE_SELECT} WHERE ({where_}) AND ($1::date IS NULL OR g.debut >= $1) AND ($2::date IS NULL OR g.debut <= $2)
         ORDER BY {order}"
    );
    Ok(sqlx::query_as::<_, DiligenceRow>(&sql)
        .bind(from)
        .bind(to)
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(Diligence::from)
        .collect())
}

#[derive(FromRow)]
struct CrRow {
    id: i32,
    date: NaiveDate,
    debut: Option<NaiveTime>,
    fin: Option<NaiveTime>,
    lieu: Option<String>,
    presidence: Option<String>,
    redacteur: Option<String>,
    cr: Option<String>,
    listep: Option<String>,
    objet: Option<String>,
    statut: Option<bool>,
    archive: Option<bool>,
}

/// Forme attendue par les templates (`compterendu.diligence`, `compterendu.statut`).
#[derive(Serialize)]
pub struct CompteRendu {
    id: i32,
    date: NaiveDate,
    debut: Option<NaiveTime>,
    fin: Option<NaiveTime>,
    lieu: Option<String>,
    presidence: Option<String>,
    redacteur: Option<String>,
    cr: Option<String>,
    listep: Option<String>,
    objet: Option<String>,
    /// Public.
    statut: bool,
    archive: bool,
    diligence: Vec<Diligence>,
}

/// Comptes rendus filtrés, avec leurs diligences (deux requêtes au lieu d'une par compte rendu).
pub async fn comptes_rendus(state: &AppState, where_: &str) -> AppResult<Vec<CompteRendu>> {
    let rows: Vec<CrRow> = sqlx::query_as(&format!(
        "SELECT id, date, debut, fin, lieu, presidence, redacteur, cr, listep, objet, statut, archive
         FROM compterendu c WHERE {where_} ORDER BY date DESC, id DESC"
    ))
    .fetch_all(&state.db)
    .await?;
    let toutes = diligences(state, "g.cr_id IS NOT NULL", "g.id", None).await?;
    Ok(rows
        .into_iter()
        .map(|c| CompteRendu {
            diligence: toutes.iter().filter(|d| d.d.cr_id == Some(c.id)).cloned().collect(),
            id: c.id,
            date: c.date,
            debut: c.debut,
            fin: c.fin,
            lieu: c.lieu,
            presidence: c.presidence,
            redacteur: c.redacteur,
            cr: c.cr,
            listep: c.listep,
            objet: c.objet,
            statut: c.statut.unwrap_or(false),
            archive: c.archive.unwrap_or(false),
        })
        .collect())
}

async fn cr_find(state: &AppState, id: i32) -> AppResult<CompteRendu> {
    comptes_rendus(state, &format!("c.id = {id}")).await?.into_iter().next().ok_or(crate::error::AppError::NotFound)
}

// =============================================================================================
// Comptes rendus
// =============================================================================================

async fn render(state: &AppState, session: &Session, user: &CurrentUser, template: &str, menu: (&str, &str), ctx: minijinja::Value) -> AppResult<Response> {
    Ok(layout::render(state, session, user, template, menu, ctx).await?.into_response())
}

pub async fn cr_index(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    ensure(u.can_manage_cabinet())?;
    let compterendus = comptes_rendus(&s, "COALESCE(c.archive, false) = false").await?;
    render(&s, &se, &u, "admin/compterendu/index.html", ("4", "1"), minijinja::context! { compterendus }).await
}

pub async fn cr_archives(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    ensure(u.can_manage_cabinet())?;
    let compterendus = comptes_rendus(&s, "c.archive = true").await?;
    render(&s, &se, &u, "admin/compterendu/archive.html", ("7", "4"), minijinja::context! { compterendus }).await
}

pub async fn cr_show(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    ensure(u.can_manage_cabinet())?;
    let compterendu = cr_find(&s, id).await?;
    render(&s, &se, &u, "admin/compterendu/show.html", ("4", "1"), minijinja::context! { compterendu }).await
}

#[derive(Serialize, Default)]
struct CrValeurs {
    date: Option<String>,
    debut: Option<String>,
    fin: Option<String>,
    objet: Option<String>,
    lieu: Option<String>,
    presidence: Option<String>,
    redacteur: Option<String>,
    statut: bool,
    cr: Option<String>,
    listep: Option<String>,
}

const PRESIDENCES: &[&str] = &["MINISTRE", "DIRCAB", "IG"];

async fn cr_form(state: &AppState, session: &Session, user: &CurrentUser, edition: bool, v: CrValeurs, errors: Vec<String>) -> AppResult<Response> {
    let titre = if edition { "Editer un CR" } else { "Ajouter un CR" };
    render(state, session, user, "admin/compterendu/form.html", ("4", "1"), minijinja::context! { titre, v, errors, presidences => PRESIDENCES }).await
}

pub async fn cr_new(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    ensure(u.can_manage_cabinet())?;
    cr_form(&s, &se, &u, false, CrValeurs { statut: true, ..Default::default() }, vec![]).await
}

pub async fn cr_edit(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    ensure(u.can_manage_cabinet())?;
    let c = cr_find(&s, id).await?;
    let v = CrValeurs {
        date: Some(c.date.to_string()),
        debut: c.debut.map(|t| t.format("%H:%M").to_string()),
        fin: c.fin.map(|t| t.format("%H:%M").to_string()),
        objet: c.objet,
        lieu: c.lieu,
        presidence: c.presidence,
        redacteur: c.redacteur,
        statut: c.statut,
        cr: c.cr,
        listep: c.listep,
    };
    cr_form(&s, &se, &u, true, v, vec![]).await
}

fn heure(form: &FormData, name: &str, label: &str, errors: &mut Vec<String>) -> Option<NaiveTime> {
    let v = form.text(name);
    if v.is_none() {
        errors.push(format!("Le champ « {label} » est obligatoire."));
        return None;
    }
    let v = v.unwrap();
    NaiveTime::parse_from_str(&v, "%H:%M").or_else(|_| NaiveTime::parse_from_str(&v, "%H:%M:%S")).ok().or_else(|| {
        errors.push(format!("Le champ « {label} » n'est pas une heure valide."));
        None
    })
}

async fn cr_save(state: AppState, session: Session, user: CurrentUser, id: Option<i32>, mp: Multipart) -> AppResult<Response> {
    ensure(user.can_manage_cabinet())?;
    let existant = match id {
        Some(id) => Some(cr_find(&state, id).await?),
        None => None,
    };
    let mut form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let date = form.date("date", "Date du conseil", &mut errors);
    if form.text("date").is_none() {
        errors.push("Le champ « Date du conseil » est obligatoire.".into());
    }
    let debut = heure(&form, "debut", "Heure de debut", &mut errors);
    let fin = heure(&form, "fin", "Heure de fin", &mut errors);
    let lieu = form.required("lieu", "Lieu", &mut errors);
    let redacteur = form.required("redacteur", "Rédacteur", &mut errors);
    let presidence = form.text("presidence").filter(|p| PRESIDENCES.contains(&p.as_str()));
    if presidence.is_none() {
        errors.push("Présidence invalide.".into());
    }
    let mut v = CrValeurs {
        date: form.text("date"),
        debut: form.text("debut"),
        fin: form.text("fin"),
        objet: form.text("objet"),
        lieu: lieu.clone(),
        presidence: presidence.clone(),
        redacteur: redacteur.clone(),
        statut: form.flag("statut"),
        cr: existant.as_ref().and_then(|c| c.cr.clone()),
        listep: existant.as_ref().and_then(|c| c.listep.clone()),
    };
    let mut cr_file = None;
    let mut listep_file = None;
    if errors.is_empty() {
        for (champ, slot) in [("cr", &mut cr_file), ("listep", &mut listep_file)] {
            if let Some(up) = form.file(champ) {
                match files::save(&state, Categorie::CompteRendu, &up, files::DOCUMENTS).await {
                    Ok(name) => *slot = Some(name),
                    Err(e) => errors.push(e),
                }
            }
        }
    }
    if !errors.is_empty() {
        v.cr = existant.as_ref().and_then(|c| c.cr.clone());
        return cr_form(&state, &session, &user, id.is_some(), v, errors).await;
    }
    match id {
        Some(id) => sqlx::query(
            "UPDATE compterendu SET date = $1, debut = $2, fin = $3, objet = $4, lieu = $5, presidence = $6, redacteur = $7,
                    statut = $8, cr = COALESCE($9, cr), listep = COALESCE($10, listep) WHERE id = $11",
        )
        .bind(date).bind(debut).bind(fin).bind(&v.objet).bind(&lieu).bind(&presidence).bind(&redacteur)
        .bind(v.statut).bind(cr_file).bind(listep_file).bind(id),
        None => sqlx::query(
            "INSERT INTO compterendu (date, debut, fin, objet, lieu, presidence, redacteur, statut, cr, listep, create_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, (now() AT TIME ZONE 'UTC'))",
        )
        .bind(date).bind(debut).bind(fin).bind(&v.objet).bind(&lieu).bind(&presidence).bind(&redacteur)
        .bind(v.statut).bind(cr_file).bind(listep_file),
    }
    .execute(&state.db)
    .await?;
    Ok(Redirect::to(CRS).into_response())
}

pub async fn cr_create(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, mp: Multipart) -> AppResult<Response> {
    cr_save(s, se, u, None, mp).await
}
pub async fn cr_update(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, mp: Multipart) -> AppResult<Response> {
    cr_save(s, se, u, Some(id), mp).await
}

pub async fn cr_delete(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, h: HeaderMap) -> AppResult<Response> {
    ensure(u.can_manage_cabinet())?;
    let used: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM diligence WHERE cr_id = $1)").bind(id).fetch_one(&s.db).await?;
    if used {
        flash(&se, "warning", "Ce compte rendu a des diligences : supprimez-les d'abord.").await?;
    } else {
        sqlx::query("DELETE FROM compterendu WHERE id = $1").bind(id).execute(&s.db).await?;
        flash(&se, "success", "Compte rendu supprimé").await?;
    }
    Ok(redirect_back(&h, CRS))
}

async fn cr_set_archive(s: &AppState, u: &CurrentUser, id: i32, value: bool) -> AppResult<()> {
    ensure(u.can_manage_cabinet())?;
    sqlx::query("UPDATE compterendu SET archive = $1 WHERE id = $2").bind(value).bind(id).execute(&s.db).await?;
    Ok(())
}

pub async fn cr_archive(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, h: HeaderMap) -> AppResult<Response> {
    cr_set_archive(&s, &u, id, true).await?;
    flash(&se, "success", "Dossier archivé avec succès").await?;
    Ok(redirect_back(&h, CRS))
}
pub async fn cr_unarchive(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, h: HeaderMap) -> AppResult<Response> {
    cr_set_archive(&s, &u, id, false).await?;
    flash(&se, "success", "Dossier retourné avec succès").await?;
    Ok(redirect_back(&h, "/admin/comptes-rendus/archives"))
}

// =============================================================================================
// Diligences
// =============================================================================================

#[derive(Deserialize)]
pub struct ListQuery {
    dates: Option<String>,
}

const ANNEE: &str = "EXTRACT(YEAR FROM COALESCE(g.debut, CURRENT_DATE)) = EXTRACT(YEAR FROM CURRENT_DATE)";

pub async fn dil_index(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Query(q): Query<ListQuery>) -> AppResult<Response> {
    ensure(u.can_manage_cabinet())?;
    let list = diligences(&s, &format!("COALESCE(g.archive, false) = false AND {ANNEE}"), "g.debut DESC NULLS LAST, g.id DESC", parse_range(q.dates.as_deref())).await?;
    render(&s, &se, &u, "admin/diligence/index.html", ("4", "2"), minijinja::context! { diligences => list, dates => q.dates }).await
}

pub async fn dil_archives(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Query(q): Query<ListQuery>) -> AppResult<Response> {
    ensure(u.can_manage_cabinet())?;
    let list = diligences(&s, "g.archive = true", "g.debut DESC NULLS LAST, g.id DESC", parse_range(q.dates.as_deref())).await?;
    render(&s, &se, &u, "admin/diligence/archive.html", ("7", "5"), minijinja::context! { diligences => list, dates => q.dates }).await
}

async fn dil_find(s: &AppState, id: i32) -> AppResult<Diligence> {
    diligences(s, &format!("g.id = {id}"), "g.id", None).await?.into_iter().next().ok_or(crate::error::AppError::NotFound)
}

pub async fn dil_show(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    ensure(u.can_manage_cabinet())?;
    let diligence = dil_find(&s, id).await?;
    render(&s, &se, &u, "admin/diligence/show.html", ("4", "2"), minijinja::context! { diligence }).await
}

#[derive(Serialize, Default)]
struct DilValeurs {
    titre: Option<String>,
    libelle: Option<String>,
    ressource: Option<String>,
    debut: Option<String>,
    delai: Option<String>,
    taux: Option<String>,
    etat: bool,
    observation: Option<String>,
}

async fn dil_form(s: &AppState, se: &Session, u: &CurrentUser, edition: bool, v: DilValeurs, errors: Vec<String>) -> AppResult<Response> {
    let titre = if edition { "Editer une diligence" } else { "Ajouter une diligence" };
    render(s, se, u, "admin/diligence/form.html", ("4", "2"), minijinja::context! { titre, v, errors }).await
}

pub async fn dil_new(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    ensure(u.can_manage_cabinet())?;
    dil_form(&s, &se, &u, false, DilValeurs { etat: true, ..Default::default() }, vec![]).await
}

pub async fn dil_new_cr(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(cr_id): Path<i32>) -> AppResult<Response> {
    ensure(u.can_manage_cabinet())?;
    cr_find(&s, cr_id).await?;
    dil_form(&s, &se, &u, false, DilValeurs { etat: true, ..Default::default() }, vec![]).await
}

pub async fn dil_edit(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    ensure(u.can_manage_cabinet())?;
    let d = dil_find(&s, id).await?;
    let v = DilValeurs {
        titre: d.d.titre,
        libelle: d.d.libelle,
        ressource: d.d.ressource,
        debut: d.d.debut.map(|x| x.to_string()),
        delai: d.d.delai.map(|x| x.to_string()),
        taux: d.d.taux.map(|x| x.to_string()),
        etat: d.etat,
        observation: d.d.observation,
    };
    dil_form(&s, &se, &u, true, v, vec![]).await
}

/// `cible` : None = nouvelle diligence libre, Some(Ok(cr)) = nouvelle sur un CR, Some(Err(id)) = modification.
async fn dil_save(state: AppState, session: Session, user: CurrentUser, cible: Option<Result<i32, i32>>, mp: Multipart) -> AppResult<Response> {
    ensure(user.can_manage_cabinet())?;
    let form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let titre = form.required("titre", "Titre", &mut errors);
    let ressource = form.required("ressource", "Ressource(s)", &mut errors);
    let debut = form.date("debut", "Début exécution", &mut errors);
    let delai = form.date("delai", "Délai exécution", &mut errors);
    let taux = form.int("taux", "Taux", &mut errors);
    let v = DilValeurs {
        titre: titre.clone(),
        libelle: form.text("libelle"),
        ressource: ressource.clone(),
        debut: form.text("debut"),
        delai: form.text("delai"),
        taux: form.text("taux"),
        etat: form.flag("etat"),
        observation: form.text("observation"),
    };
    if !errors.is_empty() {
        return dil_form(&state, &session, &user, matches!(cible, Some(Err(_))), v, errors).await;
    }
    let target = match cible {
        Some(Err(id)) => {
            sqlx::query(
                "UPDATE diligence SET titre = $1, libelle = $2, ressource = $3, debut = $4, delai = $5, taux = $6, etat = $7,
                        observation = $8 WHERE id = $9",
            )
            .bind(&titre).bind(&v.libelle).bind(&ressource).bind(debut).bind(delai).bind(taux).bind(v.etat)
            .bind(&v.observation).bind(id)
            .execute(&state.db)
            .await?;
            DILIGENCES
        }
        nouvelle => {
            let cr_id = match nouvelle {
                Some(Ok(cr)) => Some(cr),
                _ => None,
            };
            sqlx::query(
                "INSERT INTO diligence (titre, libelle, ressource, debut, delai, taux, etat, observation, cr_id, create_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, (now() AT TIME ZONE 'UTC'))",
            )
            .bind(&titre).bind(&v.libelle).bind(&ressource).bind(debut).bind(delai).bind(taux).bind(v.etat)
            .bind(&v.observation).bind(cr_id)
            .execute(&state.db)
            .await?;
            if cr_id.is_some() { CRS } else { DILIGENCES }
        }
    };
    Ok(Redirect::to(target).into_response())
}

pub async fn dil_create(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, mp: Multipart) -> AppResult<Response> {
    dil_save(s, se, u, None, mp).await
}
pub async fn dil_create_cr(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(cr): Path<i32>, mp: Multipart) -> AppResult<Response> {
    dil_save(s, se, u, Some(Ok(cr)), mp).await
}
pub async fn dil_update(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, mp: Multipart) -> AppResult<Response> {
    dil_save(s, se, u, Some(Err(id)), mp).await
}

pub async fn dil_delete(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, h: HeaderMap) -> AppResult<Response> {
    ensure(u.can_manage_cabinet())?;
    sqlx::query("DELETE FROM diligence WHERE id = $1").bind(id).execute(&s.db).await?;
    flash(&se, "success", "Diligence supprimée").await?;
    Ok(redirect_back(&h, DILIGENCES))
}

async fn dil_set_archive(s: &AppState, u: &CurrentUser, id: i32, value: bool) -> AppResult<()> {
    ensure(u.can_manage_cabinet())?;
    sqlx::query("UPDATE diligence SET archive = $1 WHERE id = $2").bind(value).bind(id).execute(&s.db).await?;
    Ok(())
}

pub async fn dil_archive(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, h: HeaderMap) -> AppResult<Response> {
    dil_set_archive(&s, &u, id, true).await?;
    flash(&se, "success", "Dossier archivé avec succès").await?;
    Ok(redirect_back(&h, DILIGENCES))
}
pub async fn dil_unarchive(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, h: HeaderMap) -> AppResult<Response> {
    dil_set_archive(&s, &u, id, false).await?;
    flash(&se, "success", "Dossier retourné avec succès").await?;
    Ok(redirect_back(&h, "/admin/diligences/archives"))
}
