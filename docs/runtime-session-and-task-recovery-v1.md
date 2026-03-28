# Runtime Session 与任务恢复规则 v1

本文档用于固化 Spotlight 当前阶段关于以下问题的确定性结论：

- 当前任务与运行态是否使用数据库持久化
- task / session / thread 各自的所有权与边界
- 什么情况下会维持
- 什么情况下会断掉
- 断掉后如何回收
- 客户端和后续 review 应如何理解这些状态，避免返工

## 1. 当前阶段的事实

截至当前 `0.1.x` 自举阶段，Spotlight 服务端业务状态仍然使用文件持久化，而不是数据库。

当前落盘位置：

- `C:\Users\zuoho\code\spotlight\.spotlight\server-state.json`

这里持久化的是服务端工作状态，包括但不限于：

- `users`
- `projects`
- `tasks`
- `agents`
- `task_run_history`
- `pending_questions`
- `project_scans`
- `project_sessions`
- `project_chat_messages`
- `memory_items`
- `memory_revisions`
- `memory_tags`
- `memory_edges`

结论：

- 当前没有把任务运行态持久化到数据库
- 因此不能再依赖“扫大 JSON 文件”作为常规诊断手段
- 后续巡检、验证、客户端判断，应优先走结构化 API 与独立验证器

## 2. 所有权边界

### 2.1 task

`task` 是平台级事实对象，代表一条需要被推进、可被排序、可被审计的工作项。

它负责：

- 标题
- 描述
- 状态
- 优先级
- 活动日志
- 运行日志入口
- 是否被某个 Agent 认领

### 2.2 provider thread

provider thread 不是平台共享对象，而是执行上下文对象。

当前规则：

- thread 归属于某一次本地 runtime 会话
- 它服务的是当前 task run / project session
- 它不是多人共享写入对象
- 它也不是平台唯一事实来源

### 2.3 project session

`project_session` 是项目问答和项目级上下文交互对象。

它可以持有：

- `thread_id`
- `active_turn_id`
- 会话消息
- 会话日志

但它仍不等于长期记忆层。

### 2.4 versioned fact memory

长期有效的约束、摘要、决策，不应该只挂在 thread 或 session 上。

它们应落到版本化事实记忆层中：

- `memory_items`
- `memory_revisions`
- `memory_tags`
- `memory_edges`

## 3. 什么情况下会维持

### 3.1 task 状态维持

任务状态会在以下前提下持续维持：

- 服务端进程没有丢失当前状态
- 任务活动与运行日志正常写入
- 当前 Agent 仍持有该任务
- runtime 会话仍在服务端 `runtime_sessions` 中被追踪

### 3.2 thread 维持

当前 thread 能维持的前提是：

- 本地 provider runtime 进程还活着
- 对应 task / project session 的 runtime session 没有丢失
- `thread_id` 在 provider 侧仍然有效
- 当前 turn 没有被显式结束、打断或丢失

### 3.3 project session 维持

项目会话会在以下条件下维持：

- `project_session` 已落盘
- `thread_id` 已写回
- 后续继续追问时 provider 侧 thread 仍存在

## 4. 什么情况下会断掉

### 4.1 服务端重启

当前最关键的断点之一是：

- 服务端重启后，内存中的 `runtime_sessions` 会丢失

即使 `task.runtime.thread_id` 已落盘，也不等于本地 runtime 进程还能继续流式接管。

### 4.2 provider 返回 thread 不存在

如果 provider 返回如下语义错误：

- `thread not found`

则说明：

- 平台保存的 `thread_id` 已经不能继续用于恢复执行

### 4.3 watchdog 判定超时

如果任务在阈值内没有新的日志或事件输出，watchdog 会认定该运行已失去有效推进能力。

当前阈值：

- `300` 秒

### 4.4 并行执行冲突

当前平台在主线上已从“全局串行”收缩为“按工作区串行”。

如果系统检测到同一工作区同时存在多个主动执行任务，会触发回收与重排。

不同工作区可以并行推进。

如果多个项目指向同一个物理工作区，仍然按同一个串行 lane 处理。

## 5. 当前阶段的确定性恢复规则

### 5.1 watchdog 回收后的任务状态

当前阶段的确定性规则已经明确为：

- 有过真实运行痕迹
- 因 runtime 丢失、thread 丢失或超时被回收

这类任务回收后应进入：

- `PAUSED`

而不是：

- `OPEN`

原因：

- `OPEN` 表示尚未开始或等待首次推进
- `PAUSED` 表示已经开始过、存在中断点、应该优先考虑恢复

如果把这类任务重新打回 `OPEN`，会产生以下问题：

