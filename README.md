# Spotlight

Spotlight 是一个面向多人、多项目、多任务、多 Agent 协作的软件执行平台原型。

当前 `0.1.0` 版本先实现最小可用闭环：

- 多项目任务看板
- 中文 Agent 操作界面
- 本地 Codex CLI 长会话运行时
- 任务暂停、补充提示词、恢复执行
- 自动认领开关
- 项目目录探索任务
- 本地编译重启任务模板
- 云端安装重启任务模板
- Spotlight 自举任务播种

## 当前目录结构

- `apps/server`
  - Axum 服务端，提供 JSON API 和最小中文 Web 界面
- `crates/platform-core`
  - 项目、任务、Agent、运行日志等共享领域模型
- `crates/workflow-engine`
  - 预留给工作流与状态机能力
- `crates/provider-runtime`
  - 预留给多模型 CLI / Runtime 抽象
- `crates/insight-engine`
  - 预留给 AI 洞察、日报、分析能力
- `docs`
  - 产品约束、架构、数据模型、协议决策等文档

## 运行

```powershell
cargo run -p spotlight-server
```

启动后打开 [http://127.0.0.1:3000](http://127.0.0.1:3000)。

## 当前交互说明

- 默认会先打开一个“普通项目”视角，不假设它一定有源码
- `Spotlight 平台自身` 项目会自动从 `docs` 和 `AGENTS.md` 播种自举任务
- “探索目录”会创建一条预制探索任务，交给 Agent 学习当前路径内容并给出建议任务列表
- “本地编译重启”会基于当前工作目录创建一条预制任务，附带技术栈初步识别、本地编译/重启目标和 `deploy.md` 维护要求
- “云端安装重启”会要求输入服务器地址、SSH 用户与认证说明，并创建一条远端部署/重启任务；明文密码不会写入任务描述
- 真实运行时通过 `codex app-server` 接入本机 Codex CLI
