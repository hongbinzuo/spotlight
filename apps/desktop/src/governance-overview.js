const STATUS_LABELS = {
  OPEN: "待处理",
  CLAIMED: "已认领",
  APPROVAL_REQUESTED: "待审批",
  APPROVED: "已批准",
  RUNNING: "运行中",
  PAUSED: "待恢复",
  PENDING_ACCEPTANCE: "待验收",
  ACCEPTED: "已验收",
  DONE: "已完成",
  FAILED: "执行失败",
  MANUAL_REVIEW: "人工复核",
  CANCELED: "已撤销"
};

const STALE_SIGNAL_MS = 5 * 60 * 1000;
const HOT_SIGNAL_MS = 60 * 1000;

function normalizedText(value) {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function parseTimestamp(value) {
  if (typeof value === "number" && Number.isFinite(value)) {
    return value < 1_000_000_000_000 ? value * 1000 : value;
  }

  const text = normalizedText(value);
  if (!text) {
    return null;
  }

  if (/^\d+$/.test(text)) {
    const numeric = Number(text);
    if (Number.isFinite(numeric)) {
      return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
    }
  }

  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? parsed : null;
}

function relativeTimeFrom(ageMs) {
  if (ageMs < 45_000) {
    return "刚刚";
  }
  if (ageMs < 60 * 60 * 1000) {
    return `${Math.max(1, Math.round(ageMs / 60_000))} 分钟前`;
  }
  if (ageMs < 24 * 60 * 60 * 1000) {
    return `${Math.max(1, Math.round(ageMs / (60 * 60 * 1000)))} 小时前`;
  }
  return `${Math.max(1, Math.round(ageMs / (24 * 60 * 60 * 1000)))} 天前`;
}

function lastItem(items) {
  return Array.isArray(items) && items.length ? items[items.length - 1] : null;
}

function taskRuntimeEntries(board, task) {
  const directEntry = lastItem(task?.runtime?.log);
  if (directEntry) {
    return directEntry;
  }

  const runHistory = board?.task_run_history?.[task?.id];
  const latestRun = lastItem(runHistory);
  return lastItem(latestRun?.log);
}

function latestTaskActivity(task) {
  return lastItem(task?.activities);
}

function latestTaskTimestamp(board, task) {
  return Math.max(
    parseTimestamp(taskRuntimeEntries(board, task)?.at) || 0,
    parseTimestamp(latestTaskActivity(task)?.at) || 0
  );
}

function taskPriority(task) {
  if (task?.state_snapshot?.needs_attention) {
    return 0;
  }

  switch (task?.status) {
    case "RUNNING":
      return 1;
    case "PAUSED":
      return 2;
    case "FAILED":
    case "MANUAL_REVIEW":
      return 3;
    case "CLAIMED":
    case "APPROVED":
      return 4;
    case "APPROVAL_REQUESTED":
    case "OPEN":
      return 5;
    default:
      return 6;
  }
}

function pickGovernanceTask(tasks, focus, board) {
  const focusTaskId = normalizedText(focus?.taskId);
  if (focusTaskId) {
    const focusedTask = tasks.find(task => task.id === focusTaskId);
    if (focusedTask) {
      return focusedTask;
    }
  }

  const focusProjectId = normalizedText(focus?.projectId);
  const candidates = focusProjectId
    ? tasks.filter(task => task.project_id === focusProjectId)
    : tasks;

  if (!candidates.length) {
    return null;
  }

  return [...candidates].sort((left, right) => {
    const priority = taskPriority(left) - taskPriority(right);
    if (priority !== 0) {
      return priority;
    }
    return latestTaskTimestamp(board, right) - latestTaskTimestamp(board, left);
  })[0];
}

function pickGovernanceProject(projects, focus, task) {
  const focusProjectId = normalizedText(focus?.projectId);
  if (focusProjectId) {
    return projects.find(project => project.id === focusProjectId) || null;
  }

  if (task) {
    return projects.find(project => project.id === task.project_id) || null;
  }

  return projects[0] || null;
}

function deriveFreshness(board, task, now) {
  const latestOutput = taskRuntimeEntries(board, task);
  const latestActivity = latestTaskActivity(task);
  const outputAt = parseTimestamp(latestOutput?.at);
  const activityAt = parseTimestamp(latestActivity?.at);
  const latestAt = Math.max(outputAt || 0, activityAt || 0);

  if (!latestAt) {
    return {
      tone: "busy",
      label: "暂无最新信号",
      detail: "还没有输出或活动记录。",
      primaryMessage: normalizedText(latestOutput?.message) || normalizedText(latestActivity?.message)
    };
  }

  const ageMs = Math.max(0, now - latestAt);
  if (ageMs <= HOT_SIGNAL_MS) {
    return {
      tone: "ready",
      label: "最新输出活跃",
      detail: `${relativeTimeFrom(ageMs)} 仍有新信号`,
      primaryMessage:
        normalizedText(latestOutput?.message) || normalizedText(latestActivity?.message)
    };
  }

  if (ageMs <= STALE_SIGNAL_MS) {
    return {
      tone: "ready",
      label: "最近刚更新",
      detail: `${relativeTimeFrom(ageMs)} 有新信号`,
      primaryMessage:
        normalizedText(latestOutput?.message) || normalizedText(latestActivity?.message)
    };
  }

  return {
    tone: "error",
    label: "链路可能停住",
    detail: `已超过 ${Math.max(1, Math.round(ageMs / 60_000))} 分钟没有新的输出或活动。`,
    primaryMessage: normalizedText(latestOutput?.message) || normalizedText(latestActivity?.message)
  };
}

function includesRecoverySignal(text) {
  const normalized = normalizedText(text);
  if (!normalized) {
    return false;
  }
  return /恢复|重试|thread|watchdog|中断/.test(normalized);
}

function deriveRecoveryLabel(task) {
  const stateReason = normalizedText(task?.state_snapshot?.reason);
  const lastError = normalizedText(task?.runtime?.last_error);
  if (task?.status === "PAUSED") {
    if (includesRecoverySignal(stateReason) || includesRecoverySignal(lastError)) {
      return "任务处于可恢复状态，优先等待自动恢复或人工继续。";
    }
    return "任务已暂停，建议先回顾最近活动和运行输出后再继续。";
  }

  if (task?.status === "RUNNING") {
    return "任务仍在推进，当前无需恢复。";
  }

  if (task?.status === "FAILED" || task?.status === "MANUAL_REVIEW") {
    return "任务已进入异常或复核状态，需要人工接管。";
  }

  if (includesRecoverySignal(stateReason) || includesRecoverySignal(lastError)) {
    return "当前已有恢复相关信号，继续操作前应先核对状态依据。";
  }

  return "当前没有挂起的恢复流程。";
}

function deriveAutomationLabel(autoModeCount, busyProjectAgents, task) {
  if (autoModeCount === 0) {
    return "当前没有自动运行 Agent 待命。";
  }

  if (task?.status === "PAUSED") {
    return `有 ${autoModeCount} 个自动运行 Agent 待命；恢复仍以服务端调度结果为准。`;
  }

  if (task?.status === "RUNNING") {
    return `有 ${autoModeCount} 个自动运行 Agent，其中 ${busyProjectAgents} 个正在当前项目推进。`;
  }

  return `自动运行已开启，当前共有 ${autoModeCount} 个 Agent 可参与后续推进。`;
}

function deriveTone(task, freshness) {
  if (!task) {
    return "idle";
  }

  if (
    task?.state_snapshot?.needs_attention ||
    task?.status === "FAILED" ||
    task?.status === "MANUAL_REVIEW" ||
    normalizedText(task?.runtime?.last_error) ||
    freshness.tone === "error"
  ) {
    return "error";
  }

  if (
    task?.status === "RUNNING" ||
    task?.status === "DONE" ||
    task?.status === "ACCEPTED" ||
    freshness.tone === "ready"
  ) {
    return "ready";
  }

  if (task?.status === "PAUSED" || task?.status === "CLAIMED" || task?.status === "APPROVED") {
    return "busy";
  }

  return "idle";
}

function buildEvidenceSummary(task, freshness) {
  const evidence = Array.isArray(task?.state_snapshot?.evidence)
    ? task.state_snapshot.evidence
        .map(item => normalizedText(item))
        .filter(Boolean)
        .slice(0, 3)
    : [];

  if (evidence.length) {
    return evidence.join("；");
  }

  return freshness.detail;
}

function fallbackStateReason(task) {
  return (
    normalizedText(task?.state_snapshot?.reason) ||
    normalizedText(task?.runtime?.last_error) ||
    normalizedText(latestTaskActivity(task)?.message) ||
    "服务端尚未生成结构化状态依据。"
  );
}

function deriveAlert(task, freshness) {
  const runtimeError = normalizedText(task?.runtime?.last_error);
  if (runtimeError) {
    return runtimeError;
  }

  if (task?.state_snapshot?.needs_attention) {
    return "状态已被标记为需复核，请优先检查状态依据、恢复信号和最近输出。";
  }

  if (freshness.tone === "error") {
    return freshness.detail;
  }

  return "";
}

export function deriveGovernanceOverview(board, focus = {}, now = Date.now()) {
  const tasks = Array.isArray(board?.tasks) ? board.tasks : [];
  const projects = Array.isArray(board?.projects) ? board.projects : [];
  const pendingQuestions = Array.isArray(board?.pending_questions) ? board.pending_questions : [];
  const agents = Array.isArray(board?.agents) ? board.agents : [];

  if (!tasks.length) {
    return {
      tone: "idle",
      title: "等待治理快照",
      message: "服务端返回的当前看板里还没有任务。",
      projectName: normalizedText(focus?.projectName) || "未选择项目",
      taskTitle: normalizedText(focus?.taskTitle) || "未选择任务",
      taskStatusLabel: "暂无任务",
      freshnessLabel: "暂无最新信号",
      freshnessDetail: "还没有输出或活动记录。",
      recoveryLabel: "当前没有挂起的恢复流程。",
      automationLabel: "当前还没有可用的自动运行信号。",
      countsLabel: "运行中 0 · 待恢复 0 · 自动运行 0 · 待答 0",
      stateReason: "客户端会优先显示状态依据、恢复信号和关键输出。",
      evidenceSummary: "等待服务端生成任务后再继续推进。",
      primaryOutput: "当前没有可展示的关键输出。",
      alert: ""
    };
  }

  const task = pickGovernanceTask(tasks, focus, board);
  const project = pickGovernanceProject(projects, focus, task);
  const projectId = project?.id || task?.project_id || normalizedText(focus?.projectId);
  const scopedTasks = projectId ? tasks.filter(item => item.project_id === projectId) : tasks;
  const runningCount = scopedTasks.filter(item => item.status === "RUNNING").length;
  const pausedCount = scopedTasks.filter(item => item.status === "PAUSED").length;
  const questionCount = pendingQuestions.filter(item => !projectId || item.project_id === projectId).length;
  const autoModeCount = agents.filter(agent => agent.auto_mode).length;
  const busyProjectAgents = agents.filter(agent =>
    agent.current_task_id && scopedTasks.some(item => item.id === agent.current_task_id)
  ).length;
  const freshness = deriveFreshness(board, task, now);
  const stateReason = fallbackStateReason(task);
  const tone = deriveTone(task, freshness);

  return {
    tone,
    title:
      tone === "error"
        ? "治理状态需要复核"
        : tone === "busy"
          ? "治理状态正在推进"
          : "治理状态已前置暴露",
    message: `${STATUS_LABELS[task?.status] || task?.status || "未知状态"}；${freshness.label}`,
    projectName: project?.name || normalizedText(focus?.projectName) || "未命名项目",
    taskTitle: normalizedText(task?.title) || normalizedText(focus?.taskTitle) || "未命名任务",
    taskStatusLabel: STATUS_LABELS[task?.status] || task?.status || "未知状态",
    freshnessLabel: freshness.label,
    freshnessDetail: freshness.detail,
    recoveryLabel: deriveRecoveryLabel(task),
    automationLabel: deriveAutomationLabel(autoModeCount, busyProjectAgents, task),
    countsLabel: `运行中 ${runningCount} · 待恢复 ${pausedCount} · 自动运行 ${autoModeCount} · 待答 ${questionCount}`,
    stateReason,
    evidenceSummary: buildEvidenceSummary(task, freshness),
    primaryOutput: freshness.primaryMessage || normalizedText(task?.runtime?.last_error) || stateReason,
    alert: deriveAlert(task, freshness)
  };
}
