import {
  BACKEND_URL,
  BLANK_FRAME_URL,
  deriveWorkspaceErrorState,
  deriveWorkspaceState
} from "./workspace-state.js";
import {
  buildBoardUrl,
  clearDesktopFocus,
  hasBoardFocus,
  parseBoardFocusMessage,
  readDesktopFocus,
  writeDesktopFocus
} from "./board-focus-state.js";
import { deriveDesktopHealth, deriveDesktopHealthError } from "./desktop-health.js";
import { deriveDesktopProjectList } from "./desktop-project-list.js";
import { deriveDesktopTaskList } from "./desktop-task-list.js";
import { deriveGovernanceOverview } from "./governance-overview.js";
import { deriveTaskActivityView } from "./task-activity-view.js";

const elements = {
  boardFrame: document.getElementById("boardFrame"),
  checkButton: document.getElementById("checkButton"),
  probeButton: document.getElementById("probeButton"),
  reloadButton: document.getElementById("reloadButton"),
  browserButton: document.getElementById("browserButton"),
  copyUrlButton: document.getElementById("copyUrlButton"),
  clearFocusButton: document.getElementById("clearFocusButton"),
  statusCard: document.getElementById("statusCard"),
  statusTitle: document.getElementById("statusTitle"),
  statusMessage: document.getElementById("statusMessage"),
  healthCard: document.getElementById("healthCard"),
  healthShellValue: document.getElementById("healthShellValue"),
  healthTcpValue: document.getElementById("healthTcpValue"),
  healthHttpValue: document.getElementById("healthHttpValue"),
  healthAutoValue: document.getElementById("healthAutoValue"),
  healthActionValue: document.getElementById("healthActionValue"),
  platformValue: document.getElementById("platformValue"),
  urlValue: document.getElementById("urlValue"),
  probeCard: document.getElementById("probeCard"),
  probeTitle: document.getElementById("probeTitle"),
  probeMessage: document.getElementById("probeMessage"),
  workspaceBadge: document.getElementById("workspaceBadge"),
  workspacePlaceholder: document.getElementById("workspacePlaceholder"),
  placeholderTitle: document.getElementById("placeholderTitle"),
  placeholderMessage: document.getElementById("placeholderMessage"),
  restoredProjectValue: document.getElementById("restoredProjectValue"),
  restoredTaskValue: document.getElementById("restoredTaskValue"),
  restoredSessionValue: document.getElementById("restoredSessionValue"),
  restoredFocusHint: document.getElementById("restoredFocusHint"),
  projectQueueSummary: document.getElementById("projectQueueSummary"),
  projectQueueList: document.getElementById("projectQueueList"),
  taskQueueScope: document.getElementById("taskQueueScope"),
  taskQueueSummary: document.getElementById("taskQueueSummary"),
  taskQueueList: document.getElementById("taskQueueList"),
  governanceCard: document.getElementById("governanceCard"),
  governanceTitle: document.getElementById("governanceTitle"),
  governanceMessage: document.getElementById("governanceMessage"),
  governanceProjectValue: document.getElementById("governanceProjectValue"),
  governanceTaskValue: document.getElementById("governanceTaskValue"),
  governanceTaskStatusValue: document.getElementById("governanceTaskStatusValue"),
  governanceFreshnessValue: document.getElementById("governanceFreshnessValue"),
  governanceReasonValue: document.getElementById("governanceReasonValue"),
  governanceEvidenceValue: document.getElementById("governanceEvidenceValue"),
  governanceRecoveryValue: document.getElementById("governanceRecoveryValue"),
  governanceAutomationValue: document.getElementById("governanceAutomationValue"),
  governanceCountsValue: document.getElementById("governanceCountsValue"),
  governanceOutputValue: document.getElementById("governanceOutputValue"),
  governanceAlert: document.getElementById("governanceAlert"),
  activitySimpleButton: document.getElementById("activitySimpleButton"),
  activityDiagnosticButton: document.getElementById("activityDiagnosticButton"),
  activitySearchRow: document.getElementById("activitySearchRow"),
  activitySearchInput: document.getElementById("activitySearchInput"),
  activityDetailCard: document.getElementById("activityDetailCard")
};

const ACTIVITY_DETAIL_MODE_STORAGE_KEY = "spotlight.desktop.activity-mode.v1";

