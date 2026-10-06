use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

/// 跨班调拨状态机：
///   pending  ──交出方放行(release)──▶ released ──接收方确认(confirm)──▶ confirmed（此刻才移动资产）
///      │                                │
///      └────双方均可拒绝(reject)─────────┴──接收方拒收(reject)──▶ rejected（资产留在交出方，从未移动）
///
/// 资产位置 assets.location_class_id 只在 confirm 同一事务里更新一次；
/// pending/released 期间建议页与分配都不得把它算入接收班（建议池排除在途资产，分配接口拒绝在途资产）。

#[derive(Deserialize)]
pub struct CreateReq {
    asset_code: String,
    from_class_id: i64,
    to_class_id: i64,
}

fn conflict_on_unique(e: sqlx::Error, msg: &str) -> ApiError {
    if let sqlx::Error::Database(db) = &e {
        if db.message().contains("UNIQUE") {
            return ApiError::Conflict(msg.into());
        }
    }
    ApiError::Sqlx(e)
}

/// 发起调拨：后勤或接收班班主任可发起（替本班申请）。
pub async fn create(
    user: AuthUser,
    State(state): State<AppState>,
    Json(req): Json<CreateReq>,
) -> ApiResult<(axum::http::StatusCode, Json<Value>)> {
    if req.from_class_id == req.to_class_id {
        return Err(ApiError::BadRequest("交出方与接收方不能是同一班级".into()));
    }
    // 班主任只能替自己班申请调入
    if !user.is_logistics() && user.class_id != Some(req.to_class_id) {
        return Err(ApiError::Forbidden);
    }

    for cid in [req.from_class_id, req.to_class_id] {
        let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM classes WHERE id=?")
            .bind(cid)
            .fetch_optional(&state.pool)
            .await?;
        if exists.is_none() {
            return Err(ApiError::NotFound(format!("班级 {cid} 不存在")));
        }
    }

    let asset: Option<(String, String, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT code, status, location_class_id, assigned_student_id FROM assets WHERE code=?",
    )
    .bind(&req.asset_code)
    .fetch_optional(&state.pool)
    .await?;
    let Some((code, status, location, assigned)) = asset else {
        return Err(ApiError::NotFound("桌椅编号不存在".into()));
    };
    if status == "damaged" {
        return Err(ApiError::Conflict("损坏桌椅不能调拨".into()));
    }
    if assigned.is_some() {
        return Err(ApiError::Conflict("桌椅仍被学生占用，请先收回再调拨".into()));
    }
    if location != Some(req.from_class_id) {
        let loc_name = location.map(|c| c.to_string()).unwrap_or_else(|| "后勤仓库".into());
        return Err(ApiError::Conflict(format!(
            "该桌椅当前不在交出方班级（实际位置：{loc_name}）"
        )));
    }
    let active: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM transfers WHERE asset_code=? AND status IN ('pending','released')",
    )
    .bind(&code)
    .fetch_optional(&state.pool)
    .await?;
    if active.is_some() {
        return Err(ApiError::Conflict("该桌椅已有在途调拨单".into()));
    }

    let result = sqlx::query(
        "INSERT INTO transfers (asset_code, from_class_id, to_class_id, requested_by, created_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))",
    )
    .bind(&code)
    .bind(req.from_class_id)
    .bind(req.to_class_id)
    .bind(user.id)
    .execute(&state.pool)
    .await;
    let inserted = match result {
        Ok(r) => r,
        Err(e) => return Err(conflict_on_unique(e, "该桌椅已有在途调拨单，发起失败")),
    };
    let transfer_id = inserted.last_insert_rowid();

    sqlx::query(
        "INSERT INTO asset_history (asset_code, event_type, from_class_id, to_class_id,
                                    actor_user_id, detail, created_at)
         VALUES (?1,'transfer_requested',?2,?3,?4,'发起跨班调拨，等待交出方确认',datetime('now'))",
    )
    .bind(&code)
    .bind(req.from_class_id)
    .bind(req.to_class_id)
    .bind(user.id)
    .execute(&state.pool)
    .await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "ok": true, "transfer_id": transfer_id, "status": "pending",
            "notice": "调拨单已创建：需交出方放行、接收方确认两步完成，期间资产位置不变",
        })),
    ))
}

