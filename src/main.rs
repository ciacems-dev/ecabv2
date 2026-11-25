mod auth;
mod config;
mod error;
mod files;
mod forms;
mod handlers;
mod layout;
mod pdf;
mod routes;
mod session;
mod state;
mod templates;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use axum::Router;
use axum::http::{HeaderValue, header};
use axum::middleware;
use axum::response::Redirect;
use axum::routing::get;
use sqlx::postgres::PgPoolOptions;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tower_sessions::cookie::SameSite;
use tower_sessions::session_store::ExpiredDeletion;
use tower_sessions::{Expiry, SessionManagerLayer};
use tower_sessions_sqlx_store::PostgresStore;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::state::{AppState, LoginLimiter};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "ecab=info,tower_http=info".into()))
        .init();

    let cfg = Config::from_env()?;
    let db = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&cfg.database_url)
        .await
        .context("connexion PostgreSQL")?;
    sqlx::migrate!("./migrations").run(&db).await.context("migrations")?;

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("serve") => serve(cfg, db).await,
        Some("create-user") => create_user(&db, &args[1..]).await,
        Some(other) => bail!("commande inconnue : {other} (attendu : serve | create-user)"),
    }
}

async fn serve(cfg: Config, db: sqlx::PgPool) -> Result<()> {
    let store = PostgresStore::new(db.clone());
    store.migrate().await.context("table des sessions")?;
    tokio::spawn(store.clone().continuously_delete_expired(Duration::from_secs(3600)));

    let sessions = SessionManagerLayer::new(store)
        .with_name("ecab_session")
        .with_secure(cfg.cookie_secure)
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(time::Duration::hours(8)));

    let state = AppState {
        db,
        tpl: Arc::new(templates::build(&cfg.templates_dir)),
        cfg: Arc::new(cfg.clone()),
        login_limiter: Arc::new(LoginLimiter::new(5, Duration::from_secs(15 * 60))),
    };

    use handlers::{courrier_arrive as ca, destinataire, imputation as imp};
    use axum::routing::post;

    // Actions simples (supprimer, archiver, traité…) : POST + jeton CSRF vérifié par middleware.
    let actions = Router::new()
        .route("/admin/courriers/arrives/{id}/supprimer", post(ca::delete))
        .route("/admin/courriers/arrives/{id}/supprimer/cab", post(ca::delete_cab))
        .route("/admin/courriers/arrives/{id}/archiver", post(ca::archive))
        .route("/admin/courriers/arrives/{id}/desarchiver", post(ca::unarchive))
        .route("/admin/imputations/{id}/supprimer", post(imp::delete))
        .route("/admin/imputations/{id}/traite", post(imp::traite))
        .route("/admin/imputations/{id}/non-traite", post(imp::non_traite))
        .route("/admin/imputations/{id}/archiver", post(imp::archive))
        .route("/admin/imputations/{id}/desarchiver", post(imp::unarchive))
        .route_layer(middleware::from_fn(session::require_csrf_form));

    let admin = Router::new()
        .route("/admin", get(handlers::admin_home::index))
        // Courrier arrivé
        .route("/admin/courriers/arrives", get(ca::index))
        .route("/admin/courriers/arrives/dircab", get(ca::index_dircab))
        .route("/admin/courriers/arrives/archives", get(ca::index_annee_precedente))
        .route("/admin/courriers/arrives/classes", get(ca::index_classes))
        .route("/admin/courriers/arrives/nouveau", get(ca::new).post(ca::create_courrier))
        .route("/admin/courriers/arrives/nouveau/confidentiel", get(ca::new_confidentiel).post(ca::create_confidentiel))
        .route("/admin/courriers/arrives/{id}", get(ca::show))
        .route("/admin/courriers/arrives/{id}/modifier", get(ca::edit).post(ca::update_courrier))
        .route("/admin/courriers/arrives/{id}/modifier/cab", get(ca::edit_cab).post(ca::update_cab))
        .route("/admin/courriers/arrives/{id}/fiche", get(ca::fiche))
        // Imputations
        .route("/admin/imputations", get(imp::index))
        .route("/admin/imputations/classees", get(imp::classees))
        .route("/admin/imputations/archives", get(imp::archives))
        .route("/admin/imputations/archives/perso", get(imp::archives_perso))
        .route("/admin/imputations/nouvelle/{id}", get(imp::new_dircab).post(imp::create_dircab))
        .route("/admin/imputations/nouvelle/{id}/sc", get(imp::new_sc).post(imp::create_sc))
        .route("/admin/imputations/transmettre/{id}", get(imp::new_transmission).post(imp::create_transmission))
        .route("/admin/imputations/{id}", get(imp::show))
        .route("/admin/imputations/{id}/modifier", get(imp::edit).post(imp::edit_submit))
        .route("/admin/imputations/{id}/retour", get(imp::retour).post(imp::retour_submit))
        .route("/admin/imputations/{id}/observation", get(imp::observation).post(imp::observation_submit))
        .route("/admin/imputations/{id}/document-final", get(imp::document_final).post(imp::document_final_submit))
        // Espace service
        .route("/admin/services/{id}", get(destinataire::show))
        // Fichiers joints (téléchargement contrôlé)
        .route("/fichiers/{dir}/{name}", get(files::download))
        .merge(actions)
        .layer(axum::extract::DefaultBodyLimit::max(files::MAX_UPLOAD + 1024 * 1024))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::require_admin));

    let app = Router::new()
        .route("/", get(|| async { Redirect::to("/admin") }))
        .route("/login", get(auth::login_page).post(auth::login_submit))
        .route("/logout", axum::routing::post(auth::logout))
        .merge(admin)
        .nest_service("/assets", ServeDir::new(format!("{}/assets", cfg.static_dir)))
        .layer(sessions)
        .layer(SetResponseHeaderLayer::overriding(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("same-origin"),
        ))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(cfg.bind_addr).await?;
    tracing::info!("ECAB-MS en écoute sur http://{}", cfg.bind_addr);
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}

