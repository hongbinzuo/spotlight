export const BACKEND_URL = "http://127.0.0.1:3000";
export const BLANK_FRAME_URL = "about:blank";

export function deriveWorkspaceState(status) {
  if (status.server_running) {
    return {
      statusKind: "ready",
      statusTitle: "本地服务已连接",
      statusMessage: status.message,
      workspaceBadge: "已连接",
      placeholderTitle: "",
      placeholderMessage: "",
      shouldLoadFrame: true
    };
  }

  return {
    statusKind: "idle",
    statusTitle: "本地服务未连接",
    statusMessage: status.message,
    workspaceBadge: "等待连接",
    placeholderTitle: "服务端未启动",
    placeholderMessage:
      "客户端界面已正常加载。请单独启动 spotlight-server，然后点击“检查服务状态”或“刷新内嵌面板”。",
    shouldLoadFrame: false
  };
}

export function deriveWorkspaceErrorState(error) {
  return {
    statusKind: "error",
    statusTitle: "无法获取状态",
    statusMessage: String(error),
    workspaceBadge: "连接失败",
    placeholderTitle: "暂时无法检查服务状态",
    placeholderMessage:
      "客户端界面已正常加载。请确认桌面端环境正常，然后稍后重新检查连接状态。",
    shouldLoadFrame: false
  };
}
