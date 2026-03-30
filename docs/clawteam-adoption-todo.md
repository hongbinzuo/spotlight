# ClawTeam / Network-AI 借鉴落地 TODO

## 1. 目标说明

这份 TODO 文档用于把“借鉴 ClawTeam 与 Network-AI”从一次性讨论，变成可逐步交付的实现清单。

这里说的“借鉴完成”，不是复制它们的 CLI 外壳、OpenClaw 兼容胶水、本地 JSON 存储或 npm 包结构，而是把对 Spotlight 真正关键的执行内核与协调治理能力吸收进来，并用 Spotlight 自己的架构语言重写。

当前以以下文档为事实基础：

- [docs/clawteam-reference-gap-analysis.md](./clawteam-reference-gap-analysis.md)
- [docs/network-ai-reference-gap-analysis.md](./network-ai-reference-gap-analysis.md)
- [docs/reference-project-refresh-2026-03-30.md](./reference-project-refresh-2026-03-30.md)

## 2. 借鉴完成定义

满足以下条件，才算这轮参考借鉴真正完成：

- ClawTeam 的执行层 4 类能力和 Network-AI 的治理层 5 类能力都已落地，或已明确记录“不采纳”的平台级原因
- 执行模型从 `task + thread + 主工作区` 升级到 `execution slot + workspace lease + runtime session`
- 同一仓库内的并行不再依赖共享主工作区切分支
- worker 间至少具备最小结构化协作原语
- 任务依赖、handoff、自动解锁进入数据模型与调度逻辑
- 长任务恢复从“把 thread 挂回 task”升级到“恢复某个执行槽”
- 协调层具备原子共享状态、能力授权、预算熔断、journey 合规与稳定上下文包
- `main.rs` 收敛成真正的入口文件，而不是继续承载大量业务逻辑

## 3. 当前状态

### 已完成

- 已形成 ClawTeam 差距分析文档，明确 4 类最值得借鉴的能力
- 已形成 Network-AI 差距分析文档，明确其更适合作为协调治理层参考
- 已把执行约束从“全局串行”收缩到“按工作区串行”
- 已补按工作区串行相关回归测试
- 已把该过渡规则写入状态机与恢复文档
- `automation.rs` 已接管当前主线的 watchdog / auto-start / auto-resume 行为，`main.rs` 中对应 legacy 自动化实现已物理删除
- 已补自动启动 / 自动恢复会写入 run history 的回归测试：
  - `automation_cycle_auto_start_records_run_history`
  - `automation_cycle_auto_resume_records_run_history`
- 已在较大重构前创建基线 tag：
  - `pre-workspace-serialization-refactor-20260328`
  - `pre-main-rs-refactor-20260329`
  - `pre-automation-legacy-prune-20260329`
  - `pre-execution-slot-coordination-model-20260329`
- 已落地 `execution slot + workspace lease + coordination_write_intent` 的最小数据模型
- 已让 persisted state 在服务端启动时为旧 run 回填 slot / lease，并补最小治理意图记录
- 已让 `record_task_run_start / record_task_run_transition` 接通 slot / lease 生命周期
- 已补执行槽位相关最小回归测试：
  - `normalize_persisted_state_backfills_execution_slot_and_workspace_lease_for_paused_task`
  - `record_task_run_start_and_completion_manage_execution_slot_lifecycle`

### 未完成

- 还没有把 `execution slot` 从最小模型升级到 slot-level heartbeat / watchdog / recovery policy
- 还没有把 `workspace lease` 从共享主工作区租约影子模型升级到真正的隔离 worktree 实例
- 还没有独立 worktree / clone 隔离
- 还没有 worker mailbox
- 还没有依赖图与 handoff 原语
- 还没有 slot 级 heartbeat / recovery policy
- 还没有原子共享协调状态与冲突仲裁策略
- 还没有 `scoped capability grant`
- 还没有联邦预算与超支硬阻断
- 还没有 run / slot 级 journey、compliance、quality gate
- 还没有稳定的项目上下文包契约
- `main.rs` 顶层业务入口已基本收口完成；当前剩余主要是测试承载，执行底座主线已切到 slot heartbeat / recovery / worktree 隔离
- `automation.rs` 中的 `strategy sweep` / `reassess_stale_tasks` / `quick_reassess_gate` 仍未进入当前主线自动化循环，后续必须以显式需求和测试推进

## 4. 落地顺序表

