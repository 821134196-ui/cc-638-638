// 端到端集成测试：并发领用 / 双确认调拨 / 班级隔离 / 损坏品 / 建议不移动资产 / 历史保留
use std::sync::Arc;

use deskflow::{db, router, AppState};
use serde_json::Value;
use sqlx::Row;
use tempfile::TempDir;

struct TestApp {
    base: String,
    pool: sqlx::SqlitePool,
    _dir: TempDir, // 保活临时目录
}

async fn spawn_app() -> TestApp {
    let dir = TempDir::new().unwrap();
    let db_file = dir.path().join("test.db");
    let url = format!("sqlite://{}", db_file.display());
    let pool = db::init(&url).await.unwrap();
    let state = AppState::new(pool.clone(), "test-secret".into());
    let app = router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    TestApp {
        base: format!("http://{addr}/api"),
        pool,
        _dir: dir,
    }
}

impl TestApp {
    fn client(&self) -> reqwest::Client {
        reqwest::Client::new()
    }
    async fn login(&self, username: &str, password: &str) -> String {
        let v: Value = self
            .client()
            .post(format!("{}/auth/login", self.base))
            .json(&serde_json::json!({"username": username, "password": password}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        v["token"].as_str().unwrap().to_string()
    }
}

async fn active_assignment_count(pool: &sqlx::SqlitePool, code: &str) -> i64 {
    sqlx::query("SELECT COUNT(*) FROM assignments WHERE asset_code=? AND status='active'")
        .bind(code)
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0)
}

#[tokio::test]
async fn concurrent_claim_only_one_succeeds() {
    let app = spawn_app().await;
    let http = app.client();
    let logistics = app.login("logistics", "admin1234").await;

    // 直接造 12 名无桌椅学生参与“抢用”同一套桌椅
    let now = chrono::Utc::now().to_rfc3339();
    for _ in 0..12 {
        sqlx::query("INSERT INTO students(name,class_id,height_cm,updated_at) VALUES ('抢用学生',2,135,?)")
            .bind(&now)
            .execute(&app.pool)
            .await
            .unwrap();
    }
    let ids: Vec<i64> = sqlx::query(
        "SELECT id FROM students WHERE name='抢用学生' ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap()
    .into_iter()
    .map(|r| r.get::<i64, _>(0))
    .collect();
    assert_eq!(ids.len(), 12);

    // DESK-0019：2班、2号、完好、未占用、无调拨
    let asset = "DESK-0019";
    assert_eq!(active_assignment_count(&app.pool, asset).await, 0);

    let base = Arc::new(app.base.clone());
    let token = Arc::new(logistics);
    let mut handles = Vec::new();
    for sid in ids {
        let base = base.clone();
        let token = token.clone();
        let asset = asset.to_string();
        handles.push(tokio::spawn(async move {
            http_post_assign(&base, &token, &asset, sid).await
        }));
    }
    let statuses: Vec<u16> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    let ok = statuses.iter().filter(|&&s| s == 200).count();
    let conflict = statuses.iter().filter(|&&s| s == 409).count();
    assert_eq!(ok, 1, "必须恰好一个领用成功，实际 {ok}；状态码 {statuses:?}");
    assert_eq!(conflict, 11, "其余请求必须冲突失败");

    // 数据库层面：active 分配只有一条
    assert_eq!(active_assignment_count(&app.pool, asset).await, 1);

    // 损坏品在并发窗口中被标记 → 任何新分配也不能成功（单独验证一次）
    let _ = http
        .post(format!("{}/assets/DESK-0002/damaged", app.base))
        .bearer_auth(token.as_str())
        .json(&serde_json::json!({"note": "测试损坏"}))
        .send()
        .await
        .unwrap();
    let resp = http
        .post(format!("{}/assignments", app.base))
        .bearer_auth(token.as_str())
        .json(&serde_json::json!({"student_id": 9, "asset_code": "DESK-0002"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "损坏桌椅不能被分配");
}

async fn http_post_assign(base: &str, token: &str, asset: &str, sid: i64) -> u16 {
    reqwest::Client::new()
        .post(format!("{base}/assignments"))
        .bearer_auth(token)
        .json(&serde_json::json!({"student_id": sid, "asset_code": asset, "reason": "并发领用"}))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn transfer_requires_both_confirmations_and_blocks_outsiders() {
    let app = spawn_app().await;
    let http = app.client();
    let logistics = app.login("logistics", "admin1234").await;
    let teacher1 = app.login("teacher1", "pass1234").await; // 1班（接收方）
    let teacher2 = app.login("teacher2", "pass1234").await; // 2班（无关）
    let teacher3 = app.login("teacher3", "pass1234").await; // 3班（交出方）

    // 后勤发起：DESK-0020 在 3 班 → 1 班
    let resp = http
        .post(format!("{}/transfers", app.base))
        .bearer_auth(&logistics)
        .json(&serde_json::json!({"asset_code": "DESK-0020", "to_class_id": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let asset_class = || async {
        let row: (i64,) = sqlx::query_as("SELECT class_id FROM assets WHERE code='DESK-0020'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
        row.0
    };
    assert_eq!(asset_class().await, 3);

    // 无关班主任不能确认
    let r = http
        .post(format!("{}/transfers/1/confirm", app.base))
        .bearer_auth(&teacher2)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);

    // 后勤不能代替班主任确认
    let r = http
        .post(format!("{}/transfers/1/confirm", app.base))
        .bearer_auth(&logistics)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);

    // 接收方先确认：资产仍在交出方
    let v: Value = http
        .post(format!("{}/transfers/1/confirm", app.base))
        .bearer_auth(&teacher1)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["completed"], false);
    assert_eq!(asset_class().await, 3, "单方确认不得移动资产");

    // 重复确认报错
    let r = http
        .post(format!("{}/transfers/1/confirm", app.base))
        .bearer_auth(&teacher1)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    // 交出方确认 → 完成，资产移到 1 班
    let v: Value = http
        .post(format!("{}/transfers/1/confirm", app.base))
        .bearer_auth(&teacher3)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["completed"], true);
    assert_eq!(asset_class().await, 1, "双方确认后资产必须转移");

    let status: String = sqlx::query_scalar("SELECT status FROM transfers WHERE id=1")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(status, "completed");

    // 完成后再确认 → 409
    let r = http
        .post(format!("{}/transfers/1/confirm", app.base))
        .bearer_auth(&teacher3)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);

    // 流水完整保留请求/两次确认/完成
    let events: Vec<String> = sqlx::query_scalar(
        "SELECT event FROM asset_trail WHERE asset_code='DESK-0020' ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    for e in ["transfer_request", "transfer_confirm", "transfer_confirm", "transfer_complete"] {
        assert!(events.contains(&e.to_string()), "流水缺少 {e}：{events:?}");
    }

    // 另一个方向（交出方先确认）：DESK-0018 3班→2班
    http.post(format!("{}/transfers", app.base))
        .bearer_auth(&logistics)
        .json(&serde_json::json!({"asset_code": "DESK-0018", "to_class_id": 2}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    http.post(format!("{}/transfers/2/confirm", app.base))
        .bearer_auth(&teacher3)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let still_3: i64 = sqlx::query_scalar("SELECT class_id FROM assets WHERE code='DESK-0018'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(still_3, 3, "交出方单方确认也不得移动资产");
    let v: Value = http
        .post(format!("{}/transfers/2/confirm", app.base))
        .bearer_auth(&teacher2)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["completed"], true);
    let now_2: i64 = sqlx::query_scalar("SELECT class_id FROM assets WHERE code='DESK-0018'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(now_2, 2);
}

#[tokio::test]
async fn teacher_cannot_access_other_classes() {
    let app = spawn_app().await;
    let http = app.client();
    let t1 = app.login("teacher1", "pass1234").await; // 只绑定 1 班
    let t2 = app.login("teacher2", "pass1234").await;
    let logistics = app.login("logistics", "admin1234").await;

    // 越权读 2 班
    for path in [
        "/classes/2/students",
        "/classes/2/board",
        "/assets?class_id=2",
        "/suggestions?class_id=2&status=open",
    ] {
        let r = http
            .get(format!("{}{}", app.base, path))
            .bearer_auth(&t1)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403, "{path} 应被拒绝");
    }

    // 越权改 2 班学生身高（学生10 在 2 班）
    let r = http
        .patch(format!("{}/students/10/height", app.base))
        .bearer_auth(&t1)
        .json(&serde_json::json!({"height_cm": 130}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);

    // 无令牌 401
    let r = http
        .get(format!("{}/classes/1/students", app.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    // 本班访问正常
    let r = http
        .get(format!("{}/classes/1/students", app.base))
        .bearer_auth(&t1)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // 后勤跨班正常
    let r = http
        .get(format!("{}/classes/2/students", app.base))
        .bearer_auth(&logistics)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // 班主任只能看到与本班相关的调拨：造一张 2↔3 的单子，teacher1 不应看到
    http.post(format!("{}/transfers", app.base))
        .bearer_auth(&logistics)
        .json(&serde_json::json!({"asset_code": "DESK-0018", "to_class_id": 2}))
        .send()
        .await
        .unwrap();
    let list: Value = http
        .get(format!("{}/transfers?status=pending", app.base))
        .bearer_auth(&t1)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(list.as_array().unwrap().is_empty(), "teacher1 不应看到 2↔3 调拨");
    let list2: Value = http
        .get(format!("{}/transfers?status=pending", app.base))
        .bearer_auth(&t2)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list2.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn damaged_assets_leave_pool_and_release_occupant() {
    let app = spawn_app().await;
    let http = app.client();
    let logistics = app.login("logistics", "admin1234").await;

    // DESK-0003 原本由学生4在用
    assert_eq!(active_assignment_count(&app.pool, "DESK-0003").await, 1);

    http.post(format!("{}/assets/DESK-0003/damaged", app.base))
        .bearer_auth(&logistics)
        .json(&serde_json::json!({"note": "桌面开裂"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // 在用分配被终结（行保留），损坏品不出现在可分配列表
    assert_eq!(active_assignment_count(&app.pool, "DESK-0003").await, 0);
    let avail: Value = http
        .get(format!("{}/assets?available=1", app.base))
        .bearer_auth(&logistics)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        !avail.as_array().unwrap().iter().any(|a| a["code"] == "DESK-0003"),
        "损坏品不得进入可分配列表"
    );
    // 扫码显示不可分配
    let scan: Value = http
        .post(format!("{}/assets/scan", app.base))
        .bearer_auth(&logistics)
        .json(&serde_json::json!({"code": "DESK-0003"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(scan["allocatable"], false);

    // 历史分配行没有被删除
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM assignments WHERE asset_code='DESK-0003'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(total >= 1, "损坏处理不得删除分配历史");

    // 修复后重新可用
    http.post(format!("{}/assets/DESK-0003/repair", app.base))
        .bearer_auth(&logistics)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let avail: Value = http
        .get(format!("{}/assets?available=1", app.base))
        .bearer_auth(&logistics)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(avail.as_array().unwrap().iter().any(|a| a["code"] == "DESK-0003"));
}

#[tokio::test]
async fn suggestions_never_move_assets() {
    let app = spawn_app().await;
    let http = app.client();
    let logistics = app.login("logistics", "admin1234").await;

    // 生成前快照
    let before: Vec<(String, i64)> =
        sqlx::query_as("SELECT code, class_id FROM assets ORDER BY code")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    let active_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM assignments WHERE status='active'")
            .fetch_one(&app.pool)
            .await
            .unwrap();

    let v: Value = http
        .post(format!("{}/suggestions/generate", app.base))
        .bearer_auth(&logistics)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(v["created"].as_i64().unwrap() >= 1, "种子数据应产生建议");

    // 资产位置、占用完全不变
    let after: Vec<(String, i64)> =
        sqlx::query_as("SELECT code, class_id FROM assets ORDER BY code")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(before, after, "生成建议不得改变资产位置");
    let active_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM assignments WHERE status='active'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(active_before, active_after, "生成建议不得改变分配状态");

    // 不匹配学生10（126cm 被配 2 号）应出现 open 建议
    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM suggestions WHERE student_id=10 AND status='open' AND model='1'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(open, 1);
}

#[tokio::test]
async fn student_move_and_reassign_keep_full_history() {
    let app = spawn_app().await;
    let http = app.client();
    let logistics = app.login("logistics", "admin1234").await;

    // 学生10 在 2 班，使用 DESK-0011
    let before_class: i64 = sqlx::query_scalar("SELECT class_id FROM students WHERE id=10")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(before_class, 2);

    // 换班到 3 班
    http.patch(format!("{}/students/10/class", app.base))
        .bearer_auth(&logistics)
        .json(&serde_json::json!({"class_id": 3}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // 学生已换班，桌椅仍在 2 班
    let now_class: i64 = sqlx::query_scalar("SELECT class_id FROM students WHERE id=10")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(now_class, 3);
    let desk_class: i64 = sqlx::query_scalar("SELECT class_id FROM assets WHERE code='DESK-0011'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(desk_class, 2, "换班不得把桌椅带到新班");

    // 原分配记录保留但已 released
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT asset_code, released_at FROM assignments WHERE student_id=10 ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert!(rows.iter().any(|(c, r)| c == "DESK-0011" && r.is_some()));
    assert!(!rows.is_empty(), "换班不得删除分配历史");

    // 该桌椅在 2 班可被再次分配给别人（学生11），旧行仍保留 → 同一资产出现多条历史
    http.post(format!("{}/assignments", app.base))
        .bearer_auth(&logistics)
        .json(&serde_json::json!({"student_id": 11, "asset_code": "DESK-0011", "reason": "重新调配"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let hist: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM assignments WHERE asset_code='DESK-0011'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(hist >= 2, "重新分配应追加历史而不是覆盖");
    let active_for: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM assignments WHERE asset_code='DESK-0011' AND status='active' AND student_id=11",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(active_for, 1);
}

#[tokio::test]
async fn occupied_or_damaged_asset_cannot_be_transferred() {
    let app = spawn_app().await;
    let http = app.client();
    let logistics = app.login("logistics", "admin1234").await;

    // 在用中的 DESK-0001 不能调拨
    let r = http
        .post(format!("{}/transfers", app.base))
        .bearer_auth(&logistics)
        .json(&serde_json::json!({"asset_code": "DESK-0001", "to_class_id": 2}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    // 损坏的 DESK-0007 不能调拨
    let r = http
        .post(format!("{}/transfers", app.base))
        .bearer_auth(&logistics)
        .json(&serde_json::json!({"asset_code": "DESK-0007", "to_class_id": 2}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
}