#[derive(sqlx::FromRow)]
struct TransferRow {
    id: i64,
    asset_code: String,
    from_class_id: i64,
    to_class_id: i64,
    status: String,
    created_at: String,
    released_at: Option<String>,
    confirmed_at: Option<String>,
    rejected_at: Option<String>,
    from_name: String,
    to_name: String,
}

const LIST_SELECT: &str = r#"SELECT t.id, t.asset_code, t.from_class_id, t.to_class_id, t.status,
           t.created_at, t.released_at, t.confirmed_at, t.rejected_at,
           cf.name AS from_name, ct.name AS to_name
    FROM transfers t
    JOIN classes cf ON cf.id = t.from_class_id
    JOIN classes ct ON ct.id = t.to_class_id"#;

/// 调拨单列表。后勤看全部；班主任只看与本班相关的。
pub async fn list(user: AuthUser, State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let rows: Vec<TransferRow> = if user.is_logistics() {
        sqlx::query_as(&format!("{LIST_SELECT} ORDER BY t.id DESC"))
            .fetch_all(&state.pool)
            .await?
    } else if let Some(cid) = user.class_id {
        sqlx::query_as(&format!(
            "{LIST_SELECT} WHERE t.from_class_id=?1 OR t.to_class_id=?1 ORDER BY t.id DESC"
        ))
        .bind(cid)
        .fetch_all(&state.pool)
        .await?
    } else {
        vec![]
    };
    let transfers: Vec<Value> = rows
        .iter()
        .map(|t| {
            json!({
                "id": t.id, "asset_code": t.asset_code,
                "from_class_id": t.from_class_id, "from_class_name": t.from_name,
                "to_class_id": t.to_class_id, "to_class_name": t.to_name,
                "status": t.status, "created_at": t.created_at,
                "released_at": t.released_at, "confirmed_at": t.confirmed_at,
                "rejected_at": t.rejected_at,
            })
        })
        .collect();
    Ok(Json(json!({ "transfers": transfers })))
}

async fn load_active(state: &AppState, id: i64) -> ApiResult<(String, i64, i64, String)> {
    sqlx::query_as(
        "SELECT asset_code, from_class_id, to_class_id, status FROM transfers WHERE id=?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("调拨单不存在".into()))
}

/// 第一步：交出方放行。只有交出班班主任或后勤可操作。
pub async fn release(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let (code, from_class, to_class, status) = load_active(&state, id).await?;
    if !user.is_logistics() && user.class_id != Some(from_class) {
        return Err(ApiError::Forbidden);
    }
    if status != "pending" {
        return Err(ApiError::Conflict(format!("调拨单当前状态为 {status}，不能放行")));
    }

    let mut tx = crate::db::ImmediateTx::begin(&state.pool).await?;
    let res = sqlx::query(
        "UPDATE transfers SET status='released', released_at=datetime('now'), released_by=?1
         WHERE id=?2 AND status='pending'",
    )
    .bind(user.id)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() != 1 {
        return Err(ApiError::Conflict("调拨单状态已变化，放行失败".into()));
    }
    sqlx::query(
        "INSERT INTO asset_history (asset_code, event_type, from_class_id, to_class_id,
                                    actor_user_id, detail, created_at)
         VALUES (?1,'transfer_released',?2,?3,?4,'交出方已放行，等待接收方确认；资产仍在原班',datetime('now'))",
    )
    .bind(&code).bind(from_class).bind(to_class).bind(user.id)
    .execute(&mut *tx).await?;
    tx.commit().await?;

    Ok(Json(json!({"ok": true, "transfer_id": id, "status": "released",
                   "notice": "已放行，资产仍留在原班；接收方确认后才更新资产位置"})))
}

