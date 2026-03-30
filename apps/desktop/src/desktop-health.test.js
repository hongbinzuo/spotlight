import test from "node:test";
import assert from "node:assert/strict";

import { deriveDesktopHealth, deriveDesktopHealthError } from "./desktop-health.js";

test("服务就绪时显示客户端和后端均正常", () => {
  const view = deriveDesktopHealth({
    server_running: true,
    backend_state: "ready",
    tcp_connected: true,
    http_responding: true,
    auto_launching: false,
    message: "桌面端已连接到本地服务"
  });

  assert.equal(view.tone, "ready");
  assert.equal(view.shellLabel, "已启动");
  assert.equal(view.tcpLabel, "已连通");
  assert.equal(view.httpLabel, "已就绪");
  assert.equal(view.autoLabel, "无需拉起");
});

test("自动拉起过程中显示端口和接口分层状态", () => {
  const view = deriveDesktopHealth({
    server_running: false,
    backend_state: "starting",
    tcp_connected: false,
    http_responding: false,
    auto_launching: true,
    last_launch_message: "正在自动拉起本地 Spotlight 服务"
  });

  assert.equal(view.tone, "busy");
  assert.equal(view.tcpLabel, "未连通");
  assert.equal(view.httpLabel, "未响应");
  assert.equal(view.autoLabel, "进行中");
  assert.match(view.recentAction, /自动拉起/);
});

test("原生桥异常时给出错误态健康视图", () => {
  const view = deriveDesktopHealthError(new Error("native bridge timeout"));

  assert.equal(view.tone, "error");
  assert.equal(view.shellLabel, "异常");
  assert.equal(view.tcpLabel, "未知");
  assert.equal(view.recentAction, "Error: native bridge timeout");
});
