# Spotlight Desktop

This is the real desktop shell for Spotlight.

当前目标：

- package Spotlight as a cross-platform desktop client
- support Windows and macOS from the same codebase
- start or attach to the local `spotlight-server`
- render a simple Chinese desktop shell around the local task board

## Current Mode

The desktop app is a Tauri shell that:

- loads a local front-end
- checks whether `http://127.0.0.1:3000` is alive
- tries to start the local `spotlight-server` if needed
- embeds the running task board in an in-app view
- can hand off desktop rebuild and restart to an external helper process on local development machines

当前阶段默认版本号采用 `0.1.x` 规则，工程包和配置中的版本号使用完整语义化格式，例如 `0.1.0`。

目前桌面壳会优先在仓库工作区里寻找本地服务端二进制。

Preferred path during development:

- `../../target/debug/spotlight-server.exe` on Windows
- `../../target/debug/spotlight-server` on macOS

## Windows

```powershell
npm install
npm run tauri dev
```

## macOS

Build on a Mac machine:

```bash
npm install
npm run tauri dev
```

or build an app bundle:

```bash
npm run tauri build
```

Important:

- macOS application bundles must be built on macOS
- a Windows machine cannot produce a signed native macOS `.app` for your friend
- release builds should be produced with `npm run tauri build`
- do not rely on `src-tauri/target/release/spotlight-desktop.exe` if it was created by plain `cargo build --release`, because that binary can still expect the Vite dev server at `http://127.0.0.1:1421`

## Self Restart

The desktop shell now includes a native `rebuild_and_restart_desktop` command.

On local development machines it works like this:

- the client writes a helper script into the system temp directory
- the helper waits for the current `spotlight-desktop.exe` process to exit
- the helper runs `npm run tauri build -- --no-bundle`
- the helper launches the rebuilt release executable again

This is intended for Spotlight self-hosted local iteration, so the client can keep evolving itself without requiring a manual rebuild every time the desktop code changes.

## Next Desktop Steps

- replace the embedded web board with a native left-right desktop layout
- route desktop actions directly to Tauri commands where helpful
- bundle the local server as a sidecar for packaged builds
- add window/session restoration and richer desktop status handling