let serverRunning = false;
let currentBackendUrl = BACKEND_URL;
let restoredBoardFocus = readDesktopFocus(globalThis.localStorage);
let governanceRequestToken = 0;
let latestBoardSnapshot = null;
let activityDetailMode = readStoredActivityMode();
let activitySearchQuery = "";

function readStoredActivityMode() {
  try {
    const value = globalThis.localStorage?.getItem(ACTIVITY_DETAIL_MODE_STORAGE_KEY);
    return value === "diagnostic" ? "diagnostic" : "simple";
  } catch (_) {
    return "simple";
  }
}

function writeStoredActivityMode(mode) {
  activityDetailMode = mode === "diagnostic" ? "diagnostic" : "simple";
  try {
    globalThis.localStorage?.setItem(ACTIVITY_DETAIL_MODE_STORAGE_KEY, activityDetailMode);
  } catch (_) {
    // Ignore storage failures in fallback mode.
  }
}

function setupDesktopMaintenanceActions() {
  const actionList = document.querySelector(".action-list");
  if (!actionList || document.getElementById("rebuildDesktopButton")) {
    return;
  }

  const rebuildButton = document.createElement("button");
  rebuildButton.id = "rebuildDesktopButton";
  rebuildButton.className = "secondary";
  rebuildButton.textContent = "重建并重启客户端";
  rebuildButton.addEventListener("click", rebuildDesktop);
  actionList.appendChild(rebuildButton);
  elements.rebuildDesktopButton = rebuildButton;
}

function markReady() {
  window.__SPOTLIGHT_DESKTOP_MARK_READY__?.();
}

function requestFallback(error) {
  window.__SPOTLIGHT_DESKTOP_REQUEST_FALLBACK__?.(String(error));
}

