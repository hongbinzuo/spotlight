# ClawTeam 参考差距分析

## 1. 结论先行

ClawTeam 和 `ClawTeam-OpenClaw` 适合作为 Spotlight 的执行层参考，但不适合作为 Spotlight 的平台底座。

原因很简单：

- 它们擅长的是“单仓库、本地、多 agent 协同执行”
- Spotlight 要做的是“多人、多项目、多端、多 provider、可审计、可治理的执行平台”

所以，我们最该借的不是它的 CLI 外壳，也不是它的本地文件存储，而是它在执行内核上已经验证过的 4 类能力：

1. 工作区隔离
2. agent 间协作通信
3. leader-worker 依赖编排
4. 长任务生命周期管理

这 4 类能力正好击中 Spotlight 当前实现里最容易在后续版本放大的结构性短板。

## 2. 最值得借鉴的 4 类能力

### 2.1 工作区隔离：每个执行槽都有独立工作目录

参考实现：

- `C:\Users\zuoho\code\spotlight\.tmp\clawteam-openclaw\clawteam\workspace\manager.py`

ClawTeam 的关键不是“会自动切分支”，而是“每个 agent 拿到独立 worktree”，因此具备：

- 独立分支
- 独立文件视图
- 独立未提交改动
- 崩溃后可单独清理或回收

这件事为什么重要：

- 多个 agent 真并行时，不会互相污染工作目录
- 一个 agent 卡住、失败、撤销时，不会把主工作区搞脏
- 后续做快照、恢复、审计、回滚时，有稳定的执行边界

对 Spotlight 的启发：

- Spotlight 未来的最小执行单元不该只是 `task + thread`
- 应该升级为 `execution slot + isolated workspace + runtime session`
- `workspace_root` 不能总是等于项目主工作区

### 2.2 agent mailbox：worker 之间能直接交换结构化消息

参考实现：

- `C:\Users\zuoho\code\spotlight\.tmp\clawteam-openclaw\clawteam\team\models.py`
- `C:\Users\zuoho\code\spotlight\.tmp\clawteam-openclaw\docs\transport-architecture.md`
- `C:\Users\zuoho\code\spotlight\.tmp\clawteam-openclaw\clawteam\team\manager.py`

ClawTeam 的通信不是“leader 代传所有上下文”，而是提供 mailbox / P2P message：

- 消息有明确类型
- 收件人明确
- 支持点对点
- 支持离线兜底

这件事为什么重要：

- 真实团队协作不是只有“拆任务”，还包括“补材料、请确认、移交结果、请求解锁”
- 如果没有 mailbox，多 agent 只能通过共享看板和任务描述绕路交流
- 一旦任务依赖复杂，所有协作都会退化成 leader 串行转述，吞吐量很快崩掉

对 Spotlight 的启发：

- 不需要照搬 ZeroMQ
- 但必须尽快定义平台内的结构化协作原语，例如：
  - task delegation
  - unblock request
  - artifact handoff
  - dependency resolved
  - review request

### 2.3 leader-worker 编排：依赖显式化，阻塞自动解锁

参考实现：

- `C:\Users\zuoho\code\spotlight\.tmp\clawteam-openclaw\clawteam\team\models.py`

ClawTeam 的 `TaskItem` 已经有：

- `blocks`
- `blocked_by`
- `locked_by`

这意味着它不是简单的“任务列表”，而是最小依赖图。

这件事为什么重要：

- 多 agent 系统一旦没有依赖原语，就只能用“谁先抢到谁做”的粗糙模式
- 当任务存在前置条件时，系统会出现错误抢占、重复劳动、上下文碰撞
- 依赖显式化之后，调度器才有机会做自动解锁、阻塞传播、优先级重排

对 Spotlight 的启发：

- 当前任务模型里的 `source_task_id` 只表达“来源”
- 它不能表达“谁阻塞了我”“我完成后解锁谁”“谁正在等待我的产物”
- Spotlight 如果要走向多 agent 编排，任务模型必须补齐 dependency / handoff / artifact reference

### 2.4 长任务生命周期管理：能跑很久，也能恢复回来

参考实现：

- `C:\Users\zuoho\code\spotlight\.tmp\clawteam-openclaw\docs\transport-architecture.md`
- `C:\Users\zuoho\code\spotlight\.tmp\clawteam-openclaw\clawteam\workspace\manager.py`

ClawTeam 的一个现实优势是，它把 agent 看成长期运行的执行体，而不是“一次请求响应”。

它的思路包括：

- inbox 持久化
- worktree 持久化
- 成员配置持久化
- 崩溃后按本地状态重建

这件事为什么重要：

