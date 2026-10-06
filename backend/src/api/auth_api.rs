use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::response::IntoResponse;
use axum::Json;
use rand::Rng;
use serde::Deserialize;
use serde_json::json;

const SESSION_TTL_SECONDS: i64 = 12 * 60 * 60;

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Deserialize)]
pub struct LoginReq {
    username: String,
    password: String,
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginReq>,
) -> ApiResult<impl IntoResponse> {
    let row: Option<(i64, String, String, String, Option<i64>)> = sqlx::query_as(
        "SELECT id, username, display_name, role, class_id FROM users WHERE username=? AND password=?",
    )
    .bind(&req.username)
    .bind(&req.password)
    .fetch_optional(&state.pool)
    .await?;

    let (id, username, display_name, role, class_id) =
        row.ok_or_else(|| ApiError::Unauthorized)?;

    let token = new_token();
    sqlx::query(
        "INSERT INTO sessions (token, user_id, created_at, expires_at)
         VALUES (?1, ?2, datetime('now'), datetime('now', ?3))",
    )
    .bind(&token)
    .bind(id)
    .bind(format!("+{SESSION_TTL_SECONDS} seconds"))
    .execute(&state.pool)
    .await?;

    let cookie = format!(
        "session={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={SESSION_TTL_SECONDS}"
    );
    let body = json!({
        "user": { "id": id, "username": username, "display_name": display_name, "role": role, "class_id": class_id }
    });
    Ok(([(SET_COOKIE, cookie)], Json(body)))
}

pub async fn logout(State(state): State<AppState>, user: AuthUser) -> ApiResult<impl IntoResponse> {
    sqlx::query("DELETE FROM sessions WHERE user_id=?")
        .bind(user.id)
        .execute(&state.pool)
        .await?;
    let cookie = "session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0".to_string();
    Ok(([(SET_COOKIE, cookie)], Json(json!({"ok": true}))))
}

pub async fn me(user: AuthUser) -> impl IntoResponse {
    Json(json!(user))
}
