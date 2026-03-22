# 版本化事实记忆层 v1

## 1. 背景

Spotlight 的长期目标不是“只有任务列表和聊天框”的工具，而是能持续自动运行、可恢复、可审计、可自我迭代的 AI 执行平台。

随着桌面端、本地长会话、任务运行历史、项目聊天、验收、洞察逐步接入，系统会越来越依赖一类稳定能力：

- 把跨会话的长期约束沉淀下来
- 把临时结论和长期决策区分开
- 在新事实出现时保留修订历史，而不是直接覆盖
- 为 AI 洞察提供结构化事实输入，而不是反复重吃原始日志
- 为客户端恢复、移动端摘要、后台审计提供统一事实来源

近期对 Kumiho / graph-native cognitive memory 方向的评估表明，Spotlight 适合借鉴其“版本化记忆原语”和“显式修订语义”，但不应在当前阶段直接引入 Redis + Neo4j 双存储或完整 AGM 推理引擎。

本设计定义 Spotlight 第一版“版本化事实记忆层”的边界和最小原语。

## 2. 设计目标

### 2.1 要解决的问题

- 让 Agent 能跨 task run 和记忆“当前有效约束”
- 让项目关键决策、取消范围、已知风险、验收结论可版本化追踪
- 让 AI 洞察优先消费结构化事实，而不是直接消费整段对话和全量日志
- 让客户端恢复与后台/移动端摘要基于同一套事实对象，而不是各自缓存一套私有状态
- 让冲突信息具备“谁在何时基于什么来源更新了什么”的审计链路

### 2.2 明确不做

- 不替代 `task`、`task_run`、`project_session`、`audit_event` 这些主模型
- 不把 thread 变成多人共享写入的实时总线
- 不在 `0.1.x` 引入 Neo4j、Redis 或其他新基础设施依赖
- 不实现完整 AGM 公理证明或通用逻辑推理器

## 3. 在 Spotlight 架构中的位置

版本化事实记忆层不是执行层，也不是 UI 状态层，而是位于中心服务内、供多个子系统复用的一层“事实沉淀层”。

它主要服务：

- 本地执行后的事实回传与沉淀
- 项目/任务/会话恢复
- AI 洞察输入准备
- 审计与风险解释
- 后续自动运行、自我迭代时的长期规则读取

它不改变以下边界：

- task/thread/session 的所有权仍然按现有模型管理
- 执行 Agent 仍以 task run 和 provider session 为核心执行上下文
- 移动端仍然只读取中心服务的摘要接口，不直接依赖桌面端本地缓存

## 4. 核心原语

第一版建议引入四类核心对象。

### 4.1 `memory_item`

表示一个被持续跟踪的事实主题或记忆槽位。

示例：

- 某个项目的“当前约束”
- 某个任务的“最新执行摘要”
- 某个会话的“工作简报”
- 某条架构决策的“当前生效版本”

建议字段：

- `id`
- `scope_kind`
- `scope_id`
- `memory_kind`
- `stable_key`
- `created_at`
- `created_by`

其中：

- `scope_kind` 可为 `org`、`project`、`task`、`task_run`、`session`
- `memory_kind` 可为 `constraint`、`decision`、`summary`、`risk`、`acceptance_note`、`workspace_fact`
- `stable_key` 用于表达“这是一条长期可追踪主题”

### 4.2 `memory_revision`

表示某个事实主题的一次不可变修订。

建议字段：

- `id`
- `memory_item_id`
- `revision_no`
- `status`
- `title`
- `content`
- `structured_payload`
- `source_kind`
- `source_id`
- `confidence`
- `supersedes_revision_id`
- `created_at`
- `created_by`

关键语义：

- 修订是不可变的
- 新信息进入时，新增 revision，而不是覆盖旧 revision
- `supersedes_revision_id` 用来表达“新版本替代旧版本”

### 4.3 `memory_tag`

表示指向“当前有效 revision”的可变标签。

示例：

- `project/<id>/active-constraints`
- `project/<id>/active-decisions`
- `task/<id>/latest-summary`
- `session/<id>/working-brief`

建议字段：

- `id`
- `memory_item_id`
- `tag`
- `target_revision_id`
- `updated_at`
- `updated_by`

关键语义：

- revision 不可变
- tag 可移动
- 外部读取“当前值”时，优先走 tag，而不是自己找最新 revision

### 4.4 `memory_edge`

表示事实之间的显式关系。

建议字段：

- `id`
- `from_revision_id`
- `to_revision_id`
- `edge_kind`
- `created_at`

推荐边类型：

- `derived_from`
- `supports`
- `conflicts_with`
- `supersedes`
- `applies_to`
- `caused_by`
- `resolved_by`

## 5. 与现有核心模型的关系

### 5.1 不替代任务和运行模型

当前系统里的：

- `task`
- `task_run`
- `project_session`
- `audit_event`
- `artifact`

仍然是业务主实体。

版本化事实记忆层只是从这些主实体中提取“可复用、可追踪、可修订”的结构化结论。

### 5.2 和 thread / session 的关系

thread 或 provider session 仍然负责长上下文执行。

记忆层只沉淀跨会话仍然有效的内容，例如：

- 当前已经确认的范围约束
- 已取消的方向
- 当前建议接手时先读的摘要
- 上次失败的根因与修复结论

因此：

- thread 不是系统级共享记忆
- memory 也不是 thread 的替代品
- 二者是“执行上下文”和“长期事实沉淀”的关系

### 5.3 和客户端恢复的关系

桌面端当前已经有“最近焦点恢复”机制，但它偏向视图状态。

