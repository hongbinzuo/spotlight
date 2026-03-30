const STATUS_LABELS = {
  OPEN: "待处理",
  CLAIMED: "已认领",
  APPROVAL_REQUESTED: "待审批",
  APPROVED: "已批准",
  RUNNING: "处理中",
  PAUSED: "待恢复",
  PENDING_ACCEPTANCE: "待验收",
  ACCEPTED: "已验收",
  DONE: "已完成",
  FAILED: "执行失败",
  MANUAL_REVIEW: "人工复核",
  CANCELED: "已取消"
};

const HOT_SIGNAL_MS = 60 * 1000;
const STALE_SIGNAL_MS = 5 * 60 * 1000;
const MAX_VISIBLE_PROJECTS = 6;

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
    return "等待服务端生成项目状态依据。";
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

function pickHeadlineTask(board, tasks, focusedTaskId) {
  if (!tasks.length) {
    return null;
  }

  return tasks.slice().sort((left, right) => compareTasks(left, right, board, focusedTaskId))[0];
}

function projectTone(headlineTask, signal, counts) {
  if (counts.attentionCount > 0 || signal.tone === "error") {
    return "error";
  }

  if (
    counts.runningCount > 0 ||
    counts.pausedCount > 0 ||
    headlineTask?.status === "CLAIMED" ||
    headlineTask?.status === "APPROVED"
  ) {
    return "busy";
  }

  if (signal.tone === "ready" || headlineTask?.status === "DONE" || headlineTask?.status === "ACCEPTED") {
    return "ready";
  }

  return "idle";
}

function compareProjects(left, right, focusedProjectId) {
  if (left.id === focusedProjectId && right.id !== focusedProjectId) {
    return -1;
  }
  if (right.id === focusedProjectId && left.id !== focusedProjectId) {
    return 1;
  }

  if (left.attentionCount !== right.attentionCount) {
    return right.attentionCount - left.attentionCount;
  }

  const leftActiveCount = left.runningCount + left.pausedCount;
  const rightActiveCount = right.runningCount + right.pausedCount;
  if (leftActiveCount !== rightActiveCount) {
    return rightActiveCount - leftActiveCount;
  }

  if (left.latestTimestamp !== right.latestTimestamp) {
    return right.latestTimestamp - left.latestTimestamp;
  }

  if (left.taskCount !== right.taskCount) {
    return right.taskCount - left.taskCount;
  }

  return left.name.localeCompare(right.name, "zh-CN");
}

export function deriveDesktopProjectList(board, focus = {}, now = Date.now()) {
  const projects = Array.isArray(board?.projects) ? board.projects : [];
  const tasks = Array.isArray(board?.tasks) ? board.tasks : [];
  const agents = Array.isArray(board?.agents) ? board.agents : [];
  const pendingQuestions = Array.isArray(board?.pending_questions) ? board.pending_questions : [];
  const focusedProjectId = normalizedText(focus?.projectId);
  const focusedTaskId = normalizedText(focus?.taskId);

  if (!projects.length) {
    return {
      summary: "服务端返回的当前看板里还没有项目。",
      items: []
    };
  }

  const items = projects
    .map(project => {
      const projectTasks = tasks.filter(task => task.project_id === project.id);
      const headlineTask = pickHeadlineTask(board, projectTasks, focusedTaskId);
      const signal = headlineTask
        ? signalLabel(board, headlineTask, now)
        : {
            tone: "idle",
            label: "暂无最新信号"
          };
      const runningCount = projectTasks.filter(task => task.status === "RUNNING").length;
      const pausedCount = projectTasks.filter(task => task.status === "PAUSED").length;
      const attentionCount = projectTasks.filter(task => task.state_snapshot?.needs_attention).length;
      const pendingQuestionCount = pendingQuestions.filter(
        question => question.project_id === project.id && question.status !== "answered"
      ).length;
      const busyAgentCount = agents.filter(
        agent =>
          agent.current_task_id &&
          projectTasks.some(task => task.id === agent.current_task_id)
      ).length;
      const autoModeCount = agents.filter(agent => agent.auto_mode).length;
      const latestTimestamp = headlineTask ? latestTaskTimestamp(board, headlineTask) : 0;

      return {
        id: project.id,
        name: normalizedText(project?.name) || "未命名项目",
        active: project.id === focusedProjectId,
        tone: projectTone(
          headlineTask,
          signal,
          {
            runningCount,
            pausedCount,
            attentionCount
          }
        ),
        signalLabel: signal.label,
        taskCount: projectTasks.length,
        runningCount,
        pausedCount,
        attentionCount,
        pendingQuestionCount,
        busyAgentCount,
        autoModeCount,
        latestTimestamp,
        headlineTaskTitle: normalizedText(headlineTask?.title) || "当前项目还没有任务",
        headlineTaskStatus:
          STATUS_LABELS[headlineTask?.status] || normalizedText(headlineTask?.status) || null,
        countsLabel: `任务 ${projectTasks.length} · 处理中 ${runningCount} · 待恢复 ${pausedCount} · 待答 ${pendingQuestionCount}`,
        reasonSummary: headlineTask
          ? stateSummary(board, headlineTask)
          : "当前项目还没有任务，可先从文档、AGENTS.md 或手动创建需求开始。",
        needsAttention: attentionCount > 0
      };
    })
    .sort((left, right) => compareProjects(left, right, focusedProjectId))
    .slice(0, MAX_VISIBLE_PROJECTS);

  const attentionProjectCount = items.filter(item => item.attentionCount > 0).length;
  const activeProjectCount = items.filter(item => item.runningCount > 0 || item.pausedCount > 0).length;

  return {
    summary: `已关联 ${projects.length} 个项目 · 需复核 ${attentionProjectCount} · 推进中 ${activeProjectCount}`,
    items
  };
}
