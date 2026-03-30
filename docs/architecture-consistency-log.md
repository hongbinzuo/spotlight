# 架构一致性记录

本文档用于记录 Spotlight 当前已经确认的状态模型、流程模型和实现缺口。

目标不是“写个说明”。
目标是让后续任何会话、任何 Agent 接手时，都能基于同一份约束继续推进，不把系统带回混乱状态。

## 2026-03-30：`auto_claim_next_task` 选择规则与唯一事实来源收口

### 1. 当前发现的问题

- `task_ops.rs` 与 `git_ops.rs` 曾同时保留 `auto_claim_next_task` 相关实现
- 两处实现的选择顺序不一致，历史上已经出现过“先优先级还是先队列归属”的语义分叉
- 如果不明确唯一事实来源，后续很容易再次在 legacy 分支上演化出第二套规则

### 2. 本次对齐结论

- 当前生产语义统一以 `apps/server/src/task_ops.rs` 为唯一事实来源
- `auto_claim_next_task` 的筛选与排序规则收敛为：
  - 仅从 `Open` 且未被认领的任务中选择
  - 先过滤掉与当前 Agent 不匹配的定向任务、不可见队列任务、以及存在活动冲突的任务
  - 排序时先看队列归属，再看优先级，再看创建时间
- 当前队列归属顺序为：
  - `AssignedAgent` 定向 Agent 队列
  - 绑定到 Agent 所属用户的本人待办队列
  - 共享待办队列
- 当前同一队列内的优先级顺序为：
  - `High`
  - `Medium`
  - `Low`
  - `None`
- 若 Agent 未开启 `auto_mode`，或当前已有进行中任务，则本轮不自动认领

### 3. 本次新增约束

- 后续修改 `auto_claim_next_task` 行为时，只允许在 `apps/server/src/task_ops.rs` 演进主实现
- `apps/server/src/git_ops.rs` 中残留的 `auto_claim_*` 只视为待删除 legacy 分支，在编码污染修复前不得重新接回主入口
- 若调整队列归属或优先级语义，必须同时更新：
  - 决策记录
  - 回归测试
  - 任务活动中的选择依据文案

### 4. 关联实现与验证

- 实现：`apps/server/src/task_ops.rs`
- legacy 残留：`apps/server/src/git_ops.rs`
- 回归测试：`auto_claim_next_task_prefers_owner_queue_before_shared_priority`

## 2026-03-29：服务端入口路由装配与 `main.rs` 边界收口

### 1. 当前发现的问题

- `handlers.rs` 与 `task_ops.rs` 已经存在明确职责归属
- 但 `server.rs` 的路由装配仍然直接依赖 `main.rs` 中的私有 handler
- 结果是模块已经拆出，真实入口却仍挂在旧实现上，`main.rs` 收口边界不成立

### 2. 本次对齐结论

- `server.rs` 的路由装配直接绑定 `handlers.rs`
- 入口装配继续通过 `state.rs` 初始化状态、通过 `automation.rs` 启动后台循环
- `main.rs` 的角色收敛为：
  - 模块声明
  - 顶层共享类型
  - 启动入口
  - 极少量跨模块共享胶水

### 3. 本次新增约束

- 新增服务端 API 时，默认写入 `handlers.rs`，再由 `server.rs` 装配
- 已经在专属模块中存在实现的职责，不允许继续在 `main.rs` 保留并演进重复版本
- 后续继续清理 `main.rs` 时，应优先删除重复实现，而不是再扩张入口文件

### 4. 关联文档

- `docs/main-rs-entry-boundary-v1.md`
- `docs/clawteam-adoption-todo.md`

## 2026-03-20：服务端骨架 API 前缀与统一入口对齐

### 1. 当前发现的问题

- 文档在 `docs/api-design.md` 中明确把中心服务前缀定义为 `/api/v1`
- 现有 Rust 服务端和入口页脚本主要走 `/api/...`
- 结果是统一入口页面能用，但代码实现与文档事实来源不一致

### 2. 本次对齐结论

- `GET /` 继续作为统一入口页面
- 服务端新增 `/api/v1` 版本化路由装配，覆盖当前入口页依赖的服务骨架能力
- `/api` 继续保留为兼容层，避免打断现有脚本、测试和后续渐进迁移
- 统一入口页请求封装默认改为优先访问 `/api/v1`

### 3. 本次最小骨架范围

- `GET /api/v1/board`
- `GET /api/v1/projects`
- `GET /api/v1/projects/{project_id}/tasks`
- `GET /api/v1/agents`

其中：

- 项目列表用于左侧项目切换
- 项目任务列表用于任务看板骨架
- Agent 列表用于右侧 Agent 面板骨架
- `board` 仍保留为统一入口页的聚合快照能力

### 4. 后续约束

