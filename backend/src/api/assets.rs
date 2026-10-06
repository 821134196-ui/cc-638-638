use crate::auth::{AuthUser, LogisticsUser};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};

#[derive(sqlx::FromRow)]
struct AssetRow {
    code: String,
    model_code: String,
    status: String,
    location_class_id: Option<i64>,
    location_name: Option<String>,
    assigned_student_id: Option<i64>,
    student_name: Option<String>,
}

const ASSET_SELECT: &str = r#"SELECT a.code, a.model_code, a.status, a.location_class_id,
           c.name AS location_name, a.assigned_student_id, s.name AS student_name
    FROM assets a
    LEFT JOIN classes c ON c.id = a.location_class_id
    LEFT JOIN students s ON s.id = a.assigned_student_id"#;

fn asset_json(a: &AssetRow, allocatable: bool, active_transfer: Option<i64>) -> Value {
    json!({
        "code": a.code, "model": a.model_code, "status": a.status,
        "location_class_id": a.location_class_id,
        "location_name": a.location_name.clone().unwrap_or_else(|| "后勤仓库".into()),
        "assigned_student_id": a.assigned_student_id,
        "student_name": a.student_name,
        "allocatable": allocatable,
        "active_transfer_id": active_transfer,
    })
}

async fn active_transfer_of(pool: &sqlx::SqlitePool, code: &str) -> ApiResult<Option<i64>> {
    Ok(sqlx::query_as::<_, (i64,)>(
        "SELECT id FROM transfers WHERE asset_code=? AND status IN ('pending','released')",
    )
    .bind(code)
    .fetch_optional(pool)
    .await?
    .map(|(id,)| id))
}

