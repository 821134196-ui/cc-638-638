use sqlx::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub static_dir: String,
}

impl AppState {
    pub fn new(pool: SqlitePool, static_dir: String) -> Self {
        Self { pool, static_dir }
    }
}
