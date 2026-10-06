use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

use crate::auth::hash_password;

/// 打开/创建 SQLite 文件库并执行迁移
pub async fn init(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;

    // SQLite 写并发测试需要忙等待，避免偶发 SQLITE_BUSY
    sqlx::query("PRAGMA busy_timeout = 5000")
        .execute(&pool)
        .await?;

    migrate(&pool).await?;
    seed(&pool).await?;
    Ok(pool)
}

/// 测试用：打开内存库（每连接独立，因此测试改为临时文件库）
pub async fn init_file(path: &str) -> Result<SqlitePool, sqlx::Error> {
    init(&format!("sqlite://{path}")).await
}

async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let schema = include_str!("../migrations/0001_schema.sql");
    sqlx::raw_sql(schema).execute(pool).await?;
    Ok(())
}

/// 首次启动写入演示数据（已存在用户则跳过）
async fn seed(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let existing: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    if existing.0 > 0 {
        return Ok(());
    }

    let now = chrono::Utc::now().to_rfc3339();

    // 班级
    let classes = [
        ("一(1)班",),
        ("一(2)班",),
        ("二(1)班",),
    ];
    for c in classes {
        sqlx::query("INSERT INTO classes(name) VALUES (?)")
            .bind(c.0)
            .execute(pool)
            .await?;
    }

    // 桌椅型号（身高 cm）
    let models = [
        ("0", "0号桌", 110, 119, 52, 30),
        ("1", "1号桌", 120, 129, 55, 32),
        ("2", "2号桌", 130, 139, 58, 35),
        ("3", "3号桌", 140, 149, 61, 37),
        ("4", "4号桌", 150, 159, 64, 40),
        ("5", "5号桌", 160, 175, 67, 42),
    ];
    for m in models {
        sqlx::query(
            "INSERT INTO desk_models(code,name,height_min,height_max,desk_cm,chair_cm) \
             VALUES (?,?,?,?,?,?)",
        )
        .bind(m.0).bind(m.1).bind(m.2).bind(m.3).bind(m.4).bind(m.5)
        .execute(pool)
        .await?;
    }

    // 用户（演示口令统一：教师 pass1234，后勤 admin1234）
    let pw_t = hash_password("pass1234");
    let pw_l = hash_password("admin1234");
    let users = [
        ("teacher1", "王老师", "teacher", Some(1_i64), &pw_t),
        ("teacher2", "李老师", "teacher", Some(2), &pw_t),
        ("teacher3", "赵老师", "teacher", Some(3), &pw_t),
        ("logistics", "后勤张老师", "logistics", None, &pw_l),
    ];
    for u in users {
        sqlx::query(
            "INSERT INTO users(username,display_name,role,class_id,password_hash) \
             VALUES (?,?,?,?,?)",
        )
        .bind(u.0).bind(u.1).bind(u.2).bind(u.3).bind(u.4)
        .execute(pool)
        .await?;
    }

    // 学生：(班级, 姓名, 身高)
    let students = [
        (1_i64, "陈一鸣", 115), (1, "林小雨", 122), (1, "黄浩然", 131),
        (1, "吴思思", 138), (1, "周子轩", 145), (1, "徐若曦", 152),
        (1, "何俊豪", 128), (1, "高安琪", 160),
        (2, "马天佑", 118), (2, "罗梓萱", 126), (2, "郑博文", 135),
        (2, "梁欣怡", 142), (2, "宋子墨", 150), (2, "谢佳宁", 133),
        (3, "韩磊", 148), (3, "唐糖", 155), (3, "冯瑞", 163), (3, "董梦瑶", 121),
    ];
    for s in students {
        sqlx::query(
            "INSERT INTO students(name,class_id,height_cm,updated_at) VALUES (?,?,?,?)",
        )
        .bind(s.1).bind(s.0).bind(s.2).bind(&now)
        .execute(pool)
        .await?;
    }

    // 资产：(编号, 型号, 班级, 状态, 备注)
    let assets = [
        ("DESK-0001", "0", 1, "ok", ""),
        ("DESK-0002", "1", 1, "ok", ""),
        ("DESK-0003", "2", 1, "ok", ""),
        ("DESK-0004", "3", 1, "ok", ""),
        ("DESK-0005", "4", 1, "ok", ""),
        ("DESK-0006", "1", 1, "ok", ""),
        ("DESK-0007", "0", 1, "damaged", "椅面开裂"),
        ("DESK-0008", "2", 1, "ok", ""),
        ("DESK-0009", "0", 2, "ok", ""),
        ("DESK-0010", "1", 2, "ok", ""),
        ("DESK-0011", "2", 2, "ok", ""),
        ("DESK-0012", "3", 2, "ok", ""),
        ("DESK-0013", "3", 2, "ok", ""),
        ("DESK-0014", "4", 2, "damaged", "桌腿晃动"),
        ("DESK-0015", "3", 3, "ok", ""),
        ("DESK-0016", "4", 3, "ok", ""),
        ("DESK-0017", "5", 3, "ok", ""),
        ("DESK-0018", "1", 3, "ok", ""),
        ("DESK-0019", "2", 2, "ok", ""),
        ("DESK-0020", "0", 3, "ok", ""),
    ];
    for a in assets {
        sqlx::query(
            "INSERT INTO assets(code,model,class_id,status,note,updated_at) \
             VALUES (?,?,?,?,?,?)",
        )
        .bind(a.0).bind(a.1).bind(a.2).bind(a.3).bind(a.4).bind(&now)
        .execute(pool)
        .await?;
    }

    // 初始分配（故意制造一些身高/型号不匹配）
    // (学生id, 资产编号)
    let assigns: [(i64, &str); 12] = [
        (1, "DESK-0001"),  // 115 / 0号 匹配
        (2, "DESK-0002"),  // 122 / 1号 匹配
        (3, "DESK-0008"),  // 131 / 2号 匹配
        (4, "DESK-0003"),  // 138 应配2号，实配2号 匹配
        (5, "DESK-0004"),  // 145 / 3号 匹配
        (6, "DESK-0005"),  // 152 / 4号 匹配
        (7, "DESK-0006"),  // 128 应配1号，实配1号 匹配
        (9, "DESK-0009"),  // 118 应配0号，实配0号 匹配
        (10, "DESK-0011"), // 126 应配1号，实配2号 不匹配
        (11, "DESK-0012"), // 135 应配2号，实配3号 不匹配
        (12, "DESK-0013"), // 142 应配3号，实配3号 匹配
        (15, "DESK-0015"), // 148 应配3号，实配3号 匹配
    ];
    for (sid, code) in assigns {
        sqlx::query(
            "INSERT INTO assignments(asset_code,student_id,assigned_by,status,created_at) \
             VALUES (?,?,(SELECT id FROM users WHERE username='logistics'),'active',?)",
        )
        .bind(code).bind(sid).bind(&now)
        .execute(pool)
        .await?;
        sqlx::query("INSERT INTO asset_trail(asset_code,event,detail,actor_id,created_at) \
                     VALUES (?,'assign','初始分配',(SELECT id FROM users WHERE username='logistics'),?)")
            .bind(code).bind(&now)
            .execute(pool)
            .await?;
    }

    Ok(())
}
