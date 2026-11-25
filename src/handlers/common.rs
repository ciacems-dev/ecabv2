//! Aides partagées par les modules métier.

use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{NaiveDate, NaiveDateTime};
use serde::Serialize;
use sqlx::FromRow;

use crate::state::AppState;

/// Retour à la page précédente (comportement `redirect($request->headers->get('referer'))`),
/// limité aux pages de l'application.
pub fn redirect_back(headers: &HeaderMap, fallback: &str) -> Response {
    let target = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .and_then(|r| {
            // Ne garder que le chemin : jamais de redirection vers un autre site.
            let path = r.split_once("://").map(|(_, rest)| rest.find('/').map(|i| &rest[i..]).unwrap_or("/")).unwrap_or(r);
            path.starts_with("/admin").then(|| path.to_string())
        })
        .unwrap_or_else(|| fallback.to_string());
    Redirect::to(&target).into_response()
}

#[derive(Serialize, FromRow, Clone)]
pub struct Choix {
    pub id: i32,
    pub libelle: String,
}

/// Liste des services pour les listes déroulantes (EntityType Destinataire : tous, par id).
pub async fn destinataires(state: &AppState) -> sqlx::Result<Vec<Choix>> {
    sqlx::query_as("SELECT id, libelle FROM destinataire ORDER BY id").fetch_all(&state.db).await
}

#[derive(Serialize)]
pub struct Libelle {
    pub id: i32,
    pub libelle: String,
}

#[derive(Serialize)]
pub struct Nom {
    pub id: i32,
    pub nom: Option<String>,
}

/// Courrier tel que vu depuis une imputation (`imputation.courrier.xxx` dans les templates).
#[derive(Serialize)]
pub struct CourrierRef {
    pub id: i32,
    pub numero: Option<i32>,
    pub objet: Option<String>,
    pub expeditaire: Option<String>,
    pub courrier: Option<String>,
    pub date: Option<NaiveDate>,
    pub datearrive: Option<NaiveDateTime>,
}

/// Choix de la liste « Instruction » (ImputationType). La valeur de « Pour nécessaire »
/// est en minuscule dans l'original : conservée pour rester cohérente avec les données.
pub const INSTRUCTIONS: &[(&str, &str)] = &[
    ("Pour diffusion", "Pour diffusion"),
    ("Pour instruction", "Pour instruction"),
    ("Pour suite à donner", "Pour suite à donner"),
    ("Pour m'en parler", "Pour m'en parler"),
    ("Pour suivi", "Pour suivi"),
    ("Pour nécessaire", "pour nécessaire"),
    ("Pour examen et compte rendu", "Pour examen et compte rendu"),
    ("Pour action", "Pour action"),
];
