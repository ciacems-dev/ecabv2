//! Tableau de bord `/admin` (AdminHomeController::index). Pour l'instant : les compteurs.

use axum::Extension;
use axum::extract::State;
use axum::response::Html;
use tower_sessions::Session;

use crate::auth::CurrentUser;
use crate::error::AppResult;
use crate::layout;
use crate::state::AppState;

pub async fn index(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
) -> AppResult<Html<String>> {
    let (nb_user, nb_arrive, nb_sorti, nb_comptr): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM users), (SELECT COUNT(*) FROM carrive),
                (SELECT COUNT(*) FROM csortir), (SELECT COUNT(*) FROM compterendu)",
    )
    .fetch_one(&state.db)
    .await?;

    layout::render(
        &state,
        &session,
        &user,
        "admin/home/index.html",
        ("1", "0"),
        minijinja::context! {
            nbUser => nb_user,
            nbarrive => nb_arrive,
            nbsorti => nb_sorti,
            nbcomptr => nb_comptr,
        },
    )
    .await
}
