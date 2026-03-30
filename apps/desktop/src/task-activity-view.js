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

const HOT_SIGNAL_MS = 60 * 1000;
const STALE_SIGNAL_MS = 5 * 60 * 1000;
const SIMPLE_ACTIVITY_LIMIT = 4;
const DIAGNOSTIC_ACTIVITY_LIMIT = 16;
const SIMPLE_HIDDEN_ACTIVITY_KINDS = new Set(["task.created", "task.auto_claim_reason"]);
const PRIMARY_RUNTIME_KINDS = ["error", "stderr", "assistant", "command", "plan"];

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

function previewText(value, maxLength = 132) {
  const text = normalizedText(value);
  if (!text) {
    return "等待新的活动或输出。";
  }

  if (text.length <= maxLength) {
    return text;
  }

  return `${text.slice(0, Math.max(0, maxLength - 1)).trimEnd()}...`;
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

function latestTaskTimestamp(board, task) {
  const runtimeEntries = taskRuntimeEntries(board, task);
  const latestRuntimeAt = parseTimestamp(lastItem(runtimeEntries)?.at) || 0;
  const latestActivityAt = parseTimestamp(lastInterestingActivity(task)?.at) || 0;
  return Math.max(latestRuntimeAt, latestActivityAt);
}

function pickTask(board, focus = {}) {
  const tasks = Array.isArray(board?.tasks) ? board.tasks : [];
  if (!tasks.length) {
    return null;
  }

  const focusedTaskId = normalizedText(focus?.taskId);
  if (focusedTaskId) {
    const focusedTask = tasks.find(task => task.id === focusedTaskId);
    if (focusedTask) {
      return focusedTask;
    }
  }

  const focusedProjectId = normalizedText(focus?.projectId);
  const scopedTasks = focusedProjectId
    ? tasks.filter(task => task.project_id === focusedProjectId)
    : tasks;
  const candidates = scopedTasks.length ? scopedTasks : tasks;

  return [...candidates].sort((left, right) => {
    const priorityDelta = taskPriority(left) - taskPriority(right);
    if (priorityDelta !== 0) {
      return priorityDelta;
    }
    return latestTaskTimestamp(board, right) - latestTaskTimestamp(board, left);
  })[0];
}

function taskRuntimeEntries(board, task) {
  if (Array.isArray(task?.runtime?.log) && task.runtime.log.length) {
    return task.runtime.log;
  }

  const runHistory = board?.task_run_history?.[task?.id];
  const latestRun = lastItem(runHistory);
  return Array.isArray(latestRun?.log) ? latestRun.log : [];
}

function lastInterestingActivity(task) {
  const activities = Array.isArray(task?.activities) ? task.activities : [];
  const filtered = activities.filter(item => !SIMPLE_HIDDEN_ACTIVITY_KINDS.has(item?.kind));
  return lastItem(filtered) || lastItem(activities);
}

function runtimeEntryLabel(kind) {
  return {
    assistant: "Agent 输出",
    user: "用户提示",
    command: "命令输出",
    plan: "计划输出",
    stderr: "标准错误",
    error: "运行错误",
    watchdog: "系统回收",
    system: "系统记录"
  }[kind] || kind || "未知输出";
}

function runtimeEntryTone(kind) {
  if (["error", "stderr"].includes(kind)) {
    return "error";
  }
  if (["watchdog", "system"].includes(kind)) {
    return "warn";
  }
  if (["command", "plan"].includes(kind)) {
    return "busy";
  }
  return "ready";
}

function runtimeFragmentIsTiny(message) {
  const text = String(message || "").trim();
  return text && text.length <= 24 && !text.includes("\n");
}

function joinRuntimeFragments(fragments = []) {
  return fragments.reduce((combined, fragment) => {
    const next = String(fragment || "");
    if (!next) {
      return combined;
    }
    if (!combined) {
      return next;
    }
    if (combined.endsWith("\n") || next.startsWith("\n")) {
      return `${combined}${next}`;
    }
    if (runtimeFragmentIsTiny(fragment)) {
      return `${combined}${next}`;
    }
    return `${combined}\n${next}`;
  }, "");
}

function groupedRuntimeEntryLabel(kinds = []) {
  const labels = [...new Set(kinds.filter(Boolean))].map(runtimeEntryLabel);
  if (!labels.length) {
    return "未知输出";
  }
  if (labels.length === 1) {
    return labels[0];
  }
  return `连续输出（${labels.join(" / ")}）`;
}

function groupedRuntimeEntryTone(kinds = []) {
  const uniqueKinds = [...new Set(kinds.filter(Boolean))];
  if (uniqueKinds.some(kind => ["error", "stderr"].includes(kind))) {
    return "error";
  }
  if (uniqueKinds.some(kind => ["watchdog", "system"].includes(kind))) {
    return "warn";
  }
  if (uniqueKinds.some(kind => ["command", "plan"].includes(kind))) {
    return "busy";
  }
  return "ready";
}

function groupRuntimeEntries(entries = [], options = {}) {
  const burstWindowMs = Number(options.burstWindowMs || 12_000);
  const maxCharsPerGroup = Number(options.maxCharsPerGroup || 1_200);
  const groups = [];

  entries
    .filter(entry => normalizedText(entry?.message))
    .forEach(entry => {
      const kind = entry?.kind || "system";
      const message = String(entry?.message || "");
      const atMs = parseTimestamp(entry?.at) || 0;
      const lastGroup = lastItem(groups);
      const shouldStartNewGroup = !lastGroup
        || (atMs && lastGroup.lastAtMs && (atMs - lastGroup.lastAtMs) > burstWindowMs)
        || lastGroup.totalChars >= maxCharsPerGroup;

      if (shouldStartNewGroup) {
        groups.push({
          at: entry?.at || null,
          atMs,
          lastAt: entry?.at || null,
          lastAtMs: atMs,
          totalChars: 0,
          segments: []
        });
      }

      const currentGroup = lastItem(groups);
      currentGroup.lastAt = entry?.at || currentGroup.lastAt;
      currentGroup.lastAtMs = atMs || currentGroup.lastAtMs;
      currentGroup.totalChars += message.length;

      const lastSegment = lastItem(currentGroup.segments);
      if (lastSegment && lastSegment.kind === kind) {
        lastSegment.fragments.push(message);
        return;
      }

      currentGroup.segments.push({
        kind,
        fragments: [message]
      });
    });

  return groups.map(group => {
    const kinds = group.segments.map(segment => segment.kind);
    const singleKind = [...new Set(kinds)].length === 1;
    const message = group.segments
      .map(segment => {
        const text = joinRuntimeFragments(segment.fragments);
        return singleKind ? text : `[${runtimeEntryLabel(segment.kind)}]\n${text}`;
      })
      .join("\n\n");

    return {
      kind: singleKind ? kinds[0] : "mixed",
      label: groupedRuntimeEntryLabel(kinds),
      tone: groupedRuntimeEntryTone(kinds),
      message,
      rawMessage: message,
      at: group.lastAt,
      atMs: group.lastAtMs
    };
  });
}

function activityEntryLabel(kind) {
  return {
    "task.seeded": "任务已播种",
    "task.created": "任务已创建",
    "task.auto_claimed": "系统自动认领",
    "task.auto_started": "系统自动启动",
    "task.auto_resumed": "系统自动恢复",
    "task.auto_retry_queued": "系统已排入自动重试",
    "task.watchdog_recovered": "Watchdog 已回收任务",
    "task.pause_requested": "已请求暂停",
    "task.paused": "任务已暂停",
    "task.resume_requested": "已请求恢复",
    "task.resumed": "任务已恢复",
    "task.runtime_session_lost": "运行会话已丢失",
    "task.done": "任务已完成",
    "task.canceled": "任务已撤销",
    "task.reassessed_reopened": "任务已重评估重开",
    "task.question_answered": "问题已记录回答",
    "agent.invoked": "已调起 Agent",
    "runtime.thread_started": "已建立长会话",
    "runtime.turn_completed": "当前轮次已完成",
    "runtime.error": "运行时错误",
    "runtime.exited": "运行进程已退出",
    "git.pre_run_snapshot": "已创建预执行快照"
  }[kind] || kind || "未命名活动";
}

function activityTone(kind) {
  if (!kind) {
    return "idle";
  }
  if (kind.includes("error") || kind.includes("failed")) {
    return "error";
  }
  if (kind.includes("done") || kind.includes("completed")) {
    return "ready";
  }
  if (
    kind.includes("watchdog")
    || kind.includes("pause")
    || kind.includes("canceled")
    || kind.includes("lost")
  ) {
    return "warn";
  }
  if (kind.includes("auto") || kind.includes("retry") || kind.includes("resume")) {
    return "busy";
  }
  return "idle";
}

function itemRank(entry) {
  if (entry.kind === "error" || entry.kind === "stderr" || entry.tone === "error") {
    return 0;
  }
  if (PRIMARY_RUNTIME_KINDS.includes(entry.kind) || entry.kind === "mixed") {
    return 1;
  }
  if (entry.tone === "warn") {
    return 2;
  }
  return 3;
}

function taskPrimaryOutput(board, task) {
  const runtimeEntries = [...groupRuntimeEntries(taskRuntimeEntries(board, task))].reverse();
  const preferredRuntime = runtimeEntries.find(entry => {
    return (
      (PRIMARY_RUNTIME_KINDS.includes(entry?.kind) || entry?.kind === "mixed")
      && normalizedText(entry?.message)
    );
  });

  if (preferredRuntime) {
    return {
      label: runtimeEntryLabel(preferredRuntime.kind),
      tone: runtimeEntryTone(preferredRuntime.kind),
      message: preferredRuntime.message,
      at: preferredRuntime.at
    };
  }

  const activity = lastInterestingActivity(task);
  if (activity) {
    return {
      label: activityEntryLabel(activity.kind),
      tone: activityTone(activity.kind),
      message: activity.message,
      at: activity.at
    };
  }

  return null;
}

function deriveFreshness(board, task, now) {
  const latestRuntimeAt = parseTimestamp(lastItem(taskRuntimeEntries(board, task))?.at) || 0;
  const latestActivityAt = parseTimestamp(lastInterestingActivity(task)?.at) || 0;
  const latestAt = Math.max(latestRuntimeAt, latestActivityAt);

  if (!latestAt) {
    return {
      tone: "idle",
      label: "暂无最新信号",
      detail: "还没有活动或输出。"
    };
  }

  const ageMs = Math.max(0, now - latestAt);
  if (ageMs <= HOT_SIGNAL_MS) {
    return {
      tone: "ready",
      label: "最新输出活跃",
      detail: `${relativeTimeFrom(ageMs)} 仍有新信号`
    };
  }

  if (ageMs <= STALE_SIGNAL_MS) {
    return {
      tone: "busy",
      label: "最近刚更新",
      detail: `${relativeTimeFrom(ageMs)} 有新信号`
    };
  }

  return {
    tone: "error",
    label: "链路可能停住",
    detail: `已超过 ${Math.max(1, Math.round(ageMs / 60_000))} 分钟没有新的输出或活动。`
  };
}

function buildEvidenceSummary(task) {
  const evidence = Array.isArray(task?.state_snapshot?.evidence)
    ? task.state_snapshot.evidence.map(item => normalizedText(item)).filter(Boolean)
    : [];

  if (!evidence.length) {
    return "等待服务端补充结构化证据。";
  }

  return previewText(evidence.slice(0, 3).join(" / "), 112);
}

function taskStateReason(task) {
  return (
    normalizedText(task?.state_snapshot?.reason)
    || normalizedText(task?.runtime?.last_error)
    || normalizedText(lastInterestingActivity(task)?.message)
    || "等待服务端生成状态依据。"
  );
}

function buildFlowEntries(board, task, now) {
  const runtimeEntries = groupRuntimeEntries(taskRuntimeEntries(board, task)).map(entry => {
    return {
      id: `runtime:${entry?.at || "none"}:${entry?.kind || "unknown"}:${entry?.message || ""}`,
      tone: entry?.tone || runtimeEntryTone(entry?.kind),
      kind: entry?.kind || "runtime",
      label: entry?.label || runtimeEntryLabel(entry?.kind),
      message: previewText(entry?.message, 220),
      rawMessage: normalizedText(entry?.rawMessage || entry?.message) || "",
      ageLabel: entry?.atMs ? relativeTimeFrom(Math.max(0, now - entry.atMs)) : "时间未知",
      atMs: entry?.atMs || 0
    };
  });

  const activityEntries = (Array.isArray(task?.activities) ? task.activities : [])
    .filter(entry => !SIMPLE_HIDDEN_ACTIVITY_KINDS.has(entry?.kind))
    .map(entry => {
      const atMs = parseTimestamp(entry?.at) || 0;
      return {
        id: `activity:${entry?.at || "none"}:${entry?.kind || "unknown"}:${entry?.message || ""}`,
        tone: activityTone(entry?.kind),
        kind: entry?.kind || "activity",
        label: activityEntryLabel(entry?.kind),
        message: previewText(entry?.message),
        rawMessage: normalizedText(entry?.message) || "",
        ageLabel: atMs ? relativeTimeFrom(Math.max(0, now - atMs)) : "时间未知",
        atMs
      };
    });

  return [...runtimeEntries, ...activityEntries].sort((left, right) => {
    const timeDelta = right.atMs - left.atMs;
    if (timeDelta !== 0) {
      return timeDelta;
    }
    return itemRank(left) - itemRank(right);
  });
}

function filterFlowEntries(entries, query) {
  const pattern = normalizedText(query)?.toLowerCase();
  if (!pattern) {
    return entries;
  }

  return entries.filter(entry => {
    return (
      String(entry?.label || "").toLowerCase().includes(pattern)
      || String(entry?.kind || "").toLowerCase().includes(pattern)
      || String(entry?.rawMessage || "").toLowerCase().includes(pattern)
    );
  });
}

function detailTone(task, freshness) {
  if (
    task?.state_snapshot?.needs_attention
    || task?.status === "FAILED"
    || task?.status === "MANUAL_REVIEW"
    || normalizedText(task?.runtime?.last_error)
    || freshness.tone === "error"
  ) {
    return "error";
  }

  if (
    task?.status === "RUNNING"
    || task?.status === "DONE"
    || task?.status === "ACCEPTED"
    || freshness.tone === "ready"
  ) {
    return "ready";
  }

  if (task?.status === "PAUSED" || task?.status === "CLAIMED" || task?.status === "APPROVED") {
    return "busy";
  }

  return "idle";
}

export function deriveTaskActivityView(board, focus = {}, options = {}) {
  const now = Number.isFinite(options?.now) ? options.now : Date.now();
  const mode = options?.mode === "diagnostic" ? "diagnostic" : "simple";
  const searchQuery = normalizedText(options?.searchQuery) || "";
  const task = pickTask(board, focus);
  const projects = Array.isArray(board?.projects) ? board.projects : [];

  if (!task) {
    return {
      tone: "idle",
      mode,
      title: "等待焦点任务",
      taskTitle: normalizedText(focus?.taskTitle) || "未选择任务",
      projectName: normalizedText(focus?.projectName) || "未选择项目",
      statusLabel: "暂无任务",
      freshnessLabel: "暂无最新信号",
      freshnessDetail: "还没有活动或输出。",
      hint:
        mode === "diagnostic"
          ? "诊断模式会显示更详细的活动和日志，并支持搜索。"
          : "默认只显示最近且最重要的输出；详细日志请切到诊断模式。",
      stateReason: "服务端返回任务后，这里会优先显示状态依据、关键输出和活动流。",
      evidenceSummary: "等待服务端返回结构化证据。",
      primaryOutputLabel: "关键输出",
      primaryOutputMessage: "当前没有可展示的关键输出。",
      primaryOutputAt: "暂无时间戳",
      summaryLabel: "暂无活动",
      items: [],
      emptyMessage: "当前没有可展示的任务活动。",
      alert: "",
      searchEnabled: mode === "diagnostic",
      searchQuery
    };
  }

  const project =
    projects.find(projectItem => projectItem.id === task.project_id)
    || projects.find(projectItem => projectItem.id === focus?.projectId)
    || null;
  const freshness = deriveFreshness(board, task, now);
  const primaryOutput = taskPrimaryOutput(board, task);
  const allFlowEntries = buildFlowEntries(board, task, now);
  const filteredFlowEntries = filterFlowEntries(allFlowEntries, searchQuery);
  const visibleEntries = filteredFlowEntries.slice(
    0,
    mode === "diagnostic" ? DIAGNOSTIC_ACTIVITY_LIMIT : SIMPLE_ACTIVITY_LIMIT
  );
  const summaryLabel = searchQuery
    ? `搜索“${searchQuery}”后命中 ${filteredFlowEntries.length} 条，当前展示 ${visibleEntries.length} 条`
    : mode === "diagnostic"
      ? `详细模式展示最近 ${visibleEntries.length}/${allFlowEntries.length} 条活动与日志`
      : `默认仅展示最近且最重要的 ${visibleEntries.length}/${allFlowEntries.length} 条更新`;

  return {
    tone: detailTone(task, freshness),
    mode,
    title: "焦点任务活动",
    taskTitle: normalizedText(task?.title) || "未命名任务",
    projectName: project?.name || normalizedText(focus?.projectName) || "未命名项目",
    statusLabel: STATUS_LABELS[task?.status] || task?.status || "未知状态",
    freshnessLabel: freshness.label,
    freshnessDetail: freshness.detail,
    hint:
      mode === "diagnostic"
        ? "诊断模式显示更详细的活动和日志；可用搜索框筛选当前任务输出。"
        : "默认只看最近且最重要的输出；详细日志保留在诊断模式，并支持搜索。",
    stateReason: taskStateReason(task),
    evidenceSummary: buildEvidenceSummary(task),
    primaryOutputLabel: primaryOutput?.label || "关键输出",
    primaryOutputMessage: previewText(
      primaryOutput?.message || normalizedText(task?.runtime?.last_error) || taskStateReason(task),
      mode === "diagnostic" ? 220 : 132
    ),
    primaryOutputAt: primaryOutput?.at
      ? relativeTimeFrom(Math.max(0, now - (parseTimestamp(primaryOutput.at) || now)))
      : "暂无时间戳",
    summaryLabel,
    items: visibleEntries,
    emptyMessage: searchQuery ? "当前搜索条件下没有匹配活动。" : "当前任务还没有活动或输出。",
    alert:
      normalizedText(task?.runtime?.last_error)
      || (task?.state_snapshot?.needs_attention
        ? "当前状态已被标记为需复核，请先核对状态依据、恢复信号和最近输出。"
        : freshness.tone === "error"
          ? freshness.detail
          : ""),
    searchEnabled: mode === "diagnostic",
    searchQuery
  };
}
