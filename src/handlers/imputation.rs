//! Imputations (ImputationController).

use axum::Extension;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tower_sessions::Session;

use super::common::{self, CourrierRef, INSTRUCTIONS, Libelle, Nom, redirect_back};
use crate::auth::{CurrentUser, ensure};
use crate::error::{AppError, AppResult};
use crate::files::{self, Categorie};
use crate::forms::{FormData, parse_range};
use crate::layout;
use crate::session::flash;
use crate::state::AppState;

// ---------------------------------------------------------------------------------------------
// Données
// ---------------------------------------------------------------------------------------------

#[derive(FromRow)]
struct Row {
    id: i32,
    date: Option<NaiveDate>,
    delais: Option<NaiveDate>,
    reponse: Option<String>,
    statut: Option<bool>,
    commentaire: Option<String>,
    etat: Option<String>,
    doctoure: Option<String>,
    dateretour: Option<NaiveDateTime>,
    observationdir: Option<String>,
    dateobs: Option<NaiveDateTime>,
    docfinal: Option<String>,
    datefinale: Option<NaiveDateTime>,
    effet: bool,
    archive: Option<bool>,
    dans_les_delais: bool,
    courrier_id: Option<i32>,
    c_numero: Option<i32>,
    c_objet: Option<String>,
    c_expeditaire: Option<String>,
    c_courrier: Option<String>,
    c_date: Option<NaiveDate>,
    c_datearrive: Option<NaiveDateTime>,
    destinataire_id: Option<i32>,
    d_libelle: Option<String>,
    responsable_id: Option<i32>,
    r_nom: Option<String>,
}

/// Forme attendue par les templates (`imputation.courrier.numero`, `imputation.responsable is none`…).
#[derive(Serialize)]
pub struct Imputation {
    pub id: i32,
    date: Option<NaiveDate>,
    delais: Option<NaiveDate>,
    reponse: Option<String>,
    /// Traité.
    statut: bool,
    commentaire: Option<String>,
    etat: Option<String>,
    doctoure: Option<String>,
    dateretour: Option<NaiveDateTime>,
    observationdir: Option<String>,
    dateobs: Option<NaiveDateTime>,
    docfinal: Option<String>,
    datefinale: Option<NaiveDateTime>,
    /// Extrêmement urgent (`effet == 1`).
    effet: bool,
    archive: bool,
    /// Délai non dépassé (un délai vide compte comme aujourd'hui, comme `null|date` en Twig).
    dans_les_delais: bool,
    courrier: Option<CourrierRef>,
    pub destinataire: Option<Libelle>,
    responsable: Option<Nom>,
}

impl From<Row> for Imputation {
    fn from(r: Row) -> Self {
        Imputation {
            id: r.id,
            date: r.date,
            delais: r.delais,
            reponse: r.reponse,
            statut: r.statut.unwrap_or(false),
            commentaire: r.commentaire,
            etat: r.etat,
            doctoure: r.doctoure,
            dateretour: r.dateretour,
            observationdir: r.observationdir,
            dateobs: r.dateobs,
            docfinal: r.docfinal,
            datefinale: r.datefinale,
            effet: r.effet,
            archive: r.archive.unwrap_or(false),
            dans_les_delais: r.dans_les_delais,
            courrier: r.courrier_id.map(|id| CourrierRef {
                id,
                numero: r.c_numero,
                objet: r.c_objet,
                expeditaire: r.c_expeditaire,
                courrier: r.c_courrier,
                date: r.c_date,
                datearrive: r.c_datearrive,
            }),
            destinataire: r.destinataire_id.zip(r.d_libelle).map(|(id, libelle)| Libelle { id, libelle }),
            responsable: r.responsable_id.map(|id| Nom { id, nom: r.r_nom }),
        }
    }
}

pub const SELECT: &str = "SELECT i.id, i.date, i.delais, i.reponse, i.statut, i.commentaire, i.etat, i.doctoure,
        i.dateretour, i.observationdir, i.dateobs, i.docfinal, i.datefinale,
        COALESCE(trim(i.effet) = '1', false) AS effet, i.archive,
        COALESCE(i.delais, CURRENT_DATE) >= CURRENT_DATE AS dans_les_delais,
        i.courrier_id, c.numero AS c_numero, c.objet AS c_objet, c.expeditaire AS c_expeditaire,
        c.courrier AS c_courrier, c.date AS c_date, c.datearrive AS c_datearrive,
        i.destinataire_id, d.libelle AS d_libelle, i.responsable_id, u.nom AS r_nom
     FROM imputation i
     LEFT JOIN carrive c ON c.id = i.courrier_id
     LEFT JOIN destinataire d ON d.id = i.destinataire_id
     LEFT JOIN users u ON u.id = i.responsable_id";

