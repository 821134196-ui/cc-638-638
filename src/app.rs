use axum::routing::{get, patch, post};
use axum::Router;
use tower_http::trace::TraceLayer;

use crate::routes;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        // 认证
        .route("/auth/login", post(routes::login))
        .route("/auth/me", get(routes::me))
        // 基础数据
        .route("/classes", get(routes::list_classes))
        .route("/models", get(routes::list_models))
        // 学生 / 身高 / 建议
        .route("/classes/{class_id}/students", get(routes::list_students))
        .route("/students/{student_id}/height", patch(routes::update_height))
        .route("/suggestions/generate", post(routes::generate_suggestions))
        .route("/suggestions", get(routes::list_suggestions))
        // 资产
        .route("/assets", get(routes::list_assets))
        .route("/assets/scan", post(routes::scan))
        .route("/assets/{code}", get(routes::asset_detail))
        .route("/assets/{code}/trail", get(routes::asset_trail))
        .route("/assets/{code}/release", post(routes::release_asset))
        .route("/assets/{code}/damaged", post(routes::mark_damaged))
        .route("/assets/{code}/repair", post(routes::repair_asset))
        // 分配
        .route("/assignments", post(routes::assign_asset))
        // 学生换班
        .route("/students/{student_id}/class", patch(routes::move_student_class))
        // 跨班调拨
        .route("/transfers", post(routes::create_transfer).get(routes::list_transfers))
        .route("/transfers/{id}/confirm", post(routes::confirm_transfer))
        .route("/transfers/{id}/cancel", post(routes::cancel_transfer))
        // 班级看板
        .route("/classes/{class_id}/board", get(routes::class_board));

    Router::new()
        .nest("/api", api)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// 生产用：API + 前端静态文件
pub fn app_with_static(state: AppState, static_dir: &std::path::Path) -> Router {
    let serve = tower_http::services::ServeDir::new(static_dir)
        .append_index_html_on_directories(true)
        .fallback(tower_http::services::ServeFile::new(static_dir.join("index.html")));
    router(state).fallback_service(serve)
}
