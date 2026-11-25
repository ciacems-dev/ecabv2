//! Tableau de bord `/admin` (AdminHomeController::index + admin/home/index.html.twig).
//!
//! Chaque bloc n'est chargé que pour le profil qui le voit (voir docs/regles-cabinet-tableau-de-bord.md).

use axum::Extension;
use axum::extract::State;
use axum::response::Html;
use chrono::Datelike;
use serde::Serialize;
use sqlx::FromRow;
use tower_sessions::Session;

use super::{audiences, cabinet, courrier_depart, imputation};
use crate::auth::CurrentUser;
use crate::error::AppResult;
use crate::layout;
use crate::state::AppState;

#[derive(Serialize, FromRow)]
struct Mission {
    id: i32,
    objet: String,
    datedebut: Option<chrono::NaiveDate>,
    datefin: Option<chrono::NaiveDate>,
    participant: Option<String>,
    matricule: Option<String>,
    fonction: Option<String>,
    destination: Option<String>,
    ordrem: Option<String>,
    rapportm: Option<String>,
    lieu: Option<String>,
}

pub async fn index(
    State(state): State<AppState>,
    session: Session,
    Extension(user): Extension<CurrentUser>,
) -> AppResult<Html<String>> {
    let accueil = user.is_accueil();
    let annee = chrono::Utc::now().year();

    // Compteurs : super admin (hors accueil).
    let (nb_user, nb_arrive, nb_sorti, nb_comptr): (i64, i64, i64, i64) = if user.is_super_admin() && !accueil {
        sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM users), (SELECT COUNT(*) FROM carrive),
                    (SELECT COUNT(*) FROM csortir), (SELECT COUNT(*) FROM compterendu)",
        )
        .fetch_one(&state.db)
        .await?
    } else {
        (0, 0, 0, 0)
    };

    // Onglets : tous sauf accueil et service courrier (conditions `libelle != ...` d'origine, sur le profil).
    let profil_courrier = user.destinataire.as_ref().and_then(|d| d.profil.as_deref()) == Some("courrier");
    let onglets = !accueil && !profil_courrier;
    let (imputations, compterendus, diligences, missions) = if onglets {
        let imputations = match user.destinataire_id() {
            Some(d) => {
                imputation::query(
                    &state,
                    "i.destinataire_id = $3 AND COALESCE(i.archive, false) = false
                     AND EXTRACT(YEAR FROM COALESCE(i.date, CURRENT_DATE)) = EXTRACT(YEAR FROM CURRENT_DATE)",
                    "i.id DESC",
                    None,
                    Some(d),
                )
                .await?
            }
            None => vec![],
        };
        let compterendus = cabinet::comptes_rendus(
            &state,
            "EXTRACT(YEAR FROM c.date) = EXTRACT(YEAR FROM CURRENT_DATE) AND c.statut = true",
        )
        .await?;
        let diligences = cabinet::diligences(&state, "true", "g.debut DESC NULLS LAST, g.id DESC", None).await?;
        let missions: Vec<Mission> = sqlx::query_as(
            "SELECT id, objet, datedebut, datefin, participant, matricule, fonction, destination, ordrem, rapportm, lieu
             FROM mission WHERE destinataire_id = $1 ORDER BY id DESC",
        )
        .bind(user.destinataire_id())
        .fetch_all(&state.db)
        .await?;
        (imputations, compterendus, diligences, missions)
    } else {
        (vec![], vec![], vec![], vec![])
    };

    // Service courrier : départs de l'année non envoyés (CsortirRepository::homesc).
    let csortirs = if profil_courrier {
        courrier_depart::liste(
            &state,
            "COALESCE(s.statut, false) = false
             AND EXTRACT(YEAR FROM COALESCE(s.date, CURRENT_DATE)) = EXTRACT(YEAR FROM CURRENT_DATE)",
            "s.id DESC",
        )
        .await?
    } else {
        vec![]
    };

    // Accueil : audiences.
    let audiences = if accueil { audiences::audiences(&state, "true").await? } else { vec![] };

    layout::render(
        &state,
        &session,
        &user,
        "admin/home/index.html",
        ("1", "0"),
        minijinja::context! {
            annee,
            nbUser => nb_user, nbarrive => nb_arrive, nbsorti => nb_sorti, nbcomptr => nb_comptr,
            imputations, compterendus, diligences, missions, csortirs, audiences,
        },
    )
    .await
}
