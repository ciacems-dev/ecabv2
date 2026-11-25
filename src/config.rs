use anyhow::{Context, Result};
use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    /// Cookie de session `Secure` (obligatoire en production derrière HTTPS).
    pub cookie_secure: bool,
    /// Lire l'IP client dans `X-Forwarded-For` (uniquement derrière un reverse proxy de confiance, ex. Traefik / Dokploy).
    pub trust_proxy: bool,
    pub templates_dir: String,
    pub static_dir: String,
    /// Dossier des fichiers joints (sous-dossiers d'origine : courrier_arrives, imputations…).
    pub uploads_dir: String,
    /// Navigateur sans interface pour la fiche de suivi PDF (`chromium` dans l'image Docker).
    pub chrome_bin: String,
}

fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: std::env::var("DATABASE_URL").context("DATABASE_URL manquant")?,
            bind_addr: std::env::var("BIND_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:8080".into())
                .parse()
                .context("BIND_ADDR invalide")?,
            cookie_secure: env_bool("COOKIE_SECURE", false),
            trust_proxy: env_bool("TRUST_PROXY", false),
            templates_dir: std::env::var("TEMPLATES_DIR").unwrap_or_else(|_| "templates".into()),
            static_dir: std::env::var("STATIC_DIR").unwrap_or_else(|_| "static".into()),
            uploads_dir: std::env::var("UPLOADS_DIR").unwrap_or_else(|_| "uploads".into()),
            chrome_bin: std::env::var("CHROME_BIN").unwrap_or_else(|_| "chromium".into()),
        })
    }
}
