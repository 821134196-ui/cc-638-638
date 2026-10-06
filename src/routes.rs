use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::state::AppState;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ============ 认证 ============

#[derive(Deserialize)]
pub struct LoginReq {
    username: String,
    password: String,
}

pub async fn login(
    State(st): State<AppState>,
    Json(req): Json<LoginReq>,
) -> AppResult<Json<Value>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?")
        .bind(&req.username)
        .fetch_optional(&st.pool)
        .await?
        .ok_or_else(|| AppError::unauthorized("用户名或密码错误"))?;
    if !crate::auth::verify_password(&req.password, &user.password_hash) {
        return Err(AppError::unauthorized("用户名或密码错误"));
    }
    let token = crate::auth::issue_token(&st.secret, user.id);
    Ok(Json(json!({ "token": token, "user": user })))
}

pub async fn me(user: User) -> Json<User> {
    Json(user)
}

// ============ 基础数据 ============

pub async fn list_classes(State(st): State<AppState>) -> AppResult<Json<Vec<Class>>> {
    Ok(Json(
        sqlx::query_as::<_, Class>("SELECT id, name FROM classes ORDER BY id")
            .fetch_all(&st.pool)
            .await?,
    ))
}

pub async fn list_models(State(st): State<AppState>) -> AppResult<Json<Vec<DeskModel>>> {
    Ok(Json(
        sqlx::query_as::<_, DeskModel>(
            "SELECT code,name,height_min,height_max,desk_cm,chair_cm \
             FROM desk_models ORDER BY CAST(code AS INTEGER)",
        )
        .fetch_all(&st.pool)
        .await?,
    ))
}

// ============ 学生与身高 ============

/// 班主任只能看到本班学生；后勤可看任意班
pub async fn list_students(
    State(st): State<AppState>,
    user: User,
    Path(class_id): Path<i64>,
) -> AppResult<Json<Vec<Student>>> {
    user.ensure_class(class_id)?;
    let students = sqlx::query_as::<_, Student>(
        "SELECT * FROM students WHERE class_id = ? AND active = 1 ORDER BY id",
    )
    .bind(class_id)
    .fetch_all(&st.pool)
    .await?;
    Ok(Json(students))
}

#[derive(Deserialize)]
pub struct HeightReq {
    height_cm: i64,
}

/// 班主任更新本班学生身高。只产生/刷新“建议”，绝不改动任何资产。
pub async fn update_height(
    State(st): State<AppState>,
    user: User,
    Path(student_id): Path<i64>,
    Json(req): Json<HeightReq>,
) -> AppResult<Json<Value>> {
    if !(100..=180).contains(&req.height_cm) {
        return Err(AppError::bad_request("身高应在 100~180 cm 之间"));
    }
    let student = sqlx::query_as::<_, Student>("SELECT * FROM students WHERE id = ?")
        .bind(student_id)
        .fetch_optional(&st.pool)
        .await?
        .ok_or_else(|| AppError::not_found("学生不存在"))?;
    user.ensure_class(student.class_id)?;

    let desired = desired_model(req.height_cm);

    // 当前在用桌椅型号
    let current = sqlx::query_as::<_, (String, String)>(
        "SELECT a.code, a.model FROM assignments asg \
         JOIN assets a ON a.code = asg.asset_code \
         WHERE asg.student_id = ? AND asg.status = 'active' LIMIT 1",
    )
    .bind(student_id)
    .fetch_optional(&st.pool)
    .await?;

    let mut tx = st.pool.begin().await?;

    sqlx::query(
        "UPDATE students SET height_cm = ?, desired_model = ?, updated_at = ? WHERE id = ?",
    )
    .bind(req.height_cm)
    .bind(desired)
    .bind(now())
    .bind(student_id)
    .execute(&mut *tx)
    .await?;

    // 该生旧的待处理建议全部作废，再按最新身高重新判断
    sqlx::query(
        "UPDATE suggestions SET status='obsolete', resolved_at=? \
         WHERE student_id=? AND status='open'",
    )
    .bind(now())
    .bind(student_id)
    .execute(&mut *tx)
    .await?;

    let (reason, need) = match &current {
        Some((code, model)) if model == desired => (String::new(), false),
        Some((code, model)) => (
            format!("身高 {}cm 应配 {} 号桌，现用 {}（{}）不符",
                req.height_cm, desired, code, model),
            true,
        ),
        None => (
            format!("身高 {}cm，尚未分配桌椅，建议 {} 号桌", req.height_cm, desired),
            true,
        ),
    };

    let mut suggested = false;
    if need {
        sqlx::query(
            "INSERT INTO suggestions(student_id,model,reason,status,created_at) \
             VALUES (?,?,?,'open',?)",
        )
        .bind(student_id)
        .bind(desired)
        .bind(&reason)
        .bind(now())
        .execute(&mut *tx)
        .await?;
        suggested = true;
    }

    tx.commit().await?;
    Ok(Json(json!({
        "ok": true,
        "height_cm": req.height_cm,
        "desired_model": desired,
        "suggestion_created": suggested,
    })))
}