| 波次 | 目标 | 主要输出 | 前置依赖 | 当前状态 |
|------|------|----------|----------|----------|
| Wave 0 | 按工作区串行过渡 | lane key、冲突判定、auto-claim 过滤、同工作区回收 | 无 | 已完成 |
| Wave 1 | 收口状态初始化与入口文件 | `state.rs` 接管初始化/归一化；`main.rs` 只保留入口胶水 | Wave 0 | 已完成 |
| Wave 2 | 引入 execution slot 骨架 | `execution_slot_id`、slot 状态、slot 与 task-run 关联 | Wave 1 | 进行中 |
| Wave 3 | 引入 workspace lease | slot 绑定独立工作区实例；共享主工作区不再直接承载执行现场 | Wave 2 | 进行中 |
| Wave 4 | Git worktree 隔离执行 | worktree 创建/复用/回收、失败现场保留、成功后可选合并 | Wave 3 | 未开始 |
| Wave 5 | slot 级恢复与 heartbeat | slot heartbeat、slot-level recovery policy、恢复诊断信息 | Wave 3 | 未开始 |
| Wave 6 | worker mailbox | `delegation` / `unblock_request` / `artifact_handoff` / `review_request` 事件 | Wave 2 | 未开始 |
| Wave 7 | 依赖编排 | `blocked_by` / `blocks` / `handoff_artifact_ids` / 自动解锁 | Wave 6 | 未开始 |
| Wave 8 | 受控并行调度 | 同仓库受控并行、lane 与 lease 结合调度、回收策略收口 | Wave 4、Wave 5、Wave 7 | 未开始 |
| Wave 9 | 生产加固 | 并行回归、安全回归、恢复压测、审计闭环 | Wave 8 | 未开始 |
| Governance G1 | 原子共享协调状态 | write intent、冲突策略、追加式协调审计 | Wave 2 | 进行中 |
| Governance G2 | 能力授权治理 | scoped grant、ttl、risk score、revoke、危险操作校验 | Wave 2 | 未开始 |
| Governance G3 | 联邦预算治理 | per-agent/task/project/provider budget pool、预扣/实扣、超支阻断 | Governance G1 | 未开始 |
| Governance G4 | Journey / 合规 / 质量门 | phase gate、违规检测、quality gate、矛盾检测 | Wave 2、Wave 7 | 未开始 |
| Governance G5 | 项目上下文包 | goals/decisions/milestones/constraints/banned patterns context pack | Wave 1 | 未开始 |

## 5. 每波实施清单

### Wave 1：先把入口和状态初始化收口

- [x] 让 `state.rs` 成为 persisted state 的唯一实现归属
- [x] 让 `main.rs` 不再重复定义 `default_state / default_projects / default_users / default_agents`
- [x] 让 `main.rs` 不再重复定义 `load_or_initialize_state / normalize_persisted_state`
- [x] 让 `automation.rs` 接管 watchdog / auto-start / auto-resume 主线自动化逻辑
- [x] 删除 `main.rs` 中对应的 legacy 自动化实现与关联 helper
- [x] 删除 `main.rs` 中只做 prompt 转发的 wrapper，直接走 `prompt.rs` 唯一实现
- [x] 补自动启动 / 自动恢复 run history 回归测试
- [x] 补一份“入口文件重构边界”说明，明确哪些逻辑留在 `main.rs`
- [x] 跑状态归一化相关测试

### Wave 2：execution slot 骨架

- [x] 在 `TaskRunRecord` 中加入 `execution_slot_id`
- [x] 引入 `ExecutionSlotRecord`，明确 `task-run -> slot` 与 `slot -> task` 关系
- [x] 为 slot 增加最小状态字段，并在 start / resume / completed / failed / interrupted 上联动
- [x] 让 persisted state 启动归一化时可为旧 run 回填 slot
- [x] 补 slot 创建、释放最小回归测试
- [ ] 把恢复入口从“恢复 task thread”继续推进到“恢复 slot”
- [ ] 为 slot 增加 heartbeat / watchdog / recovery policy
- [ ] 评估第二阶段是否需要给 `Task` 增加“当前活动 slot 投影字段”

### Wave 3：workspace lease

- [x] 在 slot 上绑定 `workspace_lease_id`
- [x] 引入 `WorkspaceLeaseRecord` 最小生命周期字段与回收原因
- [x] 让 start / resume / terminal transition 联动 lease 获取与释放
- [x] 让 persisted state 启动归一化时可为旧 slot 回填 lease
- [ ] 将项目主工作区与执行工作区实例真正解耦
- [ ] 把按工作区串行升级为按 lease / lane 调度
- [ ] 补 lease 冲突、泄漏、回收测试

