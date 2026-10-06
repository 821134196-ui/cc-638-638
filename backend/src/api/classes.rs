use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::matching;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};
use serde::Serialize;

#[derive(Serialize, sqlx::FromRow)]
struct ClassRow {
    id: i64,
    name: String,
    grade: i64,
}

pub async fn list_classes(user: AuthUser, State(state): State<AppState>) -> ApiResult<Json<Value>> {
    // 班主任只返回自己班；后勤返回全部
    let classes: Vec<ClassRow> = if user.is_logistics() {
        sqlx::query_as("SELECT id, name, grade FROM classes ORDER BY grade, id")
            .fetch_all(&state.pool)
            .await?
    } else if let Some(cid) = user.class_id {
        sqlx::query_as("SELECT id, name, grade FROM classes WHERE id=? ORDER BY id")
            .bind(cid)
            .fetch_all(&state.pool)
            .await?
    } else {
        vec![]
    };
    Ok(Json(json!({ "classes": classes })))
}

pub async fn list_models(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let models: Vec<(i64, String, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT model_no, code, height_min_cm, height_max_cm, desk_height_cm, chair_height_cm
         FROM desk_models ORDER BY model_no",
    )
    .fetch_all(&state.pool)
    .await?;
    let models: Vec<Value> = models
        .iter()
        .map(|(no, code, hmin, hmax, dh, ch)| {
            json!({"model_no": no, "code": code, "height_min_cm": hmin, "height_max_cm": hmax,
                   "desk_height_cm": dh, "chair_height_cm": ch})
        })
        .collect();
    Ok(Json(json!({ "models": models })))
}

#[derive(sqlx::FromRow)]
struct StudentAssetRow {
    id: i64,
    name: String,
    class_id: i64,
    height_cm: i64,
    updated_at: String,
    asset_code: Option<String>,
    asset_model: Option<String>,
}

async fn fetch_class_students(state: &AppState, class_id: i64) -> ApiResult<Vec<StudentAssetRow>> {
    let rows = sqlx::query_as(
        r#"SELECT s.id, s.name, s.class_id, s.height_cm, s.updated_at,
                  a.code AS asset_code, a.model_code AS asset_model
           FROM students s
           LEFT JOIN assets a ON a.assigned_student_id = s.id
           WHERE s.class_id = ?1
           ORDER BY s.id"#,
    )
    .bind(class_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}

/// 班级学生清单（带匹配状态）。班主任只能访问本班——权限在本函数内强制。
pub async fn list_students(
    user: AuthUser,
    State(state): State<AppState>,
    Path(class_id): Path<i64>,
) -> ApiResult<Json<Value>> {
    user.ensure_class_access(class_id)?;
    let rows = fetch_class_students(&state, class_id).await?;
    let students: Vec<Value> = rows
        .iter()
        .map(|r| {
            let mismatch = matching::evaluate(r.height_cm, r.asset_model.as_deref());
            json!({
                "id": r.id, "name": r.name, "class_id": r.class_id,
                "height_cm": r.height_cm, "updated_at": r.updated_at,
                "asset_code": r.asset_code, "asset_model": r.asset_model,
                "match_status": if mismatch.is_none() && r.asset_code.is_some() { "matched" }
                                else if r.asset_code.is_none() { "unassigned" }
                                else { mismatch.as_ref().unwrap().kind.as_str() },
                "mismatch": mismatch,
            })
        })
        .collect();
    Ok(Json(json!({ "students": students })))
}

/// 班级页面：待调换 / 已分配 / 损坏 / 本班空闲
pub async fn dashboard(
    user: AuthUser,
    State(state): State<AppState>,
    Path(class_id): Path<i64>,
) -> ApiResult<Json<Value>> {
    user.ensure_class_access(class_id)?;

    let class: Option<(String, i64)> =
        sqlx::query_as("SELECT name, grade FROM classes WHERE id=?")
            .bind(class_id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((class_name, grade)) = class else {
        return Err(crate::error::ApiError::NotFound("班级不存在".into()));
    };

    // 待调换：身高与现占型号不符，或没有桌椅
    let rows = fetch_class_students(&state, class_id).await?;
    let mut pending = Vec::new();
    let mut assigned = Vec::new();
    for r in &rows {
        let mismatch = matching::evaluate(r.height_cm, r.asset_model.as_deref());
        let entry = json!({
            "student_id": r.id, "student_name": r.name, "height_cm": r.height_cm,
            "asset_code": r.asset_code, "asset_model": r.asset_model,
            "updated_at": r.updated_at,
        });
        if let Some(m) = mismatch {
            pending.push(json!({
                "student_id": r.id, "student_name": r.name, "height_cm": r.height_cm,
                "updated_at": r.updated_at,
                "current_asset_code": r.asset_code,
                "current_model": r.asset_model,
                "needed_model": m.needed_model,
                "kind": m.kind, "message": m.message,
            }));
        } else if r.asset_code.is_some() {
            assigned.push(entry);
        }
    }

    #[derive(sqlx::FromRow)]
    struct AssetRow {
        code: String,
        model_code: String,
        status: String,
        student_name: Option<String>,
        student_id: Option<i64>,
    }
    let assets: Vec<AssetRow> = sqlx::query_as(
        r#"SELECT a.code, a.model_code, a.status,
                  s.name AS student_name, a.assigned_student_id AS student_id
           FROM assets a LEFT JOIN students s ON s.id = a.assigned_student_id
           WHERE a.location_class_id = ?1
           ORDER BY a.status, a.code"#,
    )
    .bind(class_id)
    .fetch_all(&state.pool)
    .await?;

    let mut damaged = Vec::new();
    let mut available = Vec::new();
    let mut in_use = Vec::new();
    for a in assets {
        let v = json!({"code": a.code, "model": a.model_code, "status": a.status,
                       "student_id": a.student_id, "student_name": a.student_name});
        match a.status.as_str() {
            "damaged" => damaged.push(v),
            "available" => available.push(v),
            _ => in_use.push(v),
        }
    }

    Ok(Json(json!({
        "class": { "id": class_id, "name": class_name, "grade": grade },
        "pending_swap": pending,
        "assigned": assigned,
        "in_use": in_use,
        "available": available,
        "damaged": damaged,
    })))
}
