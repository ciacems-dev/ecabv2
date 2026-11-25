//! Comptes (AdminUserController) : listes, fiche, création, modification, mot de passe,
//! activation et historique des connexions.

use axum::Extension;
use axum::extract::{Multipart, Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use serde::Serialize;
use sqlx::FromRow;
use tower_sessions::Session;

use super::common::{self, Choix, Libelle, redirect_back};
use super::imputation;
use crate::auth::{CurrentUser, ensure};
use crate::error::{AppError, AppResult};
use crate::forms::FormData;
use crate::layout;
use crate::session::flash;
use crate::state::AppState;

const ADMINS: &str = "/admin/utilisateurs";
pub const MIN_PASSWORD: usize = 8;

#[derive(FromRow)]
struct Row {
    id: i32,
    email: String,
    roles: sqlx::types::Json<Vec<String>>,
    nom: Option<String>,
    prenom: Option<String>,
    civilite: Option<String>,
    telephone: Option<String>,
    active: Option<bool>,
    pays_id: Option<i32>,
    nompays: Option<String>,
    iso_code2: Option<String>,
    destinataire_id: Option<i32>,
    d_libelle: Option<String>,
}

#[derive(Serialize)]
struct Pays {
    id: i32,
    nompays: Option<String>,
    isocode2: Option<String>,
}

/// Forme attendue par les templates (`user.pays.nompays`, `user.destinataire.libelle`).
#[derive(Serialize)]
struct Utilisateur {
    id: i32,
    email: String,
    roles: Vec<String>,
    nom: Option<String>,
    prenom: Option<String>,
    civilite: Option<String>,
    telephone: Option<String>,
    active: bool,
    pays: Option<Pays>,
    destinataire: Option<Libelle>,
}

impl From<Row> for Utilisateur {
    fn from(r: Row) -> Self {
        Utilisateur {
            id: r.id,
            email: r.email,
            roles: r.roles.0,
            nom: r.nom,
            prenom: r.prenom,
            civilite: r.civilite,
            telephone: r.telephone,
            active: r.active.unwrap_or(false),
            pays: r.pays_id.map(|id| Pays { id, nompays: r.nompays, isocode2: r.iso_code2 }),
            destinataire: r.destinataire_id.zip(r.d_libelle).map(|(id, libelle)| Libelle { id, libelle }),
        }
    }
}

const SELECT: &str = "SELECT u.id, u.email, u.roles, u.nom, u.prenom, u.civilite, u.telephone, u.active,
        u.pays_id, p.nompays, p.iso_code2, u.destinataire_id, d.libelle AS d_libelle
     FROM users u
     LEFT JOIN pays p ON p.id = u.pays_id
     LEFT JOIN destinataire d ON d.id = u.destinataire_id";

async fn find(state: &AppState, id: i32) -> AppResult<Utilisateur> {
    Ok(sqlx::query_as::<_, Row>(&format!("{SELECT} WHERE u.id = $1")).bind(id).fetch_one(&state.db).await?.into())
}

// ---------------------------------------------------------------------------------------------
// Listes
// ---------------------------------------------------------------------------------------------

async fn render_list(state: AppState, session: Session, user: CurrentUser, where_: &str, titre: &str) -> AppResult<Response> {
    ensure(user.is_super_admin())?;
    let users: Vec<Utilisateur> = sqlx::query_as::<_, Row>(&format!("{SELECT} WHERE {where_} ORDER BY u.id"))
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(Utilisateur::from)
        .collect();
    Ok(layout::render(
        &state,
        &session,
        &user,
        "admin/user/index.html",
        ("2", "0"),
        minijinja::context! { users, titre_liste => titre },
    )
    .await?
    .into_response())
}

/// UserRepository::findAdmins : ROLE_ADMIN, ROLE_TECH ou ROLE_SUPER_ADMIN.
pub async fn admins(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    render_list(s, se, u, "u.roles ?| array['ROLE_ADMIN', 'ROLE_TECH', 'ROLE_SUPER_ADMIN']", "Liste des Administrateurs système").await
}

/// UserRepository::findMembers : ROLE_USER.
pub async fn membres(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    render_list(s, se, u, "u.roles ? 'ROLE_USER'", "Liste des Membres").await
}

// ---------------------------------------------------------------------------------------------
// Fiche
// ---------------------------------------------------------------------------------------------

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
}

