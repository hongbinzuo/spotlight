# 参考项目再理解与 TODO 梳理（2026-03-30）

## 1. 目的

这份文档用于重新梳理 Spotlight 当前明确参考的开源项目，并把“参考结论”转成可执行的中文 TODO。

这里的目标不是复制某个现成框架，而是回答三个问题：

1. 这些项目今天各自最值得借鉴的设计到底是什么。
2. 哪些设计和 Spotlight 当前阶段最相关。
3. 这些设计应该如何落到接下来 2 到 4 个迭代波次里。

## 2. 本轮重新核对的参考项目

本轮重点重新核对了以下来源：

- ComposioHQ `agent-orchestrator`
- GitHub Blog: `How Squad runs coordinated AI agents inside your repository`
- GitHub Blog: `Multi-agent workflows often fail. Here’s how to engineer ones that don’t.`
- `crewAI`
- `OxyGent`
- `self_improving_coding_agent`
- Addy Osmani: `Self-Improving Coding Agents`

## 3. 当前理解

### 3.1 Agent Orchestrator：重点不是“多 agent”，而是“隔离执行槽 + 反应式治理”

当前最值得借鉴的不是它的 dashboard，而是这几个执行内核能力：

- 每个 agent 在独立 git worktree 中工作，而不是共享同一个工作目录。
- orchestration 层自动接住 CI 失败、review comment、merge feedback 这类外部反应事件，再把问题路由回对应 agent。
- runtime、agent、workspace、tracker、notifier 都是可替换槽位，不把编排器锁死在某个单一供应商上。

对 Spotlight 的直接启发：

- `execution slot` 不该只停留在内存记录上，而要继续推进到“一个 slot 对应一个真实执行现场”。
- `workspace lease` 的下一步不应只是租约，而是 worktree / clone 级别的可回收执行实例。
- 当前 `task-run -> slot -> lease` 已经起步，接下来该补的是 `reaction engine`，而不是继续在入口文件里堆业务逻辑。

不建议照搬的部分：

- 不需要把 Spotlight 做成“单 orchestrator 驱动一切”的单中心工具。
- Spotlight 的目标仍然是平台能力，不是一个专门服务 PR/CI 的 dashboard 产品。

### 3.2 Squad：重点是 repo-native 共享记忆，而不是实时对话同步

Squad 当前最值得借鉴的是 repository-native 协作思路：

- 共享记忆放在版本化文件里，而不是依赖实时上下文同步。
- 协调 agent 做薄路由，不把所有 reasoning 都塞回一个 coordinator。
- 每个 specialist 共享仓库上下文，但保留独立推理窗口。

对 Spotlight 的直接启发：

- Spotlight 需要一个正式的 `decisions / milestones / constraints / banned patterns` 上下文包，而不是继续散落在任务活动、项目聊天和 memory revision 里。
- `worker mailbox` 最小版本不应该先做成“实时聊天”，而应该先做成“结构化 drop-box 事件 + 可审计持久化”。
- 当前 `prompt.rs` 虽然已经能拼装上下文，但还不是稳定 contract；后续应该升级成真正的 `project context pack`。

### 3.3 GitHub 多 Agent 工程文章：重点是把 Agent 当分布式系统工程

GitHub 这两篇文章给出的信号非常一致：

- 多 agent 失败的根因通常不是模型不够强，而是状态、顺序、动作边界没有被结构化。
- typed schema 是边界契约，不是锦上添花。
- action schema 要把“能做什么”压缩成少量、明确、可验证的动作集合。
- 协调层要像分布式系统一样设计，而不是像聊天机器人一样设计。

对 Spotlight 的直接启发：

- `worker mailbox`
- `handoff`
- `unblock_request`
- `review_request`
- `budget reservation`
- `write intent`

这些都不应再只是自由文本或松散 JSON，而应该有明确 schema。

这意味着接下来最优先的不是“再多接几个模型”，而是补齐：

- typed event schema
- typed decision schema
- typed recovery schema
- typed quality gate result schema

### 3.4 crewAI：重点是角色合同，不是框架依赖

crewAI 仍然有价值，但更适合作为“角色建模参考”，而不是 Spotlight 的底层依赖。

可借鉴点：

- 角色职责明确
- 任务与角色绑定关系清晰
- human-in-the-loop 有正式位置

不建议直接引入的部分：

- Spotlight 不应围绕某个 Python agent framework 反向设计自己的平台抽象。
- role 概念应该进入 Spotlight 的平台模型，但实现仍然应该服从当前 Rust 服务端与多端产品边界。

### 3.5 OxyGent：重点是依赖图、弹性拓扑、评估闭环

OxyGent 最值得借鉴的不是它宣传的“万物皆可组合”，而是三个方向：

- dynamic planning
- dependency mapping
- evaluation / feedback loop

对 Spotlight 的直接启发：

- 当前 TODO 里的 `blocked_by / blocks / handoff_artifact_ids` 是必须继续推进的，不然多 worker 协作只能停留在弱耦合并行。
- 执行拓扑不能永远停留在“一个任务一个 agent”的单跳模型，后续必须支持 handoff 和依赖解锁。
- `strategy sweep / reassess_stale_tasks / quick_reassess_gate` 这类逻辑不该继续散着，而应并回正式调度策略层。

### 3.6 SICA / Addy：重点是 benchmark 驱动的自改进，而不是无限循环

这类参考项目最值得借鉴的是：

- 自改进必须绑定 benchmark 或 regression gate。
- 每轮改进都需要可比较的结果存档。
- 长循环要靠“小任务 + 明确验收 + 每轮重置上下文”保证稳定性。

对 Spotlight 的直接启发：