/// Liste filtrée. `where_` utilise `$1`/`$2` pour la période si elle est fournie, `$3` pour un service.
pub async fn query(
    state: &AppState,
    where_: &str,
    order: &str,
    range: Option<(NaiveDate, NaiveDate)>,
    destinataire: Option<i32>,
) -> AppResult<Vec<Imputation>> {
    let periode = if range.is_some() { "AND i.date >= $1 AND i.date <= $2" } else { "AND ($1::date IS NULL OR $2::date IS NULL)" };
    // PostgreSQL exige que chaque paramètre lié soit utilisé : `$3` est toujours référencé.
    let sql = format!("{SELECT} WHERE ({where_}) {periode} AND ($3::int IS NULL OR $3::int IS NOT NULL) ORDER BY {order}");
    let (from, to) = range.map(|(a, b)| (Some(a), Some(b))).unwrap_or((None, None));
    let rows = sqlx::query_as::<_, Row>(&sql).bind(from).bind(to).bind(destinataire).fetch_all(&state.db).await?;
    Ok(rows.into_iter().map(Imputation::from).collect())
}

async fn find(state: &AppState, id: i32) -> AppResult<Imputation> {
    let sql = format!("{SELECT} WHERE i.id = $1");
    Ok(sqlx::query_as::<_, Row>(&sql).bind(id).fetch_one(&state.db).await?.into())
}

fn dest_id(i: &Imputation) -> Option<i32> {
    i.destinataire.as_ref().map(|d| d.id)
}

/// Voir une imputation : gestionnaires, service destinataire, ou utilisateur qui l'a transmise.
fn can_see(user: &CurrentUser, i: &Imputation) -> bool {
    user.can_manage_imputations() || user.owns(dest_id(i)) || i.responsable.as_ref().is_some_and(|r| r.id == user.id)
}

// ---------------------------------------------------------------------------------------------
// Listes
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListQuery {
    dates: Option<String>,
}

const ANNEE_EN_COURS: &str =
    "EXTRACT(YEAR FROM COALESCE(i.date, CURRENT_DATE)) = EXTRACT(YEAR FROM CURRENT_DATE)";

#[allow(clippy::too_many_arguments)]
async fn render_list(
    state: AppState,
    session: Session,
    user: CurrentUser,
    q: ListQuery,
    where_: &str,
    order: &str,
    template: &str,
    menu: (&str, &str),
) -> AppResult<Response> {
    let range = parse_range(q.dates.as_deref());
    let imputations = query(&state, where_, order, range, user.destinataire_id()).await?;
    Ok(layout::render(&state, &session, &user, template, menu, minijinja::context! { imputations, dates => q.dates })
        .await?
        .into_response())
}

/// « Imputations » : imputations directes (sans `responsable`) de l'année en cours.
pub async fn index(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<ListQuery>,
) -> AppResult<Response> {
    ensure(user.can_manage_imputations())?;
    let where_ = format!("i.responsable_id IS NULL AND {ANNEE_EN_COURS}");
    render_list(state, session, user, q, &where_, "i.id DESC", "admin/imputation/index.html", ("3", "3")).await
}

/// « Courriers classés › Imputations » : imputations archivées.
pub async fn classees(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<ListQuery>,
) -> AppResult<Response> {
    ensure(user.can_manage_imputations())?;
    render_list(state, session, user, q, "i.archive = true", "i.date DESC NULLS LAST, i.id DESC", "admin/imputation/archive.html", ("6", "1")).await
}

/// « Archivages › Imputations » : imputations directes, toutes années.
pub async fn archives(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<ListQuery>,
) -> AppResult<Response> {
    ensure(user.can_manage_imputations())?;
    render_list(state, session, user, q, "i.responsable_id IS NULL", "i.id DESC", "admin/imputation/archiveimpu.html", ("3", "3")).await
}

/// Archives du service de l'utilisateur.
pub async fn archives_perso(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<ListQuery>,
) -> AppResult<Response> {
    ensure(user.destinataire_id().is_some())?;
    render_list(state, session, user, q, "i.destinataire_id = $3 AND i.archive = true", "i.id DESC", "admin/imputation/archivep.html", ("5", "4")).await
}