- 复杂代码任务和研究任务经常超过单次交互窗口
- 如果生命周期模型只有“启动一次 turn，结束即完成”，系统会天然偏向短任务
- 真正的平台必须能处理暂停、恢复、僵尸检测、超时回收、上下文续跑

对 Spotlight 的启发：

- Spotlight 已经有 `task_run_history`、`thread_id`、`resume_thread`
- 但恢复语义还不完整，因为它还没有绑定到“独立执行槽”和“隔离工作区租约”
- 所以后续一旦支持真正并行，恢复会变得不稳定

## 3. 为什么其他能力现在不重要

### 3.1 不重要的不是“完全没价值”，而是“不是当前主矛盾”

下面这些能力可以参考，但不应该排在前 4：

### 3.2 OpenClaw 兼容胶水不重要

例如：

- 自动审批
- 适配 OpenClaw 会话模型
- 特定 CLI 的安装脚本
- skill 触发约定

原因：

- 这些是宿主环境适配层
- Spotlight 的目标不是做 OpenClaw 插件，而是做独立平台
- 现在把精力放在这些兼容胶水上，会把架构重心拉偏

### 3.3 本地 JSON/文件存储不重要

ClawTeam 用本地文件存团队、任务、邮箱，是为了轻量和便于本地运行。

但 Spotlight 当前和未来要解决的是：

- 多用户
- 多项目
- 中心服务
- 审计
- 后台管理
- 移动端摘要

所以真正要借的是“数据模型里的协作语义”，不是“把状态存在 JSON 文件里”。

### 3.4 tmux / shell 外壳能力不重要

ClawTeam 的很多可感知体验来自 CLI 编排和本地进程编组。

但对于 Spotlight 来说：

- shell 编排只是 runtime 适配层的一种表现
- 产品真正的难点不在“开多少终端”
- 而在“任务如何被隔离、调度、恢复、审计”

换句话说，终端开得再漂亮，也不能替代平台级执行模型。

### 3.5 外部看板和可视化表盘不重要

这些东西确实提高可观测性，但它们不是基础能力。

如果底层仍然是：

- 全局串行
- 共享工作区
- 无依赖图
- 无 mailbox

那再漂亮的看板，也只是把结构性缺陷展示得更清楚。

## 4. 对 Spotlight 当前实现的缺陷审视

下面这些不是“未来可以优化”的泛泛建议，而是已经会影响产品目标对齐的结构性问题。

### 4.1 当前实现把活跃任务做成了全局串行

证据：

- `C:\Users\zuoho\code\spotlight\apps\server\src\main.rs:964`
- `C:\Users\zuoho\code\spotlight\apps\server\src\task_ops.rs:135`
- `C:\Users\zuoho\code\spotlight\apps\server\src\task_ops.rs:731`
- `C:\Users\zuoho\code\spotlight\apps\server\src\automation.rs:633`

当前行为：

- `Claimed` 和 `Running` 被统一视为 serialized active
- 如果已有一个活跃任务，第二个任务认领会冲突
- 自动模式发现多个活跃任务时，会把额外任务强制打回等待队列

这和产品文档冲突的地方：

- `docs/product-constraints-v1.md:5` 明确目标包含 multiple projects / multiple local Agents per project
- `docs/system-architecture.md:163` 也明确每个 Agent 应该独立运行

判断：

- 当前实现能支撑单人自举和最小自动化
- 但它不是“尚未优化的并行系统”
- 它本质上是“主动禁止并行执行的系统”

### 4.2 当前 Git 执行模型不是隔离模型，只是共享工作区切分支

证据：

- `C:\Users\zuoho\code\spotlight\apps\server\src\task_ops.rs:305`
- `C:\Users\zuoho\code\spotlight\apps\server\src\prompt.rs:786`
- `C:\Users\zuoho\code\spotlight\apps\server\src\handlers.rs:450`
- `C:\Users\zuoho\code\spotlight\apps\server\src\git_ops.rs:135`

当前行为：

- 任务执行上下文默认总是取项目主工作区
- Git 准备逻辑会直接在共享工作区里：
  - `checkout` 主分支
  - 拉远端
  - 创建或切换任务分支
  - 任务完成后再切回主分支并尝试自动合并

问题不只是“将来并行会冲突”，还有两个现实风险：

- 即使今天还是串行，也会主动改动用户主工作区
- 恢复或失败回滚时，没有独立沙箱承接中间状态

### 4.3 当前任务模型缺少真正的协作与依赖原语

证据：

- `C:\Users\zuoho\code\spotlight\crates\platform-core\src\lib.rs:41`

当前 `Task` 里有：

