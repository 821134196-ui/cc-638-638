pub mod api;
pub mod auth;
pub mod db;
pub mod error;
pub mod matching;
pub mod state;

pub use state::AppState;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tower::ServiceExt;
use tower_http::services::ServeDir;
use std::path::PathBuf;

/// 构建完整应用：/api 下是 JSON 接口，其余路径交给前端静态资源（SPA）。
pub fn build_router(state: AppState) -> Router {
    let static_dir = state.static_dir.clone();
    let index_path = PathBuf::from(&static_dir).join("index.html");
    let api = api::router();

    let app = Router::new().nest("/api", api);

    if index_path.exists() {
        app.fallback(move |s, req| spa_fallback(s, req, static_dir.clone(), index_path.clone()))
            .with_state(state)
    } else {
        // 测试或仅后端模式：没有前端构建产物时，未知路径返回 404 JSON
        app.fallback(|| async {
            (
                StatusCode::NOT_FOUND,
                [(header::CONTENT_TYPE, "application/json")],
                r#"{"error":"not found"}"#,
            )
        })
        .with_state(state)
    }
}

/// 全局回退：
/// - /api 下未匹配的路径 → JSON 404（不泄露前端页面）；
/// - 其余路径交 ServeDir；文件不存在时回退 index.html 且状态为 200（前端路由接管）。
async fn spa_fallback(
    State(_state): State<AppState>,
    req: Request,
    static_dir: String,
    index_path: PathBuf,
) -> Response {
    let path = req.uri().path().to_string();
    if path == "/api" || path.starts_with("/api/") {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"error":"api endpoint not found"}"#,
        )
            .into_response();
    }

    let spa = get(move || spa_index(index_path.clone()));
    let serve = ServeDir::new(&static_dir).not_found_service(spa);
    match serve.oneshot(req).await {
        Ok(resp) => {
            // ServeDir 会把回退服务的响应状态标记为 404；内容是 index.html，改回 200
            let mut resp = resp.map(Body::new);
            if resp.status() == StatusCode::NOT_FOUND {
                *resp.status_mut() = StatusCode::OK;
            }
            resp
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// SPA 回退页面内容。
async fn spa_index(index_path: PathBuf) -> Response {
    match tokio::fs::read(&index_path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            bytes,
        )
            .into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"error":"index.html missing"}"#,
        )
            .into_response(),
    }
}
