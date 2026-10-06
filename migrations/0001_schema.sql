-- 课桌椅高度匹配与跨班调拨系统 schema (SQLite)

PRAGMA foreign_keys = ON;

-- 班级 ----------------------------------------------------------
CREATE TABLE IF NOT EXISTS classes (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

-- 用户：班主任(teacher) / 后勤(logistics)
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    display_name  TEXT NOT NULL,
    role          TEXT NOT NULL CHECK (role IN ('teacher', 'logistics')),
    class_id      INTEGER REFERENCES classes(id), -- 班主任所属班级，后勤为 NULL
    password_hash TEXT NOT NULL
);

-- 桌椅型号（身高 cm → 推荐型号）
CREATE TABLE IF NOT EXISTS desk_models (
    code        TEXT PRIMARY KEY,       -- 0~5 号
    name        TEXT NOT NULL,
    height_min  INTEGER NOT NULL,       -- 适用身高下限（含）
    height_max  INTEGER NOT NULL,       -- 适用身高上限（含）
    desk_cm     INTEGER NOT NULL,       -- 桌面高度参考
    chair_cm    INTEGER NOT NULL        -- 椅面高度参考
);

-- 学生 ----------------------------------------------------------
CREATE TABLE IF NOT EXISTS students (
    id            INTEGER PRIMARY KEY,
    name          TEXT NOT NULL,
    class_id      INTEGER NOT NULL REFERENCES classes(id),
    height_cm     INTEGER,
    desired_model TEXT REFERENCES desk_models(code),
    active        INTEGER NOT NULL DEFAULT 1,
    updated_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_students_class ON students(class_id);

-- 资产：每套桌椅独立编号，物理位置 = class_id --------------------
CREATE TABLE IF NOT EXISTS assets (
    code       TEXT PRIMARY KEY,        -- 独立编号，如 DESK-0001
    model      TEXT NOT NULL REFERENCES desk_models(code),
    class_id   INTEGER NOT NULL REFERENCES classes(id),
    status     TEXT NOT NULL CHECK (status IN ('ok', 'damaged')),
    note       TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_assets_class ON assets(class_id);

-- 分配记录（重新分配/换班只新增或终结记录，旧行永久保留）--------
CREATE TABLE IF NOT EXISTS assignments (
    id          INTEGER PRIMARY KEY,
    asset_code  TEXT NOT NULL REFERENCES assets(code),
    student_id  INTEGER NOT NULL REFERENCES students(id),
    assigned_by INTEGER NOT NULL REFERENCES users(id),
    status      TEXT NOT NULL CHECK (status IN ('active', 'released')),
    reason      TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    released_at TEXT
);
-- 关键约束：同一套桌椅任何时刻最多只有一条 active 分配
CREATE UNIQUE INDEX IF NOT EXISTS idx_assign_active_asset
    ON assignments(asset_code) WHERE status = 'active';
CREATE INDEX IF NOT EXISTS idx_assign_student ON assignments(student_id);

-- 系统建议：只生成建议，绝不据此直接移动资产 --------------------
CREATE TABLE IF NOT EXISTS suggestions (
    id          INTEGER PRIMARY KEY,
    student_id  INTEGER NOT NULL REFERENCES students(id),
    model       TEXT NOT NULL REFERENCES desk_models(code),
    reason      TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'open'
                CHECK (status IN ('open', 'fulfilled', 'obsolete')),
    created_at  TEXT NOT NULL,
    resolved_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_suggestions_student ON suggestions(student_id);

-- 跨班调拨：交出方与接收方教师双确认后才移动资产 ----------------
CREATE TABLE IF NOT EXISTS transfers (
    id                 INTEGER PRIMARY KEY,
    asset_code         TEXT NOT NULL REFERENCES assets(code),
    from_class_id      INTEGER NOT NULL REFERENCES classes(id),
    to_class_id        INTEGER NOT NULL REFERENCES classes(id),
    requested_by       INTEGER NOT NULL REFERENCES users(id),
    from_confirmed_by  INTEGER REFERENCES users(id),
    to_confirmed_by    INTEGER REFERENCES users(id),
    from_confirmed_at  TEXT,
    to_confirmed_at    TEXT,
    status             TEXT NOT NULL
                       CHECK (status IN ('pending', 'completed', 'cancelled')),
    created_at         TEXT NOT NULL,
    completed_at       TEXT,
    cancelled_at       TEXT,
    cancel_reason      TEXT NOT NULL DEFAULT '',
    CHECK (from_class_id <> to_class_id)
);
CREATE INDEX IF NOT EXISTS idx_transfers_asset ON transfers(asset_code);
-- 同一套桌椅同时只能有一条进行中的调拨
CREATE UNIQUE INDEX IF NOT EXISTS idx_transfers_pending_asset
    ON transfers(asset_code) WHERE status = 'pending';

-- 资产去向流水（分配/释放/损坏/调拨全过程）----------------------
CREATE TABLE IF NOT EXISTS asset_trail (
    id         INTEGER PRIMARY KEY,
    asset_code TEXT NOT NULL REFERENCES assets(code),
    event      TEXT NOT NULL,
    detail     TEXT NOT NULL DEFAULT '',
    actor_id   INTEGER REFERENCES users(id),
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_trail_asset ON asset_trail(asset_code);

-- 扫码服务（本地模拟）------------------------------------------
CREATE TABLE IF NOT EXISTS scan_events (
    id         INTEGER PRIMARY KEY,
    asset_code TEXT NOT NULL REFERENCES assets(code),
    actor_id   INTEGER NOT NULL REFERENCES users(id),
    action     TEXT NOT NULL,
    result     TEXT NOT NULL,
    created_at TEXT NOT NULL
);
