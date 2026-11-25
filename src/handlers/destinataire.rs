//! Espace d'un service (DestinataireController::show) : « Mes courriers imputés » et
//! « Mes courriers initiés au Cabinet ». Les autres actions du module viendront avec les référentiels.

use axum::Extension;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::FromRow;
use tower_sessions::Session;

use super::common::Libelle;
use super::imputation;
use crate::auth::{CurrentUser, ensure};
use crate::error::AppResult;
use crate::layout;
use crate::state::AppState;

#[derive(FromRow)]
struct DepartRow {
    id: i32,
    date: Option<NaiveDate>,
    numsortir: Option<i32>,
    objet: Option<String>,
    destinataire: Option<String>,
    agent: Option<String>,
    courrier: Option<String>,
    decharge: Option<String>,
    type_libelle: Option<String>,
    auteur_libelle: Option<String>,
}

#[derive(Serialize)]
struct Depart {
    id: i32,
    date: Option<NaiveDate>,
    numsortir: Option<i32>,
    objet: Option<String>,
    destinataire: Option<String>,
    agent: Option<String>,
    courrier: Option<String>,
    decharge: Option<String>,
    r#type: Option<Libelle>,
    auteur: Option<Libelle>,
}

pub async fn show(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    // Avant : n'importe qui pouvait ouvrir l'espace de n'importe quel service par l'URL.
    ensure(user.owns(Some(id)) || user.is_super_admin())?;
    let destinataire: Libelle = sqlx::query_as::<_, (i32, String)>("SELECT id, libelle FROM destinataire WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await
        .map(|(id, libelle)| Libelle { id, libelle })?;

    let imputations = imputation::query(
        &state,
        "i.destinataire_id = $3 AND COALESCE(i.archive, false) = false",
        "i.id DESC",
        None,
        Some(id),
    )
    .await?;

    let departs: Vec<Depart> = sqlx::query_as::<_, DepartRow>(
        "SELECT s.id, s.date, s.numsortir, s.objet, s.destinataire, s.agent, s.courrier, s.decharge,
                t.libelle AS type_libelle, a.libelle AS auteur_libelle
         FROM csortir s
         LEFT JOIN typedepart t ON t.id = s.type_id
         LEFT JOIN destinataire a ON a.id = s.auteur_id
         WHERE s.initiateur_id = $1 AND COALESCE(s.archive, false) = false
         ORDER BY s.id DESC",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|r| Depart {
        id: r.id,
        date: r.date,
        numsortir: r.numsortir,
        objet: r.objet,
        destinataire: r.destinataire,
        agent: r.agent,
        courrier: r.courrier,
        decharge: r.decharge,
        r#type: r.type_libelle.map(|libelle| Libelle { id: 0, libelle }),
        auteur: r.auteur_libelle.map(|libelle| Libelle { id: 0, libelle }),
    })
    .collect();

    Ok(layout::render(
        &state,
        &session,
        &user,
        "admin/destinataire/show.html",
        ("6", "0"),
        minijinja::context! { destinataire, imputations, departs },
    )
    .await?
    .into_response())
}
