use crate::auth::LogisticsUser;
use crate::error::{ApiError, ApiResult};
use crate::matching;
use crate::state::AppState;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(sqlx::FromRow)]
struct StudentRow {
    id: i64,
    name: String,
    class_id: i64,
    class_name: String,
    height_cm: i64,
    asset_code: Option<String>,
    asset_model: Option<String>,
}

const SELECT_STUDENTS: &str = r#"SELECT s.id, s.name, s.class_id, c.name AS class_name, s.height_cm,
           a.code AS asset_code, a.model_code AS asset_model
    FROM students s
    JOIN classes c ON c.id = s.class_id
    LEFT JOIN assets a ON a.assigned_student_id = s.id
    ORDER BY c.grade, c.id, s.id"#;

async fn all_students(state: &AppState) -> ApiResult<Vec<StudentRow>> {
    Ok(sqlx::query_as(SELECT_STUDENTS).fetch_all(&state.pool).await?)
}

/// 全校不匹配学生清单（后勤视角）。只读。
pub async fn mismatches(
    _user: LogisticsUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Value>> {
    let rows = all_students(&state).await?;
    let list: Vec<Value> = rows
        .iter()
        .filter_map(|r| {
            matching::evaluate(r.height_cm, r.asset_model.as_deref()).map(|m| {
                json!({
                    "student_id": r.id, "student_name": r.name,
                    "class_id": r.class_id, "class_name": r.class_name,
                    "height_cm": r.height_cm,
                    "current_asset_code": r.asset_code,
                    "current_model": m.current_model,
                    "needed_model": m.needed_model,
                    "kind": m.kind, "message": m.message,
                })
            })
        })
        .collect();
    Ok(Json(json!({ "mismatches": list, "count": list.len() })))
}

/// 后勤总览数字
pub async fn overview(
    _user: LogisticsUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Value>> {
    let (students,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM students")
        .fetch_one(&state.pool)
        .await?;
    let (in_use,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM assets WHERE status='in_use'")
        .fetch_one(&state.pool)
        .await?;
    let (available,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM assets WHERE status='available'")
            .fetch_one(&state.pool)
            .await?;
    let (damaged,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM assets WHERE status='damaged'")
        .fetch_one(&state.pool)
        .await?;
    let (active_transfers,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM transfers WHERE status IN ('pending','released')",
    )
    .fetch_one(&state.pool)
    .await?;

    let rows = all_students(&state).await?;
    let mismatch_count = rows
        .iter()
        .filter(|r| matching::evaluate(r.height_cm, r.asset_model.as_deref()).is_some())
        .count() as i64;

    Ok(Json(json!({
        "students": students, "in_use": in_use, "available": available,
        "damaged": damaged, "active_transfers": active_transfers,
        "mismatch_count": mismatch_count,
    })))
}

/// 匹配建议：为每个不匹配学生列出当前可直接使用的空闲资产。
/// 关键：本接口纯查询，不预留、不锁定、不改变任何资产归属或位置；
/// 真正归属变更只可能发生在 /allocations（核实后领用）或调拨双方确认完成时。
pub async fn suggestions(
    _user: LogisticsUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Value>> {
    let students = all_students(&state).await?;

    #[derive(sqlx::FromRow)]
    struct Cand {
        code: String,
        model_code: String,
        location_class_id: Option<i64>,
        location_name: Option<String>,
    }
    // 可分配池：status=available 且没有在途调拨。损坏品被 status 条件排除。
    let pool_rows: Vec<Cand> = sqlx::query_as(
        r#"SELECT a.code, a.model_code, a.location_class_id, c.name AS location_name
           FROM assets a LEFT JOIN classes c ON c.id = a.location_class_id
           WHERE a.status='available'
             AND NOT EXISTS (
                 SELECT 1 FROM transfers t
                 WHERE t.asset_code = a.code AND t.status IN ('pending','released'))
           ORDER BY a.code"#,
    )
    .fetch_all(&state.pool)
    .await?;

    let mut suggestions = Vec::new();
    for r in &students {
        if let Some(m) = matching::evaluate(r.height_cm, r.asset_model.as_deref()) {
            let same_class: Vec<&Cand> = pool_rows
                .iter()
                .filter(|a| a.model_code == m.needed_model && a.location_class_id == Some(r.class_id))
                .collect();
            let elsewhere: Vec<&Cand> = pool_rows
                .iter()
                .filter(|a| a.model_code == m.needed_model && a.location_class_id != Some(r.class_id))
                .collect();
            let to_json = |a: &Cand| {
                json!({"asset_code": a.code, "model": a.model_code,
                       "location_class_id": a.location_class_id,
                       "location_name": a.location_name.clone().unwrap_or_else(|| "后勤仓库".into()),
                       "requires_transfer": a.location_class_id.is_some()
                                            && a.location_class_id != Some(r.class_id)})
            };
            suggestions.push(json!({
                "student_id": r.id, "student_name": r.name,
                "class_id": r.class_id, "class_name": r.class_name,
                "height_cm": r.height_cm,
                "current_asset_code": r.asset_code,
                "kind": m.kind, "needed_model": m.needed_model, "message": m.message,
                "assets_in_class": same_class.iter().map(|a| to_json(a)).collect::<Vec<_>>(),
                "assets_elsewhere": elsewhere.iter().map(|a| to_json(a)).collect::<Vec<_>>(),
                "advisory": "建议仅供参考，未占用任何资产；请线下核实后再分配或发起调拨",
            }));
        }
    }
    Ok(Json(json!({ "suggestions": suggestions, "readonly": true })))
}

#[derive(Deserialize)]
pub struct AllocReq {
    student_id: i64,
    asset_code: String,
}

/// 后勤核实后领用/分配一套桌椅给学生。
/// 并发安全：条件 UPDATE（… AND assigned_student_id IS NULL AND status='available'）
/// 是序列化点——多人同时领用同一套桌椅时，只有一个事务 rows_affected=1，其余返回 409。
/// assets.assigned_student_id 的 UNIQUE 索引是第二道防线（一个学生也只能占一套）。
pub async fn create_allocation(
    user: LogisticsUser,
    State(state): State<AppState>,
    Json(req): Json<AllocReq>,
) -> ApiResult<(axum::http::StatusCode, Json<Value>)> {
    let student: Option<(i64, String, i64, i64)> = sqlx::query_as(
        "SELECT id, name, class_id, height_cm FROM students WHERE id=?",
    )
    .bind(req.student_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((sid, sname, class_id, height)) = student else {
        return Err(ApiError::NotFound("学生不存在".into()));
    };

    let asset: Option<(String, String, String, Option<i64>)> = sqlx::query_as(
        "SELECT code, model_code, status, location_class_id FROM assets WHERE code=?",
    )
    .bind(&req.asset_code)
    .fetch_optional(&state.pool)
    .await?;
    let Some((code, model, status, location)) = asset else {
        return Err(ApiError::NotFound("桌椅编号不存在".into()));
    };
    if status == "damaged" {
        return Err(ApiError::Conflict("该桌椅已损坏，不能分配".into()));
    }

    let mut tx = crate::db::ImmediateTx::begin(&state.pool).await?;

    // 事务内复核给出清晰报错（最终防线是条件 UPDATE 与 UNIQUE 索引）
    let active_transfer: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM transfers WHERE asset_code=? AND status IN ('pending','released')",
    )
    .bind(&code)
    .fetch_optional(&mut *tx)
    .await?;
    if active_transfer.is_some() {
        return Err(ApiError::Conflict("该桌椅正在跨班调拨流程中，暂不能分配".into()));
    }

    let existing: Option<(String,)> =
        sqlx::query_as("SELECT code FROM assets WHERE assigned_student_id=?")
            .bind(sid)
            .fetch_optional(&mut *tx)
            .await?;
    if existing.is_some() {
        return Err(ApiError::Conflict(
            "该学生已占用一套桌椅，请先收回旧桌椅再分配".into(),
        ));
    }

    // 序列化点：仅当仍为空闲且无人占用时才能成功
    let res = sqlx::query(
        "UPDATE assets
            SET assigned_student_id = ?1, status='in_use', updated_at=datetime('now')
          WHERE code = ?2
            AND assigned_student_id IS NULL
            AND status = 'available'",
    )
    .bind(sid)
    .bind(&code)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() != 1 {
        // 并发下另一个请求抢先成功，或资产在此期间损坏/进入调拨
        return Err(ApiError::Conflict(
            "该桌椅刚被其他人领用或状态已变化，分配未成功".into(),
        ));
    }

    sqlx::query(
        "INSERT INTO asset_history (asset_code, event_type, from_class_id, to_class_id,
                                    to_student_id, actor_user_id, detail, created_at)
         VALUES (?1,'assign',?2,?3,?4,?5,?6,datetime('now'))",
    )
    .bind(&code)
    .bind(location)
    .bind(class_id)
    .bind(sid)
    .bind(user.0.id)
    .bind(format!("分配给{sname}（{model}）"))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let needed = matching::recommend_model(height);
    let fit = model == needed;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "ok": true, "asset_code": code, "student_id": sid,
            "model_match": fit,
            "warning": if fit { Value::Null } else {
                json!(format!("注意：分配的是 {model}，按身高建议 {needed}，请确认线下核实结果"))
            },
            "notice": "分配已登记，资产归属已更新；建议页本身不会产生任何归属变化",
        })),
    ))
}
