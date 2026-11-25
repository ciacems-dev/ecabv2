use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use minijinja::Environment;
use sqlx::PgPool;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub tpl: Arc<Environment<'static>>,
    pub cfg: Arc<Config>,
    pub login_limiter: Arc<LoginLimiter>,
}

/// Limite les tentatives de connexion échouées (absent de l'ancienne application).
/// En mémoire : suffisant pour une instance unique ; à déplacer en base si l'on passe à plusieurs instances.
pub struct LoginLimiter {
    failures: Mutex<HashMap<String, Vec<Instant>>>,
    max: usize,
    window: Duration,
}

impl LoginLimiter {
    pub fn new(max: usize, window: Duration) -> Self {
        Self { failures: Mutex::new(HashMap::new()), max, window }
    }

    pub fn is_blocked(&self, key: &str) -> bool {
        let mut map = self.failures.lock().unwrap();
        let now = Instant::now();
        let entry = map.entry(key.to_string()).or_default();
        entry.retain(|t| now.duration_since(*t) < self.window);
        entry.len() >= self.max
    }

    pub fn record_failure(&self, key: &str) {
        self.failures.lock().unwrap().entry(key.to_string()).or_default().push(Instant::now());
    }

    pub fn reset(&self, key: &str) {
        self.failures.lock().unwrap().remove(key);
    }
}
