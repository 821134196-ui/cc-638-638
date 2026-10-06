-- 课桌椅高度匹配与跨班调拨系统 · SQLite schema
-- 设计要点：
-- 1) assets.assigned_student_id 上的 UNIQUE 索引保证“一个学生最多一套”，
--    而 assets 一行只存一个 assigned_student_id，天然保证“一套桌椅不能同时分给两名学生”；
--    并发领用由条件 UPDATE（assigned_student_id IS NULL）+ BEGIN IMMEDIATE 共同保证只有一个成功。
-- 2) 损坏品 status='damaged'，可分配池查询只取 status='available'。
-- 3) 调拨单 pending/released 期间资产位置不变，只有双方确认完成才 UPDATE assets.location_class_id。
-- 4) asset_history 只追加，不修改不删除——换班/重新配桌椅后原分配与调拨经过仍可追溯。

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS classes (
    id      INTEGER PRIMARY KEY,
    name    TEXT NOT NULL UNIQUE,
    grade   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
    id          INTEGER PRIMARY KEY,
    username    TEXT NOT NULL UNIQUE,
    password    TEXT NOT NULL,                     -- 演示用明文，仅本地
    display_name TEXT NOT NULL,
    role        TEXT NOT NULL CHECK (role IN ('faculty', 'logistics')),
    class_id    INTEGER REFERENCES classes(id)     -- faculty 必填；logistics 为 NULL
);

-- 课桌椅型号（依据 GB/T 3976 学生身高分档，单位厘米，区间为整数化后的 [min, max]）
CREATE TABLE IF NOT EXISTS desk_models (
    model_no       INTEGER PRIMARY KEY,            -- 0 号最高 … 10 号最低
    code           TEXT NOT NULL UNIQUE,           -- '0号' … '10号'
    height_min_cm  INTEGER NOT NULL,
    height_max_cm  INTEGER NOT NULL,
    desk_height_cm INTEGER NOT NULL,
    chair_height_cm INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS students (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    class_id    INTEGER NOT NULL REFERENCES classes(id),
    height_cm   INTEGER NOT NULL CHECK (height_cm BETWEEN 80 AND 220),
    updated_at  TEXT NOT NULL,
    UNIQUE (class_id, name)
);
CREATE INDEX IF NOT EXISTS idx_students_class ON students(class_id);

-- 每套桌椅独立编号
CREATE TABLE IF NOT EXISTS assets (
    code                TEXT PRIMARY KEY,
    model_code          TEXT NOT NULL REFERENCES desk_models(code),
    -- in_use 使用中 / available 空闲可分配 / damaged 损坏（不进入可分配列表）
    status              TEXT NOT NULL DEFAULT 'available'
                        CHECK (status IN ('in_use', 'available', 'damaged')),
    location_class_id   INTEGER REFERENCES classes(id),  -- 当前实物所在班级（调拨完成才变）
    assigned_student_id INTEGER UNIQUE REFERENCES students(id), -- 现占用人；NULL=未占用
    updated_at          TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_assets_class ON assets(location_class_id);
CREATE INDEX IF NOT EXISTS idx_assets_status ON assets(status);

-- 跨班调拨单
CREATE TABLE IF NOT EXISTS transfers (
    id            INTEGER PRIMARY KEY,
    asset_code    TEXT NOT NULL REFERENCES assets(code),
    from_class_id INTEGER NOT NULL REFERENCES classes(id),
    to_class_id   INTEGER NOT NULL REFERENCES classes(id),
    -- pending 待交出方确认 / released 交出方已放行、待接收方确认 /
    -- confirmed 双方确认完成 / rejected 任一方拒绝 / cancelled 作废
    status        TEXT NOT NULL DEFAULT 'pending'
                  CHECK (status IN ('pending', 'released', 'confirmed', 'rejected', 'cancelled')),
    requested_by  INTEGER NOT NULL REFERENCES users(id),
    released_at   TEXT,
    released_by   INTEGER REFERENCES users(id),
    confirmed_at  TEXT,
    confirmed_by  INTEGER REFERENCES users(id),
    rejected_at   TEXT,
    rejected_by   INTEGER REFERENCES users(id),
    created_at    TEXT NOT NULL,
    completed_at  TEXT,
    -- 表级约束必须放在所有列定义之后（SQLite 语法要求）
    CHECK (from_class_id <> to_class_id)
);
CREATE INDEX IF NOT EXISTS idx_transfers_asset ON transfers(asset_code);
CREATE INDEX IF NOT EXISTS idx_transfers_status ON transfers(status);
-- 同一套桌椅在途调拨唯一：防止被多次调拨、也防止调拨中被领用
CREATE UNIQUE INDEX IF NOT EXISTS uq_active_transfer
    ON transfers(asset_code) WHERE status IN ('pending', 'released');

-- 只追加的资产履历：分配 / 收回 / 调拨各节点 / 损坏 / 维修 / 学生换班
CREATE TABLE IF NOT EXISTS asset_history (
    id              INTEGER PRIMARY KEY,
    asset_code      TEXT,
    event_type      TEXT NOT NULL,
    from_class_id   INTEGER,
    to_class_id     INTEGER,
    from_student_id INTEGER,
    to_student_id   INTEGER,
    actor_user_id   INTEGER,
    detail          TEXT,
    created_at      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_history_asset ON asset_history(asset_code);
CREATE INDEX IF NOT EXISTS idx_history_student ON asset_history(to_student_id, from_student_id);

CREATE TABLE IF NOT EXISTS sessions (
    token      TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