### Wave 4：Git worktree 隔离执行

- [ ] 引入 worktree 准备器
- [ ] 任务执行默认进入隔离 worktree
- [ ] 失败时保留现场，成功后按策略合并或回收
- [ ] 记录 worktree 与 branch 的审计链路
- [ ] 补 worktree 脏目录、切换失败、回收失败测试

### Wave 5：slot 级恢复

- [ ] 建立 slot heartbeat
- [ ] 建立 slot watchdog 与僵尸检测
- [ ] 恢复时优先恢复 slot，而不是只恢复 thread
- [ ] 把 thread not found、workspace missing、lease expired 分成不同恢复分支
- [ ] 补跨重启恢复和失败回退测试

### Wave 6：worker mailbox

- [ ] 设计结构化协作事件模型
- [ ] 最小支持点对点消息和离线待收件箱
- [ ] 让任务可以引用 mailbox 消息或 handoff 结果
- [ ] 为协作事件补审计字段
- [ ] 补 inbox 去重、离线投递、无效收件人测试

### Wave 7：依赖编排

- [ ] 为任务模型增加 `blocked_by`
- [ ] 为任务模型增加 `blocks`
- [ ] 为任务模型增加 `delegated_from_task_id`
- [ ] 为任务模型增加 `handoff_artifact_ids`
- [ ] 调度器支持自动解锁与阻塞传播
- [ ] 补依赖环、错误解锁、重复 handoff 测试

### Wave 8：受控并行

- [ ] 在 slot、lease、依赖图稳定后，再放开同仓库受控并行
- [ ] 限制并行度与资源上限
- [ ] 引入并行异常回收策略
- [ ] 补并行回归、安全回归、性能基线

### Governance G1：原子共享协调状态

- [x] 为 `slot / lease` 引入最小 `CoordinationWriteIntent` 写入边界
- [x] 先落一阶段 `write intent -> committed` 追加式治理痕迹
- [x] 在数据模型中预留 `first-commit-wins / priority-wins / last-write-wins`
- [ ] 把 `write_intent / validation / commit` 扩展到 task / artifact / decision / budget
- [ ] 真正落地 `first-commit-wins` 与 `priority-wins` 的冲突仲裁逻辑
- [ ] 关键协调写入进入更完整的追加式审计链路
- [ ] 补并发冲突、陈旧写入、重复提交回归测试

### Governance G2：能力授权治理

- [ ] 定义 `scoped capability grant` 数据模型，至少包含 `resource / action / scope / ttl`
- [ ] 为高风险动作引入 justification、risk score、trust level 校验
- [ ] 支持 grant 的过期、撤销、审计追踪
- [ ] 先接入 Git 危险操作、Provider 凭据、导出动作与自动回滚
- [ ] 补未授权、越权 scope、grant 过期回归测试

### Governance G3：联邦预算治理

- [ ] 定义 `per-agent / per-task / per-project / per-provider` budget pool
- [ ] 模型调用前做预算预检，调用后做实际记账
- [ ] 对并发预算扣减使用原子协调语义，避免双花
- [ ] 超支时硬阻断，而不是只告警
- [ ] 补超支、预算隔离、并发双花回归测试

### Governance G4：Journey / 合规 / 质量门

- [ ] 为 `task-run / execution-slot` 定义更细的 phase gate
- [ ] 检测 timeout、tool misuse、turn 违规、策略违规
- [ ] 对关键共享写入引入 quality gate / contradiction detection
- [ ] 将违规与质量门结果接入审计事件和后台视图
- [ ] 补违规检测、误报、门禁拦截回归测试

### Governance G5：项目上下文包

- [ ] 定义项目上下文包 contract，至少包含 goals / decisions / milestones / constraints / banned patterns
- [ ] 从事实记忆、项目约束、任务摘要、项目聊天中生成稳定 context pack
- [ ] 为任务执行与项目会话统一注入 context pack，而不是各自拼装
- [ ] 记录 context pack version、source 和注入时间
- [ ] 补上下文缺失、过期、冲突回归测试

## 6. `main.rs` 模块化收口计划

### 目标

`main.rs` 最终只保留：

- 模块声明
- 顶层类型装配
- 应用启动入口
- 少量跨模块共享的胶水

业务逻辑要按职责回收到对应模块。

### 拆分原则

参考 Rust 官方文档：

