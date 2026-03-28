import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const DEFAULT_STATE_PATH = path.resolve(process.cwd(), ".spotlight/server-state.json");
const PROGRESS_ACTIVITY_KINDS = new Set([
  "runtime.thread_started",
  "runtime.turn_started",
  "runtime.turn_completed",
  "runtime.error",
  "task.watchdog_recovered",
  "task.auto_retry_queued",
  "task.runtime_session_lost",
  "task.state_normalized"
]);
const RECOVERY_LOOP_ACTIVITY_KINDS = new Set([
  "task.watchdog_recovered",
  "task.auto_retry_queued",
  "task.runtime_session_lost"
]);
const TERMINAL_STATUSES = new Set(["DONE", "ACCEPTED", "CANCELED"]);

function hasFlag(argv, flag) {
  return argv.includes(flag);
}

function optionValue(argv, name, fallback) {
  const prefix = `${name}=`;
  const item = argv.find((entry) => entry.startsWith(prefix));
  return item ? item.slice(prefix.length) : fallback;
}

function lastActivity(task) {
  return Array.isArray(task?.activities) && task.activities.length
    ? task.activities[task.activities.length - 1]
    : null;
}

function taskVersion(title) {
  const match = String(title || "").match(/^\[(\d+(?:\.\d+){2,})\]/);
  return match ? match[1] : null;
}

function buildReopenReason(task) {
  if (!task?.runtime) return "missing_runtime";
  if (!task.runtime.thread_id) return "missing_thread_id";
  const lastError = String(task.runtime.last_error || "");
  const lastKind = String(lastActivity(task)?.kind || "");
  if (/thread not found/i.test(lastError)) return "thread_not_found";
  if (
    lastKind === "task.state_normalized"
    && /重连|会话不存在|会话已丢失|reconnecting|session/i.test(lastError)
  ) {
    return "lost_runtime_context";
  }
  const recoveryLoopCount = (Array.isArray(task.activities) ? task.activities : []).filter((activity) =>
    RECOVERY_LOOP_ACTIVITY_KINDS.has(String(activity?.kind || ""))
  ).length;
  if (recoveryLoopCount >= 3) return "recovery_loop_exhausted";
  return null;
}

export function analyzeTaskState(state) {
  const tasks = Array.isArray(state?.tasks) ? state.tasks : [];
  const reopenCandidates = [];
  const closeCandidates = [];

  for (const task of tasks) {
    if (task?.status === "PAUSED") {
      const reopenReason = buildReopenReason(task);
      if (reopenReason) {
        reopenCandidates.push({
          id: String(task.id || ""),
          title: String(task.title || ""),
          reason: reopenReason,
          last_activity_kind: lastActivity(task)?.kind || null
        });
      }
    }

    const version = taskVersion(task?.title);
    if (!version || version.split(".").length !== 3) continue;
    const childPrefix = `${version}.`;
    const childTasks = tasks.filter((candidate) => {
      const childVersion = taskVersion(candidate?.title);
      return childVersion?.startsWith(childPrefix);
    });
    if (!childTasks.length) continue;
    const allTerminal = childTasks.every((candidate) => TERMINAL_STATUSES.has(String(candidate?.status || "")));
    if (!allTerminal) continue;
    closeCandidates.push({
      id: String(task.id || ""),
      title: String(task.title || ""),
      child_count: childTasks.length,
      statuses: childTasks.map((candidate) => String(candidate?.status || "UNKNOWN"))
    });
  }

  return {
    reopenCandidates,
    closeCandidates
  };
}

export function applyReassessment(state, analyzed, now = `${Date.now()}000000`) {
  const reopenIds = new Map(analyzed.reopenCandidates.map((item) => [item.id, item]));
  let changed = 0;

  for (const task of state.tasks || []) {
    const candidate = reopenIds.get(String(task.id || ""));
    if (!candidate) continue;

    task.status = "OPEN";
    task.claimed_by = null;
    task.assignee_user_id = null;
    task.runtime = null;
    task.activities = (Array.isArray(task.activities) ? task.activities : []).filter(
      (activity) => !PROGRESS_ACTIVITY_KINDS.has(String(activity?.kind || ""))
    );
    task.activities.push({
      kind: "task.reassessed_reopened",
      message: "系统重新评估后判定该任务缺少可靠恢复上下文，已清理失效恢复痕迹并转回待处理队列。",
      at: now
    });
    task.state_snapshot = {
      reason: "任务曾有历史执行痕迹，但恢复上下文已失效，现已重新打开等待重新评估。",
      evidence: [
        `last_activity:task.reassessed_reopened@${now}`,
        `reopen.reason:${candidate.reason}`
      ],
      last_evaluated_at: now,
      last_evaluated_by: "task-state-reassess",
      needs_attention: true
    };
    changed += 1;
  }

  return changed;
}

function main(argv = process.argv.slice(2)) {
  const statePath = path.resolve(optionValue(argv, "--state", DEFAULT_STATE_PATH));
  const apply = hasFlag(argv, "--apply");
  const raw = fs.readFileSync(statePath, "utf8");
  const state = JSON.parse(raw);
  const analyzed = analyzeTaskState(state);

  let changed = 0;
  if (apply) {
    changed = applyReassessment(state, analyzed);
    fs.writeFileSync(statePath, JSON.stringify(state));
  }

  console.log(JSON.stringify({
    statePath,
    apply,
    changed,
    reopenCandidates: analyzed.reopenCandidates,
    closeCandidates: analyzed.closeCandidates
  }, null, 2));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
