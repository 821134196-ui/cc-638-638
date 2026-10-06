// 端到端集成测试：通过真实 HTTP 服务器 + SQLite 验证关键业务约束。
use desk_match::{build_router, db, AppState};
use serde_json::{json, Value};

async fn spawn_app() -> (String, reqwest::Client) {
    let _ = tracing_subscriber::fmt().with_env_filter("error").try_init();
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    // 临时目录必须存活到进程结束：WAL 模式下并发连接会重开 -wal/-shm，
    // 目录提前删除会导致 SQLITE_CANTOPEN。
    std::mem::forget(dir);
    let pool = db::connect(db_path.to_str().unwrap()).await.unwrap();
    db::init(&pool).await.unwrap();
    let state = AppState::new(pool, "/nonexistent-static-dir".into());
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .unwrap();
    (format!("http://{addr}/api"), client)
}

async fn login(base: &str, client: &reqwest::Client, username: &str) {
    let resp = client
        .post(format!("{base}/auth/login"))
        .json(&json!({"username": username, "password": username}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "login {username} failed");
}

async fn get_json(client: &reqwest::Client, url: &str) -> (reqwest::StatusCode, Value) {
    let resp = client.get(url).send().await.unwrap();
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(json!({}));
    (status, body)
}

async fn post_json(client: &reqwest::Client, url: &str, body: Value) -> (reqwest::StatusCode, Value) {
    let resp = client.post(url).json(&body).send().await.unwrap();
    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(json!({}));
    (status, v)
}

fn asset_location(v: &Value) -> Option<i64> {
    v["location_class_id"].as_i64()
}

// ───────────────────────── 1. 并发领用：多人抢同一套桌椅只有一个成功 ─────────────────────────

#[tokio::test]
async fn concurrent_allocation_only_one_succeeds() {
    let (base, client) = spawn_app().await;
    login(&base, &client, "logistics").await;

    // DSK-009（6号，仓库空闲）；学生 4、5 都需要 6号，两个后勤终端同时领用
    let url = format!("{base}/allocations");
    let mut futs = Vec::new();
    for student_id in [4i64, 5] {
        let c = client.clone();
        let u = url.clone();
        futs.push(tokio::spawn(async move {
            let resp = c
                .post(u)
                .json(&json!({"student_id": student_id, "asset_code": "DSK-009"}))
                .send()
                .await
                .unwrap();
            resp.status()
        }));
    }
    let statuses: Vec<_> = futures_join(futs).await;
    let created = statuses.iter().filter(|s| s.is_success()).count();
    let conflict = statuses.iter().filter(|s| s.as_u16() == 409).count();
    assert_eq!(created, 1, "必须恰好一个领用成功，实际 {created}");
    assert_eq!(conflict, 1, "失败方必须收到 409，实际 {conflict}");

    // 库里该资产只分给了一个学生
    let (_, asset) = get_json(&client, &format!("{base}/assets/DSK-009")).await;
    let winner = asset["assigned_student_id"].as_i64();
    assert!(winner == Some(4) || winner == Some(5));
    assert_eq!(asset["status"], "in_use");

    // 再领一次仍然失败
    let (st, _) = post_json(
        &client,
        &url,
        json!({"student_id": 99, "asset_code": "DSK-009"}),
    )
    .await;
    // 学生 99 不存在时会先 404；用真实学生再验一次
    assert!(st == reqwest::StatusCode::CONFLICT || st == reqwest::StatusCode::NOT_FOUND);
    let other_student = if winner == Some(4) { 5 } else { 4 };
    let (st, body) = post_json(
        &client,
        &url,
        json!({"student_id": other_student, "asset_code": "DSK-009"}),
    )
    .await;
    assert_eq!(st, 409, "已被占用的资产再次领用必须 409: {body}");

    // 一个学生不能同时占两套
    let (st, _) = post_json(
        &client,
        &url,
        json!({"student_id": winner.unwrap(), "asset_code": "DSK-010"}),
    )
    .await;
    assert_eq!(st, 409, "同一学生不能占用两套桌椅");
}

#[tokio::test]
async fn many_concurrent_claims_still_exactly_one_winner() {
    // 更高并发：8 个请求同时抢同一套，配合每个学生只能占一套的约束，胜者仍只能有 1 个
    let (base, client) = spawn_app().await;
    login(&base, &client, "logistics").await;

    // 先保证 8 个未分配学生：种子里只有 4、5、8 未分配，其余需先收回。直接用 DSK-010 抢给学生 4/5，
    // 其余学生先 unassign 占用。简单起见：收回 1、2、3、6、7、9、10
    for sid in [1i64, 2, 3, 6, 7, 9, 10] {
        let _ = post_json(&client, &format!("{base}/students/{sid}/unassign"), json!({})).await;
    }
    let free_students = [1i64, 2, 3, 4, 5, 6, 7, 8];

    let url = format!("{base}/allocations");
    let mut futs = Vec::new();
    for sid in free_students {
        let c = client.clone();
        let u = url.clone();
        futs.push(tokio::spawn(async move {
            let resp = c
                .post(u)
                .json(&json!({"student_id": sid, "asset_code": "DSK-010"}))
                .send()
                .await
                .unwrap();
            resp.status().as_u16()
        }));
    }
    let statuses = futures_join(futs).await;
    let winners = statuses.iter().filter(|s| **s == 201).count();
    assert_eq!(winners, 1, "8 个并发请求中必须恰好 1 个 201，实际 {winners}: {statuses:?}");
}

async fn futures_join<F>(futs: Vec<tokio::task::JoinHandle<F>>) -> Vec<F> {
    let mut out = Vec::new();
    for f in futs {
        out.push(f.await.unwrap());
    }
    out
}

// ───────────────────────── 2. 跨班调拨：必须双方确认，完成前资产不动 ─────────────────────────

#[tokio::test]
async fn transfer_requires_both_parties_and_moves_only_at_end() {
    let (base, logi) = spawn_app().await;
    login(&base, &logi, "logistics").await;

    // DSK-008（4号空闲）位于四年1班(class 3)，调给三年1班(class 1)
    let (_, before) = get_json(&logi, &format!("{base}/assets/DSK-008")).await;
    assert_eq!(asset_location(&before), Some(3));

    let (st, body) = post_json(
        &logi,
        &format!("{base}/transfers"),
        json!({"asset_code": "DSK-008", "from_class_id": 3, "to_class_id": 1}),
    )
    .await;
    assert_eq!(st, 201, "{body}");
    let tid = body["transfer_id"].as_i64().unwrap();

    // 发起后资产仍在交出班
    let (_, a) = get_json(&logi, &format!("{base}/assets/DSK-008")).await;
    assert_eq!(asset_location(&a), Some(3), "调拨待确认期间资产位置不得变化");

    // 调拨中的资产不能被领用
    let (st, _) = post_json(
        &logi,
        &format!("{base}/allocations"),
        json!({"student_id": 4, "asset_code": "DSK-008"}),
    )
    .await;
    assert_eq!(st, 409, "在途调拨资产不可分配");

    // 接收方(faculty1)想跳过交出方直接确认 → 409
    let recv = reqwest::Client::builder().cookie_store(true).build().unwrap();
    login(&base, &recv, "faculty1").await;
    let (st, _) = post_json(&recv, &format!("{base}/transfers/{tid}/confirm"), json!({})).await;
    assert_eq!(st, 409, "交出方未放行时接收方确认必须被拒绝");

    // 接收方无权放行；无关班班主任也无权
    let (st, _) = post_json(&recv, &format!("{base}/transfers/{tid}/release"), json!({})).await;
    assert_eq!(st, 403, "接收方不能替交出方放行");
    let other = reqwest::Client::builder().cookie_store(true).build().unwrap();
    login(&base, &other, "faculty2").await;
    let (st, _) = post_json(&other, &format!("{base}/transfers/{tid}/release"), json!({})).await;
    assert_eq!(st, 403, "非当事班级不能操作调拨");

    // 交出方(faculty3)放行
    let give = reqwest::Client::builder().cookie_store(true).build().unwrap();
    login(&base, &give, "faculty3").await;
    let (st, body) = post_json(&give, &format!("{base}/transfers/{tid}/release"), json!({})).await;
    assert_eq!(st, 200, "{body}");

    // 放行后、接收确认前：资产仍在交出班
    let (_, a) = get_json(&logi, &format!("{base}/assets/DSK-008")).await;
    assert_eq!(asset_location(&a), Some(3), "仅放行不确认，资产仍不动");

    // 交出方不能替接收方确认
    let (st, _) = post_json(&give, &format!("{base}/transfers/{tid}/confirm"), json!({})).await;
    assert_eq!(st, 403, "交出方不能替接收方确认");

    // 接收方确认 → 此刻资产才移动
    let (st, body) = post_json(&recv, &format!("{base}/transfers/{tid}/confirm"), json!({})).await;
    assert_eq!(st, 200, "{body}");
    let (_, a) = get_json(&logi, &format!("{base}/assets/DSK-008")).await;
    assert_eq!(asset_location(&a), Some(1), "双方确认后资产必须更新到接收班");

    // 重复确认无效
    let (st, _) = post_json(&recv, &format!("{base}/transfers/{tid}/confirm"), json!({})).await;
    assert_eq!(st, 409);

    // 接收班仪表盘里应出现该空闲资产
    let (st, dash) = get_json(&recv, &format!("{base}/classes/1/dashboard")).await;
    assert_eq!(st, 200);
    let codes: Vec<&str> = dash["available"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"DSK-008"), "接收班应看到调入资产: {codes:?}");

    // 履历完整保留四个节点
    let (_, hist) = get_json(&logi, &format!("{base}/assets/DSK-008/history")).await;
    let types: Vec<&str> = hist["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event_type"].as_str().unwrap())
        .collect();
    for t in ["transfer_confirmed", "transfer_released", "transfer_requested"] {
        assert!(types.contains(&t), "履历缺少 {t}: {types:?}");
    }
}

