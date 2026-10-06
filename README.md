# 课桌椅高度匹配与跨班调拨系统（DeskFlow）

小学场景：班主任登记学生身高 → 系统按身高区间列出**桌椅不匹配的学生**与所需型号 →
后勤从**现有资产**中核实并安排调换；跨班调拨须**交出方与接收方班主任双方确认**后，
资产位置才会更新。系统建议**只做提示，绝不自动改变实物归属**。

技术栈：**Rust + Axum + SQLx + SQLite** 后端，**Svelte (Vite)** 前端，扫码服务本地模拟。

## 一条命令启动

```bash
./run.sh
```

- 首次运行自动安装前端依赖并编译 Rust（需要本机有 `cargo` 与 `node`；没有 Rust 时脚本会通过 rustup 自动安装）。
- 启动后访问 <http://localhost:8080>（前端静态文件由 Axum 一并托管）。
- 数据库自动创建于 `data/deskflow.db`，并写入演示班级、师生、桌椅与初始分配。
- 开发模式（前端热更新）：`./run.sh dev`（前端 :5173，后端 :8080，`/api` 已配置代理）。

可用 `PORT` / `DATABASE_URL` / `APP_SECRET` 环境变量覆盖默认值。

## 演示账号

| 角色 | 用户名 | 口令 | 说明 |
| --- | --- | --- | --- |
| 后勤 | `logistics` | `admin1234` | 全校建议、分配、报损/修复、调拨发起、学生换班 |
| 班主任 | `teacher1` | `pass1234` | 一(1)班 |
| 班主任 | `teacher2` | `pass1234` | 一(2)班 |
| 班主任 | `teacher3` | `pass1234` | 二(1)班 |

## 业务规则与实现要点

- **建议不改变资产**：更新身高或“重新生成建议”只写 `suggestions` 表；
  任何资产的 `class_id`、占用状态均保持不变。分配只能由后勤手动执行。
- **一物一码**：每套桌椅独立编号（如 `DESK-0001`）。
- **并发领用不重复**：`assignments` 上有部分唯一索引
  `UNIQUE(asset_code) WHERE status='active'`，分配使用“条件 INSERT + 事务”，
  多人同时领用同一套桌椅时恰好一个成功，其余收到 409。
- **损坏品不进入可分配列表**：可分配 = 完好 + 无 active 分配 + 无进行中调拨；
  报损会终结占用但保留全部历史。
- **跨班调拨双确认**：`transfers` 记录交出方/接收方两个确认位，只有两方都确认后，
  才在同一事务里条件更新 `assets.class_id`；单方确认时资产不动。
  只有对应班级的班主任能确认本方，后勤不能代确认。
- **历史全程保留**：重新分配只终结旧记录并新增，`asset_trail` 记录
  分配 / 收回 / 报损 / 修复 / 调拨请求、确认、完成、取消全过程。
- **换班不带资产**：学生换班只改学生所属班级，原桌椅留在原班（结束占用、保留记录），
  避免“建议一生成就把资产算到另一个班”。
- **班级数据隔离**：班主任所有查询/修改都校验 `class_id`，访问他班返回 401/403。
- **扫码本地模拟**：`POST /api/assets/scan` 输入编号即可返回资产状态、占用情况与可否分配，
  并写入 `scan_events`；可用 `x-scanner-device` 请求头模拟设备号。

### 身高与型号对照（种子数据）

| 型号 | 0 | 1 | 2 | 3 | 4 | 5 |
| --- | --- | --- | --- | --- | --- | --- |
| 身高 cm | 110–119 | 120–129 | 130–139 | 140–149 | 150–159 | 160–175 |

## 主要接口（均在 `/api` 下，Bearer 令牌认证）

- `POST /auth/login`、`GET /auth/me`
- `GET  /classes`、`GET /models`
- `GET  /classes/{id}/students`、`PATCH /students/{id}/height`
- `POST /suggestions/generate`（后勤）、`GET /suggestions?class_id=&status=`
- `GET  /assets?class_id=&model=&status=&available=1`、`GET /assets/{code}`
- `POST /assets/scan`、`POST /assets/{code}/release|damaged|repair`
- `POST /assignments`（后勤手动分配）
- `PATCH /students/{id}/class`（后勤办理换班）
- `POST /transfers`、`POST /transfers/{id}/confirm`、`POST /transfers/{id}/cancel`、`GET /transfers`
- `GET  /classes/{id}/board`（班级看板：待调换 / 已分配 / 损坏 / 空余 / 调拨）
- `GET  /assets/{code}/trail`（资产去向流水）

## 测试（重点验收项）

```bash
cargo test
```

7 个端到端集成测试覆盖：

1. `concurrent_claim_only_one_succeeds` —— 12 个并发请求抢用同一套桌椅，恰好 1 个 200、11 个 409；
2. `transfer_requires_both_confirmations_and_blocks_outsiders` —— 单方确认资产不动、双方确认才移交，
   无关班主任与后勤代确认均被拒，流水完整；
3. `teacher_cannot_access_other_classes` —— 班主任读写他班数据全部 403、无令牌 401、调拨列表按班过滤；
4. `damaged_assets_leave_pool_and_release_occupant` —— 报损后退出可分配列表、释放占用、历史保留、修复后恢复；
5. `suggestions_never_move_assets` —— 生成建议前后资产位置与占用完全不变；
6. `student_move_and_reassign_keep_full_history` —— 换班后桌椅留原班，重新分配追加历史而非覆盖；
7. `occupied_or_damaged_asset_cannot_be_transferred` —— 在用/损坏资产不能发起调拨。
