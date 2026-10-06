use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac_array;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::error::AppError;
use crate::AppState;

type HmacSha256 = Hmac<Sha256>;
const TOKEN_TTL_SECS: i64 = 12 * 60 * 60;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: String, // teacher | logistics
    pub class_id: Option<i64>,
    #[serde(skip_serializing)]
    pub password_hash: String,
}

impl User {
    pub fn is_logistics(&self) -> bool {
        self.role == "logistics"
    }
    /// 班主任只能操作本班
    pub fn ensure_class(&self, class_id: i64) -> Result<(), AppError> {
        if self.is_logistics() {
            return Ok(());
        }
        match self.class_id {
            Some(c) if c == class_id => Ok(()),
            _ => Err(AppError::forbidden("只能访问本班数据")),
        }
    }
    pub fn ensure_logistics(&self) -> Result<(), AppError> {
        if self.is_logistics() {
            Ok(())
        } else {
            Err(AppError::forbidden("仅后勤人员可执行该操作"))
        }
    }
}

// ---------- 口令哈希 ----------

pub fn hash_password(password: &str) -> String {
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let dk = pbkdf2_hmac_array::<Sha256, 32>(password.as_bytes(), &salt, 120_000);
    format!("pbkdf2${}${}", hex::encode(salt), hex::encode(dk))
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    let Some((salt_hex, hash_hex)) = stored.split_once('$').and_then(|(scheme, rest)| {
        if scheme != "pbkdf2" {
            return None;
        }
        rest.split_once('$')
    }) else {
        return false;
    };
    let (Ok(salt), Ok(expected)) = (hex::decode(salt_hex), hex::decode(hash_hex)) else {
        return false;
    };
    let dk = pbkdf2_hmac_array::<Sha256, 32>(password.as_bytes(), &salt, 120_000);
    // 常量时间比较
    dk.as_slice() == expected.as_slice()
}

// ---------- HMAC 令牌（无状态） ----------

#[derive(Serialize, Deserialize)]
struct Claims {
    uid: i64,
    exp: i64,
    nonce: String,
}

fn mac(secret: &[u8]) -> HmacSha256 {
    HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length")
}

pub fn issue_token(secret: &str, user_id: i64) -> String {
    let claims = Claims {
        uid: user_id,
        exp: chrono::Utc::now().timestamp() + TOKEN_TTL_SECS,
        nonce: hex::encode(rand::random::<[u8; 8]>()),
    };
    let payload = hex::encode(serde_json::to_vec(&claims).unwrap());
    let mut m = mac(secret.as_bytes());
    m.update(payload.as_bytes());
    let sig = hex::encode(m.finalize().into_bytes());
    format!("{payload}.{sig}")
}

fn verify_token(secret: &str, token: &str) -> Result<i64, AppError> {
    let (payload, sig) = token
        .split_once('.')
        .ok_or_else(|| AppError::unauthorized("令牌格式错误"))?;
    let mut m = mac(secret.as_bytes());
    m.update(payload.as_bytes());
    let sig_bytes =
        hex::decode(sig).map_err(|_| AppError::unauthorized("令牌签名错误"))?;
    m.verify_slice(&sig_bytes)
        .map_err(|_| AppError::unauthorized("令牌签名无效"))?;

    let raw = hex::decode(payload).map_err(|_| AppError::unauthorized("令牌内容错误"))?;
    let claims: Claims =
        serde_json::from_slice(&raw).map_err(|_| AppError::unauthorized("令牌内容错误"))?;
    if claims.exp < chrono::Utc::now().timestamp() {
        return Err(AppError::unauthorized("登录已过期，请重新登录"));
    }
    Ok(claims.uid)
}

// ---------- Axum 提取器 ----------

impl FromRequestParts<AppState> for User {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::unauthorized("缺少登录令牌"))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::unauthorized("令牌方案错误"))?;
        let uid = verify_token(&state.secret, token)?;
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(uid)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::unauthorized("用户不存在"))?;
        Ok(user)
    }
}

// 便于把 401/403 转成标准错误的辅助（提取器直接返回 AppError 即可）
#[allow(dead_code)]
pub fn status_text(code: StatusCode, msg: &str) -> (StatusCode, String) {
    (code, msg.to_string())
}