#[tokio::test]
async fn transfer_rejection_keeps_asset_in_place() {
    let (base, logi) = spawn_app().await;
    login(&base, &logi, "logistics").await;

    let (_, body) = post_json(
        &logi,
        &format!("{base}/transfers"),
        json!({"asset_code": "DSK-013", "from_class_id": 2, "to_class_id": 1}),
    )
    .await;
    let tid = body["transfer_id"].as_i64().unwrap();

    let give = reqwest::Client::builder().cookie_store(true).build().unwrap();
    login(&base, &give, "faculty2").await;
    let (st, _) = post_json(&give, &format!("{base}/transfers/{tid}/reject"), json!({})).await;
    assert_eq!(st, 200);

    let (_, a) = get_json(&logi, &format!("{base}/assets/DSK-013")).await;
    assert_eq!(asset_location(&a), Some(2), "拒绝后资产必须留在交出班");
}

// ───────────────────────── 3. 班主任只能看本班数据 ─────────────────────────

#[tokio::test]
async fn faculty_isolation() {
    let (base, f1) = spawn_app().await;
    login(&base, &f1, "faculty1").await; // 三年1班 class_id=1

    // 直接访问别班学生/仪表盘 → 403
    let (st, _) = get_json(&f1, &format!("{base}/classes/2/students")).await;
    assert_eq!(st, 403);
    let (st, _) = get_json(&f1, &format!("{base}/classes/2/dashboard")).await;
    assert_eq!(st, 403);
    let (st, _) = get_json(&f1, &format!("{base}/classes/3/dashboard")).await;
    assert_eq!(st, 403);
    // 学生 7 属于 class 2
    let (st, _) = get_json(&f1, &format!("{base}/students/7")).await;
    assert_eq!(st, 403);
    let (st, _) = get_json(&f1, &format!("{base}/students/7/history")).await;
    assert_eq!(st, 403);

    // 修改别班学生身高 → 403（接口为 PATCH）
    let resp = f1.patch(format!("{base}/students/7/height"))
        .json(&json!({"height_cm": 130})).send().await.unwrap();
    assert_eq!(resp.status(), 403);

    // 后勤接口对班主任关闭
    let (st, _) = get_json(&f1, &format!("{base}/logistics/mismatches")).await;
    assert_eq!(st, 403);
    let (st, _) = get_json(&f1, &format!("{base}/logistics/suggestions")).await;
    assert_eq!(st, 403);
    let (st, _) = post_json(
        &f1,
        &format!("{base}/allocations"),
        json!({"student_id": 4, "asset_code": "DSK-009"}),
    )
    .await;
    assert_eq!(st, 403);

    // 班级列表只返回本班
    let (_, body) = get_json(&f1, &format!("{base}/classes")).await;
    let ids: Vec<i64> = body["classes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, vec![1]);

    // 资产列表不含别班/仓库
    let (_, body) = get_json(&f1, &format!("{base}/assets")).await;
    for a in body["assets"].as_array().unwrap() {
        assert_eq!(a["location_class_id"].as_i64(), Some(1), "泄露了外班资产: {a}");
    }

    // 本班数据可以正常查看
    let (st, _) = get_json(&f1, &format!("{base}/classes/1/dashboard")).await;
    assert_eq!(st, 200);
    let (st, _) = get_json(&f1, &format!("{base}/students/1")).await;
    assert_eq!(st, 200);

    // 未登录访问 → 401
    let anon = reqwest::Client::new();
    let (st, _) = get_json(&anon, &format!("{base}/classes/1/dashboard")).await;
    assert_eq!(st, 401);
}

// ───────────────────────── 4. 建议只读 / 损坏品不可分配 ─────────────────────────

#[tokio::test]
async fn suggestions_are_readonly_and_damaged_excluded() {
    let (base, logi) = spawn_app().await;
    login(&base, &logi, "logistics").await;

    // 损坏品 DSK-012 绝不出现在建议池里
    let (_, sug) = get_json(&logi, &format!("{base}/logistics/suggestions")).await;
    let text = sug.to_string();
    assert!(!text.contains("DSK-012"), "损坏桌椅不得出现在建议中");

    // 强分损坏品 → 409
    let (st, _) = post_json(
        &logi,
        &format!("{base}/allocations"),
        json!({"student_id": 4, "asset_code": "DSK-012"}),
    )
    .await;
    assert_eq!(st, 409);

    // 多次刷新建议，归属不发生任何变化
    let (_, before) = get_json(&logi, &format!("{base}/assets")).await;
    for _ in 0..3 {
        let _ = get_json(&logi, &format!("{base}/logistics/suggestions")).await;
        let _ = get_json(&logi, &format!("{base}/logistics/mismatches")).await;
    }
    let (_, after) = get_json(&logi, &format!("{base}/assets")).await;
    assert_eq!(before, after, "建议/清单接口不得改变任何资产状态");

    // 报修一件空闲资产后它退出可分配池；维修后恢复
    let (st, _) = post_json(&logi, &format!("{base}/assets/DSK-004/damage"), json!({})).await;
    assert_eq!(st, 200);
    let (st, _) = post_json(
        &logi,
        &format!("{base}/allocations"),
        json!({"student_id": 4, "asset_code": "DSK-004"}),
    )
    .await;
    assert_eq!(st, 409, "损坏后不可分配");
    let (st, _) = post_json(&logi, &format!("{base}/assets/DSK-004/repair"), json!({})).await;
    assert_eq!(st, 200);

    // 身高更新只产生建议，不动资产（路由为 PATCH）
    let resp = logi.patch(format!("{base}/students/2/height"))
        .json(&json!({"height_cm": 120})).send().await.unwrap();
    let st = resp.status();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["needed_model"], "8号");
    let (_, a) = get_json(&logi, &format!("{base}/assets/DSK-002")).await;
    assert_eq!(a["assigned_student_id"].as_i64(), Some(2), "身高更新不得改动资产归属");
}