- 后续新增中心服务接口时，优先落在 `/api/v1`
- 若暂时保留 `/api` 兼容路由，必须在文档中明确说明兼容性质
- 到 `0.1.1` 看板增强时，再决定是否缩减 `board` 聚合面的职责

## 2026-03-19：任务撤销语义与自动执行链路缺口

### 1. 当前发现的问题

#### 1.1 任务状态缺少“撤销 / 不做了”的终态

当前 `TaskStatus` 只有：

- `OPEN`
- `CLAIMED`
- `RUNNING`
- `PAUSED`
- `DONE`
- `FAILED`

这会导致一个明显问题：

- 用户中途改变方向
- 某个需求被明确叫停
- 某个任务被后续方案替代

这些都不是“做完了”，也不是“失败了”。

如果仍然把这类任务留在：

- `OPEN`
- `PAUSED`
- `DONE`
- `FAILED`

都会污染统计、流程和后续自动认领逻辑。

#### 1.2 当前自动认领 / 自动执行依赖 UI 轮询，不是服务端自治

当前自动链路本质上在服务端页面前端脚本里：

- 页面 `loadBoard()`
- 页面里的 `runAutoModeQueue()`
- 前端定时器 `setInterval(loadBoard, 2500)`

这意味着：

- 只有嵌入页面真的在跑，这个自动链路才会触发
- 如果只是桌面壳启动了，但内嵌页面没有稳定运行，自动链路不会发生
- 如果页面卡住、隐藏、被替换或没有渲染，服务端本身不会主动拉任务并启动执行

结论：

- 当前“自动执行”还不是系统能力
- 当前只是“页面驱动的自动执行”

这和 Spotlight 的目标不一致。

### 2. 架构结论

#### 2.1 任务需要区分“流程状态”和“关闭结论”

当前只用一个 `TaskStatus` 承担两件事：

- 过程状态
- 最终结论

这是不稳定的。

推荐目标模型：

- `lifecycle_status`
  - `OPEN`
  - `CLAIMED`
  - `RUNNING`
  - `PAUSED`
  - `CLOSED`
- `resolution`
  - `DONE`
  - `FAILED`
  - `CANCELED`
  - `SUPERSEDED`

如果暂时不想一次性重构成双字段，最小可行方案是先补一个终态：

- `CANCELED`

并增加关闭原因字段，例如：

- `close_reason`
- `closed_by_user_id`
- `closed_at`
- `canceled_from_task_id`（如被替代）

#### 2.2 “用户明确叫停”必须进入可审计终态

当出现以下语义时：

- 不做了
- 先不要做
- 这个方案撤销
- 改成另一条任务
- 刚才那个需求取消

系统不应该简单保留原任务继续挂着。

系统应该：

1. 识别到这是“中止 / 撤销”而不是“完成”
2. 把相关未完成任务关闭到撤销态
3. 写活动日志
4. 记录是谁、何时、因为什么撤销
5. 如果新方向替代旧方向，建立任务关联

#### 2.3 自动执行必须迁移到服务端后台调度器

正确的职责边界应该是：

- 桌面端负责展示、输入、重启、前台交互
- 服务端负责：
  - 任务选择
  - 自动认领
  - 自动启动
  - 卡死恢复
  - 状态推进

也就是说：

- 不应依赖页面 `setInterval` 驱动自动执行
- 应由服务端后台 loop / scheduler 周期性扫描

### 3. 当前仓库中的直接证据

已确认：

- `crates/platform-core/src/lib.rs`
  - `TaskStatus` 只有 `Open / Claimed / Running / Paused / Done / Failed`
- `apps/server/src/main.rs`
  - 自动认领入口：`/api/agents/{agent_id}/pull-next`
  - 启动入口：`/api/tasks/{task_id}/start/{agent_id}`
  - 任务自动选择逻辑：`auto_claim_next_task(...)`
- `apps/desktop/src/main.js`
  - 只有桌面壳状态探测和 iframe 加载
  - 没有原生侧的任务调度逻辑

### 4. 必做改造

#### 4.1 第一优先级

- 增加任务终态 `CANCELED`
- 增加撤销 API
- 在 UI 中展示“已撤销”而不是误显示“已完成/失败”
- 自动认领逻辑跳过撤销任务

#### 4.2 第二优先级

- 把自动认领 + 自动启动从页面 JS 挪到服务端后台 loop
- 服务端启动后即自治运行
- 桌面端只负责显示状态，不负责触发调度

#### 4.3 第三优先级

- 增加“干预识别器”
- 对用户最新指令做语义判断：
  - 是补充说明
  - 是暂停
  - 是取消
  - 是替代方案
- 自动把受影响的任务做状态收口

### 5. 后续会话约束

后续任何会话继续这块时，必须遵守：

- 不再把“取消 / 不做了”的任务混入 `DONE`
- 不再把“自动执行”建立在 UI 轮询之上
- 服务端必须成为自动调度的唯一事实执行者
- 任何新增状态必须同步补测试和迁移逻辑