/// 资产列表。后勤看全部；班主任只看本班位置上的桌椅。
pub async fn list_assets(
    user: AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Value>> {
    let rows: Vec<AssetRow> = if user.is_logistics() {
        sqlx::query_as(&format!("{ASSET_SELECT} ORDER BY a.code"))
            .fetch_all(&state.pool)
            .await?
    } else if let Some(cid) = user.class_id {
        sqlx::query_as(&format!("{ASSET_SELECT} WHERE a.location_class_id=?1 ORDER BY a.code"))
            .bind(cid)
            .fetch_all(&state.pool)
            .await?
    } else {
        vec![]
    };

    let mut assets = Vec::new();
    for a in &rows {
        let at = active_transfer_of(&state.pool, &a.code).await?;
        let allocatable = a.status == "available" && at.is_none();
        assets.push(asset_json(a, allocatable, at));
    }
    Ok(Json(json!({ "assets": assets })))
}

/// 本地模拟扫码：扫一套桌椅编号 → 返回该资产的当前状态与可分配性。
/// 不连接真实扫码枪；前端用输入/下拉/“模拟扫码”按钮触发本接口。
pub async fn scan_asset(
    user: AuthUser,
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> ApiResult<Json<Value>> {
    let row: Option<AssetRow> = sqlx::query_as(&format!("{ASSET_SELECT} WHERE a.code=?1"))
        .bind(&code)
        .fetch_optional(&state.pool)
        .await?;
    let Some(a) = row else {
        return Err(ApiError::NotFound(format!("未找到编号 {code} 的桌椅")));
    };
    // 班主任扫到外班/仓库资产时只返回最小信息，不泄露占用学生等信息
    if !user.is_logistics() {
        let own = a.location_class_id == user.class_id;
        if !own {
            return Ok(Json(json!({
                "code": a.code, "model": a.model_code, "status": a.status,
                "location_name": a.location_name.clone().unwrap_or_else(|| "后勤仓库".into()),
                "notice": "该资产不在您的班级",
            })));
        }
    }
    let at = active_transfer_of(&state.pool, &a.code).await?;
    let allocatable = a.status == "available" && at.is_none();
    Ok(Json(asset_json(&a, allocatable, at)))
}

pub async fn history(
    user: AuthUser,
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> ApiResult<Json<Value>> {
    let loc: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT location_class_id FROM assets WHERE code=?")
            .bind(&code)
            .fetch_optional(&state.pool)
            .await?;
    let Some((loc,)) = loc else {
        return Err(ApiError::NotFound("桌椅编号不存在".into()));
    };
    if let Some(cid) = loc {
        user.ensure_class_access(cid)?;
    } else if !user.is_logistics() {
        // 仓库资产履历只有后勤能看
        return Err(ApiError::Forbidden);
    }

    let rows: Vec<(i64, String, String, Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<i64>, String)> =
        sqlx::query_as(
            r#"SELECT id, asset_code, event_type, from_class_id, to_class_id,
                      from_student_id, to_student_id, actor_user_id, created_at
               FROM asset_history WHERE asset_code=?1
               ORDER BY id DESC, created_at DESC"#,
        )
        .bind(&code)
        .fetch_all(&state.pool)
        .await?;
    let events: Vec<Value> = rows
        .iter()
        .map(|(id, c, ty, fc, tc, fs, ts, actor, at)| {
            json!({"id": id, "asset_code": c, "event_type": ty,
                   "from_class_id": fc, "to_class_id": tc,
                   "from_student_id": fs, "to_student_id": ts,
                   "actor_user_id": actor, "created_at": at})
        })
        .collect();
    Ok(Json(json!({ "asset_code": code, "history": events })))
}

/// 标记损坏：仅限空闲桌椅（占用中的须先收回）。损坏品立即退出可分配池。
pub async fn mark_damaged(
    user: LogisticsUser,
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> ApiResult<Json<Value>> {
    let current: Option<(String, Option<i64>)> =
        sqlx::query_as("SELECT status, assigned_student_id FROM assets WHERE code=?")
            .bind(&code)
            .fetch_optional(&state.pool)
            .await?;
    let Some((status, assigned)) = current else {
        return Err(ApiError::NotFound("桌椅编号不存在".into()));
    };
    if status == "damaged" {
        return Err(ApiError::BadRequest("该桌椅已处于损坏状态".into()));
    }
    if assigned.is_some() {
        return Err(ApiError::Conflict("桌椅仍被学生占用，请先收回再报修".into()));
    }
    if active_transfer_of(&state.pool, &code).await?.is_some() {
        return Err(ApiError::Conflict("桌椅在途调拨中，不能标记损坏".into()));
    }

    let mut tx = crate::db::ImmediateTx::begin(&state.pool).await?;
    sqlx::query("UPDATE assets SET status='damaged', updated_at=datetime('now') WHERE code=?")
        .bind(&code)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO asset_history (asset_code, event_type, actor_user_id, detail, created_at)
         VALUES (?1,'damaged',?2,'报修，退出可分配列表',datetime('now'))",
    )
    .bind(&code)
    .bind(user.0.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok": true, "code": code, "status": "damaged"})))
}

/// 维修完成：损坏 → 空闲可分配，位置不变。
pub async fn repair(
    user: LogisticsUser,
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> ApiResult<Json<Value>> {
    let status: Option<(String,)> = sqlx::query_as("SELECT status FROM assets WHERE code=?")
        .bind(&code)
        .fetch_optional(&state.pool)
        .await?;
    let Some((status,)) = status else {
        return Err(ApiError::NotFound("桌椅编号不存在".into()));
    };
    if status != "damaged" {
        return Err(ApiError::BadRequest("只有损坏状态的桌椅可以维修入库".into()));
    }

    let mut tx = crate::db::ImmediateTx::begin(&state.pool).await?;
    sqlx::query("UPDATE assets SET status='available', updated_at=datetime('now') WHERE code=?")
        .bind(&code)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO asset_history (asset_code, event_type, actor_user_id, detail, created_at)
         VALUES (?1,'repaired',?2,'维修完成，重新进入可分配列表',datetime('now'))",
    )
    .bind(&code)
    .bind(user.0.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok": true, "code": code, "status": "available"})))
}