// ---------------------------------------------------------------------------------------------
// Page d'une imputation
// ---------------------------------------------------------------------------------------------

pub async fn show(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    let imputation = find(&state, id).await?;
    ensure(can_see(&user, &imputation))?;
    // « Parcours du courrier » : dans l'original, les imputations transmises par l'utilisateur connecté
    // (`app.user.imputation`), triées par date décroissante. Comportement conservé tel quel.
    let parcours = query(&state, "i.responsable_id = $3", "i.date DESC NULLS LAST, i.id DESC", None, Some(user.id)).await?;
    Ok(layout::render(
        &state,
        &session,
        &user,
        "admin/imputation/show.html",
        ("3", "3"),
        minijinja::context! { imputation, parcours },
    )
    .await?
    .into_response())
}

// ---------------------------------------------------------------------------------------------
// Création (trois points d'entrée d'origine)
// ---------------------------------------------------------------------------------------------

#[derive(Serialize, Default)]
struct Valeurs {
    courrier: Option<i32>,
    destinataire: Option<i32>,
    date: Option<String>,
    reponse: Option<String>,
    effet: bool,
    delais: Option<String>,
    statut: bool,
    commentaire: Option<String>,
    etat: Option<String>,
    observationdir: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Entree {
    /// Depuis la vue « Dircab » (app_imputation_dest) → retour à cette vue.
    Dircab,
    /// Depuis « Arrivées » (app_imputation_sc) → retour à « Arrivées ».
    ServiceCourrier,
    /// « Transmet à » (app_imputation_reimp) → retour au tableau de bord, responsable = utilisateur.
    Transmission,
}

#[derive(FromRow, Serialize)]
struct CourrierEntete {
    id: i32,
    numero: Option<i32>,
    datearrive: Option<NaiveDateTime>,
    objet: Option<String>,
    expeditaire: Option<String>,
    confi: Option<bool>,
}

async fn courrier_entete(state: &AppState, id: i32) -> AppResult<CourrierEntete> {
    Ok(sqlx::query_as("SELECT id, numero, datearrive, objet, expeditaire, confi FROM carrive WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?)
}

async fn may_create(state: &AppState, user: &CurrentUser, entree: Entree, c: &CourrierEntete) -> AppResult<()> {
    if c.confi == Some(true) {
        ensure(user.is_super_admin())?;
    }
    let ok = match entree {
        Entree::Dircab => user.is_super_admin(),
        Entree::ServiceCourrier => user.can_manage_courrier(),
        Entree::Transmission => {
            user.can_manage_imputations()
                || sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS (SELECT 1 FROM imputation WHERE courrier_id = $1 AND destinataire_id = $2)",
                )
                .bind(c.id)
                .bind(user.destinataire_id())
                .fetch_one(&state.db)
                .await?
        }
    };
    ensure(ok)
}

#[allow(clippy::too_many_arguments)]
async fn render_form(
    state: &AppState,
    session: &Session,
    user: &CurrentUser,
    template: &str,
    titre: &str,
    courrier: &CourrierEntete,
    v: Valeurs,
    errors: Vec<String>,
    extra: minijinja::Value,
) -> AppResult<Response> {
    let destinataires = common::destinataires(state).await?;
    let instructions: Vec<_> = INSTRUCTIONS.iter().map(|(label, value)| minijinja::context! { label, value }).collect();
    Ok(layout::render(
        state,
        session,
        user,
        template,
        ("3", "3"),
        minijinja::context! { titre, courrier, v, errors, destinataires, instructions, ..extra },
    )
    .await?
    .into_response())
}

const TITRE_NOUVELLE: &str = "Imputer un courrier au destinataire";
const TITRE_EDITION: &str = "Editer l'imptation";

async fn new_form(state: AppState, session: Session, user: CurrentUser, courrier_id: i32, e: Entree) -> AppResult<Response> {
    let c = courrier_entete(&state, courrier_id).await?;
    may_create(&state, &user, e, &c).await?;
    let v = Valeurs { destinataire: None, ..Default::default() };
    render_form(&state, &session, &user, "admin/imputation/new.html", TITRE_NOUVELLE, &c, v, vec![], minijinja::context! {}).await
}

pub async fn new_dircab(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    new_form(s, se, u, id, Entree::Dircab).await
}
pub async fn new_sc(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    new_form(s, se, u, id, Entree::ServiceCourrier).await
}
pub async fn new_transmission(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    new_form(s, se, u, id, Entree::Transmission).await
}

/// Champs communs à ImputationType / ImputationEditType.
fn lire(form: &FormData, errors: &mut Vec<String>) -> (Valeurs, Option<NaiveDate>, Option<NaiveDate>) {
    let date = form.date("date", "Date d'imputation", errors);
    let delais = form.date("delais", "Delais d'exécution", errors);
    if form.text("date").is_none() {
        errors.push("Le champ « Date d'imputation » est obligatoire.".into());
    }
    if form.text("delais").is_none() {
        errors.push("Le champ « Delais d'exécution » est obligatoire.".into());
    }
    let destinataire = form.int("destinataire", "Destinataire", errors);
    if destinataire.is_none() {
        errors.push("Le champ « Destinataire » est obligatoire.".into());
    }
    let reponse = form.text("reponse");
    if reponse.as_deref().is_none_or(|r| !INSTRUCTIONS.iter().any(|(_, v)| *v == r)) {
        errors.push("Instruction invalide.".into());
    }
    let v = Valeurs {
        courrier: form.int("courrier", "Courrier", errors),
        destinataire,
        date: form.text("date"),
        reponse,
        effet: form.flag("effet"),
        delais: form.text("delais"),
        statut: form.flag("statut"),
        commentaire: form.text("commentaire"),
        etat: form.text("etat"),
        observationdir: form.text("observationdir"),
    };
    (v, date, delais)
}

async fn create(state: AppState, session: Session, user: CurrentUser, courrier_id: i32, e: Entree, mp: Multipart) -> AppResult<Response> {
    let c = courrier_entete(&state, courrier_id).await?;
    may_create(&state, &user, e, &c).await?;
    let form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let (v, date, delais) = lire(&form, &mut errors);
    if !errors.is_empty() {
        return render_form(&state, &session, &user, "admin/imputation/new.html", TITRE_NOUVELLE, &c, v, errors, minijinja::context! {}).await;
    }
    let responsable = (e == Entree::Transmission).then_some(user.id);
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO imputation (courrier_id, destinataire_id, date, reponse, effet, delais, commentaire, transmet, responsable_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, false, $8)",
    )
    .bind(c.id)
    .bind(v.destinataire)
    .bind(date)
    .bind(&v.reponse)
    .bind(if v.effet { "1" } else { "0" })
    .bind(delais)
    .bind(&v.commentaire)
    .bind(responsable)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE carrive SET action = true WHERE id = $1").bind(c.id).execute(&mut *tx).await?;
    tx.commit().await?;
    let target = match e {
        Entree::Dircab => "/admin/courriers/arrives/dircab",
        Entree::ServiceCourrier => "/admin/courriers/arrives",
        Entree::Transmission => "/admin",
    };
    Ok(Redirect::to(target).into_response())
}

pub async fn create_dircab(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, mp: Multipart) -> AppResult<Response> {
    create(s, se, u, id, Entree::Dircab, mp).await
}
pub async fn create_sc(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, mp: Multipart) -> AppResult<Response> {
    create(s, se, u, id, Entree::ServiceCourrier, mp).await
}
pub async fn create_transmission(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, mp: Multipart) -> AppResult<Response> {
    create(s, se, u, id, Entree::Transmission, mp).await
}

// ---------------------------------------------------------------------------------------------
// Modification (gestionnaires), retour (service), observation et document final (super admin)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Etape {
    Edition,
    Retour,
    Observation,
    DocumentFinal,
}