/// 后勤：全量重算建议（只写 suggestions 表，不移动任何资产）
pub async fn generate_suggestions(
    State(st): State<AppState>,
    user: User,
) -> AppResult<Json<Value>> {
    user.ensure_logistics()?;

    let students = sqlx::query_as::<_, Student>(
        "SELECT * FROM students WHERE active = 1 AND height_cm IS NOT NULL",
    )
    .fetch_all(&st.pool)
    .await?;

    let mut created = 0i64;
    let mut obsolete = 0i64;
    let mut open_items: Vec<Value> = Vec::new();
    let ts = now();

    for s in &students {
        let height = s.height_cm.unwrap();
        let desired = desired_model(height).to_string();

        let current = sqlx::query_as::<_, (String, String)>(
            "SELECT a.code, a.model FROM assignments asg \
             JOIN assets a ON a.code = asg.asset_code \
             WHERE asg.student_id = ? AND asg.status='active' LIMIT 1",
        )
        .bind(s.id)
        .fetch_optional(&st.pool)
        .await?;

        let matched = matches!(&current, Some((_, m)) if *m == desired);

        let mut tx = st.pool.begin().await?;
        sqlx::query(
            "UPDATE students SET desired_model = ? WHERE id = ?",
        )
        .bind(&desired)
        .bind(s.id)
        .execute(&mut *tx)
        .await?;

        // 已无问题的待处理建议 → obsolete
        if matched {
            let r = sqlx::query(
                "UPDATE suggestions SET status='obsolete', resolved_at=? \
                 WHERE student_id=? AND status='open'",
            )
            .bind(&ts)
            .bind(s.id)
            .execute(&mut *tx)
            .await?;
            obsolete += r.rows_affected() as i64;
            tx.commit().await?;
            continue;
        }

        let reason = match &current {
            Some((code, model)) => format!(
                "身高 {}cm 应配 {} 号桌，现用 {}（{} 号）不符", height, desired, code, model
            ),
            None => format!("身高 {}cm，尚未分配桌椅，建议 {} 号桌", height, desired),
        };

        // 已有同型号 open 建议则不重复生成
        let exists: i64 = sqlx::query(
            "SELECT COUNT(*) FROM suggestions WHERE student_id=? AND model=? AND status='open'",
        )
        .bind(s.id)
        .bind(&desired)
        .fetch_one(&st.pool)
        .await?
        .get(0);

        if exists == 0 {
            sqlx::query(
                "INSERT INTO suggestions(student_id,model,reason,status,created_at) \
                 VALUES (?,?,?,'open',?)",
            )
            .bind(s.id)
            .bind(&desired)
            .bind(&reason)
            .bind(&ts)
            .execute(&mut *tx)
            .await?;
            created += 1;
        }
        tx.commit().await?;
        open_items.push(json!({
            "student_id": s.id, "class_id": s.class_id,
            "height_cm": height, "model": desired, "reason": reason,
        }));
    }

    Ok(Json(json!({
        "created": created,
        "obsoleted": obsolete,
        "open_count": open_items.len(),
        "items": open_items,
        "note": "建议仅用于后勤核实，生成建议不会改变任何资产归属",
    })))
}

#[derive(Deserialize)]
pub struct SuggestionQuery {
    status: Option<String>,
    class_id: Option<i64>,
}

const SUGGESTION_SELECT: &str =
    "SELECT s.id, s.student_id, st.name AS student_name, st.class_id, st.height_cm, \
     s.model, \
     (SELECT a.code FROM assignments asg2 JOIN assets a ON a.code = asg2.asset_code \
      WHERE asg2.student_id = s.student_id AND asg2.status='active' LIMIT 1) AS current_asset, \
     (SELECT a.model FROM assignments asg3 JOIN assets a ON a.code = asg3.asset_code \
      WHERE asg3.student_id = s.student_id AND asg3.status='active' LIMIT 1) AS current_model, \
     s.reason, s.status, s.created_at \
     FROM suggestions s JOIN students st ON st.id = s.student_id";

