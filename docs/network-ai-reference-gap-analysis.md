# Network-AI 参考差距分析

## 1. 结论先行

`Network-AI` 适合作为 Spotlight 的协调治理层参考，但不适合作为 Spotlight 的执行层底座替代品。

它和 ClawTeam 解决的是相邻但不同的问题：

- ClawTeam 更强在执行隔离、长任务持续运行、worker 间协作和依赖编排
- Network-AI 更强在共享状态并发控制、预算治理、权限护栏、FSM 合规和质量门

所以对 Spotlight 来说，最合理的吸收方式不是二选一，而是：

- 继续以 ClawTeam 为执行层参考
- 把 Network-AI 作为协调治理层补充参考

当前判断它值得纳入长期参考列表，原因有三点：

- 问题匹配度高：它直击多 agent 的竞态、超支、越权、静默失败
- 架构可提取：多数价值在治理原语和状态机，而不是 TypeScript 包装层
- 成熟度尚可：README 标注 MIT、17 个适配器、1684 个通过测试，说明它至少不是只停留在 demo 阶段

参考来源：

- [README](https://github.com/Jovancoding/Network-AI/blob/main/README.md)
- [ARCHITECTURE.md](https://github.com/Jovancoding/Network-AI/blob/main/ARCHITECTURE.md)
- [INTEGRATION_GUIDE.md](https://github.com/Jovancoding/Network-AI/blob/main/INTEGRATION_GUIDE.md)
- [references/auth-guardian.md](https://github.com/Jovancoding/Network-AI/blob/main/references/auth-guardian.md)
- [references/trust-levels.md](https://github.com/Jovancoding/Network-AI/blob/main/references/trust-levels.md)

## 2. 最值得借鉴的 5 类能力

### 2.1 原子共享协调状态：不是“能写黑板”，而是“写入必须可仲裁”

Network-AI 的 `LockedBlackboard` 关键不在“有黑板”，而在：

- 写入采用 `propose -> validate -> commit`
- 冲突有显式策略，例如 `first-commit-wins`、`priority-wins`、`last-write-wins`
- 写入会进入追加式审计链路

这件事为什么重要：

- Spotlight 一旦进入真正多 agent 并发，很多共享信息都不再只是“读写字段”
- 例如：slot 绑定、lease 分配、artifact handoff、dependency 解锁、decision 决议、budget 扣减
- 如果这些动作没有原子提交语义，系统就会出现静默覆盖、重复解锁、双花预算和状态分叉

对 Spotlight 的启发：

- 不照搬文件锁和 Markdown 黑板
- 要在 Spotlight 的数据模型里引入“协调写入”概念，例如 write intent / compare-and-swap / append-only audit
- 对关键共享状态定义冲突仲裁策略，而不是默认最后写入覆盖

参考来源：

- [README](https://github.com/Jovancoding/Network-AI/blob/main/README.md)
- [ARCHITECTURE.md](https://github.com/Jovancoding/Network-AI/blob/main/ARCHITECTURE.md)
- [INTEGRATION_GUIDE.md](https://github.com/Jovancoding/Network-AI/blob/main/INTEGRATION_GUIDE.md)

### 2.2 能力授权治理：高风险动作要变成短时、可审计、可撤销的 grant

`AuthGuardian` 的关键不在 token 签名算法，而在它把“能不能做某个危险动作”从静态角色判断，升级成了运行时 grant：

- grant 有 `resource / action / scope / ttl`
- 请求时要带 justification
- 审批逻辑会考虑 trust level 和 risk score
- grant 可以校验、过期、撤销，并进入审计日志

这件事为什么重要：

- Spotlight 后面一定会碰到高风险动作，例如：
  - Git 合并与回滚
  - 文件导出
  - provider 密钥访问
  - shell / patch / deploy / database 读写
- 如果只有“任务归属”和“用户角色”两层权限，不足以表达单次高风险动作的最小授权

对 Spotlight 的启发：

- 在现有审批、验收、审计之外，补一层 `scoped capability grant`
- grant 不应该替代用户权限，而是补足运行期最小授权
- 适合优先落在 Git 危险操作、外部 Provider、数据导出、自动回滚这类边界上

参考来源：

- [README](https://github.com/Jovancoding/Network-AI/blob/main/README.md)
- [references/auth-guardian.md](https://github.com/Jovancoding/Network-AI/blob/main/references/auth-guardian.md)
- [references/trust-levels.md](https://github.com/Jovancoding/Network-AI/blob/main/references/trust-levels.md)

### 2.3 联邦预算：预算不是报表字段，而是硬阻断控制面

`FederatedBudget` 的价值在于：

- 预算可以按 agent、task、pool 切分
- 并发运行时仍然要过统一预算门禁
- 超支不是告警，而是直接阻断

这件事为什么重要：

- Spotlight 未来要同时支持多 provider、多 runtime、多 agent
- 如果预算只做统计，不做硬门禁，真正出事时已经来不及
- 并发场景尤其容易出现“每个 agent 都以为自己还没超”的双花问题

对 Spotlight 的启发：

- 预算实体至少要支持 `per-agent / per-task / per-project / per-provider`
- 模型调用要拆成“预扣 / 实扣 / 对账”
- 与原子共享协调状态结合，防止并发超支

参考来源：

- [README](https://github.com/Jovancoding/Network-AI/blob/main/README.md)
- [ARCHITECTURE.md](https://github.com/Jovancoding/Network-AI/blob/main/ARCHITECTURE.md)
- [INTEGRATION_GUIDE.md](https://github.com/Jovancoding/Network-AI/blob/main/INTEGRATION_GUIDE.md)

### 2.4 Journey / 合规 / 质量门：多 agent 不该只有任务状态，还要有运行期治理

Network-AI 把运行治理拆成几层：

- `JourneyFSM`：哪些阶段允许哪些动作
- `ComplianceMonitor`：检测超时、tool abuse、turn 违规
- `QualityGateAgent` / `QAOrchestratorAgent`：在写入共享状态前做质量校验、矛盾检测和回归追踪

这件事为什么重要：

- Spotlight 现在已有 task state / task run / approval / acceptance，但还缺少“运行中”的硬护栏
- 当 agent 从“单次执行”演进到“多轮自治”后，问题往往不是任务状态错，而是中途违规、失控、卡死、产出自相矛盾

对 Spotlight 的启发：

- 需要为 `slot` 或 `run` 建立更细的 journey phase
- 对 timeout、非法工具、超额轮次、违反策略的行为产生日志和告警
- 对进入共享协调状态的关键输出引入质量门，而不是所有东西直接落库

参考来源：

- [README](https://github.com/Jovancoding/Network-AI/blob/main/README.md)
- [INTEGRATION_GUIDE.md](https://github.com/Jovancoding/Network-AI/blob/main/INTEGRATION_GUIDE.md)

### 2.5 项目上下文管理：把“记忆”变成稳定合同，而不是临时拼 prompt

`ProjectContextManager` 的核心价值不是 Python 脚本，而是它明确了要注入哪些稳定上下文：

- 项目目标
- 决策
- 技术栈
- 里程碑
- banned patterns

这件事为什么重要：

- Spotlight 已经有事实记忆、约束、任务摘要、项目聊天
- 但这些内容现在更偏“有数据”，还没有完全收敛成稳定的 context pack 合同
- 一旦多个 agent / session / slot 并行运行，没有统一上下文包，就容易出现认知漂移

对 Spotlight 的启发：

- 给任务执行和项目会话定义统一的项目上下文包
- 明确最少字段：goals / decisions / milestones / constraints / banned patterns / recent summaries
- 让上下文注入从“实现细节”变成“平台契约”

参考来源：

- [README](https://github.com/Jovancoding/Network-AI/blob/main/README.md)
- [INTEGRATION_GUIDE.md](https://github.com/Jovancoding/Network-AI/blob/main/INTEGRATION_GUIDE.md)

## 3. 为什么其他能力现在不重要

### 3.1 17 个适配器本身不重要

重要的是“零锁入协调层”设计，不是把 17 个适配器数量本身搬进 Spotlight。

Spotlight 已经有自己的 provider/runtime 抽象目标，短期主矛盾不是“先支持多少框架”，而是“先把并发执行和治理边界立住”。

### 3.2 Node.js / TypeScript 库形态不重要

Spotlight 的服务主线在 Rust。

值得借的是：

- 原语
- 数据模型
- 治理模式

不值得借的是：

- 包结构
- CLI 入口设计
- npm 生态包装方式

### 3.3 文件系统互斥锁和 Markdown 黑板不重要

这是它为了单进程、本地优先落地采用的具体实现。

Spotlight 应该借的是：

- 原子提交语义
- 冲突策略
- 追加式审计

而不是把黑板也做成 Markdown 文件。

### 3.4 MCP 服务器、CLI、OpenClaw Skill 形态暂时不重要

这些属于交付形态，不是最核心的平台能力。

在 Spotlight 当前阶段，优先级应低于：

- execution slot
- workspace lease
- worktree 隔离
- atomic coordination
- capability grant
- budget enforcement

## 4. 对 Spotlight 当前实现的补充缺陷审视

在吸收了 ClawTeam 的执行层启发之后，Network-AI 让我们更清楚地看到，Spotlight 还缺下面这些治理短板：

### 4.1 还没有真正的原子协调写入边界

当前的 in-process state lock 只能保护单进程内存状态。

它还不能表达：

- 陈旧写入检测
- 冲突策略切换
- 关键共享状态的追加式提交链
- 多实例或多执行体之间的仲裁语义

### 4.2 还没有运行期最小授权模型

现在更偏向：

- 用户角色
- task/agent 归属
- 审批/验收

但还缺：

- 单次危险动作的 scoped grant
- justification、ttl、revoke、risk score

### 4.3 还没有硬预算控制面

当前可以沉淀 token 与成本信息，但还不具备真正的预算熔断语义：

- 超支前预检
- 超支时硬阻断
- 并发预算扣减一致性

### 4.4 还没有 run / slot 级 journey 治理

当前的 task state 和 task run 已经是好起点，但离：

- 运行阶段门禁
- timeout / misuse / policy violation
- 质量门和矛盾检测

还有明显距离。

### 4.5 还没有稳定的项目上下文包契约

现在 prompt 上下文已经在增强，但“注入什么”“版本是什么”“来源是什么”还没有完全收敛成平台协议。

## 5. 推荐吸收顺序

如果把 ClawTeam 和 Network-AI 一起看，合理顺序不是“先全做治理”，而是：

1. 先完成 `execution slot`
2. 再完成 `workspace lease`
3. 再完成 `worktree` 隔离
4. 然后引入 atomic coordination / capability grant / budget enforcement
5. 再上 journey / compliance / quality gate
6. 最后把项目上下文包和控制面收口

原因：

- 没有 slot / lease / worktree，治理原语没有稳定执行对象
- 先把执行边界立住，再加治理，系统复杂度才可控

## 6. 对 TODO 的直接影响

这份分析建议把当前路线图从“只借鉴 ClawTeam”升级成：

- 执行层参考：ClawTeam
- 协调治理层参考：Network-AI

后续 TODO 应新增至少 5 条并行治理轨道：

- atomic coordination state
- scoped capability grant
- federated budget
- journey / compliance / quality gate
- project context pack
