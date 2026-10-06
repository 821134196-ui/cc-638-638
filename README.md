# 小学课桌椅高度匹配与跨班调拨系统

班主任更新学生身高后，系统列出**身高与桌椅型号不匹配**的学生及现有可用资产；由**后勤核实后**再办理分配。
系统只生成建议，**建议本身不改变任何实物归属**；跨班调拨须交出方与接收方**双方确认**后才更新资产位置。

技术栈：Rust + Axum 0.8 + SQLx + SQLite（后端，同时托管前端静态资源）· Svelte 4 + Vite（前端）。

## 一条命令启动

```bash
./run.sh            # 首次会自动 npm install、cargo build、建库并写入演示数据
# 可选：
./run.sh --release  # release 模式
./run.sh --fresh    # 清空数据库重新生成演示数据
```

启动后访问 <http://localhost:3000>。

演示账号（密码同账号）：

| 账号 | 角色 |
| --- | --- |
| `faculty1` | 三年1班班主任（只能看本班） |
| `faculty2` | 三年2班班主任 |
| `faculty3` | 四年1班班主任 |
| `logistics` | 后勤（全校资产、分配、调拨、扫码、换班） |

开发模式前端热更新：另开终端 `cd frontend && npm run dev`（5173 端口，`/api` 代理到 3000）。

## 业务规则与实现位置

- **身高→型号建议**：`src/matching.rs`（纯函数，GB/T 3976 整数化分档 0~10 号）。
  `/logistics/suggestions` 是**只读查询**，不预留、不锁定、不改归属；页面上明确提示“请核实后分配”。
- **班级页三类桌椅**：`GET /api/classes/{id}/dashboard` 返回待调换、已分配（含本班空闲）、损坏。
- **班主任隔离**：`AuthUser::ensure_class_access`（`src/auth.rs`）。学生、仪表盘、资产、调拨列表、
  身高修改全部按班级鉴权；越权返回 **403**，未登录 **401**。
- **每套桌椅独立编号、不能同时分给两名学生**：`assets.code` 主键、
  `assigned_student_id UNIQUE`，分配用条件更新
  `… WHERE assigned_student_id IS NULL AND status='available'`。
- **并发领用只成功一个**：所有写事务用 `BEGIN IMMEDIATE`（`src/db.rs::ImmediateTx`）立即取写锁、
  配合 10s busy_timeout 排队；后来者条件更新影响 0 行 → **409**。SQLite 层 UNIQUE 索引是第二道防线。
- **损坏品不进入可分配列表**：建议池只查 `status='available'`；强分损坏品返回 409；
  维修入库后恢复。
- **跨班调拨双方确认**：`pending`（交出方放行）→ `released`（接收方确认）→ `confirmed`。
  **只有** confirm 同一事务里更新 `assets.location_class_id`；在途资产既不出现在建议池也不能被领用
  （`uq_active_transfer` 部分唯一索引 + 接口校验）。
- **历史可追溯**：`asset_history` 只追加（分配/收回/调拨三节点/损坏/维修/身高更新/学生换班）。
  学生换班只改 `students.class_id`，桌椅留在原班、原分配保留。
- **扫码**：本地模拟，后勤页输入/下拉编号调 `GET /api/assets/{code}` 查看状态、报修、收回、看履历。

## 验证

```bash
cd backend && cargo test
```

9 个测试全部通过，其中覆盖题目要求的三项验收：

1. `concurrent_allocation_only_one_succeeds` / `many_concurrent_claims_still_exactly_one_winner`
   —— 2 路与 8 路同时领用同一套桌椅，**恰好 1 个 201，其余 409**，库中仅一名学生占用；
   同一学生不能占两套。
2. `transfer_requires_both_parties_and_moves_only_at_end` —— 发起后/放行后资产均不动，
   跳过交出方确认、对方越权代确认均被拒，双方确认后资产才移动，重复确认 409，履历三节点齐全；
   `transfer_rejection_keeps_asset_in_place` 验证拒绝后资产留在交出班。
3. `faculty_isolation` —— 班主任访问别班仪表盘/学生/履历/改身高均 **403**，
   后勤接口 **403**，班级与资产列表只含本班，未登录 **401**。

另有 `suggestions_are_readonly_and_damaged_excluded`（建议只读、损坏品排除与强分 409）
与 `student_transfer_keeps_asset_and_history`（换班不动资产、履历保留、权限随班变更）。

## 目录

```
backend/
  migrations/0001_init.sql   表结构与并发约束（含部分唯一索引）
  migrations/seed.sql        演示数据
  src/                       axum 路由、鉴权、匹配、分配与调拨状态机
  tests/api.rs               端到端集成测试（真实 HTTP + SQLite）
frontend/
  src/components/            登录、班主任工作台、后勤调拨台、资产履历
run.sh                       一条命令构建并启动
```
