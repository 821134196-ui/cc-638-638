use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::matching;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

async fn load_student(state: &AppState, id: i64) -> ApiResult<(i64, String, i64, i64)> {
    sqlx::query_as("SELECT id, name, class_id, height_cm FROM students WHERE id=?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::NotFound("学生不存在".into()))
}

pub async fn get_student(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let (sid, name, class_id, height) = load_student(&state, id).await?;
    // 班主任只能看本班学生
    user.ensure_class_access(class_id)?;

    let asset: Option<(String, String, String)> = sqlx::query_as(
        "SELECT code, model_code, status FROM assets WHERE assigned_student_id=?",
    )
    .bind(sid)
    .fetch_optional(&state.pool)
    .await?;

    Ok(Json(json!({
        "id": sid, "name": name, "class_id": class_id, "height_cm": height,
        "asset": asset.as_ref().map(|(code, model, status)| json!({"code": code, "model": model, "status": status})),
        "mismatch": matching::evaluate(height, asset.as_ref().map(|a| a.1.as_str())),
    })))
}

#[derive(Deserialize)]
pub struct HeightReq {
    height_cm: i64,
}

/// 班主任更新本班学生身高（后勤也可代录）。只更新身高并留痕，不产生任何资产变动。
pub async fn update_height(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HeightReq>,
) -> ApiResult<Json<Value>> {
    if !(80..=220).contains(&req.height_cm) {
        return Err(ApiError::BadRequest("身高需在 80~220cm 之间".into()));
    }
    let (sid, _name, class_id, old_height) = load_student(&state, id).await?;
    user.ensure_class_access(class_id)?;

    let mut tx = crate::db::ImmediateTx::begin(&state.pool).await?;
    sqlx::query("UPDATE students SET height_cm=?1, updated_at=datetime('now') WHERE id=?2")
        .bind(req.height_cm)
        .bind(sid)
        .execute(&mut *tx)
        .await?;

    if let Some((asset_code,)) = sqlx::query_as::<_, (String,)>(
        "SELECT code FROM assets WHERE assigned_student_id=?",
    )
    .bind(sid)
    .fetch_optional(&mut *tx)
    .await?
    {
        sqlx::query(
            "INSERT INTO asset_history (asset_code, event_type, from_student_id, to_student_id,
                                        actor_user_id, detail, created_at)
             VALUES (?1,'height_update',?2,?2,?3,?4,datetime('now'))",
        )
        .bind(asset_code)
        .bind(sid)
        .bind(user.id)
        .bind(format!("身高 {old_height}cm → {}cm（建议待核实，未改动资产）", req.height_cm))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(Json(json!({
        "ok": true,
        "needed_model": matching::recommend_model(req.height_cm),
        "notice": "身高已更新，系统仅生成匹配建议，资产归属不变",
    })))
}

pub async fn history(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let (_sid, _name, class_id, _h) = load_student(&state, id).await?;
    user.ensure_class_access(class_id)?;
    history_json(&state, id).await
}

pub(crate) async fn history_json(state: &AppState, student_id: i64) -> ApiResult<Json<Value>> {
    let rows: Vec<(i64, Option<String>, String, Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<String>, String)> =
        sqlx::query_as(
            r#"SELECT id, asset_code, event_type, from_class_id, to_class_id,
                      from_student_id, to_student_id, detail, created_at
               FROM asset_history
               WHERE from_student_id = ?1 OR to_student_id = ?1
               ORDER BY id DESC, created_at DESC"#,
        )
        .bind(student_id)
        .fetch_all(&state.pool)
        .await?;
    let events: Vec<Value> = rows
        .iter()
        .map(|(id, code, ty, fc, tc, fs, ts, detail, at)| {
            json!({"id": id, "asset_code": code, "event_type": ty,
                   "from_class_id": fc, "to_class_id": tc,
                   "from_student_id": fs, "to_student_id": ts,
                   "detail": detail, "created_at": at})
        })
        .collect();
    Ok(Json(json!({ "history": events })))
}

#[derive(Deserialize)]
pub struct TransferClassReq {
    class_id: i64,
}

/// 学生换班（后勤办理）：只改学生所属班级；其桌椅仍留在原班、原分配履历完整保留，
/// 不会因为换班或系统建议而把资产算到另一个班。
pub async fn transfer_class(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<TransferClassReq>,
) -> ApiResult<Json<Value>> {
    if !user.is_logistics() {
        return Err(ApiError::Forbidden);
    }
    let (sid, _name, old_class, _h) = load_student(&state, id).await?;
    if old_class == req.class_id {
        return Err(ApiError::BadRequest("学生已在该班级".into()));
    }
    let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM classes WHERE id=?")
        .bind(req.class_id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(ApiError::NotFound("目标班级不存在".into()));
    }

    let mut tx = crate::db::ImmediateTx::begin(&state.pool).await?;
    sqlx::query("UPDATE students SET class_id=?1, updated_at=datetime('now') WHERE id=?2")
        .bind(req.class_id)
        .bind(sid)
        .execute(&mut *tx)
        .await?;

    // 对其名下资产记一条换班履历；资产 location_class_id 不动
    let assets: Vec<(String,)> =
        sqlx::query_as("SELECT code FROM assets WHERE assigned_student_id=?")
            .bind(sid)
            .fetch_all(&mut *tx)
            .await?;
    for (code,) in assets {
        sqlx::query(
            "INSERT INTO asset_history (asset_code, event_type, from_class_id, to_class_id,
                                        from_student_id, to_student_id, actor_user_id, detail, created_at)
             VALUES (?1,'student_transfer',?2,?3,?4,?4,?5,'学生换班，桌椅暂留原班',datetime('now'))",
        )
        .bind(&code)
        .bind(old_class)
        .bind(req.class_id)
        .bind(sid)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(Json(json!({"ok": true, "student_id": sid, "class_id": req.class_id,
                   "notice": "换班已登记；资产位置与原分配记录均未改动"})))
}

/// 收回学生占用的桌椅（后勤在重新调配前办理）：只追加履历，历史分配记录保留。
pub async fn unassign(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    if !user.is_logistics() {
        return Err(ApiError::Forbidden);
    }
    let (sid, _name, _class_id, _h) = load_student(&state, id).await?;

    let mut tx = crate::db::ImmediateTx::begin(&state.pool).await?;
    let asset: Option<(String,)> =
        sqlx::query_as("SELECT code FROM assets WHERE assigned_student_id=?")
            .bind(sid)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((code,)) = asset else {
        return Err(ApiError::BadRequest("该学生当前没有占用桌椅".into()));
    };
    let res = sqlx::query(
        "UPDATE assets SET status='available', assigned_student_id=NULL, updated_at=datetime('now')
         WHERE code=?1 AND assigned_student_id=?2",
    )
    .bind(&code)
    .bind(sid)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() != 1 {
        return Err(ApiError::Conflict("资产状态已变化，收回失败".into()));
    }
    sqlx::query(
        "INSERT INTO asset_history (asset_code, event_type, from_student_id, actor_user_id,
                                    detail, created_at)
         VALUES (?1,'unassign',?2,?3,'后勤收回，等待重新分配',datetime('now'))",
    )
    .bind(&code)
    .bind(sid)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(json!({"ok": true, "asset_code": code})))
}