- Rust Book Chapter 7.5：模块变大后应拆到独立文件，文件路径要与模块树对齐
  - 来源：[Separating Modules into Different Files](https://doc.rust-lang.org/stable/book/ch07-05-separating-modules-into-different-files.html)
- Rust Book Chapter 12.3：`main` 应尽量只负责启动、配置与错误边界，逻辑应拆到其他函数或类型
  - 来源：[Refactoring to Improve Modularity and Error Handling](https://doc.rust-lang.org/beta/book/ch12-03-improving-error-handling-and-modularity.html)

基于这些来源，这里采用的实现性推论是：

- 不按“凑行数”拆文件，而按职责边界拆
- 同一职责的测试尽量跟随模块收口
- `main.rs` 不能继续保留模块中已存在的重复实现
- 优先拆“行为不变的重复代码”，再拆“带行为收口的调度逻辑”

### 推荐归属

- `state.rs`
  - 持久化状态加载、默认数据、状态归一化
- `automation.rs`
  - watchdog、自动恢复、自动认领、调度循环
- `task_ops.rs`
  - task 状态流转、认领、暂停、恢复、辅助判断
- `handlers.rs`
  - HTTP handlers
- `runtime.rs`
  - provider runtime session 和 thread/turn 操作
- `git_ops.rs`
  - branch / worktree / snapshot / merge 相关逻辑
- `snapshot.rs`
  - board/project/task 的读取视图拼装
- `prompt.rs`
  - prompt 组装与 developer instruction
- `server.rs`
  - app/router/listen addr 装配
- `main.rs`
  - 顶层胶水与极少量共享入口

## 7. 本轮先做什么

在继续推进下面这些实现前，先记录一条最新优先级结论：

- 以 [docs/reference-project-refresh-2026-03-30.md](./reference-project-refresh-2026-03-30.md) 为最新参考刷新结果
- 当前最该优先吸收的 4 条平台内核线是：
  - `execution slot -> isolated workspace instance`
  - `typed coordination schema`
  - `dependency / handoff graph`
  - `benchmark-driven self-improvement loop`
- 因此近期不再优先扩散到更多聊天形态、更多 provider 表层接入、或重 UI 的 orchestrator 外壳
- 本文后续波次排序如与该刷新文档冲突，以“先执行隔离、再协调契约、再依赖编排、最后自改进闭环”的顺序为准

当前这一轮已完成：

- [x] 完成 `state.rs` 接管状态初始化与归一化逻辑
- [x] 跑通相关回归测试，确认这是“纯重构，不改行为”
- [x] 对齐 `automation.rs` 与 `main.rs` 中的 watchdog / auto-mode 行为差异
- [x] 先补行为等价测试，再让 `automation.rs` 接管重复实现
- [x] 在较大删除前创建 `pre-automation-legacy-prune-20260329`
- [x] 删除 `main.rs` 中 legacy 自动化实现与相关 helper
- [x] 删除 `main.rs` 中只做 prompt 转发的 wrapper
- [x] 完成 Network-AI 参考评估，并把治理层吸收项并入路线图

下一步直接进入：

- [x] 继续清理 `main.rs` 中残留的启动入口周边胶水
  - 当前状态：`apps/server/src/main.rs` 顶层函数已只剩 `main()`，其余任务域与运行期逻辑已分别收口到 `state.rs`、`task_ops.rs`、`handlers.rs`、`runtime.rs`、`prompt.rs`
- [x] 评估 `execution slot` 与 atomic coordination state 的公共数据模型切入点，并落地第一阶段最小模型
- [ ] 继续把 `execution slot` 升级到 slot-level recovery / heartbeat，并把 `workspace lease` 升级到 worktree 级隔离
- [ ] 设计 `scoped capability grant` 与 budget pool 的最小可落地版本
- [ ] 将 `strategy sweep` / `reassess_stale_tasks` / `quick_reassess_gate` 整理为显式需求，再决定是否接入主循环
- [ ] 设计项目上下文包 contract，并与 `prompt.rs` / 事实记忆对齐

当前已识别的风险边界：

- `automation.rs` 不是纯抽取副本，它额外引入了 `strategy sweep`
- `automation.rs` 在自动恢复/自动认领前加入了重评估门禁
- 这类增强能力当前仍未放入主线自动化循环，必须继续保持“显式引入 + 测试先行”，不能静默改变生产行为

等这一步稳定后，再继续拆 `main.rs` 与推进 `execution slot`，但前提仍然是不静默改变自动化策略。
