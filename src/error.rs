use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

/// Erreur applicative. Les détails techniques sont journalisés, jamais affichés à l'utilisateur
/// (contrairement à l'ancienne application, qui tournait en mode `dev`).
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("page introuvable")]
    NotFound,
    #[error("accès refusé")]
    Forbidden,
    #[error("requête invalide : {0}")]
    BadRequest(&'static str),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => AppError::NotFound,
            other => AppError::Internal(other.into()),
        }
    }
}

impl From<minijinja::Error> for AppError {
    fn from(e: minijinja::Error) -> Self {
        AppError::Internal(anyhow::anyhow!("{e:#}"))
    }
}

impl From<tower_sessions::session::Error> for AppError {
    fn from(e: tower_sessions::session::Error) -> Self {
        AppError::Internal(e.into())
    }
}

fn page(status: StatusCode, titre: &str, message: &str) -> Response {
    let body = format!(
        r#"<!DOCTYPE html><html lang="fr"><head><meta charset="utf-8"><title>{titre}</title>
<link rel="stylesheet" href="/assets/admin/dist/css/adminlte.min.css"></head>
<body class="hold-transition"><div class="container mt-5"><div class="error-page">
<h2 class="headline text-warning">{code}</h2><div class="error-content"><h3>{titre}</h3>
<p>{message}</p><p><a href="/admin">Retour à l'accueil</a></p></div></div></div></body></html>"#,
        code = status.as_u16()
    );
    (status, Html(body)).into_response()
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::NotFound => page(StatusCode::NOT_FOUND, "Page introuvable", "La page demandée n'existe pas."),
            AppError::Forbidden => page(StatusCode::FORBIDDEN, "Accès refusé", "Vous n'avez pas accès à cette page."),
            AppError::BadRequest(msg) => page(StatusCode::BAD_REQUEST, "Requête invalide", msg),
            AppError::Internal(e) => {
                tracing::error!(error = ?e, "erreur interne");
                page(StatusCode::INTERNAL_SERVER_ERROR, "Erreur", "Une erreur est survenue. Réessayez plus tard.")
            }
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
