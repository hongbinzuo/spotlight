# `main.rs` 入口边界说明 V1

## 1. 目的

这份文档用于明确 `apps/server/src/main.rs` 在当前阶段还能保留什么、不能再承载什么。

目标不是机械地把代码“拆文件”，而是把服务端入口收口为稳定边界：

- `main.rs` 负责启动入口与极少量跨模块胶水
- 业务逻辑进入对应模块
- 后续继续推进 Wave 2 / Wave 3 时，不再把新逻辑重新堆回入口文件

这份边界说明对应：

- `docs/clawteam-adoption-todo.md` 中 Wave 1 的“入口文件重构边界”待办
- `docs/execution-slot-and-coordination-model-v1.md` 中的第一阶段执行槽模型落地

## 2. 当前结论

### 2.1 `main.rs` 可以保留的内容

- 模块声明
- 顶层共享类型
  - 例如 `AppState`、`BoardState`
- 启动入口
  - `main()`
- 少量真正跨模块共享、暂时没有更合适归属的基础胶水
  - 例如运行时超时包装
  - 极少量被多个模块共同依赖的状态辅助函数

### 2.2 `main.rs` 不应继续保留的内容

- HTTP handler
- task 状态流转与认领逻辑
- board / project / memory snapshot 拼装
- persisted state 默认值、加载与归一化
- 自动化调度主循环
- prompt 组装
- Git / worktree / snapshot 业务逻辑

换句话说：

- `main.rs` 不是“先临时放这里，以后再拆”的缓冲区
- 新逻辑默认必须进入对应模块

## 3. 当前模块归属

当前服务端的职责归属以以下规则为准：

- `state.rs`
  - persisted state 默认值、加载、归一化、落盘
- `server.rs`
  - router 装配、监听地址、应用启动装配
- `handlers.rs`
  - HTTP API handler
  - 项目会话入口
  - 任务启动、暂停、恢复、重评估等请求处理
- `task_ops.rs`
  - task / project 查找、任务认领、任务运行状态辅助、工作区解析、活动记录
- `snapshot.rs`
  - board / project 读取视图与裁剪
- `automation.rs`
  - watchdog、自动认领、自动恢复、自动化循环
- `runtime.rs`
  - provider runtime session 抽象与错误映射
- `git_ops.rs`
  - Git 相关行为
- `prompt.rs`
  - developer instruction 与 prompt 组装

## 4. 本次收口落地

本轮进一步确认并落实以下边界：

- `server.rs` 的路由装配直接绑定 `handlers.rs`
- 入口装配不再依赖 `main.rs` 中的私有 handler 实现
- `claim task` 路由继续走 `task_ops.rs` 中的认领逻辑

这意味着后续继续清理 `main.rs` 时，可以优先删除“已经有模块归属、但仍残留在入口文件中的旧实现”，而不会影响路由入口边界。

## 5. 后续收口规则

后续所有相关改动必须遵守：

1. 新增 API 时，优先加到 `handlers.rs`，并由 `server.rs` 负责装配。
2. 如果某个 helper 被多个 handler 共享，优先判断它是否属于 `task_ops.rs`、`snapshot.rs`、`state.rs`、`git_ops.rs` 或 `runtime.rs`，而不是直接放回 `main.rs`。
3. 如果某段逻辑已经在模块中存在实现，`main.rs` 不允许继续保留同职责重复版本。
4. 收口顺序优先做“行为不变的归属迁移”，再做“行为升级”。
5. 任何从 `main.rs` 迁出的行为，都要补编译验证；如果改到状态流转、恢复、自动化链路，还必须补回归测试。

## 6. 当前仍待继续清理的残留

虽然路由入口边界已经进一步收紧，但 `main.rs` 里仍有残留旧实现，需要后续继续清理：

- 仍保留一批与 `handlers.rs`、`task_ops.rs` 同职责的旧函数
- 仍有部分跨模块共享 helper 尚未找到最终归属
- `main.rs` 仍承担了一部分运行态与服务装配周边逻辑

所以下一轮的目标不是“重新定义边界”，而是按本文件继续删除残留重复实现。

## 7. 参考来源

- Rust Book Chapter 7.5
  - Separating Modules into Different Files
- Rust Book Chapter 12.3
  - Refactoring to Improve Modularity and Error Handling
- [docs/clawteam-adoption-todo.md](./clawteam-adoption-todo.md)
- [docs/execution-slot-and-coordination-model-v1.md](./execution-slot-and-coordination-model-v1.md)
