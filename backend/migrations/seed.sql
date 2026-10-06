-- 演示数据（仅在空库时执行）
-- 型号表：GB/T 3976 常用 0~10 号（身高区间为本系统采用的整数化分档）
INSERT INTO desk_models (model_no, code, height_min_cm, height_max_cm, desk_height_cm, chair_height_cm) VALUES
 (0, '0号',  180, 220, 76, 44),
 (1, '1号',  173, 179, 73, 42),
 (2, '2号',  165, 172, 70, 40),
 (3, '3号',  158, 164, 67, 38),
 (4, '4号',  150, 157, 64, 36),
 (5, '5号',  143, 149, 61, 34),
 (6, '6号',  135, 142, 58, 32),
 (7, '7号',  128, 134, 55, 30),
 (8, '8号',  120, 127, 52, 29),
 (9, '9号',  113, 119, 49, 27),
 (10,'10号', 80, 112, 46, 24);

INSERT INTO classes (id, name, grade) VALUES
 (1, '三年级1班', 3),
 (2, '三年级2班', 3),
 (3, '四年级1班', 4);

INSERT INTO users (username, password, display_name, role, class_id) VALUES
 ('faculty1',   'faculty1',   '王老师（三年1班班主任）', 'faculty',   1),
 ('faculty2',   'faculty2',   '李老师（三年2班班主任）', 'faculty',   2),
 ('faculty3',   'faculty3',   '张老师（四年1班班主任）', 'faculty',   3),
 ('logistics',  'logistics',  '赵师傅（后勤）',          'logistics', NULL);

-- 学生。故意制造几种情形：桌椅型号偏大/偏小、尚未分配、型号合适
INSERT INTO students (id, name, class_id, height_cm, updated_at) VALUES
 (1, '陈小华', 1, 132, datetime('now')),  -- 应用 7号，现占 6号（偏大）
 (2, '林小雨', 1, 145, datetime('now')),  -- 5号，合适
 (3, '赵大力', 1, 152, datetime('now')),  -- 应用 4号，现占 7号（偏小）
 (4, '孙小美', 1, 138, datetime('now')),  -- 6号，尚未分配
 (5, '钱多多', 1, 141, datetime('now')),  -- 6号，尚未分配（与孙小美争同一型号）
 (6, '周杰',   2, 140, datetime('now')),  -- 6号，合适
 (7, '吴敏',   2, 126, datetime('now')),  -- 应用 8号，现占 6号（偏大）
 (8, '郑磊',   2, 148, datetime('now')),  -- 5号，尚未分配
 (9, '王芳',   3, 155, datetime('now')),  -- 4号，合适
 (10,'冯强',   3, 160, datetime('now'));  -- 应用 3号，现占 5号（偏小）

-- 资产：每套独立编号。NULL 班级 = 后勤仓库
INSERT INTO assets (code, model_code, status, location_class_id, assigned_student_id, updated_at) VALUES
 ('DSK-001', '6号', 'in_use',    1,    1, datetime('now')),
 ('DSK-002', '5号', 'in_use',    1,    2, datetime('now')),
 ('DSK-003', '7号', 'in_use',    1,    3, datetime('now')),
 ('DSK-004', '7号', 'available', 1, NULL, datetime('now')),  -- 本班就有适合陈小华的
 ('DSK-012', '8号', 'damaged',   1, NULL, datetime('now')),  -- 损坏，不可分配
 ('DSK-005', '6号', 'in_use',    2,    7, datetime('now')),
 ('DSK-013', '8号', 'available', 2, NULL, datetime('now')),  -- 本班有适合吴敏的
 ('DSK-007', '5号', 'in_use',    3,   10, datetime('now')),
 ('DSK-008', '4号', 'available', 3, NULL, datetime('now')),  -- 适合赵大力但在四年1班 → 需跨班调拨
 ('DSK-009', '6号', 'available', NULL, NULL, datetime('now')),-- 仓库，适合孙小美/钱多多（仅一套）
 ('DSK-010', '5号', 'available', NULL, NULL, datetime('now')),-- 仓库，适合郑磊
 ('DSK-011', '3号', 'available', NULL, NULL, datetime('now'));-- 仓库，适合冯强

INSERT INTO asset_history (asset_code, event_type, from_class_id, to_class_id, from_student_id, to_student_id, actor_user_id, detail, created_at) VALUES
 ('DSK-001','assign', NULL, 1, NULL, 1, NULL, '初始分配', datetime('now')),
 ('DSK-002','assign', NULL, 1, NULL, 2, NULL, '初始分配', datetime('now')),
 ('DSK-003','assign', NULL, 1, NULL, 3, NULL, '初始分配', datetime('now')),
 ('DSK-005','assign', NULL, 2, NULL, 7, NULL, '初始分配', datetime('now')),
 ('DSK-007','assign', NULL, 3, NULL, 10,NULL, '初始分配', datetime('now'));