pub async fn list_suggestions(
    State(st): State<AppState>,
    user: User,
    Query(q): Query<SuggestionQuery>,
) -> AppResult<Json<Vec<SuggestionView>>> {
    // 班主任只能看本班建议
    if let Some(cid) = q.class_id {
        user.ensure_class(cid)?;
    } else if !user.is_logistics() {
        return Err(AppError::forbidden("班主任请按本班查看建议"));
    }
    let status = q.status.clone().unwrap_or_else(|| "open".to_string());
    let rows = match q.class_id {
        Some(cid) => {
            sqlx::query_as::<_, SuggestionView>(&format!(
                "{SUGGESTION_SELECT} WHERE s.status = ? AND st.class_id = ? ORDER BY s.id"
            ))
            .bind(status)
            .bind(cid)
            .fetch_all(&st.pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, SuggestionView>(&format!(
                "{SUGGESTION_SELECT} WHERE s.status = ? ORDER BY st.class_id, s.id"
            ))
            .bind(status)
            .fetch_all(&st.pool)
            .await?
        }
    };
    Ok(Json(rows))
}

// ============ 资产 ============

const ASSET_VIEW_SELECT: &str =
    "SELECT a.code, a.model, a.class_id, a.status, a.note, \
     (SELECT st.id FROM assignments asg \
       JOIN students st ON st.id = asg.student_id \
      WHERE asg.asset_code = a.code AND asg.status='active' LIMIT 1) AS occupant_student_id, \
     (SELECT st.name FROM assignments asg \
       JOIN students st ON st.id = asg.student_id \
      WHERE asg.asset_code = a.code AND asg.status='active' LIMIT 1) AS occupant_name, \
     (SELECT t.to_class_id FROM transfers t \
      WHERE t.asset_code = a.code AND t.status='pending' LIMIT 1) AS pending_transfer_to_class \
     FROM assets a";

#[derive(Deserialize)]
pub struct AssetQuery {
    class_id: Option<i64>,
    model: Option<String>,
    status: Option<String>,
    available: Option<i64>,
}

/// 资产列表。available=1 时仅返回：完好 + 未被占用 + 无进行中调拨（损坏品永不进入可分配列表）
pub async fn list_assets(
    State(st): State<AppState>,
    user: User,
    Query(q): Query<AssetQuery>,
) -> AppResult<Json<Vec<AssetView>>> {
    if let Some(cid) = q.class_id {
        user.ensure_class(cid)?;
    } else if !user.is_logistics() {
        return Err(AppError::forbidden("班主任请按本班查看资产"));
    }

    let mut sql = format!("{ASSET_VIEW_SELECT} WHERE 1=1");
    if let Some(cid) = q.class_id {
        sql.push_str(&format!(" AND a.class_id = {cid}"));
    }
    if let Some(m) = &q.model {
        // 型号为受控枚举（0~5），直接内插安全
        let m = m.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
        sql.push_str(&format!(" AND a.model = '{m}'"));
    }
    if let Some(s) = &q.status {
        let s = match s.as_str() {
            "ok" => "ok",
            "damaged" => "damaged",
            _ => return Err(AppError::bad_request("status 只能是 ok/damaged")),
        };
        sql.push_str(&format!(" AND a.status = '{s}'"));
    }
    if q.available == Some(1) {
        sql.push_str(
            " AND a.status = 'ok' \
             AND NOT EXISTS (SELECT 1 FROM assignments x WHERE x.asset_code=a.code AND x.status='active') \
             AND NOT EXISTS (SELECT 1 FROM transfers tr WHERE tr.asset_code=a.code AND tr.status='pending')",
        );
    }
    sql.push_str(" ORDER BY a.code");

    let rows = sqlx::query_as::<_, AssetView>(&sql)
        .fetch_all(&st.pool)
        .await?;
    Ok(Json(rows))
}

pub async fn asset_detail(
    State(st): State<AppState>,
    user: User,
    Path(code): Path<String>,
) -> AppResult<Json<Value>> {
    let asset = sqlx::query_as::<_, AssetView>(&format!(
        "{ASSET_VIEW_SELECT} WHERE a.code = ?"
    ))
    .bind(&code)
    .fetch_optional(&st.pool)
    .await?
    .ok_or_else(|| AppError::not_found("未找到该编号的桌椅"))?;
    user.ensure_class(asset.class_id)?;

    let history = sqlx::query_as::<_, TrailEntry>(
        "SELECT id,asset_code,event,detail,actor_id,created_at \
         FROM asset_trail WHERE asset_code = ? ORDER BY id DESC LIMIT 50",
    )
    .bind(&code)
    .fetch_all(&st.pool)
    .await?;
    Ok(Json(json!({ "asset": asset, "trail": history })))
}

// ============ 分配（后勤核实后手动执行）============

#[derive(Deserialize)]
pub struct AssignReq {
    student_id: i64,
    asset_code: String,
    reason: Option<String>,
}

/// 把一套桌椅分配给一名学生。
/// 关键：通过条件 INSERT + 部分唯一索引保证并发下同一套桌椅只能有一个 active 分配。
pub async fn assign_asset(
    State(st): State<AppState>,
    user: User,
    Json(req): Json<AssignReq>,
) -> AppResult<Json<Value>> {
    user.ensure_logistics()?;

    let student = sqlx::query_as::<_, Student>("SELECT * FROM students WHERE id = ?")
        .bind(req.student_id)
        .fetch_optional(&st.pool)
        .await?
        .ok_or_else(|| AppError::not_found("学生不存在"))?;
    if student.active != 1 {
        return Err(AppError::bad_request("该学生已不在校，无法分配"));
    }

    // 先给出明确的失败原因（真正的竞态由下面的条件写入与唯一索引兜底）
    if let Some(asset) = sqlx::query_as::<_, Asset>("SELECT * FROM assets WHERE code = ?")
        .bind(&req.asset_code)
        .fetch_optional(&st.pool)
        .await?
    {
        if asset.status == "damaged" {
            return Err(AppError::bad_request("该桌椅已损坏，不能分配"));
        }
        let occupied: i64 = sqlx::query(
            "SELECT COUNT(*) FROM assignments WHERE asset_code=? AND status='active'",
        )
        .bind(&req.asset_code)
        .fetch_one(&st.pool)
        .await?
        .get(0);
        if occupied > 0 {
            return Err(AppError::conflict("该桌椅已被其他学生领用，分配失败"));
        }
        let pending: i64 = sqlx::query(
            "SELECT COUNT(*) FROM transfers WHERE asset_code=? AND status='pending'",
        )
        .bind(&req.asset_code)
        .fetch_one(&st.pool)
        .await?
        .get(0);
        if pending > 0 {
            return Err(AppError::conflict("该桌椅正在跨班调拨流程中，暂不能分配"));
        }
    } else {
        return Err(AppError::not_found("资产编号不存在"));
    }

    let reason = req.reason.clone().unwrap_or_else(|| "后勤调配".to_string());
    let ts = now();
    let mut tx = st.pool.begin().await?;

    // 一名学生同时只保留一套：旧分配终结但记录永久保留
    sqlx::query(
        "UPDATE assignments SET status='released', released_at=? \
         WHERE student_id=? AND status='active'",
    )
    .bind(&ts)
    .bind(req.student_id)
    .execute(&mut *tx)
    .await?;

    // 条件 INSERT：只有 完好 + 无占用 + 无待处理调拨 时才能写入；
    // 部分唯一索引 idx_assign_active_asset 是最后一道并发防线。
    let inserted = sqlx::query(
        "INSERT INTO assignments(asset_code,student_id,assigned_by,status,reason,created_at) \
         SELECT ?, ?, ?, 'active', ?, ? \
         WHERE EXISTS (SELECT 1 FROM assets a WHERE a.code = ? AND a.status='ok') \
           AND NOT EXISTS (SELECT 1 FROM assignments x WHERE x.asset_code = ? AND x.status='active') \
           AND NOT EXISTS (SELECT 1 FROM transfers t WHERE t.asset_code = ? AND t.status='pending')",
    )
    .bind(&req.asset_code)
    .bind(req.student_id)
    .bind(user.id)
    .bind(&reason)
    .bind(&ts)
    .bind(&req.asset_code)
    .bind(&req.asset_code)
    .bind(&req.asset_code)
    .execute(&mut *tx)
    .await?;

    if inserted.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(AppError::conflict("该桌椅刚刚已被领用或状态变化，分配失败"));
    }

    sqlx::query(
        "INSERT INTO asset_trail(asset_code,event,detail,actor_id,created_at) \
         VALUES (?, 'assign', ?, ?, ?)",
    )
    .bind(&req.asset_code)
    .bind(format!("分配给学生#{}（{}）：{}", student.id, student.name, reason))
    .bind(user.id)
    .bind(&ts)
    .execute(&mut *tx)
    .await?;

    // 若本次分配恰好满足待处理建议 → 标记 fulfilled；否则建议保持 open
    sqlx::query(
        "UPDATE suggestions SET status='fulfilled', resolved_at=? \
         WHERE student_id=? AND status='open' AND model = \
         (SELECT model FROM assets WHERE code=?)",
    )
    .bind(&ts)
    .bind(req.student_id)
    .bind(&req.asset_code)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(json!({
        "ok": true,
        "asset_code": req.asset_code,
        "student_id": req.student_id,
        "message": "分配成功",
    })))
}

