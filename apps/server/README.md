# Spotlight Server

`apps/server` 是 Spotlight `0.1.0` 的 Rust 服务端骨架，当前承载四类最小能力：

- 项目列表与项目摘要
- 任务看板与任务详情上下文
- Agent 面板与自动认领入口
- 统一入口页面 `/`

当前实现特点：

- 使用 `axum` 组装 HTTP 路由
- 通过 `build_app` 暴露统一服务入口
- `0.1.x` 启动期先将状态落在 `.spotlight/server-state.json`
- 根页面直接返回内嵌 HTML，用于本地自举和冒烟验证

关键入口：

- `src/server.rs`
  - 监听地址解析
  - 路由装配
  - 服务端骨架启动入口
- `src/main.rs`
  - 应用状态定义
  - 运行时、任务、恢复与测试实现

当前最小核心路由：

- `GET /`
- `GET /api/v1/board`
- `GET /api/v1/projects`
- `GET /api/v1/agents`
- `GET /api/v1/projects/{project_id}/summary`

验证命令：

```powershell
cargo test -p spotlight-server service_shell_exposes_0_1_0_core_surfaces -- --nocapture
cargo check -p spotlight-server
```