function tauriInvoke(command, args = {}) {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) {
    throw new Error("当前环境没有注入 Tauri API，请使用 `npm run tauri dev` 启动桌面客户端。");
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

function setHealthMetric(element, label, tone) {
  if (!element) {
    return;
  }
  element.textContent = label;
  element.className = `health-value tone-${tone}`;
}

function applyDesktopHealth(view) {
  if (!elements.healthCard) {
    return;
  }

  elements.healthCard.className = `health-card tone-${view.tone}`;
  setHealthMetric(elements.healthShellValue, view.shellLabel, view.shellTone);
  setHealthMetric(elements.healthTcpValue, view.tcpLabel, view.tcpTone);
  setHealthMetric(elements.healthHttpValue, view.httpLabel, view.httpTone);
  setHealthMetric(elements.healthAutoValue, view.autoLabel, view.autoTone);
  elements.healthActionValue.textContent = view.recentAction;
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

function formatFocusLabel(label, fallbackPrefix, id) {
  if (label) {
    return label;
  }
  if (id) {
    return `${fallbackPrefix} ${id}`;
  }
  return "未记录";
}

function renderRestoredFocus() {
  const hasFocus = hasBoardFocus(restoredBoardFocus);

  elements.restoredProjectValue.textContent = formatFocusLabel(
    restoredBoardFocus.projectName,
    "项目",
    restoredBoardFocus.projectId
  );
  elements.restoredTaskValue.textContent = formatFocusLabel(
    restoredBoardFocus.taskTitle,
    "任务",
    restoredBoardFocus.taskId
  );
  elements.restoredSessionValue.textContent = formatFocusLabel(
    restoredBoardFocus.sessionTitle,
    "会话",
    restoredBoardFocus.sessionId
  );
  elements.restoredFocusHint.textContent = hasFocus
    ? "下次打开客户端时，会优先恢复到这里；你也可以随时清除这条记录。"
    : "当前还没有恢复记录。你在看板里切换项目、任务或项目会话后，这里会自动更新。";

  if (elements.clearFocusButton) {
    elements.clearFocusButton.disabled = !hasFocus;
  }
}

function persistBoardFocus(focus) {
  restoredBoardFocus = writeDesktopFocus(focus, globalThis.localStorage);
  renderRestoredFocus();
}

function rerenderBoardDerivedPanels() {
  if (!latestBoardSnapshot) {
    return;
  }

  renderProjectQueue(latestBoardSnapshot);
  renderTaskQueue(latestBoardSnapshot);
  applyGovernanceView(deriveGovernanceOverview(latestBoardSnapshot, restoredBoardFocus));
  renderTaskActivityPanel(
    deriveTaskActivityView(latestBoardSnapshot, restoredBoardFocus, {
      mode: activityDetailMode,
      searchQuery: activitySearchQuery
    })
  );
}

function handleBoardMessage(event) {
  const focus = parseBoardFocusMessage(event.data);
  if (!focus) {
    return;
  }

  persistBoardFocus(focus);
  rerenderBoardDerivedPanels();
}

function clearRestoredFocus() {
  restoredBoardFocus = clearDesktopFocus(globalThis.localStorage);
  renderRestoredFocus();
  setProbeStatus("ready", "已清除恢复记录", "下次重新打开客户端时，将从默认入口进入，不再自动恢复上次视图。");
  rerenderBoardDerivedPanels();
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

function buildGovernancePlaceholder(kind, title, message) {
  return {
    tone: kind,
    title,
    message,
    projectName: formatFocusLabel(
      restoredBoardFocus.projectName,
      "项目",
      restoredBoardFocus.projectId
    ),
    taskTitle: formatFocusLabel(restoredBoardFocus.taskTitle, "任务", restoredBoardFocus.taskId),
    taskStatusLabel: kind === "error" ? "等待复核" : kind === "busy" ? "同步中" : "未连接",
    freshnessLabel: "暂无最新信号",
    freshnessDetail: "还没有输出或活动记录。",
    stateReason: "客户端会优先显示状态依据、恢复信号和关键输出，而不是只保留裸状态。",
    evidenceSummary: "等待服务端返回治理快照。",
    recoveryLabel: "等待服务端返回恢复状态。",
    automationLabel: "等待服务端返回自动运行信号。",
    countsLabel: "运行中 0 · 待恢复 0 · 自动运行 0 · 待答 0",
    primaryOutput: message,
    alert: kind === "error" ? message : ""
  };
}

function applyTaskQueuePlaceholder(scopeLabel, summary, message) {
  if (!elements.taskQueueList) {
    return;
  }

  elements.taskQueueScope.textContent = scopeLabel;
  elements.taskQueueSummary.textContent = summary;
  elements.taskQueueList.innerHTML = `<div class="task-queue-empty">${escapeHtml(message)}</div>`;
}

function applyProjectQueuePlaceholder(summary, message) {
  if (!elements.projectQueueList) {
    return;
  }

  elements.projectQueueSummary.textContent = summary;
  elements.projectQueueList.innerHTML = `<div class="task-queue-empty">${escapeHtml(message)}</div>`;
}

function renderProjectQueue(board) {
  if (!elements.projectQueueList) {
    return;
  }

  const view = deriveDesktopProjectList(board, restoredBoardFocus);
  elements.projectQueueSummary.textContent = view.summary;

  if (!view.items.length) {
    elements.projectQueueList.innerHTML =
      '<div class="task-queue-empty">当前还没有可展示的项目入口。服务端返回项目后，这里会优先暴露最需要推进或复核的项目。</div>';
    return;
  }

  elements.projectQueueList.innerHTML = view.items
    .map(
      item => `
        <button
          type="button"
          class="project-queue-item project-tone-${item.tone} ${item.active ? "active" : ""}"
          data-project-id="${escapeHtml(item.id)}"
        >
          <div class="project-queue-head">
            <strong class="project-queue-title">${escapeHtml(item.name)}</strong>
            <span class="task-queue-badge strong">${escapeHtml(item.signalLabel)}</span>
          </div>
          <div class="task-queue-meta">
            <span class="task-queue-badge">${escapeHtml(item.countsLabel)}</span>
            <span class="task-queue-badge">${escapeHtml(item.headlineTaskTitle)}</span>
            ${
              item.headlineTaskStatus
                ? `<span class="task-queue-badge">${escapeHtml(item.headlineTaskStatus)}</span>`
                : ""
            }
            ${
              item.needsAttention
                ? '<span class="task-queue-badge attention">需复核</span>'
                : ""
            }
          </div>
          <p class="project-queue-reason">${escapeHtml(item.reasonSummary)}</p>
        </button>
      `
    )
    .join("");
}

function renderTaskQueue(board) {
  if (!elements.taskQueueList) {
    return;
  }

  const view = deriveDesktopTaskList(board, restoredBoardFocus);
  elements.taskQueueScope.textContent = view.scopeLabel;
  elements.taskQueueSummary.textContent = view.summary;

  if (!view.items.length) {
    elements.taskQueueList.innerHTML =
      '<div class="task-queue-empty">当前还没有可展示的任务。服务端一旦返回队列，这里会直接展示最需要推进的项。</div>';
    return;
  }

  elements.taskQueueList.innerHTML = view.items
    .map(
      item => `
        <button
          type="button"
          class="task-queue-item task-tone-${item.tone} ${item.active ? "active" : ""}"
          data-task-id="${escapeHtml(item.id)}"
          data-project-id="${escapeHtml(item.projectId)}"
        >
          <strong class="task-queue-title">${escapeHtml(item.title)}</strong>
          <div class="task-queue-meta">
            <span class="task-queue-badge strong">${escapeHtml(item.statusLabel)}</span>
            <span class="task-queue-badge">${escapeHtml(item.priorityLabel)}</span>
            <span class="task-queue-badge">${escapeHtml(item.projectName)}</span>
            <span class="task-queue-badge">${escapeHtml(item.signalLabel)}</span>
            ${
              item.needsAttention
                ? '<span class="task-queue-badge attention">需复核</span>'
                : ""
            }
          </div>
          <p class="task-queue-reason">${escapeHtml(item.reasonSummary)}</p>
          <div class="task-queue-evidence">
            <span class="task-queue-badge">${escapeHtml(item.evidenceSummary)}</span>
          </div>
        </button>
      `
    )
    .join("");
}

function selectProjectFromSidebar(projectId) {
  const boardProjects = Array.isArray(latestBoardSnapshot?.projects)
    ? latestBoardSnapshot.projects
    : [];
  const project = boardProjects.find(item => item.id === projectId);

  if (!project) {
    return;
  }

  persistBoardFocus({
    projectId: project.id,
    taskId: null,
    sessionId: null,
    projectName: project.name || null,
    taskTitle: null,
    sessionTitle: null
  });
  rerenderBoardDerivedPanels();

  if (serverRunning) {
    refreshFrame();
  }
}

function selectTaskFromSidebar(taskId) {
  const boardTasks = Array.isArray(latestBoardSnapshot?.tasks) ? latestBoardSnapshot.tasks : [];
  const boardProjects = Array.isArray(latestBoardSnapshot?.projects)
    ? latestBoardSnapshot.projects
    : [];
  const task = boardTasks.find(item => item.id === taskId);

  if (!task) {
    return;
  }

  const project = boardProjects.find(item => item.id === task.project_id);
  persistBoardFocus({
    projectId: task.project_id,
    taskId: task.id,
    sessionId: null,
    projectName: project?.name || null,
    taskTitle: task.title || null,
    sessionTitle: null
  });
  rerenderBoardDerivedPanels();

  if (serverRunning) {
    refreshFrame();
  }
}

function applyGovernanceView(view) {
  if (!elements.governanceCard) {
    return;
  }

  elements.governanceCard.className = `status-card status-${view.tone}`;
  elements.governanceTitle.textContent = view.title;
  elements.governanceMessage.textContent = view.message;
  elements.governanceProjectValue.textContent = view.projectName;
  elements.governanceTaskValue.textContent = view.taskTitle;
  elements.governanceTaskStatusValue.textContent = view.taskStatusLabel;
  elements.governanceFreshnessValue.textContent = view.freshnessLabel;
  elements.governanceReasonValue.textContent = view.stateReason;
  elements.governanceEvidenceValue.textContent = view.evidenceSummary;
  elements.governanceRecoveryValue.textContent = view.recoveryLabel;
  elements.governanceAutomationValue.textContent = view.automationLabel;
  elements.governanceCountsValue.textContent = view.countsLabel;
  elements.governanceOutputValue.textContent = view.primaryOutput;

  if (!view.alert) {
    elements.governanceAlert.hidden = true;
    elements.governanceAlert.textContent = "";
    return;
  }

  elements.governanceAlert.hidden = false;
  elements.governanceAlert.textContent = view.alert;
}

function buildTaskActivityPlaceholder(kind, message) {
  return {
    tone: kind,
    mode: activityDetailMode,
    title: "焦点任务活动",
    taskTitle: formatFocusLabel(restoredBoardFocus.taskTitle, "任务", restoredBoardFocus.taskId),
    projectName: formatFocusLabel(
      restoredBoardFocus.projectName,
      "项目",
      restoredBoardFocus.projectId
    ),
    statusLabel: kind === "error" ? "等待复核" : kind === "busy" ? "等待同步" : "未连接",
    freshnessLabel: "暂无最新信号",
    freshnessDetail: message,
    hint:
      activityDetailMode === "diagnostic"
        ? "诊断模式会显示更详细的活动和日志，并支持搜索。"
        : "默认只显示最近且最重要的输出；详细日志请切到诊断模式。",
    stateReason: "客户端会优先显示状态依据、关键输出和活动流，而不是只保留裸状态。",
    evidenceSummary: "等待服务端返回结构化证据。",
    primaryOutputLabel: "关键输出",
    primaryOutputMessage: message,
    primaryOutputAt: "暂无时间戳",
    summaryLabel: message,
    items: [],
    emptyMessage: message,
    alert: kind === "error" ? message : "",
    searchEnabled: activityDetailMode === "diagnostic",
    searchQuery: activitySearchQuery
  };
}

function renderTaskActivityPanel(view) {
  if (!elements.activityDetailCard) {
    return;
  }

  if (elements.activitySimpleButton) {
    elements.activitySimpleButton.disabled = view.mode === "simple";
  }
  if (elements.activityDiagnosticButton) {
    elements.activityDiagnosticButton.disabled = view.mode === "diagnostic";
  }
  if (elements.activitySearchRow) {
    elements.activitySearchRow.hidden = !view.searchEnabled;
  }
  if (elements.activitySearchInput) {
    elements.activitySearchInput.disabled = !view.searchEnabled;
    elements.activitySearchInput.value = view.searchEnabled ? view.searchQuery || "" : "";
  }

  const itemsMarkup = view.items.length
    ? view.items
        .map(
          item => `
            <article class="activity-flow-item tone-${item.tone}">
              <div class="activity-flow-head">
                <strong>${escapeHtml(item.label)}</strong>
                <span class="task-queue-badge">${escapeHtml(item.ageLabel)}</span>
              </div>
              <p>${escapeHtml(item.message)}</p>
            </article>
          `
        )
        .join("")
    : `<div class="task-queue-empty">${escapeHtml(view.emptyMessage)}</div>`;

  elements.activityDetailCard.className = `activity-detail-card tone-${view.tone}`;
  elements.activityDetailCard.innerHTML = `
    <div class="activity-detail-meta">
      <div class="governance-metric">
        <span>焦点任务</span>
        <strong>${escapeHtml(view.taskTitle)}</strong>
      </div>
      <div class="governance-metric">
        <span>所属项目</span>
        <strong>${escapeHtml(view.projectName)}</strong>
      </div>
      <div class="governance-metric">
        <span>任务状态</span>
        <strong>${escapeHtml(view.statusLabel)}</strong>
      </div>
      <div class="governance-metric">
        <span>最新信号</span>
        <strong>${escapeHtml(view.freshnessLabel)}</strong>
      </div>
    </div>
    <p class="activity-detail-hint">${escapeHtml(view.hint)}</p>
    <div class="activity-output-card">
      <div class="activity-flow-head">
        <strong>${escapeHtml(view.primaryOutputLabel)}</strong>
        <span class="task-queue-badge strong">${escapeHtml(view.primaryOutputAt)}</span>
      </div>
      <p>${escapeHtml(view.primaryOutputMessage)}</p>
      <small>${escapeHtml(view.freshnessDetail)}</small>
    </div>
    <div class="governance-stack">
      <div class="governance-block">
        <span>状态依据</span>
        <p>${escapeHtml(view.stateReason)}</p>
      </div>
      <div class="governance-block">
        <span>状态证据</span>
        <p>${escapeHtml(view.evidenceSummary)}</p>
      </div>
    </div>
    ${
      view.alert
        ? `<div class="activity-detail-alert">${escapeHtml(view.alert)}</div>`
        : ""
    }
    <div class="activity-flow-summary">${escapeHtml(view.summaryLabel)}</div>
    <div class="activity-flow-list">${itemsMarkup}</div>
  `;
}

function refreshFrame() {
  if (!serverRunning) {
    showPlaceholder(
      "服务端未启动",
      "客户端界面已正常加载。请先启动 `spotlight-server`，再刷新内嵌看板。"
    );
    return;
  }

  elements.boardFrame.src = buildBoardUrl(currentBackendUrl, restoredBoardFocus);
  hidePlaceholder();
}

async function refreshGovernanceOverview(status = null) {
  const requestToken = ++governanceRequestToken;

  if (!serverRunning) {
    latestBoardSnapshot = null;
    const kind =
      status?.backend_state === "starting" || status?.backend_state === "partial" ? "busy" : "idle";
    const title = kind === "busy" ? "正在同步治理快照" : "等待治理快照";
    applyProjectQueuePlaceholder(
      kind === "busy"
        ? "本地服务正在恢复，项目入口会在服务可用后自动刷新。"
        : "服务可用后，这里会优先展示最需要推进或复核的项目。",
      status?.message || "当前仍以治理概览和恢复状态作为最小可用入口。"
    );
    applyTaskQueuePlaceholder(
      normalizedText(restoredBoardFocus.projectName) || "等待同步",
      kind === "busy" ? "本地服务正在恢复，任务列表会在服务可用后自动刷新。" : "服务可用后会返回任务队列。",
      status?.message || "当前仍以治理概览和恢复状态作为最小可用入口。"
    );
    applyGovernanceView(
      buildGovernancePlaceholder(
        kind,
        title,
        status?.message || "服务可用后，这里会优先显示状态依据、恢复信号和关键输出。"
      )
    );
    renderTaskActivityPanel(
      buildTaskActivityPlaceholder(
        kind,
        status?.message || "服务可用后，这里会优先显示关键输出和活动流。"
      )
    );
    return;
  }

  try {
    const board = await invokeWithTimeout("board_snapshot", {}, 2500);
    if (requestToken !== governanceRequestToken) {
      return;
    }
    latestBoardSnapshot = board;
    renderProjectQueue(board);
    renderTaskQueue(board);
    applyGovernanceView(deriveGovernanceOverview(board, restoredBoardFocus));
    renderTaskActivityPanel(
      deriveTaskActivityView(board, restoredBoardFocus, {
        mode: activityDetailMode,
        searchQuery: activitySearchQuery
      })
    );
  } catch (error) {
    if (requestToken !== governanceRequestToken) {
      return;
    }
    latestBoardSnapshot = null;
    applyProjectQueuePlaceholder(
      "项目入口暂时无法刷新，请先检查本地服务和治理快照。",
      String(error)
    );
    applyTaskQueuePlaceholder(
      normalizedText(restoredBoardFocus.projectName) || "同步失败",
      "任务列表暂时无法刷新，请先检查本地服务和治理快照。",
      String(error)
    );
    applyGovernanceView(buildGovernancePlaceholder("error", "治理快照读取失败", String(error)));
    renderTaskActivityPanel(buildTaskActivityPlaceholder("error", String(error)));
  }
}

async function refreshStatus() {
  try {
    const status = await invokeWithTimeout("app_status");
    elements.platformValue.textContent = status.platform;
    elements.urlValue.textContent = status.backend_url;
    currentBackendUrl = status.backend_url || BACKEND_URL;
    serverRunning = status.server_running;

    applyDesktopHealth(deriveDesktopHealth(status));
    const view = deriveWorkspaceState(status);
    applyWorkspaceState(view);
    markReady();
    refreshGovernanceOverview(status);

    if (view.shouldLoadFrame) {
      refreshFrame();
    }
  } catch (error) {
    serverRunning = false;
    latestBoardSnapshot = null;
    applyDesktopHealth(deriveDesktopHealthError(error));
    applyWorkspaceState(deriveWorkspaceErrorState(error));
    applyProjectQueuePlaceholder(
      "无法读取桌面项目入口，请先恢复本地服务连接。",
      String(error)
    );
    applyTaskQueuePlaceholder(
      normalizedText(restoredBoardFocus.projectName) || "连接失败",
      "无法读取桌面任务列表，请先恢复本地服务连接。",
      String(error)
    );
    applyGovernanceView(buildGovernancePlaceholder("error", "治理快照读取失败", String(error)));
    renderTaskActivityPanel(buildTaskActivityPlaceholder("error", String(error)));
    requestFallback(error);
  }
}

async function probeBackend() {
  setProbeStatus("busy", "正在探测", "客户端正在从原生侧探测本地 Spotlight 服务。");

  try {
    const probe = await invokeWithTimeout("probe_backend");
    elements.urlValue.textContent = probe.backend_url;
    currentBackendUrl = probe.backend_url || currentBackendUrl;

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
    await invokeWithTimeout("open_backend_in_browser", {
      url: buildBoardUrl(currentBackendUrl, restoredBoardFocus)
    });
  } catch (error) {
    applyWorkspaceState(deriveWorkspaceErrorState(error));
    requestFallback(error);
  }
}

async function rebuildDesktop() {
  const button = elements.rebuildDesktopButton;
  if (button) {
    button.disabled = true;
    button.textContent = "正在准备重启";
  }

  setProbeStatus(
    "busy",
    "准备重启客户端",
    "客户端正在把重编译和重启交给外部 helper 处理，当前窗口会自动退出。"
  );

  try {
    const plan = await invokeWithTimeout("rebuild_and_restart_desktop", {}, 5000);
    setProbeStatus(
      "busy",
      "客户端即将退出",
      `${plan.message}。日志：${plan.log_path}`
    );
  } catch (error) {
    if (button) {
      button.disabled = false;
      button.textContent = "重建并重启客户端";
    }
    setProbeStatus("error", "重启准备失败", String(error));
  }
}

elements.checkButton?.addEventListener("click", refreshStatus);
elements.probeButton?.addEventListener("click", probeBackend);
elements.reloadButton?.addEventListener("click", refreshFrame);
elements.browserButton?.addEventListener("click", openInBrowser);
elements.copyUrlButton?.addEventListener("click", copyBackendUrl);
elements.clearFocusButton?.addEventListener("click", clearRestoredFocus);
elements.projectQueueList?.addEventListener("click", event => {
  if (!(event.target instanceof Element)) {
    return;
  }
  const target = event.target.closest("[data-project-id]");
  if (!target) {
    return;
  }
  selectProjectFromSidebar(target.dataset.projectId);
});
elements.taskQueueList?.addEventListener("click", event => {
  if (!(event.target instanceof Element)) {
    return;
  }
  const target = event.target.closest("[data-task-id]");
  if (!target) {
    return;
  }
  selectTaskFromSidebar(target.dataset.taskId);
});
elements.activitySimpleButton?.addEventListener("click", () => {
  writeStoredActivityMode("simple");
  rerenderBoardDerivedPanels();
});
elements.activityDiagnosticButton?.addEventListener("click", () => {
  writeStoredActivityMode("diagnostic");
  rerenderBoardDerivedPanels();
});
elements.activitySearchInput?.addEventListener("input", event => {
  if (!(event.target instanceof HTMLInputElement)) {
    return;
  }
  activitySearchQuery = event.target.value;
  rerenderBoardDerivedPanels();
});
window.addEventListener("message", handleBoardMessage);

setupDesktopMaintenanceActions();
renderRestoredFocus();
applyGovernanceView(
  buildGovernancePlaceholder(
    "busy",
    "等待治理快照",
    "客户端启动后会优先显示状态依据、恢复信号和关键输出。"
  )
);
applyProjectQueuePlaceholder(
  "服务可用后，这里会优先展示最需要推进或复核的项目。",
  "当前仍以治理概览和恢复状态作为最小可用入口。"
);
applyTaskQueuePlaceholder(
  "等待同步",
  "服务可用后，这里会优先展示当前项目里最需要推进的任务。",
  "当前仍以治理概览和恢复状态作为最小可用入口。"
);
renderTaskActivityPanel(
  buildTaskActivityPlaceholder("busy", "服务可用后，这里会优先显示关键输出和活动流。")
);
refreshStatus();
setInterval(refreshStatus, 5000);

function normalizedText(value) {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function escapeHtml(value) {
  return String(value || "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