/// 第二步：接收方确认。只有接收班班主任或后勤可操作。
/// 双方确认齐备的此刻，才在同一事务内把 assets.location_class_id 更新到接收班。
pub async fn confirm(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let (code, from_class, to_class, status) = load_active(&state, id).await?;
    if !user.is_logistics() && user.class_id != Some(to_class) {
        return Err(ApiError::Forbidden);
    }
    if status != "released" {
        return Err(ApiError::Conflict(format!(
            "调拨单当前状态为 {status}：必须先由交出方放行，接收方才能确认"
        )));
    }

    let mut tx = crate::db::ImmediateTx::begin(&state.pool).await?;
    // 条件更新：重复确认/并发确认只有一次成功
    let res = sqlx::query(
        "UPDATE transfers SET status='confirmed', confirmed_at=datetime('now'),
                              confirmed_by=?1, completed_at=datetime('now')
         WHERE id=?2 AND status='released'",
    )
    .bind(user.id)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() != 1 {
        return Err(ApiError::Conflict("调拨单状态已变化，确认失败".into()));
    }

    // 双方确认完成——这是整个流程中唯一更新资产位置的地方
    let moved = sqlx::query(
        "UPDATE assets SET location_class_id=?1, updated_at=datetime('now')
         WHERE code=?2 AND location_class_id=?3",
    )
    .bind(to_class)
    .bind(&code)
    .bind(from_class)
    .execute(&mut *tx)
    .await?;
    if moved.rows_affected() != 1 {
        // 资产在 released 期间被异常改动则回滚，保证账实一致
        return Err(ApiError::Conflict("资产位置与调拨单不符，已中止，请后勤核查".into()));
    }

    sqlx::query(
        "INSERT INTO asset_history (asset_code, event_type, from_class_id, to_class_id,
                                    actor_user_id, detail, created_at)
         VALUES (?1,'transfer_confirmed',?2,?3,?4,'接收方已确认，资产位置更新至接收班',datetime('now'))",
    )
    .bind(&code).bind(from_class).bind(to_class).bind(user.id)
    .execute(&mut *tx).await?;
    tx.commit().await?;

    Ok(Json(json!({"ok": true, "transfer_id": id, "status": "confirmed",
                   "asset_code": code, "location_class_id": to_class,
                   "notice": "双方确认完成，资产位置已更新"})))
}

/// 拒绝/作废：pending 时双方可拒；released 后仅接收方可拒收。资产始终留在交出方。
pub async fn reject(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let (code, from_class, to_class, status) = load_active(&state, id).await?;
    let is_party = user.is_logistics()
        || user.class_id == Some(from_class)
        || user.class_id == Some(to_class);
    if !is_party {
        return Err(ApiError::Forbidden);
    }
    match status.as_str() {
        "pending" => {}
        "released" => {
            if !user.is_logistics() && user.class_id != Some(to_class) {
                return Err(ApiError::Forbidden);
            }
        }
        other => return Err(ApiError::Conflict(format!("调拨单状态为 {other}，不能拒绝"))),
    }

    let detail = if status == "released" {
        "接收方拒收，资产留在交出班"
    } else {
        "调拨被拒绝，资产留在交出班"
    };
    let mut tx = crate::db::ImmediateTx::begin(&state.pool).await?;
    let res = sqlx::query(
        "UPDATE transfers SET status='rejected', rejected_at=datetime('now'), rejected_by=?1
         WHERE id=?2 AND status IN ('pending','released')",
    )
    .bind(user.id)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() != 1 {
        return Err(ApiError::Conflict("调拨单状态已变化，操作失败".into()));
    }
    sqlx::query(
        "INSERT INTO asset_history (asset_code, event_type, from_class_id, to_class_id,
                                    actor_user_id, detail, created_at)
         VALUES (?1,'transfer_rejected',?2,?3,?4,?5,datetime('now'))",
    )
    .bind(&code)
    .bind(from_class)
    .bind(to_class)
    .bind(user.id)
    .bind(detail)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(json!({"ok": true, "transfer_id": id, "status": "rejected",
                   "notice": "调拨已作废，资产位置从未改变"})))
}
