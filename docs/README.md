# Spotlight 文档索引

本目录存放 Spotlight 的产品、架构、数据模型、交互和版本切片文档。

## 文档索引

- `product-constraints-v1.md`
  - 产品目标、角色权限、任务生命周期与 MVP 边界。
- `state-machine.md`
  - 任务、运行、审批、验收、回滚等核心状态模型。
- `system-architecture.md`
  - Tauri 桌面端、Rust 服务、ACP 集成和中心服务的端到端架构。
- `data-model.md`
  - 核心实体、关系模型、索引与审计事件存储建议。
- `data-model-v2.md`
  - 面向平台演进的数据模型，减少硬编码枚举并补充 workflow、runtime、tenant、artifact 抽象。
- `api-design.md`
  - 服务 API、WebSocket 事件以及本地桌面到核心服务接口。
- `ui-information-architecture.md`
  - 桌面端页面结构、主要视图、交互流和 Agent 面板设计。
- `agent-autonomy-and-decision-rules.md`
  - Agent 自治、决策边界、等待队列、请示条件、记忆沉淀与反馈闭环规则。
- `acceptance-and-artifacts.md`
  - 任务输出包、验收契约以及任务完成后必须提交的审查材料。
- `provider-abstraction.md`
  - 如何支持 Codex、Claude、Kimi、MiniMax 等不同本地模型 CLI / Runtime。
- `runtime-protocol-decision.md`
  - 为什么优先接入原生 Codex app-server runtime，同时为标准 ACP 兼容保留扩展点。
- `extensibility-and-compatibility.md`
  - 剩余硬编码区域、长期扩展点和兼容性原则。
- `platform-abstractions-v1.md`
  - workflow、runtime、provider、policy、snapshot、artifact、event 等稳定抽象。
- `billing-and-deployment-model.md`
  - SaaS、托管私有化、自建部署，以及订阅与用量计费建议。
- `mobile-companion-strategy.md`
  - 移动伴侣的项目/任务可见性、Agent 状态监控与轻量审批范围。
- `admin-and-ops-console.md`
  - 后台控制面、项目配置、人员角色、监控、风险操作和平台维护能力。
- `admin-and-ops-slices-0.1.5.md`
  - `0.1.5` 后台与运维控制台的任务切片、边界、测试要求与实施顺序。
- `ai-management-and-insight-engine.md`
  - 多模型 AI 控制面，用于管理分析、预测、总结和低 Token 项目洞察。
- `versioned-fact-memory-v1.md`
  - 版本化事实记忆层，用于沉淀跨任务、跨会话的约束、决策、摘要与 AI 洞察输入事实。
- `runtime-session-and-task-recovery-v1.md`
  - 明确 task、session、thread、版本化记忆的边界，以及当前 `0.1.x` 阶段的恢复、回收和诊断规则。
- `ai-insight-slices-0.1.7.md`
  - `0.1.7` AI 洞察与管理能力的版本切片、场景边界、测试要求与实施顺序。
- `security-and-audit.md`
  - 工作区边界、危险操作策略、Git 标签、回滚策略和审计要求。
- `delivery-plan.md`
  - MVP 切片、里程碑与建议实施顺序。
- `clawteam-reference-gap-analysis.md`
  - 对比 ClawTeam / ClawTeam-OpenClaw 与 Spotlight 的目标差距，沉淀可借鉴的 4 类执行内核能力、当前实现缺陷与改进建议。
- `clawteam-adoption-todo.md`
  - 把 ClawTeam 与 Network-AI 的借鉴收敛成可执行路线图，包含执行层与协调治理层的波次顺序、完成定义、`main.rs` 模块化收口计划与当前进行中的 TODO。
- `network-ai-reference-gap-analysis.md`
  - 对比 Network-AI 与 Spotlight 的目标差距，沉淀原子共享状态、权限治理、预算熔断、journey 合规和项目上下文包等协调治理层借鉴点。
- `workspace-serialization-transition-2026-03-28.md`
  - 记录从“全局串行”过渡到“按工作区串行”的临时决策、实现范围、风险边界与后续重构顺序。
- `execution-slot-and-coordination-model-v1.md`
  - 记录 `execution slot + workspace lease + coordination_write_intent` 第一阶段模型、为何暂不把 `execution_slot_id` 直接挂到 `Task`、以及当前已落地的状态升级与运行期联动行为。
- `main-rs-entry-boundary-v1.md`
  - 明确 `apps/server/src/main.rs` 在当前阶段只保留入口胶水、顶层共享类型和启动逻辑，禁止继续承载 handler 与任务业务逻辑。
- `reference-project-refresh-2026-03-30.md`
  - 重新核对 Agent Orchestrator、Squad、crewAI、OxyGent、SICA 等参考项目，收敛当前阶段最该吸收的设计结论，并重排近期 TODO 优先级。

## 建议阅读顺序

1. `product-constraints-v1.md`
2. `state-machine.md`
3. `system-architecture.md`
4. `data-model.md`
5. `data-model-v2.md`
6. `platform-abstractions-v1.md`
7. `api-design.md`
8. `ui-information-architecture.md`
9. `agent-autonomy-and-decision-rules.md`
10. `acceptance-and-artifacts.md`
11. `provider-abstraction.md`
12. `runtime-protocol-decision.md`
13. `extensibility-and-compatibility.md`
14. `billing-and-deployment-model.md`
15. `mobile-companion-strategy.md`
16. `admin-and-ops-console.md`
17. `admin-and-ops-slices-0.1.5.md`
18. `ai-management-and-insight-engine.md`
19. `versioned-fact-memory-v1.md`
20. `runtime-session-and-task-recovery-v1.md`
21. `ai-insight-slices-0.1.7.md`
22. `security-and-audit.md`
23. `delivery-plan.md`
24. `clawteam-reference-gap-analysis.md`
25. `clawteam-adoption-todo.md`
26. `network-ai-reference-gap-analysis.md`
27. `workspace-serialization-transition-2026-03-28.md`
28. `execution-slot-and-coordination-model-v1.md`
29. `main-rs-entry-boundary-v1.md`
30. `reference-project-refresh-2026-03-30.md`
