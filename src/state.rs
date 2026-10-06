use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub secret: Arc<String>,
}

impl AppState {
    pub fn new(pool: SqlitePool, secret: String) -> Self {
        Self {
            pool,
            secret: Arc::new(secret),
        }
    }
}
