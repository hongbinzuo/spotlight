# 真实 Codex 长会话接入收敛说明

日期：2026-03-26

## 背景

在这次收敛前，`apps/server/src/runtime.rs` 里直接维护了一份 `codex app-server` 的 JSON-RPC 长会话实现，而 `crates/provider-runtime` 里也已经有同样的 Codex provider 适配器与会话逻辑。

这会带来两个问题：

- 真实运行时代码分散在服务端和 provider 抽象层两处，后续容易继续分叉。
- `provider-runtime` 虽然已经存在，但主执行链路没有真正通过它启动和恢复真实会话。

## 本次调整

- 服务端桥接名词从 `CodexRuntimeSession` 收敛为 `ProviderRuntimeSession`，主执行链路、恢复结果和运行时会话表都改用 provider-neutral 命名。
- 为兼容仍在拆分中的旧模块，桥接层暂时保留 `CodexRuntimeSession -> ProviderRuntimeSession` 的类型别名，但新代码不再继续扩散旧命名。
- 桥接层本身不再自己实现 `codex app-server` 协议，而是统一委托给 `crates/provider-runtime`。
- `SPOTLIGHT_PROVIDER` 从单一 provider 选择扩展为候选链配置，支持单值或逗号分隔顺序，例如 `claude,codex`。
- 服务端会按候选顺序尝试启动 provider，并把失败聚合成统一的 HTTP 错误语义，避免以后为 Claude、Kimi、MiniMax 在服务层继续追加硬编码分支。

## 结果

这次调整后，Spotlight 当前真实 Codex 长会话链路的职责边界变为：

- `crates/provider-runtime`
  - 管理 `codex app-server` 子进程
  - 发送 provider-native 请求
  - 归一化输出为 `RuntimeEvent`
  - 暴露统一的线程、turn、打断、关闭能力
- `apps/server/src/runtime.rs`
  - 负责 provider 候选链解析、启动回退和 provider 层错误到 HTTP 语义的映射
  - 作为现有服务端执行链路的薄桥接层
- `apps/server/src/main.rs`
  - 继续负责任务状态机、自动恢复、项目会话、审计和 UI 快照
  - 仅依赖 provider-neutral 的运行时桥接类型，不再依赖 Codex 命名

## 后续约束

- 后续新增 provider 时，优先扩展 `crates/provider-runtime`，不要在 `apps/server` 再复制一份协议实现。
- 后续新增 provider 时，优先把 provider id 注入候选链和 registry，不要在服务端业务逻辑里新增 `if provider == ...` 分支。
- 服务端状态机、UI、审计只依赖归一化的 `RuntimeEvent` 和 provider capability，不直接依赖某个 provider 的原始协议字段。