后续可以逐步把以下内容统一接到记忆层：

- 最近聚焦的项目/任务/会话摘要
- 当前推荐恢复入口
- 当前会话工作简报

但这不要求桌面端直接管理 thread 本体。

## 6. 简化版修订规则

Spotlight 不需要在第一版实现完整 AGM，但需要吸收“信念修正”的核心纪律。

### 6.1 新事实进入时

当系统从任务执行、项目聊天、验收或人工输入获得新信息时：

1. 先识别其属于哪个 `memory_item`
2. 判断是“补充同一主题”还是“创建新主题”
3. 如果改变当前有效结论，则创建新 `memory_revision`
4. 用 `memory_tag` 把“当前有效版本”指向新 revision
5. 如有冲突，建立 `conflicts_with` 或 `supersedes` 边

### 6.2 冲突处理

若新旧信息冲突，不直接删除旧版本，而是：

- 保留旧 revision
- 让新 revision 显式替代旧 revision
- 记录来源与时间
- 在必要时降低旧 revision 的有效状态

### 6.3 来源优先级

建议默认来源优先级：

1. 明确的用户指令
2. 已验收的任务结论
3. 管理员或审批动作
4. 结构化系统事件
5. 模型自动总结

这与 Agent 自治规则保持一致：最新明确决策优先于旧描述，人工明确取消优先于自动推断。

## 7. 第一版输入来源

第一版不要接太多来源，避免记忆层被噪音淹没。

优先接入以下四类来源：

### 7.1 任务运行摘要

从 `task_run` 提取：

- 目标
- 实际结果
- 测试结果
- 失败原因
- 下一步建议

### 7.2 项目聊天与人工指令

提取：

- 范围变更
- 取消信号
- 保留约束
- 优先级重排

### 7.3 验收与审计

提取：

- 验收结论
- 风险说明
- 危险操作确认
- 回滚说明

### 7.4 AI 洞察前置摘要

把多个低层事件先聚合成结构化事实，再交给 AI 洞察场景消费。

## 8. 第一版推荐事实类型

第一版只建议支持少量高价值类型：

- `project_constraint`
- `project_decision`
- `task_summary`
- `task_risk`
- `task_resolution`
- `session_brief`
- `acceptance_result`
- `workspace_fact`

不建议一开始就做“任意开放 schema”。

## 9. 存储策略

### 9.1 `0.1.x`

采用现有中心服务存储体系即可：

- 本地自举阶段可继续走本地状态文件
- 引入共享中心服务后优先落 Postgres

不新增：

- Neo4j
- Redis 工作内存层
- 专门的向量数据库

### 9.2 为什么先不用图数据库

虽然模型天然适合图关系，但当前阶段更重要的是：

- 先把原语和语义定稳
- 先让事实抽取和版本管理可用
- 先让洞察和恢复真正消费这层数据

当以下需求明显出现时，再评估图数据库：

- 跨项目复杂溯源查询
- 高频多跳依赖分析
- 大规模冲突事实合并
- 图级推理和高级检索

## 10. 与 AI 洞察引擎的集成

AI 洞察文档已明确要求：

- 先 collect raw signals
- 再 summarize into structured facts
- 再 route to the smallest sufficient model

版本化事实记忆层正是这个“structured facts”层的正式落点。

建议集成方式：

1. 原始事件先归档到业务实体和审计实体
2. 事实抽取器把高价值结论写入 `memory_revision`
3. AI 洞察按 scenario 读取对应 tag 和 revision 集合
4. 洞察输出也可回写成新 revision，但必须标注为分析结果，而不是源事实

## 11. 与移动端和后台的关系

该层应服务多端，但不能把多端耦合到桌面端实现上。

具体原则：

- 桌面端负责展示、恢复、执行
- 移动端读取中心服务汇总后的事实摘要
- 后台读取可审计、可回溯的事实与修订链

因此：

- 不新增桌面专属的后端协议前提
- 不改变 `/` 和 `/api/v1` 的统一入口方向
- 不让移动端依赖桌面端的本地缓存格式

## 12. 分阶段落地建议

### 12.1 第一阶段：文档与数据原语对齐

- 固化本设计文档
- 在数据模型中补 `memory_item / revision / tag / edge`
- 明确事实类型和来源优先级

### 12.2 第二阶段：最小 PoC

- 先只接 `task_summary`
- 先只接 `project_constraint`
- 先只支持 revision + tag
- 暂不开放复杂 edge 查询

### 12.3 第三阶段：接入 AI 洞察

- 日报总结读取项目约束和任务摘要
- 验收辅助读取任务结果和风险事实
- 失败解释读取失败签名与最近修订

### 12.4 第四阶段：接入自治循环

- Agent 认领新任务前读取当前项目约束
- 恢复会话前读取 session brief
- 发现用户取消信号后，自动写入 superseding revision

## 13. 测试要求

该层一旦落地，最低测试要求应包括：

- revision 新增与不可变性单元测试
- tag 切换正确性测试
- 冲突事实 supersede 回归测试
- 来源优先级测试
- AI 洞察输入拼装测试
- 移动端摘要接口不泄露无关私有事实的权限测试

## 14. 当前结论

Spotlight 对 Kumiho 方向最值得借鉴的不是“图库和检索栈”，而是：

- 版本化修订
- 可变标签指针
- 类型化关系边
- 显式冲突修正语义

因此当前推荐路线是：

- 先在现有架构内实现版本化事实记忆层
- 让它服务洞察、恢复和自治
- 等出现明确复杂图查询需求后，再评估是否升级到底层图库
