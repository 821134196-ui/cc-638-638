use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::header::{AUTHORIZATION, COOKIE};
use axum::http::request::Parts;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: String, // 'faculty' | 'logistics'
    pub class_id: Option<i64>,
}

impl AuthUser {
    pub fn is_logistics(&self) -> bool {
        self.role == "logistics"
    }

    /// 班主任只能访问本班数据；后勤可访问全部班级。
    pub fn ensure_class_access(&self, class_id: i64) -> ApiResult<()> {
        if self.is_logistics() {
            return Ok(());
        }
        match self.class_id {
            Some(c) if c == class_id => Ok(()),
            _ => Err(ApiError::Forbidden),
        }
    }
}

fn token_from_parts(parts: &Parts) -> Option<String> {
    // 1) Authorization: Bearer <token>
    if let Some(v) = parts.headers.get(AUTHORIZATION) {
        if let Ok(s) = v.to_str() {
            if let Some(t) = s.strip_prefix("Bearer ") {
                return Some(t.trim().to_string());
            }
        }
    }
    // 2) Cookie: session=<token>
    if let Some(v) = parts.headers.get(COOKIE) {
        if let Ok(s) = v.to_str() {
            for pair in s.split(';') {
                let pair = pair.trim();
                if let Some(t) = pair.strip_prefix("session=") {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = token_from_parts(parts).ok_or(ApiError::Unauthorized)?;
        let row: Option<(i64, String, String, String, Option<i64>)> = sqlx::query_as(
            r#"SELECT u.id, u.username, u.display_name, u.role, u.class_id
               FROM sessions s JOIN users u ON u.id = s.user_id
               WHERE s.token = ? AND s.expires_at > datetime('now')"#,
        )
        .bind(token)
        .fetch_optional(&state.pool)
        .await?;
        let (id, username, display_name, role, class_id) = row.ok_or(ApiError::Unauthorized)?;
        Ok(AuthUser { id, username, display_name, role, class_id })
    }
}

/// 仅后勤可访问的接口
pub struct LogisticsUser(pub AuthUser);

impl FromRequestParts<AppState> for LogisticsUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if user.is_logistics() {
            Ok(LogisticsUser(user))
        } else {
            Err(ApiError::Forbidden)
        }
    }
}
