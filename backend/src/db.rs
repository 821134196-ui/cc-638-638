use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{SqliteConnection, SqlitePool};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

/// 连接 SQLite：开启外键、WAL，并设置忙等——并发写事务（BEGIN IMMEDIATE）会排队而不是直接失败。
pub async fn connect(url: &str) -> anyhow::Result<SqlitePool> {
    // 支持传入文件路径或 sqlx URL
    let opts = if url.contains("://") {
        SqliteConnectOptions::from_str(url)?
    } else {
        if let Some(parent) = Path::new(url).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        SqliteConnectOptions::new().filename(url).create_if_missing(true)
    };

    let opts = opts
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(10));

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await?;
    Ok(pool)
}

const SCHEMA: &str = include_str!("../migrations/0001_init.sql");
const SEED: &str = include_str!("../migrations/seed.sql");

/// BEGIN IMMEDIATE 事务守卫：begin 时立即取得 RESERVED 写锁，
/// 并发写请求在 busy_timeout 内排队，避免 DEFERRED 事务“各持共享锁后升级死锁”。
/// 这是“多人同时领用同一套桌椅只有一个成功”的关键：
/// 后来者排队拿到写锁后，条件 UPDATE 影响 0 行 → 业务层返回 409。
///
/// 守卫独占一条连接；未 commit 就 drop 时连接关闭，SQLite 自动回滚。
pub struct ImmediateTx {
    conn: SqliteConnection,
    committed: bool,
}

impl ImmediateTx {
    pub async fn begin(pool: &SqlitePool) -> Result<Self, sqlx::Error> {
        let mut conn: SqliteConnection = pool.acquire().await?.detach();
        sqlx::query("BEGIN IMMEDIATE").execute(&mut conn).await?;
        Ok(Self { conn, committed: false })
    }

    pub async fn commit(mut self) -> Result<(), sqlx::Error> {
        sqlx::query("COMMIT").execute(&mut self.conn).await?;
        self.committed = true;
        Ok(())
    }
}

impl Deref for ImmediateTx {
    type Target = SqliteConnection;
    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}

impl DerefMut for ImmediateTx {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.conn
    }
}

impl Drop for ImmediateTx {
    fn drop(&mut self) {
        if !self.committed {
            tracing::warn!("写事务未提交即结束，连接关闭将自动回滚");
        }
    }
}

/// 建表（幂等）并在空库时写入演示数据。
pub async fn init(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::raw_sql(SCHEMA).execute(pool).await?;

    let user_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    if user_count.0 == 0 {
        sqlx::raw_sql(SEED).execute(pool).await?;
    }
    Ok(())
}