- 客户端误以为任务从未真正开始
- 自动认领把“恢复任务”错当成“新任务”
- 完成度与进度语义失真
- review 时容易误判为重复劳动

### 5.2 自动恢复失败后的状态

自动恢复失败后，任务也应停在：

- `PAUSED`

同时保留：

- `task.auto_retry_queued`
- `task.watchdog_recovered`
- `runtime.last_error`

### 5.3 Agent 释放规则

当任务不再处于以下状态时：

- `RUNNING`
- `CLAIMED`

或当前任务已不再归属于该 Agent，系统应释放：

- `agent.current_task_id`

并将 Agent 置为空闲待命。

### 5.4 服务启动时的历史状态归一化

服务端在启动并读取持久化状态后，应自动做一次轻量归一化，而不是继续保留明显错误的旧状态：

- 如果任务已经有 `runtime.thread_started`、`runtime.turn_completed`、`task.watchdog_recovered`、`task.auto_retry_queued` 或运行日志，但状态却仍是 `OPEN`，应自动修正为 `PAUSED`
- 如果版本任务标题带有 `[0.1.x]` 语义化前缀但缺少优先级，应在启动时补齐默认优先级，保证自动认领和排序稳定
- 如果某个 Agent 仍持有已经不再处于 `RUNNING` / `CLAIMED` 的旧任务占用，应在启动时释放该占用

这类修正应作为“状态归一化”被记录下来，而不是静默发生。

- 如果 `task_run_history` 中最后一个 run 仍停留在 `created` / `preflight` / `tagged` / `executing` 等未完成态，服务启动后应把它归一化为与任务状态一致的可审计结果：
  - 任务已完成时补齐为 `completed`
  - 任务已失败时补齐为 `failed`
  - 任务已取消时补齐为 `aborted`
  - 其他仍待恢复的情况统一收敛到 `interrupted`
- 归一化时要补齐最后一个 attempt 的 `ended_at` 与终态，并保留 thread、turn、workspace 等持久化字段，保证跨重启后仍能解释“为什么停住、是否可恢复、恢复要接哪一次 run”

## 6. 自动恢复与重试建议

当前建议采用以下顺序：

1. 优先恢复 `PAUSED` 且最近被 watchdog 回收的任务
2. 再考虑新的 `OPEN` 任务
3. 恢复前必须读取：
   - 最近任务活动
   - 最近运行日志
   - 当前有效项目约束
   - 最近任务摘要
   - 未回答问题

补充规则：

- 只有仍然保留可恢复 `thread_id`、且最近一次失败不是 `thread not found` 这类明确不可恢复错误时，系统才应继续自动恢复
- 一旦出现 `thread not found`，任务应继续停在 `PAUSED`，等待人工重新启动或人工判定，而不是进入无意义的自动重试循环
- 自动恢复或自动认领前，必须确认目标任务所在工作区当前没有其他 `Claimed` / `Running` 任务；如果该工作区已有活跃任务，当前任务应继续等待，而不是跨 lane 抢占执行

## 7. 客户端应该如何展示

客户端必须避免把以下概念混为一谈：

- 客户端壳版本
- 任务版本
- 当前任务状态
- 当前任务完成度

明确规则：

- `Spotlight 0.1.0` 之类文案如果是客户端壳版本，不应抢占任务主语义
- 任务版本应来自任务本身，例如标题中的 `[0.1.2]`
- 任务状态应基于真实执行语义，而不是简单回退为 `OPEN`
- 活动日志与运行日志必须保留，用于解释状态变化

## 8. 当前阶段推荐的诊断方式

禁止把“人工扫大 JSON”当作常规运维路径。

推荐顺序：

1. 独立验证器
2. `/api/v1/board`
3. `/api/v1/projects/{project_id}/summary`
4. `/api/v1/projects/{project_id}/context`
5. 必要时才检查 `.spotlight/server-state.json`

## 9. 下一阶段改进方向

下一阶段应继续推进：

- 将服务端业务状态从 JSON 文件迁移到数据库
- 为 task run / project session 增加更清晰的恢复状态字段
- 给客户端增加独立健康面板和恢复可视化
- 用独立验证器持续扫描“状态回退错误、线程丢失、重复回收、优先级缺失”等问题

## 10. 阶段性结论

当前可以明确写死的阶段性结论是：

- thread 所有权在服务端与本地 runtime，不在客户端 UI
- thread 不是共享长期记忆
- 长期有效事实应进入版本化记忆层
- 当前服务端持久化仍是 JSON 文件，不是数据库
- 当前主动执行约束是“按工作区串行”，不是“全局串行”
- watchdog / 自动恢复失败后的任务必须保留为 `PAUSED`
- 客户端需要以任务为中心展示，而不是让静态客户端版本文案误导用户
