import {
  BACKEND_URL,
  BLANK_FRAME_URL,
  deriveWorkspaceErrorState,
  deriveWorkspaceState
} from "./workspace-state.js";

const elements = {
  boardFrame: document.getElementById("boardFrame"),
  checkButton: document.getElementById("checkButton"),
  probeButton: document.getElementById("probeButton"),
  reloadButton: document.getElementById("reloadButton"),
  browserButton: document.getElementById("browserButton"),
  copyUrlButton: document.getElementById("copyUrlButton"),
  statusCard: document.getElementById("statusCard"),
  statusTitle: document.getElementById("statusTitle"),
  statusMessage: document.getElementById("statusMessage"),
  platformValue: document.getElementById("platformValue"),
  urlValue: document.getElementById("urlValue"),
  probeCard: document.getElementById("probeCard"),
  probeTitle: document.getElementById("probeTitle"),
  probeMessage: document.getElementById("probeMessage"),
  workspaceBadge: document.getElementById("workspaceBadge"),
  workspacePlaceholder: document.getElementById("workspacePlaceholder"),
  placeholderTitle: document.getElementById("placeholderTitle"),
  placeholderMessage: document.getElementById("placeholderMessage")
};

let serverRunning = false;

function markReady() {
  window.__SPOTLIGHT_DESKTOP_MARK_READY__?.();
}

function requestFallback(error) {
  window.__SPOTLIGHT_DESKTOP_REQUEST_FALLBACK__?.(String(error));
}

function tauriInvoke(command, args = {}) {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) {
    throw new Error("当前环境没有注入 Tauri API。请使用 `npm run tauri dev` 启动桌面客户端。");
  }
  return invoke(command, args);
}

function invokeWithTimeout(command, args = {}, timeoutMs = 1500) {
  return Promise.race([
    tauriInvoke(command, args),
    new Promise((_, reject) => {
      window.setTimeout(() => {
        reject(new Error(`Tauri 命令 ${command} 超时`));
      }, timeoutMs);
    })
  ]);
}

function setStatus(kind, title, message) {
  elements.statusCard.className = `status-card status-${kind}`;
  elements.statusTitle.textContent = title;
  elements.statusMessage.textContent = message;
  elements.workspaceBadge.textContent =
    kind === "ready" ? "已连接" : kind === "error" ? "连接失败" : "处理中";
}

function setProbeStatus(kind, title, message) {
  elements.probeCard.className = `status-card status-${kind}`;
  elements.probeTitle.textContent = title;
  elements.probeMessage.textContent = message;
}

function unloadFrame() {
  if (elements.boardFrame.src !== BLANK_FRAME_URL) {
    elements.boardFrame.src = BLANK_FRAME_URL;
  }
}

function showPlaceholder(title, message) {
  elements.placeholderTitle.textContent = title;
  elements.placeholderMessage.textContent = message;
  elements.workspacePlaceholder.hidden = false;
  unloadFrame();
}

function hidePlaceholder() {
  elements.workspacePlaceholder.hidden = true;
}

function applyWorkspaceState(view) {
  setStatus(view.statusKind, view.statusTitle, view.statusMessage);
  elements.workspaceBadge.textContent = view.workspaceBadge;

  if (view.shouldLoadFrame) {
    hidePlaceholder();
    return;
  }

  showPlaceholder(view.placeholderTitle, view.placeholderMessage);
}

function refreshFrame() {
  if (!serverRunning) {
    showPlaceholder(
      "服务端未启动",
      "客户端界面已正常加载。请先单独启动 spotlight-server，再刷新内嵌面板。"
    );
    return;
  }

  elements.boardFrame.src = `${BACKEND_URL}?ts=${Date.now()}`;
  hidePlaceholder();
}

async function refreshStatus() {
  try {
    const status = await invokeWithTimeout("app_status");
    elements.platformValue.textContent = status.platform;
    elements.urlValue.textContent = status.backend_url;
    serverRunning = status.server_running;

    const view = deriveWorkspaceState(status);
    applyWorkspaceState(view);
    markReady();

    if (view.shouldLoadFrame) {
      refreshFrame();
    }
  } catch (error) {
    serverRunning = false;
    applyWorkspaceState(deriveWorkspaceErrorState(error));
    requestFallback(error);
  }
}

async function probeBackend() {
  setProbeStatus("busy", "正在探测", "客户端正在尝试从原生侧探测本机服务。");

  try {
    const probe = await invokeWithTimeout("probe_backend");
    elements.urlValue.textContent = probe.backend_url;

    if (probe.tcp_connected && probe.http_responding) {
      setProbeStatus("ready", "探测成功", probe.message);
      return;
    }

    setProbeStatus("error", "探测异常", probe.message);
  } catch (error) {
    setProbeStatus("error", "探测失败", String(error));
  }
}

async function copyBackendUrl() {
  try {
    await navigator.clipboard.writeText(elements.urlValue.textContent);
    setProbeStatus("ready", "地址已复制", `已复制 ${elements.urlValue.textContent}`);
  } catch (error) {
    setProbeStatus("error", "复制失败", String(error));
  }
}

async function openInBrowser() {
  if (!serverRunning) {
    applyWorkspaceState(
      deriveWorkspaceErrorState("本机 Spotlight 服务未运行，请先单独启动服务端。")
    );
    return;
  }

  try {
    await invokeWithTimeout("open_backend_in_browser");
  } catch (error) {
    applyWorkspaceState(deriveWorkspaceErrorState(error));
    requestFallback(error);
  }
}

elements.checkButton.addEventListener("click", refreshStatus);
elements.probeButton.addEventListener("click", probeBackend);
elements.reloadButton.addEventListener("click", refreshFrame);
elements.browserButton.addEventListener("click", openInBrowser);
elements.copyUrlButton.addEventListener("click", copyBackendUrl);

refreshStatus();
setInterval(refreshStatus, 5000);
