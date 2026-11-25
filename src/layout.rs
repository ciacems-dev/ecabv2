//! Contexte commun à toutes les pages du layout d'administration (`layouts/admin.html`).
//!
//! L'ancienne cloche de notifications chargeait, sur chaque page et deux fois, toutes les
//! imputations du service de l'utilisateur, puis chaque courrier lié (requêtes en cascade).
//! Ici : une seule requête SQL, avec exactement le même filtre que le template Twig d'origine.

use axum::response::Html;
use chrono::NaiveDate;
use minijinja::Value;
use serde::Serialize;
use sqlx::FromRow;
use tower_sessions::Session;

use crate::auth::CurrentUser;
use crate::error::AppResult;
use crate::session;
use crate::state::AppState;

#[derive(Serialize, FromRow)]
pub struct Notification {
    pub id: i32,
    pub objet: Option<String>,
    pub reponse: Option<String>,
    pub extremement_urgent: bool,
    pub date: Option<NaiveDate>,
    pub delais: Option<NaiveDate>,
}

/// Règle d'origine (layouts/admin.html.twig) :
/// `imputation.statut == 0 and imputation.date|date('Y') == annee and imputation.delais|date("Ymd") >= jour`.
/// En Twig, `null == 0` est vrai et `null|date` vaut la date du jour : d'où les COALESCE.
/// Tri : date décroissante. `effet == 1` → « Extrêmement urgent ».
pub async fn notifications(state: &AppState, user: &CurrentUser) -> AppResult<Vec<Notification>> {
    let Some(dest) = &user.destinataire else { return Ok(Vec::new()) };
    Ok(sqlx::query_as::<_, Notification>(
        "SELECT i.id, c.objet, i.reponse, COALESCE(trim(i.effet) = '1', false) AS extremement_urgent,
                i.date, i.delais
         FROM imputation i
         LEFT JOIN carrive c ON c.id = i.courrier_id
         WHERE i.destinataire_id = $1
           AND COALESCE(i.statut, false) = false
           AND EXTRACT(YEAR FROM COALESCE(i.date, CURRENT_DATE)) = EXTRACT(YEAR FROM CURRENT_DATE)
           AND COALESCE(i.delais, CURRENT_DATE) >= CURRENT_DATE
         ORDER BY i.date DESC NULLS LAST, i.id DESC",
    )
    .bind(dest.id)
    .fetch_all(&state.db)
    .await?)
}

/// Rend une page du layout admin : ajoute `app.user`, `csrf_token`, `flashes`, `notifications`
/// et `menu` / `smenu` (entrée de menu active) au contexte propre à la page.
pub async fn render(
    state: &AppState,
    session: &Session,
    user: &CurrentUser,
    template: &str,
    menu: (&str, &str),
    page: Value,
) -> AppResult<Html<String>> {
    let common = minijinja::context! {
        app => minijinja::context! { user => user },
        csrf_token => session::csrf_token(session).await?,
        flashes => session::take_flashes(session).await?,
        notifications => notifications(state, user).await?,
        menu => menu.0,
        smenu => menu.1,
    };
    let ctx = minijinja::context! { ..page, ..common };
    Ok(Html(state.tpl.get_template(template)?.render(ctx)?))
}
