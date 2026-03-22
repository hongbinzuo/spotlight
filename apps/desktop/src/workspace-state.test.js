import test from "node:test";
import assert from "node:assert/strict";

import { deriveWorkspaceErrorState, deriveWorkspaceState } from "./workspace-state.js";

test("服务端运行时显示已连接并允许加载内嵌面板", () => {
  const view = deriveWorkspaceState({
    server_running: true,
    backend_state: "ready",
    message: "桌面客户端已经连接到本机 Spotlight 服务。"
  });

  assert.equal(view.statusKind, "ready");
  assert.equal(view.workspaceBadge, "已连接");
  assert.equal(view.shouldLoadFrame, true);
});

test("服务端未运行时保持客户端可用并提示手动启动", () => {
  const view = deriveWorkspaceState({
    server_running: false,
    backend_state: "offline",
    message: "本机 Spotlight 服务未运行。请单独启动服务端后，再回到客户端刷新连接状态。"
  });

  assert.equal(view.statusKind, "idle");
  assert.equal(view.workspaceBadge, "等待连接");
  assert.equal(view.shouldLoadFrame, false);
  assert.match(view.placeholderMessage, /单独启动 spotlight-server/);
});

test("自动拉起服务时显示忙碌态并等待自动连接", () => {
  const view = deriveWorkspaceState({
    server_running: false,
    backend_state: "starting",
    message: "正在自动拉起本地 Spotlight 服务：C:/repo/target/debug/spotlight-server.exe。客户端会在服务就绪后自动连接。"
  });

  assert.equal(view.statusKind, "busy");
  assert.equal(view.workspaceBadge, "拉起中");
  assert.equal(view.shouldLoadFrame, false);
  assert.match(view.placeholderMessage, /自动拉起本地 Spotlight 服务/);
});

test("状态检查失败时仍然保留客户端占位态", () => {
  const view = deriveWorkspaceErrorState(new Error("boom"));

  assert.equal(view.statusKind, "error");
  assert.equal(view.workspaceBadge, "连接失败");
  assert.equal(view.shouldLoadFrame, false);
  assert.equal(view.statusMessage, "Error: boom");
});
