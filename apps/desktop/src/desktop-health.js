function toneForBackend(status) {
  if (status?.server_running) {
    return "ready";
  }
  if (status?.backend_state === "starting" || status?.backend_state === "partial") {
    return "busy";
  }
  return "idle";
}

export function deriveDesktopHealth(status) {
  const tone = toneForBackend(status);
  const tcpConnected = Boolean(status?.tcp_connected);
  const httpResponding = Boolean(status?.http_responding);
  const autoLaunching = Boolean(status?.auto_launching);

  return {
    tone,
    shellLabel: "已启动",
    shellTone: "ready",
    tcpLabel: tcpConnected ? "已连通" : "未连通",
    tcpTone: tcpConnected ? "ready" : "idle",
    httpLabel: httpResponding ? "已就绪" : tcpConnected ? "等待就绪" : "未响应",
    httpTone: httpResponding ? "ready" : tcpConnected ? "busy" : "idle",
    autoLabel: autoLaunching
      ? "进行中"
      : status?.server_running
        ? "无需拉起"
        : status?.backend_state === "partial"
          ? "等待接口"
          : "空闲",
    autoTone: autoLaunching
      ? "busy"
      : status?.server_running
        ? "ready"
        : status?.backend_state === "partial"
          ? "busy"
          : "idle",
    recentAction: String(status?.last_launch_message || status?.message || "暂无最近动作")
  };
}

export function deriveDesktopHealthError(error) {
  return {
    tone: "error",
    shellLabel: "异常",
    shellTone: "error",
    tcpLabel: "未知",
    tcpTone: "error",
    httpLabel: "未知",
    httpTone: "error",
    autoLabel: "未知",
    autoTone: "error",
    recentAction: String(error)
  };
}