#[derive(Serialize, FromRow, Clone)]
struct Diligence {
    id: i32,
    cr_id: Option<i32>,
    titre: Option<String>,
    libelle: Option<String>,
    ressource: Option<String>,
    debut: Option<NaiveDate>,
    delai: Option<NaiveDate>,
    /// Exposé via `DiligenceVue::etat` (booléen) : pas de doublon dans le JSON.
    #[serde(skip_serializing)]
    etat: Option<bool>,
    observation: Option<String>,
    #[serde(skip_serializing)]
    cr_date: Option<NaiveDate>,
}

#[derive(Serialize, Clone)]
struct CrRef {
    date: NaiveDate,
}

#[derive(Serialize, Clone)]
struct DiligenceVue {
    #[serde(flatten)]
    d: Diligence,
    etat: bool,
    cr: Option<CrRef>,
}

#[derive(Serialize)]
struct CompteRendu {
    id: i32,
    date: NaiveDate,
    debut: Option<NaiveTime>,
    fin: Option<NaiveTime>,
    lieu: Option<String>,
    presidence: Option<String>,
    redacteur: Option<String>,
    cr: Option<String>,
    listep: Option<String>,
    diligence: Vec<DiligenceVue>,
}

pub async fn show(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    ensure(user.is_super_admin())?;
    let fiche = find(&state, id).await?;
    let imputations = match &fiche.destinataire {
        Some(d) => imputation::query(&state, "i.destinataire_id = $3", "i.id", None, Some(d.id)).await?,
        None => vec![],
    };
    let diligences: Vec<DiligenceVue> = sqlx::query_as::<_, Diligence>(
        "SELECT g.id, g.cr_id, g.titre, g.libelle, g.ressource, g.debut, g.delai, g.etat, g.observation,
                c.date AS cr_date
         FROM diligence g LEFT JOIN compterendu c ON c.id = g.cr_id ORDER BY g.id",
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|d| DiligenceVue { etat: d.etat.unwrap_or(false), cr: d.cr_date.map(|date| CrRef { date }), d })
    .collect();
    let crs: Vec<CrRow> = sqlx::query_as(
        "SELECT id, date, debut, fin, lieu, presidence, redacteur, cr, listep FROM compterendu ORDER BY id",
    )
    .fetch_all(&state.db)
    .await?;
    let compterendus: Vec<CompteRendu> = crs
        .into_iter()
        .map(|c| CompteRendu {
            diligence: diligences.iter().filter(|d| d.d.cr_id == Some(c.id)).cloned()
                .collect(),
            id: c.id,
            date: c.date,
            debut: c.debut,
            fin: c.fin,
            lieu: c.lieu,
            presidence: c.presidence,
            redacteur: c.redacteur,
            cr: c.cr,
            listep: c.listep,
        })
        .collect();
    Ok(layout::render(
        &state,
        &session,
        &user,
        "admin/user/show.html",
        ("5", "0"),
        minijinja::context! { user => fiche, imputations, compterendus, diligences },
    )
    .await?
    .into_response())
}

// ---------------------------------------------------------------------------------------------
// Création et modification
// ---------------------------------------------------------------------------------------------

#[derive(Serialize, Default)]
struct Valeurs {
    destinataire: Option<i32>,
    nom: Option<String>,
    prenom: Option<String>,
    civilite: Option<String>,
    pays: Option<i32>,
    telephone: Option<String>,
    email: Option<String>,
    role_user: bool,
    role_admin: bool,
}

const CIVILITES_NEW: &[(&str, &str)] = &[("M.", "Monsieur"), ("Mme", "Madame"), ("Mlle", "Mlle")];
/// UserEditType : la valeur « Mlle » y est « Mademoiselle ».
const CIVILITES_EDIT: &[(&str, &str)] = &[("M.", "Monsieur"), ("Mme", "Madame"), ("Mademoiselle", "Mlle")];