/// 释放（收回）一套桌椅
pub async fn release_asset(
    State(st): State<AppState>,
    user: User,
    Path(code): Path<String>,
) -> AppResult<Json<Value>> {
    user.ensure_logistics()?;
    let ts = now();
    let mut tx = st.pool.begin().await?;
    let r = sqlx::query(
        "UPDATE assignments SET status='released', released_at=? \
         WHERE asset_code=? AND status='active'",
    )
    .bind(&ts)
    .bind(&code)
    .execute(&mut *tx)
    .await?;
    if r.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(AppError::bad_request("该桌椅当前没有在用分配"));
    }
    sqlx::query(
        "INSERT INTO asset_trail(asset_code,event,detail,actor_id,created_at) \
         VALUES (?, 'release', '后勤收回', ?, ?)",
    )
    .bind(&code)
    .bind(user.id)
    .bind(&ts)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct DamageReq {
    note: Option<String>,
}

/// 标记损坏：立即停止占用（保留历史分配记录），损坏品不再进入可分配列表
pub async fn mark_damaged(
    State(st): State<AppState>,
    user: User,
    Path(code): Path<String>,
    Json(req): Json<DamageReq>,
) -> AppResult<Json<Value>> {
    user.ensure_logistics()?;
    let ts = now();
    let mut tx = st.pool.begin().await?;
    let r = sqlx::query("UPDATE assets SET status='damaged', note=?, updated_at=? WHERE code=?")
        .bind(req.note.clone().unwrap_or_default())
        .bind(&ts)
        .bind(&code)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(AppError::not_found("资产编号不存在"));
    }
    sqlx::query(
        "UPDATE assignments SET status='released', released_at=? \
         WHERE asset_code=? AND status='active'",
    )
    .bind(&ts)
    .bind(&code)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO asset_trail(asset_code,event,detail,actor_id,created_at) \
         VALUES (?, 'damage', ?, ?, ?)",
    )
    .bind(&code)
    .bind(format!("标记损坏：{}", req.note.unwrap_or_default()))
    .bind(user.id)
    .bind(&ts)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

/// 修复：重新转为完好（是否分配由后勤另行操作）
pub async fn repair_asset(
    State(st): State<AppState>,
    user: User,
    Path(code): Path<String>,
) -> AppResult<Json<Value>> {
    user.ensure_logistics()?;
    let ts = now();
    let r = sqlx::query("UPDATE assets SET status='ok', updated_at=? WHERE code=? AND status='damaged'")
        .bind(&ts)
        .bind(&code)
        .execute(&st.pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::bad_request("该资产不是损坏状态或不存在"));
    }
    sqlx::query(
        "INSERT INTO asset_trail(asset_code,event,detail,actor_id,created_at) \
         VALUES (?, 'repair', '修复完成，恢复为可分配', ?, ?)",
    )
    .bind(&code)
    .bind(user.id)
    .bind(&ts)
    .execute(&st.pool)
    .await?;
    Ok(Json(json!({ "ok": true })))
}

