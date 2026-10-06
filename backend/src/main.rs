use desk_match::{build_router, db, AppState};
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let db_path = std::env::var("DATABASE_URL").unwrap_or_else(|_| "data/desk_match.db".into());
    let static_dir =
        std::env::var("STATIC_DIR").unwrap_or_else(|_| "frontend/dist".into());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let pool = db::connect(&db_path).await?;
    db::init(&pool).await?;

    let state = AppState::new(pool, static_dir);
    let app = build_router(state);

    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("课桌椅调拨系统已启动: http://localhost:{port}");
    tracing::info!("演示账号: faculty1/faculty1 (班主任) · logistics/logistics (后勤)");
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("正在关闭……");
}