async fn pays(state: &AppState) -> sqlx::Result<Vec<Choix>> {
    sqlx::query_as("SELECT id, nompays AS libelle FROM pays ORDER BY id").fetch_all(&state.db).await
}

async fn render_form(
    state: &AppState,
    session: &Session,
    user: &CurrentUser,
    edition: bool,
    v: Valeurs,
    errors: Vec<String>,
) -> AppResult<Response> {
    let destinataires = common::destinataires(state).await?;
    let pays = pays(state).await?;
    let civilites: Vec<_> = (if edition { CIVILITES_EDIT } else { CIVILITES_NEW })
        .iter()
        .map(|(value, label)| minijinja::context! { value, label })
        .collect();
    let titre = if edition { "Editer " } else { "Ajouter un utilisteur" };
    Ok(layout::render(
        state,
        session,
        user,
        "admin/user/form.html",
        ("2", "0"),
        minijinja::context! { titre, edition, v, errors, destinataires, pays, civilites, min_password => MIN_PASSWORD },
    )
    .await?
    .into_response())
}

pub async fn new(State(s): State<AppState>, se: Session, Extension(u): Extension<CurrentUser>) -> AppResult<Response> {
    ensure(u.is_super_admin())?;
    render_form(&s, &se, &u, false, Valeurs::default(), vec![]).await
}

pub async fn edit(
    State(s): State<AppState>,
    se: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    ensure(u.is_super_admin())?;
    let f = find(&s, id).await?;
    let v = Valeurs {
        destinataire: f.destinataire.as_ref().map(|d| d.id),
        nom: f.nom,
        prenom: f.prenom,
        civilite: f.civilite,
        pays: f.pays.as_ref().map(|p| p.id),
        telephone: f.telephone,
        email: Some(f.email),
        role_user: f.roles.iter().any(|r| r == "ROLE_USER"),
        role_admin: f.roles.iter().any(|r| r == "ROLE_ADMIN"),
    };
    render_form(&s, &se, &u, true, v, vec![]).await
}

fn lire(form: &FormData, errors: &mut Vec<String>) -> Valeurs {
    let destinataire = form.int("destinataire", "Unité Administrative", errors);
    if destinataire.is_none() {
        errors.push("Le champ « Unité Administrative » est obligatoire.".into());
    }
    let pays = form.int("pays", "Pays", errors);
    if pays.is_none() {
        errors.push("Le champ « Pays » est obligatoire.".into());
    }
    let email = form.required("email", "Email", errors).map(|e| e.to_lowercase());
    if email.as_deref().is_some_and(|e| !e.contains('@') || e.len() > 180) {
        errors.push("Adresse email invalide.".into());
    }
    let roles: Vec<&str> = form.get("roles").split(',').filter(|r| !r.is_empty()).collect();
    Valeurs {
        destinataire,
        nom: form.required("nom", "Nom", errors),
        prenom: form.required("prenom", "Prénoms", errors),
        civilite: form.text("civilite"),
        pays,
        telephone: form.required("telephone", "Téléphone", errors),
        email,
        role_user: form.flag("role_user") || roles.contains(&"ROLE_USER"),
        role_admin: form.flag("role_admin") || roles.contains(&"ROLE_ADMIN"),
    }
}

async fn email_pris(state: &AppState, email: &str, sauf: Option<i32>) -> AppResult<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE lower(email) = lower($1) AND id IS DISTINCT FROM $2)")
        .bind(email)
        .bind(sauf)
        .fetch_one(&state.db)
        .await?)
}

/// Mot de passe + confirmation (RepeatedType). Renvoie le hash bcrypt.
async fn nouveau_mot_de_passe(form: &FormData, errors: &mut Vec<String>) -> AppResult<Option<String>> {
    let p1 = form.get("password");
    let p2 = form.get("password_confirm");
    if p1.is_empty() {
        errors.push("Veuillez saisir un mot de passe.".into());
        return Ok(None);
    }
    if p1 != p2 {
        errors.push("Veuillez entrer les mêmes mot de passe".into());
        return Ok(None);
    }
    if p1.chars().count() < MIN_PASSWORD {
        errors.push(format!("Le mot de passe doit contenir au moins {MIN_PASSWORD} caractères."));
        return Ok(None);
    }
    let p = p1.to_string();
    let hash = tokio::task::spawn_blocking(move || bcrypt::hash(p, 13))
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(Some(hash))
}