// ============ 学生换班 ============

#[derive(Deserialize)]
pub struct MoveStudentReq {
    class_id: i64,
}

/// 学生换班：只改学生所属班级；名下桌椅保留在原班（历史分配保留），
/// 绝不因为一条建议或换班动作把资产算到另一个班。
pub async fn move_student_class(
    State(st): State<AppState>,
    user: User,
    Path(student_id): Path<i64>,
    Json(req): Json<MoveStudentReq>,
) -> AppResult<Json<Value>> {
    user.ensure_logistics()?;

    let student = sqlx::query_as::<_, Student>("SELECT * FROM students WHERE id = ?")
        .bind(student_id)
        .fetch_optional(&st.pool)
        .await?
        .ok_or_else(|| AppError::not_found("学生不存在"))?;
    if student.class_id == req.class_id {
        return Err(AppError::bad_request("学生已经在该班级"));
    }
    let _target_exists = sqlx::query_as::<_, Class>("SELECT id,name FROM classes WHERE id=?")
        .bind(req.class_id)
        .fetch_optional(&st.pool)
        .await?
        .ok_or_else(|| AppError::bad_request("目标班级不存在"))?;

    let ts = now();
    let mut tx = st.pool.begin().await?;

    // 记录其在用桌椅（留在原班）
    let desk = sqlx::query_as::<_, (String,)>(
        "SELECT asset_code FROM assignments WHERE student_id=? AND status='active'",
    )
    .bind(student_id)
    .fetch_optional(&mut *tx)
    .await?;

    sqlx::query("UPDATE students SET class_id=? WHERE id=?")
        .bind(req.class_id)
        .bind(student_id)
        .execute(&mut *tx)
        .await?;

    // 原分配终结留痕；行记录本身保留；assets.class_id 不变
    sqlx::query(
        "UPDATE assignments SET status='released', released_at=?, \
         reason = reason || ' [学生换班，桌椅保留原班]' \
         WHERE student_id=? AND status='active'",
    )
    .bind(&ts)
    .bind(student_id)
    .execute(&mut *tx)
    .await?;

    if let Some((code,)) = desk {
        sqlx::query(
            "INSERT INTO asset_trail(asset_code,event,detail,actor_id,created_at) \
             VALUES (?, 'release', ?, ?, ?)",
        )
        .bind(&code)
        .bind(format!("学生#{}换班至班级#{}，桌椅保留原班", student_id, req.class_id))
        .bind(user.id)
        .bind(&ts)
        .execute(&mut *tx)
        .await?;
    }

    // 旧建议作废，到新班后由后勤重新生成
    sqlx::query(
        "UPDATE suggestions SET status='obsolete', resolved_at=? \
         WHERE student_id=? AND status='open'",
    )
    .bind(&ts)
    .bind(student_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(json!({
        "ok": true,
        "student_id": student_id,
        "new_class_id": req.class_id,
        "note": "学生已换班；原桌椅保留在原班级，资产位置未发生变化",
    })))
}

// ============ 跨班调拨（双确认）============

#[derive(Deserialize)]
pub struct TransferReq {
    asset_code: String,
    to_class_id: i64,
}

/// 后勤发起跨班调拨。仅可调拨 完好、未占用、无进行中调拨 的资产。
pub async fn create_transfer(
    State(st): State<AppState>,
    user: User,
    Json(req): Json<TransferReq>,
) -> AppResult<Json<Value>> {
    user.ensure_logistics()?;

    let asset = sqlx::query_as::<_, Asset>("SELECT * FROM assets WHERE code=?")
        .bind(&req.asset_code)
        .fetch_optional(&st.pool)
        .await?
        .ok_or_else(|| AppError::not_found("资产编号不存在"))?;
    if asset.status == "damaged" {
        return Err(AppError::bad_request("损坏桌椅不能发起调拨，请先修复"));
    }
    if asset.class_id == req.to_class_id {
        return Err(AppError::bad_request("交出方与接收方不能是同一班级"));
    }
    let _target = sqlx::query_as::<_, Class>("SELECT id,name FROM classes WHERE id=?")
        .bind(req.to_class_id)
        .fetch_optional(&st.pool)
        .await?
        .ok_or_else(|| AppError::bad_request("接收班级不存在"))?;

    let occupied: i64 = sqlx::query(
        "SELECT COUNT(*) FROM assignments WHERE asset_code=? AND status='active'",
    )
    .bind(&req.asset_code)
    .fetch_one(&st.pool)
    .await?
    .get(0);
    if occupied > 0 {
        return Err(AppError::bad_request("该桌椅尚在使用中，请先收回再调拨"));
    }

    let ts = now();
    let r = sqlx::query(
        "INSERT INTO transfers(asset_code,from_class_id,to_class_id,requested_by,status,created_at) \
         SELECT ?, ?, ?, ?, 'pending', ? \
         WHERE NOT EXISTS (SELECT 1 FROM transfers t WHERE t.asset_code=? AND t.status='pending')",
    )
    .bind(&req.asset_code)
    .bind(asset.class_id)
    .bind(req.to_class_id)
    .bind(user.id)
    .bind(&ts)
    .bind(&req.asset_code)
    .execute(&st.pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::conflict("该桌椅已有进行中的调拨单"));
    }
    sqlx::query(
        "INSERT INTO asset_trail(asset_code,event,detail,actor_id,created_at) \
         VALUES (?, 'transfer_request', ?, ?, ?)",
    )
    .bind(&req.asset_code)
    .bind(format!("后勤发起调拨：班级#{} -> 班级#{}（待双方确认）", asset.class_id, req.to_class_id))
    .bind(user.id)
    .bind(&ts)
    .execute(&st.pool)
    .await?;

    Ok(Json(json!({
        "ok": true,
        "asset_code": req.asset_code,
        "from_class_id": asset.class_id,
        "to_class_id": req.to_class_id,
        "status": "pending",
        "note": "调拨已创建，须交出方与接收方班主任均确认后才会移动资产",
    })))
}