- `source_task_id`

但缺少：

- `blocked_by`
- `blocks`
- `delegated_from_task_id`
- `handoff_artifact_ids`
- `coordination_channel_id`

这意味着当前系统更像：

- 一个可以派生任务的共享待办

而不是：

- 一个可调度、可协作、可自动解锁的多 agent 编排系统

### 4.4 恢复模型仍然绑定“task + 主工作区”假设

证据：

- `C:\Users\zuoho\code\spotlight\crates\platform-core\src\lib.rs:273`
- `C:\Users\zuoho\code\spotlight\apps\server\src\runtime.rs:74`
- `C:\Users\zuoho\code\spotlight\apps\server\src\handlers.rs:1418`

当前已经有的基础：

- `task_run_history`
- `primary_workspace_path`
- `session_threads`
- `resume_thread`

当前缺的关键一层：

- execution slot identity
- workspace lease / workspace instance
- slot-level heartbeat
- slot-level recovery policy

所以现在的恢复能力更像：

- “把一个 thread 再挂回一个 task 上”

而不是：

- “恢复某个执行槽在它自己的工作区、自己的会话、自己的运行状态”

### 4.5 状态模型存在文档和实现漂移

证据：

- `C:\Users\zuoho\code\spotlight\docs\product-constraints-v1.md:185`
- `C:\Users\zuoho\code\spotlight\crates\workflow-engine\src\lib.rs:11`
- `C:\Users\zuoho\code\spotlight\crates\platform-core\src\lib.rs`

现状：

- 产品文档仍保留 `agent_done / rejected / rolled_back`
- `workflow-engine` 模板仍是最早一版 5 状态
- `platform-core` 的 `TaskStatus` 已经演进成另一套

这会直接导致：

- 任务状态语义越来越难统一
- 自动恢复、审批、验收、回滚难以建立稳定状态机
- 后续补并行和依赖调度时，状态爆炸风险会迅速上升

## 5. 优先改进建议

### 5.1 不建议现在直接上“全面多 agent 并行”

原因：

- 一旦解除全局串行，现有共享工作区会立刻出问题
- 恢复链路也会因为没有 execution slot 而变脆
- 状态模型还没统一，贸然扩展只会放大漂移

### 5.2 建议按三个切片推进

#### 切片 A：先统一状态语义

建议版本：

- `0.1.2` 当前尾声或 `0.1.3`

目标：

- 定一份唯一任务状态机
- 同步对齐 `product-constraints`、`workflow-engine`、`platform-core`
- 明确 claim / approval / running / pending_acceptance / canceled / failed / done 的边界

这是后续并行、恢复、审批、回滚的前提。

#### 切片 B：引入 execution slot 和隔离工作区

建议版本：

- `0.1.3` 到 `0.1.4`

最小实现建议：

- 每次 task run 分配一个 `execution_slot_id`
- 每个 slot 绑定一个独立工作区实例
- 第一版可以先支持：
  - `git worktree`
  - 失败后保留现场
  - 成功后可选合并

这样即使先不开放“同项目多任务并行”，也能先把执行边界和恢复边界搭稳。

#### 切片 C：补协作原语，再做受控并行

建议版本：

- `0.1.4` 之后

最小实现建议：

- 任务模型补 `blocked_by / blocks`
- 增加 lightweight inbox / handoff event
- 并行调度先按“项目内受控并行”做，不要一步放开全局自由抢占

先有依赖图和协作原语，再做并行，系统才不会退化成冲突放大器。

## 6. 对当前项目目标的判断

如果问题是：“ClawTeam 能不能直接实现 Spotlight 当前项目目标？”

答案是：

- 不能直接实现
- 但它能非常有效地帮助我们识别 Spotlight 执行内核里哪些抽象还不够

更准确地说：

- ClawTeam 解决的是“本地多 agent 怎么协同干活”
- Spotlight 要解决的是“平台怎样安全地承载这种协同，并让多人、多项目、多端都可治理”

所以正确姿势不是迁移过去，而是把它当成执行层参考样本，提炼出：

1. 隔离
2. 协作
3. 编排
4. 恢复

然后用 Spotlight 自己的架构语言重新实现。

## 7. 本次建议沉淀

本次分析给出的最重要结论是：

- ClawTeam 最值得借的不是外壳，而是执行内核
- Spotlight 当前最明确的缺陷不是“多 agent 还没做完”，而是“代码层面主动把活跃任务做成了全局串行”
- Spotlight 当前的 Git 任务分支实现不是隔离模型，只是共享工作区上的分支切换
- 在补齐 execution slot 和 isolated workspace 之前，不应直接放开真正的并行执行
