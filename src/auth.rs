//! Authentification : connexion, déconnexion, utilisateur courant et contrôle d'accès à `/admin`.
//!
//! Reprend le comportement Symfony (`security.yaml`, `UserAuthenticator`, `LoginSuccessListener`)
//! avec ces corrections de sécurité :
//! - les comptes désactivés (`active` ≠ true) ne peuvent plus se connecter (le `UserChecker` était désactivé) ;
//! - l'identifiant de session est renouvelé à la connexion ;
//! - les tentatives échouées sont limitées ;
//! - la déconnexion se fait en POST avec jeton CSRF, et enregistre enfin `date_end`.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tower_sessions::Session;

use crate::error::{AppError, AppResult};
use crate::session::{self as sess, LAST_EMAIL, LOGIN_ATTEMPT_ID, TARGET_PATH, USER_ID};
use crate::state::AppState;
use crate::templates::role_granted;

/// Hash bcrypt d'un mot de passe aléatoire, utilisé quand l'email est inconnu pour que
/// la réponse prenne le même temps (pas de déduction des comptes existants).
/// Même coût (13) que les hash Symfony existants.
static DUMMY_HASH: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    bcrypt::hash(hex::encode(rand::random::<[u8; 16]>()), 13).expect("hash bcrypt factice")
});

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct Destinataire {
    pub id: i32,
    pub libelle: String,
}

/// Utilisateur connecté, chargé à chaque requête (une requête SQL indexée).
#[derive(Clone, Debug, Serialize)]
pub struct CurrentUser {
    pub id: i32,
    pub email: String,
    pub nom: Option<String>,
    pub prenom: Option<String>,
    pub roles: Vec<String>,
    pub destinataire: Option<Destinataire>,
}

impl CurrentUser {
    pub fn is_granted(&self, role: &str) -> bool {
        role_granted(&self.roles, role)
    }
}

#[derive(FromRow)]
struct UserRow {
    id: i32,
    email: String,
    nom: Option<String>,
    prenom: Option<String>,
    roles: sqlx::types::Json<Vec<String>>,
    password: String,
    active: Option<bool>,
    destinataire_id: Option<i32>,
    destinataire_libelle: Option<String>,
}

impl From<UserRow> for CurrentUser {
    fn from(r: UserRow) -> Self {
        let destinataire = match (r.destinataire_id, r.destinataire_libelle) {
            (Some(id), Some(libelle)) => Some(Destinataire { id, libelle }),
            _ => None,
        };
        CurrentUser { id: r.id, email: r.email, nom: r.nom, prenom: r.prenom, roles: r.roles.0, destinataire }
    }
}

const USER_SELECT: &str = "SELECT u.id, u.email, u.nom, u.prenom, u.roles, u.password, u.active,
        d.id AS destinataire_id, d.libelle AS destinataire_libelle
     FROM users u LEFT JOIN destinataire d ON d.id = u.destinataire_id";

/// Middleware des routes `/admin` : exige une session valide sur un compte actif ayant ROLE_ADMIN
/// (règle `access_control: ^/admin → ROLE_ADMIN` de l'application d'origine).
pub async fn require_admin(
    State(state): State<AppState>,
    session: Session,
    mut req: Request,
    next: Next,
) -> AppResult<Response> {
    let user_id: Option<i32> = session.get(USER_ID).await?;
    let row = match user_id {
        Some(id) => sqlx::query_as::<_, UserRow>(&format!("{USER_SELECT} WHERE u.id = $1"))
            .bind(id)
            .fetch_optional(&state.db)
            .await?,
        None => None,
    };
    let Some(row) = row.filter(|r| r.active == Some(true)) else {
        // Pas connecté, ou compte supprimé / désactivé depuis : retour à la connexion.
        if user_id.is_some() {
            session.flush().await?;
        }
        if req.method() == axum::http::Method::GET {
            session.insert(TARGET_PATH, req.uri().to_string()).await?;
        }
        return Ok(Redirect::to("/login").into_response());
    };
    let user = CurrentUser::from(row);
    if !user.is_granted("ROLE_ADMIN") {
        return Err(AppError::Forbidden);
    }
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

pub async fn login_page(State(state): State<AppState>, session: Session) -> AppResult<Response> {
    if session.get::<i32>(USER_ID).await?.is_some() {
        return Ok(Redirect::to("/admin").into_response());
    }
    render_login(&state, &session, None).await
}

async fn render_login(state: &AppState, session: &Session, error: Option<&str>) -> AppResult<Response> {
    let last_email: String = session.get(LAST_EMAIL).await?.unwrap_or_default();
    let html = state.tpl.get_template("security/login.html")?.render(minijinja::context! {
        error => error,
        last_username => last_email,
        csrf_token => sess::csrf_token(session).await?,
    })?;
    Ok(Html(html).into_response())
}

#[derive(Deserialize)]
pub struct LoginForm {
    email: String,
    password: String,
    #[serde(rename = "_csrf_token")]
    csrf: String,
}

pub fn client_ip(state: &AppState, headers: &HeaderMap, peer: SocketAddr) -> String {
    if state.cfg.trust_proxy {
        if let Some(ip) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return ip.to_string();
        }
    }
    peer.ip().to_string()
}