impl Etape {
    fn template(self) -> &'static str {
        match self {
            Etape::Edition => "admin/imputation/edit.html",
            Etape::Retour => "admin/imputation/editretour.html",
            Etape::Observation => "admin/imputation/editobser.html",
            Etape::DocumentFinal => "admin/imputation/editfinal.html",
        }
    }
    fn allowed(self, user: &CurrentUser, i: &Imputation) -> bool {
        match self {
            Etape::Edition => user.can_manage_imputations(),
            Etape::Retour => user.can_manage_imputations() || user.owns(dest_id(i)),
            Etape::Observation | Etape::DocumentFinal => user.is_super_admin(),
        }
    }
}

fn valeurs_de(i: &Imputation) -> Valeurs {
    Valeurs {
        courrier: i.courrier.as_ref().map(|c| c.id),
        destinataire: dest_id(i),
        date: i.date.map(|d| d.to_string()),
        reponse: i.reponse.clone(),
        effet: i.effet,
        delais: i.delais.map(|d| d.to_string()),
        statut: i.statut,
        commentaire: i.commentaire.clone(),
        etat: i.etat.clone(),
        observationdir: i.observationdir.clone(),
    }
}

#[derive(Serialize, FromRow)]
struct CourrierChoix {
    id: i32,
    objet: Option<String>,
}