pub async fn create(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    mp: Multipart,
) -> AppResult<Response> {
    ensure(user.is_super_admin())?;
    let form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let v = lire(&form, &mut errors);
    if let Some(e) = &v.email
        && email_pris(&state, e, None).await?
    {
        errors.push("Il existe déjà un compte avec ce email".into());
    }
    let hash = nouveau_mot_de_passe(&form, &mut errors).await?;
    if !errors.is_empty() {
        return render_form(&state, &session, &user, false, v, errors).await;
    }
    // Comme l'original : ROLE_ADMIN et compte vérifié. Créé actif (voir docs/regles-comptes-referentiels.md).
    sqlx::query(
        "INSERT INTO users (destinataire_id, nom, prenom, civilite, pays_id, telephone, email, password, roles,
                            is_verified, active)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, '[\"ROLE_ADMIN\"]', true, true)",
    )
    .bind(v.destinataire)
    .bind(&v.nom)
    .bind(&v.prenom)
    .bind(&v.civilite)
    .bind(v.pays)
    .bind(&v.telephone)
    .bind(&v.email)
    .bind(hash)
    .execute(&state.db)
    .await?;
    Ok(Redirect::to(ADMINS).into_response())
}

pub async fn update(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
    mp: Multipart,
) -> AppResult<Response> {
    ensure(user.is_super_admin())?;
    let existant = find(&state, id).await?;
    let form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    let v = lire(&form, &mut errors);
    if let Some(e) = &v.email
        && email_pris(&state, e, Some(id)).await?
    {
        errors.push("Il existe déjà un compte avec ce email".into());
    }
    if !errors.is_empty() {
        return render_form(&state, &session, &user, true, v, errors).await;
    }
    // Rôles « Editeur » / « Administrateur » du formulaire ; un rôle hors formulaire (super admin, tech)
    // est conservé (l'original le supprimait sans le dire).
    let mut roles: Vec<String> = existant
        .roles
        .into_iter()
        .filter(|r| r != "ROLE_USER" && r != "ROLE_ADMIN")
        .collect();
    if v.role_user {
        roles.push("ROLE_USER".into());
    }
    if v.role_admin {
        roles.push("ROLE_ADMIN".into());
    }
    sqlx::query(
        "UPDATE users SET destinataire_id = $1, nom = $2, prenom = $3, civilite = $4, pays_id = $5, telephone = $6,
                email = $7, roles = $8 WHERE id = $9",
    )
    .bind(v.destinataire)
    .bind(&v.nom)
    .bind(&v.prenom)
    .bind(&v.civilite)
    .bind(v.pays)
    .bind(&v.telephone)
    .bind(&v.email)
    .bind(sqlx::types::Json(roles))
    .bind(id)
    .execute(&state.db)
    .await?;
    Ok(Redirect::to(ADMINS).into_response())
}

// ---------------------------------------------------------------------------------------------
// Mot de passe
// ---------------------------------------------------------------------------------------------

/// Son propre mot de passe, ou n'importe lequel pour un super admin
/// (avant : n'importe quel admin pouvait changer celui de n'importe qui).
fn may_change_password(user: &CurrentUser, id: i32) -> bool {
    user.id == id || user.is_super_admin()
}

async fn render_password(
    state: &AppState,
    session: &Session,
    user: &CurrentUser,
    cible: &Utilisateur,
    errors: Vec<String>,
) -> AppResult<Response> {
    let soi = cible.id == user.id;
    Ok(layout::render(
        state,
        session,
        user,
        "admin/user/psedit.html",
        ("1", "0"),
        minijinja::context! { cible, soi, errors, min_password => MIN_PASSWORD, titre => if soi { "Changer votre mot de passe " } else { "Changer le mot de passe" } },
    )
    .await?
    .into_response())
}

pub async fn password_form(
    State(s): State<AppState>,
    se: Session,
    Extension(u): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    ensure(may_change_password(&u, id))?;
    let cible = find(&s, id).await?;
    render_password(&s, &se, &u, &cible, vec![]).await
}