// ───────────────────────── 5. 学生换班不动资产、历史可追溯 ─────────────────────────

#[tokio::test]
async fn student_transfer_keeps_asset_and_history() {
    let (base, logi) = spawn_app().await;
    login(&base, &logi, "logistics").await;

    // 学生 1 现占 DSK-001，位于 class 1
    let (st, _) = post_json(&logi, &format!("{base}/students/1/transfer-class"), json!({"class_id": 2})).await;
    assert_eq!(st, 200);

    let (_, a) = get_json(&logi, &format!("{base}/assets/DSK-001")).await;
    assert_eq!(asset_location(&a), Some(1), "学生换班，桌椅必须留在原班");
    assert_eq!(a["assigned_student_id"].as_i64(), Some(1), "原分配保留");

    let (_, hist) = get_json(&logi, &format!("{base}/students/1/history")).await;
    let types: Vec<&str> = hist["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event_type"].as_str().unwrap())
        .collect();
    assert!(types.contains(&"assign"));
    assert!(types.contains(&"student_transfer"));

    // 班主任 faculty2（新班）现在能看到该学生；faculty1 不再能看到
    let f2 = reqwest::Client::builder().cookie_store(true).build().unwrap();
    login(&base, &f2, "faculty2").await;
    let (st, _) = get_json(&f2, &format!("{base}/students/1")).await;
    assert_eq!(st, 200);
    let f1 = reqwest::Client::builder().cookie_store(true).build().unwrap();
    login(&base, &f1, "faculty1").await;
    let (st, _) = get_json(&f1, &format!("{base}/students/1")).await;
    assert_eq!(st, 403, "换班后原班主任不再能查看该生");
}