async fn render_etape(
    state: &AppState,
    session: &Session,
    user: &CurrentUser,
    etape: Etape,
    i: &Imputation,
    v: Valeurs,
    errors: Vec<String>,
) -> AppResult<Response> {
    let c = courrier_entete(state, i.courrier.as_ref().map(|c| c.id).ok_or(AppError::NotFound)?).await?;
    // ImputationEditType propose tous les courriers (choice_label : objet).
    let courriers: Vec<CourrierChoix> = if etape == Etape::Edition {
        sqlx::query_as("SELECT id, objet FROM carrive ORDER BY id").fetch_all(&state.db).await?
    } else {
        vec![]
    };
    render_form(state, session, user, etape.template(), TITRE_EDITION, &c, v, errors, minijinja::context! { imputation => i, courriers }).await
}

async fn etape_form(state: AppState, session: Session, user: CurrentUser, id: i32, etape: Etape) -> AppResult<Response> {
    let i = find(&state, id).await?;
    ensure(etape.allowed(&user, &i))?;
    let v = valeurs_de(&i);
    render_etape(&state, &session, &user, etape, &i, v, vec![]).await
}

pub async fn edit(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    etape_form(s, se, u, id, Etape::Edition).await
}
pub async fn retour(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    etape_form(s, se, u, id, Etape::Retour).await
}
pub async fn observation(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    etape_form(s, se, u, id, Etape::Observation).await
}
pub async fn document_final(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>) -> AppResult<Response> {
    etape_form(s, se, u, id, Etape::DocumentFinal).await
}

async fn etape_submit(state: AppState, session: Session, user: CurrentUser, id: i32, etape: Etape, mp: Multipart) -> AppResult<Response> {
    let i = find(&state, id).await?;
    ensure(etape.allowed(&user, &i))?;
    let mut form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let show = format!("/admin/imputations/{id}");

    match etape {
        Etape::Edition => {
            let (v, date, delais) = lire(&form, &mut errors);
            if v.courrier.is_none() {
                errors.push("Le champ « Courrier » est obligatoire.".into());
            }
            if !errors.is_empty() {
                return render_etape(&state, &session, &user, etape, &i, v, errors).await;
            }
            sqlx::query(
                "UPDATE imputation SET courrier_id = $1, destinataire_id = $2, date = $3, reponse = $4, effet = $5,
                        delais = $6, statut = $7, commentaire = $8, etat = $9, observationdir = $10
                 WHERE id = $11",
            )
            .bind(v.courrier)
            .bind(v.destinataire)
            .bind(date)
            .bind(&v.reponse)
            .bind(if v.effet { "1" } else { "0" })
            .bind(delais)
            .bind(v.statut)
            .bind(&v.commentaire)
            .bind(&v.etat)
            .bind(&v.observationdir)
            .bind(id)
            .execute(&state.db)
            .await?;
            Ok(Redirect::to("/admin/imputations").into_response())
        }
        Etape::Retour => {
            // ImputationretoureType : état + fichier ; la date de retour est toujours mise à jour.
            let mut fichier = i.doctoure.clone();
            if let Some(up) = form.file("doctoure") {
                match files::save(&state, Categorie::Imputation, &up, files::DOCUMENTS).await {
                    Ok(name) => fichier = Some(name),
                    Err(e) => errors.push(e),
                }
            }
            if !errors.is_empty() {
                let mut v = valeurs_de(&i);
                v.etat = form.text("etat");
                return render_etape(&state, &session, &user, etape, &i, v, errors).await;
            }
            sqlx::query("UPDATE imputation SET etat = $1, doctoure = $2, dateretour = (now() AT TIME ZONE 'UTC') WHERE id = $3")
                .bind(form.text("etat"))
                .bind(fichier)
                .bind(id)
                .execute(&state.db)
                .await?;
            Ok(Redirect::to(&show).into_response())
        }
        Etape::Observation => {
            let reponse = form.text("reponse");
            if reponse.as_deref().is_none_or(|r| !INSTRUCTIONS.iter().any(|(_, v)| *v == r)) {
                let mut v = valeurs_de(&i);
                v.commentaire = form.text("commentaire");
                v.etat = form.text("etat");
                v.observationdir = form.text("observationdir");
                return render_etape(&state, &session, &user, etape, &i, v, vec!["Instruction invalide.".into()]).await;
            }
            sqlx::query(
                "UPDATE imputation SET reponse = $1, commentaire = $2, etat = $3, observationdir = $4,
                        dateobs = (now() AT TIME ZONE 'UTC') WHERE id = $5",
            )
            .bind(reponse)
            .bind(form.text("commentaire"))
            .bind(form.text("etat"))
            .bind(form.text("observationdir"))
            .bind(id)
            .execute(&state.db)
            .await?;
            Ok(Redirect::to(&show).into_response())
        }
        Etape::DocumentFinal => {
            // ImputationfinalType : la date finale n'est posée que si un fichier est envoyé.
            let mut fichier = None;
            if let Some(up) = form.file("docfinal") {
                match files::save(&state, Categorie::Imputation, &up, files::DOCUMENTS).await {
                    Ok(name) => fichier = Some(name),
                    Err(e) => errors.push(e),
                }
            }
            if !errors.is_empty() {
                let mut v = valeurs_de(&i);
                v.etat = form.text("etat");
                v.observationdir = form.text("observationdir");
                return render_etape(&state, &session, &user, etape, &i, v, errors).await;
            }
            sqlx::query(
                "UPDATE imputation SET etat = $1, observationdir = $2,
                        docfinal = COALESCE($3, docfinal),
                        datefinale = CASE WHEN $3::text IS NULL THEN datefinale ELSE (now() AT TIME ZONE 'UTC') END
                 WHERE id = $4",
            )
            .bind(form.text("etat"))
            .bind(form.text("observationdir"))
            .bind(fichier)
            .bind(id)
            .execute(&state.db)
            .await?;
            Ok(Redirect::to(&show).into_response())
        }
    }
}