pub async fn password_submit(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
    mp: Multipart,
) -> AppResult<Response> {
    ensure(may_change_password(&user, id))?;
    let cible = find(&state, id).await?;
    let form = FormData::read(&session, mp).await?;
    let mut errors = Vec::new();
    // Pour son propre compte, le mot de passe actuel est demandé (session laissée ouverte, vol de session).
    if cible.id == user.id {
        let actuel = form.get("current_password").to_string();
        let hash: String = sqlx::query_scalar("SELECT password FROM users WHERE id = $1").bind(id).fetch_one(&state.db).await?;
        let ok = tokio::task::spawn_blocking(move || bcrypt::verify(actuel, &hash).unwrap_or(false))
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        if !ok {
            errors.push("Le mot de passe actuel est incorrect.".into());
        }
    }
    let hash = if errors.is_empty() { nouveau_mot_de_passe(&form, &mut errors).await? } else { None };
    let Some(hash) = hash.filter(|_| errors.is_empty()) else {
        return render_password(&state, &session, &user, &cible, errors).await;
    };
    sqlx::query("UPDATE users SET password = $1 WHERE id = $2").bind(hash).bind(id).execute(&state.db).await?;
    flash(&session, "success", "Mot de passe modifié").await?;
    // L'original renvoyait toujours vers la liste des administrateurs (inaccessible à un simple admin).
    let target = if user.is_super_admin() && cible.id != user.id { ADMINS } else { "/admin" };
    Ok(Redirect::to(target).into_response())
}

// ---------------------------------------------------------------------------------------------
// Activation, historique
// ---------------------------------------------------------------------------------------------

pub async fn activer(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
    h: HeaderMap,
) -> AppResult<Response> {
    ensure(user.is_super_admin())?;
    sqlx::query("UPDATE users SET active = true WHERE id = $1").bind(id).execute(&state.db).await?;
    flash(&session, "success", "Utilisateur activé").await?;
    Ok(redirect_back(&h, ADMINS))
}

pub async fn desactiver(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
    h: HeaderMap,
) -> AppResult<Response> {
    ensure(user.is_super_admin())?;
    if id == user.id {
        flash(&session, "warning", "Vous ne pouvez pas désactiver votre propre compte.").await?;
    } else {
        sqlx::query("UPDATE users SET active = false WHERE id = $1").bind(id).execute(&state.db).await?;
        flash(&session, "warning", "Utilisateur désactivé").await?;
    }
    Ok(redirect_back(&h, ADMINS))
}

#[derive(FromRow)]
struct HistoRow {
    id: i32,
    date_start: Option<NaiveDateTime>,
    date_end: Option<NaiveDateTime>,
    civilite: Option<String>,
    nom: Option<String>,
    prenom: Option<String>,
    d_libelle: Option<String>,
}

#[derive(Serialize)]
struct Historique {
    id: i32,
    debut: String,
    fin: String,
    user: minijinja::Value,
}

pub async fn historique(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
) -> AppResult<Response> {
    ensure(user.is_super_admin())?;
    let fmt = |d: Option<NaiveDateTime>| d.map(|d| d.format("%Y/%m/%d à %H:%M:%S").to_string()).unwrap_or_default();
    let historiques: Vec<Historique> = sqlx::query_as::<_, HistoRow>(
        "SELECT l.id, l.date_start, l.date_end, u.civilite, u.nom, u.prenom, d.libelle AS d_libelle
         FROM loginattemp l
         LEFT JOIN users u ON u.id = l.user_id
         LEFT JOIN destinataire d ON d.id = u.destinataire_id
         ORDER BY l.id DESC",
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|r| Historique {
        id: r.id,
        debut: fmt(r.date_start),
        fin: fmt(r.date_end),
        user: minijinja::context! {
            civilite => r.civilite, nom => r.nom, prenom => r.prenom,
            destinataire => minijinja::context! { libelle => r.d_libelle },
        },
    })
    .collect();
    Ok(layout::render(&state, &session, &user, "admin/user/histoirique.html", ("2", "0"), minijinja::context! { historiques })
        .await?
        .into_response())
}
