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

const PRIORITY_LABELS = {
  HIGH: "高优",
  MEDIUM: "中优",
  LOW: "低优"
};

const HOT_SIGNAL_MS = 60 * 1000;
const STALE_SIGNAL_MS = 5 * 60 * 1000;
const MAX_VISIBLE_TASKS = 9;

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

function lastItem(items) {
  return Array.isArray(items) && items.length ? items[items.length - 1] : null;
}

function previewText(value, maxLength = 88) {
  const text = normalizedText(value);
  if (!text) {
    return "等待服务端生成状态依据。";
  }

  if (text.length <= maxLength) {
    return text;
  }

  return `${text.slice(0, Math.max(0, maxLength - 1)).trimEnd()}...`;
}

function latestRuntimeEntry(board, task) {
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
    parseTimestamp(latestRuntimeEntry(board, task)?.at) || 0,
    parseTimestamp(latestTaskActivity(task)?.at) || 0
  );
}

function signalLabel(board, task, now) {
  const latestOutput = latestRuntimeEntry(board, task);
  const latestActivity = latestTaskActivity(task);
  const latestAt = Math.max(
    parseTimestamp(latestOutput?.at) || 0,
    parseTimestamp(latestActivity?.at) || 0
  );

  if (!latestAt) {
    return {
      tone: "idle",
      label: "暂无最新信号"
    };
  }

  const ageMs = Math.max(0, now - latestAt);
  if (ageMs <= HOT_SIGNAL_MS) {
    return {
      tone: "ready",
      label: "刚刚更新"
    };
  }

  if (ageMs <= STALE_SIGNAL_MS) {
    return {
      tone: "busy",
      label: "最近有更新"
    };
  }

  return {
    tone: "error",
    label: "链路可能停住"
  };
}

function taskOrder(task) {
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
    case "PENDING_ACCEPTANCE":
      return 6;
    case "DONE":
    case "ACCEPTED":
      return 7;
    case "CANCELED":
      return 8;
    default:
      return 9;
  }
}

function taskTone(task, signal) {
  if (
    task?.state_snapshot?.needs_attention ||
    task?.status === "FAILED" ||
    task?.status === "MANUAL_REVIEW" ||
    normalizedText(task?.runtime?.last_error) ||
    signal.tone === "error"
  ) {
    return "error";
  }

  if (
    task?.status === "RUNNING" ||
    task?.status === "CLAIMED" ||
    task?.status === "APPROVED" ||
    task?.status === "PAUSED"
  ) {
    return "busy";
  }

  if (signal.tone === "ready" || task?.status === "DONE" || task?.status === "ACCEPTED") {
    return "ready";
  }

  return "idle";
}

function stateSummary(board, task) {
  const latestOutput = latestRuntimeEntry(board, task);
  const latestActivity = latestTaskActivity(task);
  const lastError = normalizedText(task?.runtime?.last_error);
  const stateReason = normalizedText(task?.state_snapshot?.reason);
  const latestMessage =
    normalizedText(latestOutput?.message) || normalizedText(latestActivity?.message);

  if (lastError) {
    return previewText(lastError);
  }

  if (task?.status === "PAUSED") {
    return previewText(stateReason || latestMessage || "任务已暂停，等待恢复。");
  }

  return previewText(stateReason || latestMessage);
}

function evidenceSummary(task) {
  const evidence = Array.isArray(task?.state_snapshot?.evidence)
    ? task.state_snapshot.evidence.map(item => normalizedText(item)).filter(Boolean)
    : [];

  if (!evidence.length) {
    return "等待更多结构化证据。";
  }

  return previewText(evidence.slice(0, 2).join(" / "), 60);
}

function compareTasks(left, right, board, focusedTaskId) {
  if (left.id === focusedTaskId && right.id !== focusedTaskId) {
    return -1;
  }
  if (right.id === focusedTaskId && left.id !== focusedTaskId) {
    return 1;
  }

  const orderDiff = taskOrder(left) - taskOrder(right);
  if (orderDiff !== 0) {
    return orderDiff;
  }

  const timeDiff = latestTaskTimestamp(board, right) - latestTaskTimestamp(board, left);
  if (timeDiff !== 0) {
    return timeDiff;
  }

  return String(left.title || "").localeCompare(String(right.title || ""), "zh-CN");
}

export function deriveDesktopTaskList(board, focus = {}, now = Date.now()) {
  const projects = Array.isArray(board?.projects) ? board.projects : [];
  const tasks = Array.isArray(board?.tasks) ? board.tasks : [];
  const focusedTaskId = normalizedText(focus?.taskId);
  const focusedProjectId = normalizedText(focus?.projectId);
  const focusedTask = focusedTaskId ? tasks.find(task => task.id === focusedTaskId) || null : null;
  const effectiveProjectId = focusedTask?.project_id || focusedProjectId;
  const scopedTasks = effectiveProjectId
    ? tasks.filter(task => task.project_id === effectiveProjectId)
    : tasks;
  const visibleTasks = (scopedTasks.length ? scopedTasks : tasks)
    .slice()
    .sort((left, right) => compareTasks(left, right, board, focusedTaskId))
    .slice(0, MAX_VISIBLE_TASKS);
  const project =
    projects.find(item => item.id === effectiveProjectId) ||
    projects.find(item => item.id === focusedProjectId) ||
    null;

  if (!tasks.length) {
    return {
      scopeLabel: normalizedText(focus?.projectName) || "暂无任务",
      summary: "服务端返回的当前看板里还没有任务。",
      items: []
    };
  }

  const effectiveTasks = scopedTasks.length ? scopedTasks : tasks;
  const runningCount = effectiveTasks.filter(task => task.status === "RUNNING").length;
  const pausedCount = effectiveTasks.filter(task => task.status === "PAUSED").length;
  const attentionCount = effectiveTasks.filter(task => task.state_snapshot?.needs_attention).length;

  return {
    scopeLabel:
      project?.name ||
      normalizedText(focus?.projectName) ||
      (tasks.length === scopedTasks.length ? "全部任务" : "任务列表"),
    summary: `处理中 ${runningCount} · 待恢复 ${pausedCount} · 需复核 ${attentionCount}`,
    items: visibleTasks.map(task => {
      const projectName =
        projects.find(projectItem => projectItem.id === task.project_id)?.name || "未命名项目";
      const signal = signalLabel(board, task, now);
      const priority = normalizedText(task?.priority);

      return {
        id: task.id,
        projectId: task.project_id,
        projectName,
        title: normalizedText(task?.title) || "未命名任务",
        active: task.id === focusedTaskId,
        tone: taskTone(task, signal),
        statusLabel: STATUS_LABELS[task?.status] || task?.status || "未知状态",
        priorityLabel: PRIORITY_LABELS[priority] || "未定优先级",
        signalLabel: signal.label,
        reasonSummary: stateSummary(board, task),
        evidenceSummary: evidenceSummary(task),
        needsAttention: Boolean(task?.state_snapshot?.needs_attention)
      };
    })
  };
}