pub async fn edit_submit(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, mp: Multipart) -> AppResult<Response> {
    etape_submit(s, se, u, id, Etape::Edition, mp).await
}
pub async fn retour_submit(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, mp: Multipart) -> AppResult<Response> {
    etape_submit(s, se, u, id, Etape::Retour, mp).await
}
pub async fn observation_submit(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, mp: Multipart) -> AppResult<Response> {
    etape_submit(s, se, u, id, Etape::Observation, mp).await
}
pub async fn document_final_submit(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, mp: Multipart) -> AppResult<Response> {
    etape_submit(s, se, u, id, Etape::DocumentFinal, mp).await
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
    ensure(user.can_manage_imputations())?;
    sqlx::query("DELETE FROM imputation WHERE id = $1").bind(id).execute(&state.db).await?;
    flash(&session, "success", "Imputation supprimée avec succès").await?;
    Ok(redirect_back(&headers, "/admin/imputations"))
}

async fn set_field(
    state: &AppState,
    user: &CurrentUser,
    id: i32,
    field: &str,
    value: bool,
    service_allowed: bool,
) -> AppResult<()> {
    let i = find(state, id).await?;
    ensure(user.can_manage_imputations() || (service_allowed && user.owns(dest_id(&i))))?;
    // `field` ne vient jamais de l'utilisateur.
    sqlx::query(&format!("UPDATE imputation SET {field} = $1 WHERE id = $2"))
        .bind(value)
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(())
}

pub async fn traite(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, h: HeaderMap) -> AppResult<Response> {
    set_field(&s, &u, id, "statut", true, true).await?;
    flash(&se, "success", "Dossier traité avec succès").await?;
    Ok(redirect_back(&h, "/admin/imputations"))
}
pub async fn non_traite(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, h: HeaderMap) -> AppResult<Response> {
    set_field(&s, &u, id, "statut", false, true).await?;
    flash(&se, "warning", "Dossier non traité").await?;
    Ok(redirect_back(&h, "/admin/imputations"))
}
pub async fn archive(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, h: HeaderMap) -> AppResult<Response> {
    set_field(&s, &u, id, "archive", true, true).await?;
    flash(&se, "success", "Dossier archivé avec succès").await?;
    Ok(redirect_back(&h, "/admin/imputations"))
}
pub async fn unarchive(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>, Path(id): Path<i32>, h: HeaderMap) -> AppResult<Response> {
    set_field(&s, &u, id, "archive", false, true).await?;
    flash(&se, "success", "Dossier retourné avec succès").await?;
    Ok(redirect_back(&h, "/admin/imputations/classees"))
}