/// `ecab create-user <email> <ROLE_...> [libellé du service]`
/// Le mot de passe est lu dans la variable d'environnement `ECAB_NEW_PASSWORD`
/// (pas en argument, pour qu'il n'apparaisse pas dans l'historique du shell).
async fn create_user(db: &sqlx::PgPool, args: &[String]) -> Result<()> {
    let (Some(email), Some(role)) = (args.first(), args.get(1)) else {
        bail!("usage : create-user <email> <ROLE_SUPER_ADMIN|ROLE_ADMIN|ROLE_USER> [libellé du service]");
    };
    if !matches!(role.as_str(), "ROLE_SUPER_ADMIN" | "ROLE_ADMIN" | "ROLE_USER") {
        bail!("rôle invalide : {role}");
    }
    let password = std::env::var("ECAB_NEW_PASSWORD").context("définissez ECAB_NEW_PASSWORD")?;
    if password.chars().count() < 12 {
        bail!("mot de passe trop court (12 caractères minimum)");
    }
    let destinataire_id: Option<i32> = match args.get(2) {
        Some(libelle) => Some(
            sqlx::query_scalar("SELECT id FROM destinataire WHERE libelle = $1")
                .bind(libelle)
                .fetch_optional(db)
                .await?
                .with_context(|| format!("service introuvable : {libelle}"))?,
        ),
        None => None,
    };
    let hash = bcrypt::hash(password, 13)?;
    sqlx::query(
        "INSERT INTO users (email, roles, password, is_verified, active, destinataire_id)
         VALUES ($1, $2, $3, true, true, $4)",
    )
    .bind(email)
    .bind(sqlx::types::Json(vec![role.clone()]))
    .bind(hash)
    .bind(destinataire_id)
    .execute(db)
    .await
    .context("création de l'utilisateur")?;
    println!("Utilisateur {email} créé ({role}).");
    Ok(())
}
