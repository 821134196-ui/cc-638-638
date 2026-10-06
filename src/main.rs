use std::net::SocketAddr;
use std::path::PathBuf;

use deskflow::{app::app_with_static, db, AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn,deskflow=debug".into()),
        )
        .init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080);
    let db_path =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data/deskflow.db".into());
    let secret = std::env::var("APP_SECRET")
        .unwrap_or_else(|_| "deskflow-dev-secret-change-me".to_string());
    let static_dir =
        std::env::var("STATIC_DIR").unwrap_or_else(|_| "frontend/dist".to_string());

    std::fs::create_dir_all(PathBuf::from(&static_dir).parent().unwrap_or_else(||
        std::path::Path::new(".")))?;
    if let Some(parent) = std::path::Path::new(db_path.trim_start_matches("sqlite://")).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let pool = db::init(&db_path).await?;
    let state = AppState::new(pool, secret);
    let app = app_with_static(state, std::path::Path::new(&static_dir));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("课桌椅调拨系统已启动：http://localhost:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}