/// 班主任确认调拨：交出方老师确认 from，接收方老师确认 to。
/// 两方都确认后才在同一事务里把资产移到接收班级。
pub async fn confirm_transfer(
    State(st): State<AppState>,
    user: User,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let t = sqlx::query_as::<_, TransferRow>(
        "SELECT * FROM transfers WHERE id=?",
    )
    .bind(id)
    .fetch_optional(&st.pool)
    .await?
    .ok_or_else(|| AppError::not_found("调拨单不存在"))?;
    if t.status != "pending" {
        return Err(AppError::conflict(format!("调拨单当前状态为 {}，无法确认", t.status)));
    }

    // 只有交出方/接收方本班老师可以确认对应一方；后勤不能代为确认
    let side = if user.class_id == Some(t.from_class_id) {
        "from"
    } else if user.class_id == Some(t.to_class_id) {
        "to"
    } else {
        return Err(AppError::forbidden("非本调拨相关班级，无权确认"));
    };
    if user.is_logistics() {
        return Err(AppError::forbidden("调拨须由交出方与接收方班主任确认，后勤不能代确认"));
    }

    let ts = now();
    let mut tx = st.pool.begin().await?;

    if side == "from" {
        if t.from_confirmed_by.is_some() {
            tx.rollback().await?;
            return Err(AppError::bad_request("交出方已确认，请勿重复操作"));
        }
        sqlx::query(
            "UPDATE transfers SET from_confirmed_by=?, from_confirmed_at=? WHERE id=?",
        )
        .bind(user.id)
        .bind(&ts)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    } else {
        if t.to_confirmed_by.is_some() {
            tx.rollback().await?;
            return Err(AppError::bad_request("接收方已确认，请勿重复操作"));
        }
        sqlx::query(
            "UPDATE transfers SET to_confirmed_by=?, to_confirmed_at=? WHERE id=?",
        )
        .bind(user.id)
        .bind(&ts)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO asset_trail(asset_code,event,detail,actor_id,created_at) \
         VALUES (?, 'transfer_confirm', ?, ?, ?)",
    )
    .bind(&t.asset_code)
    .bind(format!(
        "{}班主任确认（班级#{}）",
        if side == "from" { "交出方" } else { "接收方" },
        user.class_id.unwrap_or(-1)
    ))
    .bind(user.id)
    .bind(&ts)
    .execute(&mut *tx)
    .await?;

    // 重新读取两方确认状态
    let cur = sqlx::query_as::<_, TransferRow>("SELECT * FROM transfers WHERE id=?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

    let mut completed = false;
    if cur.from_confirmed_by.is_some() && cur.to_confirmed_by.is_some() {
        // 双方齐了 → 条件更新资产位置：仍在原班、完好、未被占用才移动
        let moved = sqlx::query(
            "UPDATE assets SET class_id=?, updated_at=? \
             WHERE code=? AND class_id=? AND status='ok' \
             AND NOT EXISTS (SELECT 1 FROM assignments x WHERE x.asset_code=? AND x.status='active') \
             AND NOT EXISTS (SELECT 1 FROM transfers tr WHERE tr.asset_code=? AND tr.status='pending' AND tr.id<>?)",
        )
        .bind(t.to_class_id)
        .bind(&ts)
        .bind(&t.asset_code)
        .bind(t.from_class_id)
        .bind(&t.asset_code)
        .bind(&t.asset_code)
        .bind(id)
        .execute(&mut *tx)
        .await?;

        if moved.rows_affected() == 0 {
            tx.rollback().await?;
            return Err(AppError::conflict(
                "确认完成时资产状态已变化（被占用/损坏/移位），调拨未生效，请核实后重试或取消",
            ));
        }

        sqlx::query(
            "UPDATE transfers SET status='completed', completed_at=? WHERE id=? AND status='pending'",
        )
        .bind(&ts)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO asset_trail(asset_code,event,detail,actor_id,created_at) \
             VALUES (?, 'transfer_complete', ?, ?, ?)",
        )
        .bind(&t.asset_code)
        .bind(format!(
            "双方确认完成，资产由班级#{}移入班级#{}",
            t.from_class_id, t.to_class_id
        ))
        .bind(user.id)
        .bind(&ts)
        .execute(&mut *tx)
        .await?;
        completed = true;
    }

    tx.commit().await?;
    Ok(Json(json!({
        "ok": true,
        "transfer_id": id,
        "side": side,
        "completed": completed,
        "asset_class_id": if completed { t.to_class_id } else { t.from_class_id },
    })))
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct TransferRow {
    id: i64,
    asset_code: String,
    from_class_id: i64,
    to_class_id: i64,
    requested_by: i64,
    from_confirmed_by: Option<i64>,
    to_confirmed_by: Option<i64>,
    from_confirmed_at: Option<String>,
    to_confirmed_at: Option<String>,
    status: String,
    created_at: String,
    completed_at: Option<String>,
    cancelled_at: Option<String>,
    cancel_reason: String,
}

#[derive(Deserialize)]
pub struct CancelTransferReq {
    reason: Option<String>,
}

pub async fn cancel_transfer(
    State(st): State<AppState>,
    user: User,
    Path(id): Path<i64>,
    Json(req): Json<CancelTransferReq>,
) -> AppResult<Json<Value>> {
    let t = sqlx::query_as::<_, TransferRow>("SELECT * FROM transfers WHERE id=?")
        .bind(id)
        .fetch_optional(&st.pool)
        .await?
        .ok_or_else(|| AppError::not_found("调拨单不存在"))?;
    let allowed = user.is_logistics()
        || user.class_id == Some(t.from_class_id)
        || user.class_id == Some(t.to_class_id);
    if !allowed {
        return Err(AppError::forbidden("无权取消该调拨单"));
    }
    if t.status != "pending" {
        return Err(AppError::conflict("只有待确认的调拨单可以取消"));
    }
    let ts = now();
    sqlx::query(
        "UPDATE transfers SET status='cancelled', cancelled_at=?, cancel_reason=? WHERE id=?",
    )
    .bind(&ts)
    .bind(req.reason.clone().unwrap_or_default())
    .bind(id)
    .execute(&st.pool)
    .await?;
    sqlx::query(
        "INSERT INTO asset_trail(asset_code,event,detail,actor_id,created_at) \
         VALUES (?, 'transfer_cancel', ?, ?, ?)",
    )
    .bind(&t.asset_code)
    .bind(format!("调拨取消：{}", req.reason.unwrap_or_default()))
    .bind(user.id)
    .bind(&ts)
    .execute(&st.pool)
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct TransferListQuery {
    status: Option<String>,
}

pub async fn list_transfers(
    State(st): State<AppState>,
    user: User,
    Query(q): Query<TransferListQuery>,
) -> AppResult<Json<Vec<TransferView>>> {
    let status = q.status.clone().unwrap_or_else(|| "pending".to_string());
    let rows = if user.is_logistics() {
        sqlx::query_as::<_, TransferView>(
            "SELECT id,asset_code,from_class_id,to_class_id,requested_by,\
             from_confirmed_by,to_confirmed_by,status,created_at,completed_at,cancel_reason \
             FROM transfers WHERE status=? ORDER BY id DESC",
        )
        .bind(status)
        .fetch_all(&st.pool)
        .await?
    } else {
        let cid = user.class_id.ok_or_else(|| AppError::forbidden("账号未绑定班级"))?;
        sqlx::query_as::<_, TransferView>(
            "SELECT id,asset_code,from_class_id,to_class_id,requested_by,\
             from_confirmed_by,to_confirmed_by,status,created_at,completed_at,cancel_reason \
             FROM transfers WHERE status=? AND (from_class_id=? OR to_class_id=?) ORDER BY id DESC",
        )
        .bind(status)
        .bind(cid)
        .bind(cid)
        .fetch_all(&st.pool)
        .await?
    };
    Ok(Json(rows))
}

// ============ 班级看板 ============

#[derive(serde::Serialize)]
struct AssignedRow {
    asset_code: String,
    model: String,
    student_id: i64,
    student_name: String,
    height_cm: Option<i64>,
    desired_model: Option<String>,
    mismatch: bool,
}

pub async fn class_board(
    State(st): State<AppState>,
    user: User,
    Path(class_id): Path<i64>,
) -> AppResult<Json<Value>> {
    user.ensure_class(class_id)?;

    let class = sqlx::query_as::<_, Class>("SELECT id,name FROM classes WHERE id=?")
        .bind(class_id)
        .fetch_optional(&st.pool)
        .await?
        .ok_or_else(|| AppError::not_found("班级不存在"))?;

    // 待调换：本班 open 建议
    let pending = sqlx::query_as::<_, SuggestionView>(&format!(
        "{SUGGESTION_SELECT} WHERE s.status='open' AND st.class_id=? ORDER BY s.id"
    ))
    .bind(class_id)
    .fetch_all(&st.pool)
    .await?;

    // 已分配（在用）
    let assigned_raw = sqlx::query(
        "SELECT a.code AS asset_code, a.model AS model, st.id AS student_id, \
         st.name AS student_name, st.height_cm AS height_cm, st.desired_model AS desired_model \
         FROM assignments asg \
         JOIN assets a ON a.code = asg.asset_code \
         JOIN students st ON st.id = asg.student_id \
         WHERE asg.status='active' AND a.class_id=? ORDER BY a.code",
    )
    .bind(class_id)
    .fetch_all(&st.pool)
    .await?;
    let assigned: Vec<AssignedRow> = assigned_raw
        .into_iter()
        .map(|r| {
            let model: String = r.get("model");
            let desired: Option<String> = r.get("desired_model");
            AssignedRow {
                asset_code: r.get("asset_code"),
                mismatch: desired.as_deref() != Some(model.as_str()) && desired.is_some(),
                model,
                student_id: r.get("student_id"),
                student_name: r.get("student_name"),
                height_cm: r.get("height_cm"),
                desired_model: desired,
            }
        })
        .collect();

    // 损坏
    let damaged = sqlx::query_as::<_, AssetView>(&format!(
        "{ASSET_VIEW_SELECT} WHERE a.status='damaged' AND a.class_id=? ORDER BY a.code"
    ))
    .bind(class_id)
    .fetch_all(&st.pool)
    .await?;

    // 可校内调配的空余桌椅（完好/未占用/无进行中调拨）
    let available = sqlx::query_as::<_, AssetView>(&format!(
        "{ASSET_VIEW_SELECT} WHERE a.class_id=? AND a.status='ok' \
         AND NOT EXISTS (SELECT 1 FROM assignments x WHERE x.asset_code=a.code AND x.status='active') \
         AND NOT EXISTS (SELECT 1 FROM transfers t WHERE t.asset_code=a.code AND t.status='pending') \
         ORDER BY a.code"
    ))
    .bind(class_id)
    .fetch_all(&st.pool)
    .await?;

    // 与本班相关的调拨
    let transfers = sqlx::query_as::<_, TransferView>(
        "SELECT id,asset_code,from_class_id,to_class_id,requested_by,\
         from_confirmed_by,to_confirmed_by,status,created_at,completed_at,cancel_reason \
         FROM transfers WHERE status='pending' AND (from_class_id=? OR to_class_id=?) ORDER BY id",
    )
    .bind(class_id)
    .bind(class_id)
    .fetch_all(&st.pool)
    .await?;

    Ok(Json(json!({
        "class": class,
        "pending_swaps": pending,
        "assigned": assigned,
        "damaged": damaged,
        "available": available,
        "pending_transfers": transfers,
    })))
}

// ============ 扫码（本地模拟）============

#[derive(Deserialize)]
pub struct ScanReq {
    code: String,
}

/// 本地模拟扫码：返回该编号桌椅当前状态、占用与可否分配，并留下扫码记录
pub async fn scan(
    State(st): State<AppState>,
    user: User,
    headers: HeaderMap,
    Json(req): Json<ScanReq>,
) -> AppResult<Json<Value>> {
    let asset = sqlx::query_as::<_, AssetView>(&format!(
        "{ASSET_VIEW_SELECT} WHERE a.code = ?"
    ))
    .bind(&req.code)
    .fetch_optional(&st.pool)
    .await?;

    let ts = now();
    let (result, body) = match asset {
        Some(a) => {
            // 班主任只能扫描本班资产
            if let Err(e) = user.ensure_class(a.class_id) {
                let _ = sqlx::query(
                    "INSERT INTO scan_events(asset_code,actor_id,action,result,created_at) \
                     VALUES (?,?,'lookup','越权扫描他班资产',?)",
                )
                .bind(&req.code)
                .bind(user.id)
                .bind(&ts)
                .execute(&st.pool)
                .await;
                return Err(e);
            }
            let mut blockers: Vec<&str> = Vec::new();
            if a.status == "damaged" {
                blockers.push("已损坏");
            }
            if a.occupant_student_id.is_some() {
                blockers.push("已被占用");
            }
            if a.pending_transfer_to_class.is_some() {
                blockers.push("调拨流程中");
            }
            let allocatable = blockers.is_empty();
            let result = if allocatable { "可分配" } else { "不可分配" };
            (
                result.to_string(),
                json!({
                    "found": true,
                    "asset": a,
                    "allocatable": allocatable,
                    "blockers": blockers,
                }),
            )
        }
        None => (
            "未找到资产".to_string(),
            json!({ "found": false, "code": req.code, "allocatable": false }),
        ),
    };

    sqlx::query(
        "INSERT INTO scan_events(asset_code,actor_id,action,result,created_at) \
         VALUES (?,?,'lookup',?,?)",
    )
    .bind(&req.code)
    .bind(user.id)
    .bind(&result)
    .bind(&ts)
    .execute(&st.pool)
    .await
    .ok();

    // 扫码设备编号（本地模拟，可用请求头 x-scanner-device 指定）
    let device = headers
        .get("x-scanner-device")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("local-mock-scanner");
    let mut merged = body;
    merged["device"] = json!(device);
    merged["scanned_at"] = json!(ts);
    Ok(Json(merged))
}

// ============ 资产去向流水 ============

pub async fn asset_trail(
    State(st): State<AppState>,
    user: User,
    Path(code): Path<String>,
) -> AppResult<Json<Vec<TrailEntry>>> {
    let asset = sqlx::query_as::<_, Asset>("SELECT * FROM assets WHERE code=?")
        .bind(&code)
        .fetch_optional(&st.pool)
        .await?
        .ok_or_else(|| AppError::not_found("资产不存在"))?;
    user.ensure_class(asset.class_id)?;
    let rows = sqlx::query_as::<_, TrailEntry>(
        "SELECT id,asset_code,event,detail,actor_id,created_at \
         FROM asset_trail WHERE asset_code=? ORDER BY id",
    )
    .bind(&code)
    .fetch_all(&st.pool)
    .await?;
    Ok(Json(rows))
}
