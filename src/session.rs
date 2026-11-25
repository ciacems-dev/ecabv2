//! Aides autour de la session : jeton CSRF et messages flash.

use rand::RngCore;
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use crate::error::{AppError, AppResult};

pub const USER_ID: &str = "user_id";
pub const LOGIN_ATTEMPT_ID: &str = "login_attempt_id";
pub const LAST_EMAIL: &str = "last_email";
pub const TARGET_PATH: &str = "target_path";
const CSRF: &str = "csrf_token";
const FLASHES: &str = "flashes";

/// Jeton CSRF de la session (créé au besoin). Un seul jeton par session, vérifié sur chaque POST.
pub async fn csrf_token(session: &Session) -> AppResult<String> {
    if let Some(t) = session.get::<String>(CSRF).await? {
        return Ok(t);
    }
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    session.insert(CSRF, &token).await?;
    Ok(token)
}

/// Vérifie le jeton envoyé par un formulaire (comparaison en temps constant).
pub async fn verify_csrf(session: &Session, sent: &str) -> AppResult<()> {
    let expected = session.get::<String>(CSRF).await?.unwrap_or_default();
    let ok = !expected.is_empty()
        && expected.len() == sent.len()
        && expected.bytes().zip(sent.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
    if ok { Ok(()) } else { Err(AppError::BadRequest("Jeton de sécurité invalide ou expiré. Rechargez la page.")) }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Flashes {
    pub success: Vec<String>,
    pub warning: Vec<String>,
    pub error: Vec<String>,
}

#[allow(dead_code)] // utilisé par les modules métier à venir
pub async fn flash(session: &Session, kind: &str, message: impl Into<String>) -> AppResult<()> {
    let mut f: Flashes = session.get(FLASHES).await?.unwrap_or_default();
    let message = message.into();
    match kind {
        "success" => f.success.push(message),
        "warning" => f.warning.push(message),
        _ => f.error.push(message),
    }
    session.insert(FLASHES, f).await?;
    Ok(())
}

/// Récupère et efface les messages flash (affichés une seule fois).
pub async fn take_flashes(session: &Session) -> AppResult<Flashes> {
    Ok(session.remove::<Flashes>(FLASHES).await?.unwrap_or_default())
}