pub async fn login_submit(
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> AppResult<Response> {
    sess::verify_csrf(&session, &form.csrf).await?;
    let email = form.email.trim().to_string();
    session.insert(LAST_EMAIL, &email).await?;

    let ip = client_ip(&state, &headers, peer);
    let limiter_key = format!("{ip}|{}", email.to_lowercase());
    if state.login_limiter.is_blocked(&limiter_key) {
        return render_login(&state, &session, Some("Trop de tentatives. Réessayez dans quelques minutes.")).await;
    }

    // L'email était insensible à la casse avec MariaDB (collation *_ci) : on garde ce comportement.
    let row = sqlx::query_as::<_, UserRow>(&format!("{USER_SELECT} WHERE lower(u.email) = lower($1)"))
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;

    let hash = row.as_ref().map(|r| r.password.clone()).unwrap_or_else(|| DUMMY_HASH.clone());
    let password = form.password;
    let valid = tokio::task::spawn_blocking(move || bcrypt::verify(password, &hash).unwrap_or(false))
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let user = match row {
        Some(r) if valid && r.active == Some(true) => r,
        _ => {
            state.login_limiter.record_failure(&limiter_key);
            return render_login(&state, &session, Some("Identifiants invalides.")).await;
        }
    };
    state.login_limiter.reset(&limiter_key);

    // Nouvelle session (évite la fixation de session), en conservant la page demandée.
    let target: Option<String> = session.get(TARGET_PATH).await?;
    session.cycle_id().await?;
    session.remove::<String>(TARGET_PATH).await?;
    session.remove::<String>(LAST_EMAIL).await?;
    session.insert(USER_ID, user.id).await?;

    // Historique de connexion (LoginSuccessListener).
    let attempt_id: i32 = sqlx::query_scalar(
        "INSERT INTO loginattemp (user_id, ip, date_start) VALUES ($1, $2, (now() AT TIME ZONE 'UTC')) RETURNING id",
    )
    .bind(user.id)
    .bind(&ip)
    .fetch_one(&state.db)
    .await?;
    session.insert(LOGIN_ATTEMPT_ID, attempt_id).await?;

    let target = target.filter(|t| t.starts_with("/admin")).unwrap_or_else(|| "/admin".into());
    Ok(Redirect::to(&target).into_response())
}

#[derive(Deserialize)]
pub struct LogoutForm {
    #[serde(rename = "_csrf_token")]
    csrf: String,
}

pub async fn logout(State(state): State<AppState>, session: Session, Form(form): Form<LogoutForm>) -> AppResult<Response> {
    sess::verify_csrf(&session, &form.csrf).await?;
    if let Some(attempt_id) = session.get::<i32>(LOGIN_ATTEMPT_ID).await? {
        sqlx::query("UPDATE loginattemp SET date_end = (now() AT TIME ZONE 'UTC') WHERE id = $1")
            .bind(attempt_id)
            .execute(&state.db)
            .await?;
    }
    session.flush().await?;
    Ok(Redirect::to("/login").into_response())
}