- “自举”不能只靠不断新建任务，更要有 benchmark 与 regression harness。
- 未来的自改进循环应该围绕：
  - TODO 池
  - benchmark 池
  - quality gate
  - 回归结果归档

而不是围绕“让 agent 自己随便找事做”。

## 4. 对 Spotlight 的最新收敛结论

### 4.1 参考项目里最该优先吸收的，不是 UI，而是 4 条平台内核线

优先级从高到低如下：

1. `execution slot -> isolated workspace instance`
2. `typed coordination schema`
3. `dependency / handoff graph`
4. `benchmark-driven self-improvement loop`

### 4.2 当前版本最不该分散精力的方向

短期不该优先：

- 再加新的聊天形态
- 再加更多 provider 表层接入
- 做重 UI 的 orchestrator dashboard 外壳
- 做缺少 schema 约束的“智能协作”功能

原因很简单：

- 现在真正的主阻塞在执行隔离、协调契约、依赖编排、恢复与审计。
- 这些基础没立住，表面再像多 agent 平台，内部仍然是脆弱的。

## 5. 更新后的 TODO 工作项

以下 TODO 不是把旧 TODO 推翻，而是把它重新按“当前最该做什么”排序。

### P0：先收口当前执行主线

- [ ] 修复 `git_ops.rs` 中因编码污染残留的旧 `auto_claim_*` 分叉实现，并在文件编码恢复后物理删除 dead code。
- [x] 统一 `auto_claim_next_task` 的唯一事实来源到 `task_ops.rs`，禁止再次在 `git_ops.rs` 或 `main.rs` 演化第二套选择规则。
  - 当前状态：`git_ops.rs` 中的残留实现已降级为显式 `legacy_*` 命名，仅作为待删除参考块保留
- [x] 给当前 auto-claim 规则补一份中文决策记录，明确“先队列归属，后优先级”的当前平台语义。
  - 记录位置：`docs/architecture-consistency-log.md`

### P1：把执行模型从“最小 slot 记录”推进到“真实执行现场”

- [ ] 为 `execution slot` 增加 slot-level heartbeat。
- [ ] 为 `execution slot` 增加 watchdog / recovery policy。
- [ ] 把 `workspace lease` 从逻辑租约升级到 worktree / clone 级别实例。
- [ ] 为 worktree 实例补回收、失败保留、成功清理的生命周期策略。
- [ ] 为 slot / lease / worktree 三者补端到端回归测试。

### P1：把协调边界从“自由文本”推进到“typed schema”

- [ ] 为 `worker mailbox` 定义结构化事件模型，至少覆盖 `delegation`、`handoff`、`review_request`、`unblock_request`。
- [ ] 为 `CoordinationWriteIntent` 扩展 typed validation / commit 结果模型。
- [ ] 为 `quality gate`、`reassess`、`recovery` 定义统一结果 schema。
- [ ] 为关键 schema 加 version 字段，避免后续演化时无版本边界。

### P1：把项目共享记忆推进到 repo-native context pack

- [ ] 定义 `project context pack` contract，至少包含 `goals / decisions / milestones / constraints / banned_patterns`。
- [ ] 把现有 memory revision、project chat、task summary 聚合为稳定快照，而不是运行时临时拼接。
- [ ] 为 context pack 增加 version、source、generated_at、staleness 标记。
- [ ] 明确 context pack 与 prompt 注入的边界，避免 `prompt.rs` 继续承担事实存储职责。

### P2：把依赖与 handoff 从 TODO 口头描述推进到正式调度能力

- [ ] 在任务模型里正式加入 `blocked_by`。
- [ ] 在任务模型里正式加入 `blocks`。
- [ ] 在任务模型里正式加入 `handoff_artifact_ids`。
- [ ] 调度层支持自动解锁、阻塞传播、handoff 后续任务生成。
- [ ] 给依赖图补回归测试，覆盖环、孤儿节点、重复 handoff、错误解锁。

### P2：把自举推进到 benchmark 驱动

- [ ] 为 Spotlight 自身建立最小 benchmark 池，而不是只维护任务池。
- [ ] 给 `automation` / `task_ops` / `handlers` 关键路径建立 regression harness。
- [ ] 设计 nightly 或手动触发的“基准回归 -> 输出改进任务”闭环。
- [ ] 把 benchmark 结果接入项目洞察与管理视图，而不是只存在测试输出目录。

## 6. 建议的下一轮实施顺序

建议按以下顺序推进：

1. 收口 `git_ops.rs` 遗留分叉与编码污染
2. 推进 slot heartbeat / recovery policy
3. 推进 worktree 实例化
4. 推进 mailbox typed schema
5. 推进 project context pack
6. 推进 blocked_by / handoff 编排
7. 推进 benchmark-driven self-improvement

## 7. 参考链接

- Agent Orchestrator
  - https://github.com/ComposioHQ/agent-orchestrator
- GitHub Blog: How Squad runs coordinated AI agents inside your repository
  - https://github.blog/ai-and-ml/github-copilot/how-squad-runs-coordinated-ai-agents-inside-your-repository/
- GitHub Blog: Multi-agent workflows often fail. Here’s how to engineer ones that don’t.
  - https://github.blog/ai-and-ml/generative-ai/multi-agent-workflows-often-fail-heres-how-to-engineer-ones-that-dont/
- crewAI
  - https://github.com/crewAIInc/crewAI
- OxyGent
  - https://github.com/jd-opensource/OxyGent
- Self-Improving Coding Agent
  - https://github.com/MaximeRobeyns/self_improving_coding_agent
- Addy Osmani: Self-Improving Coding Agents
  - https://addyosmani.com/blog/self-improving-agents/
