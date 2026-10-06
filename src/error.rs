use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug)]
pub enum AppError {
    /// 明确的 HTTP 错误（状态码 + 中文说明）
    Status(StatusCode, String),
    Sqlx(sqlx::Error),
}

impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::Status(StatusCode::BAD_REQUEST, msg.into())
    }
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Status(StatusCode::UNAUTHORIZED, msg.into())
    }
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Status(StatusCode::FORBIDDEN, msg.into())
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::Status(StatusCode::NOT_FOUND, msg.into())
    }
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Status(StatusCode::CONFLICT, msg.into())
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Sqlx(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::Status(code, msg) => (code, Json(json!({ "error": msg }))).into_response(),
            AppError::Sqlx(e) => {
                // 唯一约束/检查约束冲突 → 409（并发抢用同一资产时依赖此映射）
                let msg = e.to_string();
                if matches!(&e, sqlx::Error::Database(db) if db.is_unique_violation()
                    || db.is_check_violation())
                {
                    return (
                        StatusCode::CONFLICT,
                        Json(json!({ "error": format!("数据冲突，操作未生效：{msg}") })),
                    )
                        .into_response();
                }
                if matches!(e, sqlx::Error::RowNotFound) {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(json!({ "error": "记录不存在" })),
                    )
                        .into_response();
                }
                tracing::error!("database error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "服务器内部错误" })),
                )
                    .into_response()
            }
        }
    }
}
