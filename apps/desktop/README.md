# Spotlight Desktop

这是 Spotlight 的真实桌面执行壳，而不是简单打开一个浏览器标签页。

当前目标：

- 把 Spotlight 打包成跨平台桌面客户端
- 用同一套代码支持 Windows 和 macOS
- 自动连接或拉起本地 `spotlight-server`
- 在桌面壳内承载中文任务看板与 Agent 工作区

## 当前模式

当前桌面端是一个 Tauri shell，它会：

- 加载本地前端界面
- 检查 `http://127.0.0.1:3000` 是否可用
- 当后端离线时优先尝试自动拉起本地 `spotlight-server`
- 记住最近聚焦的项目、任务和项目会话，并在重开后恢复
- 在侧边栏展示最近恢复位置，并支持一键清除恢复记录
- 以内嵌视图承载运行中的统一任务看板
- 在本地开发机上把桌面端重建和重启交给外部 helper 处理

当前阶段默认采用 `0.1.x` 版本规则，工程包和配置中的版本号使用完整语义化格式，例如 `0.1.0`。

开发时桌面壳会优先在仓库工作区内查找本地服务端二进制。推荐路径：

- Windows：`../../target/debug/spotlight-server.exe`
- macOS：`../../target/debug/spotlight-server`
- 本地 release 构建后：`../../target/release/spotlight-server(.exe)`

如果这些二进制不存在，桌面壳会退回到手动运行 `cargo run -p spotlight-server` 的方式。

## Windows

```powershell
npm install
npm run tauri dev
```

## macOS

请在 Mac 机器上构建：

```bash
npm install
npm run tauri dev
```

或者构建 app bundle：

```bash
npm run tauri build
```

注意：

- macOS 应用包必须在 macOS 上构建
- Windows 机器不能直接产出签名后的原生 macOS `.app`
- 正式构建请使用 `npm run tauri build`
- 不要依赖仅通过 `cargo build --release` 生成的 `src-tauri/target/release/spotlight-desktop.exe`，因为它仍可能依赖 Vite 开发服务器 `http://127.0.0.1:1421`

## 自重启

桌面壳已经包含原生命令 `rebuild_and_restart_desktop`。

在本地开发机上的工作方式是：

- 客户端把 helper 脚本写到系统临时目录
- helper 等待当前 `spotlight-desktop.exe` 进程退出
- helper 执行 `npm run tauri build -- --no-bundle`
- helper 再次拉起重建后的 release 可执行文件

这套机制是为 Spotlight 本地自举迭代准备的，让客户端能在桌面代码变化后继续自我演进，而不要求每次都手动重建。

## 下一步

- 把当前嵌入式网页看板逐步替换为更原生的左右分栏桌面布局
- 在合适的地方把桌面动作直接收拢到 Tauri 命令
- 为打包版本引入本地服务 sidecar
- 在后续版本里把恢复入口、自动运行状态和会话控制做得更完整
