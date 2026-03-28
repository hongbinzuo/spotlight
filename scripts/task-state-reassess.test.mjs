import test from "node:test";
import assert from "node:assert/strict";

import { analyzeTaskState, applyReassessment } from "./task-state-reassess.mjs";

function task(overrides = {}) {
  return {
    id: overrides.id || "task-id",
    title: overrides.title || "[0.1.0] 示例任务",
    status: overrides.status || "OPEN",
    claimed_by: null,
    assignee_user_id: null,
    activities: overrides.activities || [],
    runtime: overrides.runtime ?? null,
    state_snapshot: overrides.state_snapshot || {},
    ...overrides
  };
}

test("analyzeTaskState finds non-resumable paused tasks", () => {
  const state = {
    tasks: [
      task({
        id: "a",
        title: "[0.1.0.4] 内存版任务看板",
        status: "PAUSED",
        runtime: null,
        activities: [{ kind: "task.state_normalized", at: "1" }]
      }),
      task({
        id: "b",
        title: "[0.1.7] 规划 AI 洞察与管理能力",
        status: "PAUSED",
        runtime: {
          thread_id: "dead-thread",
          last_error: "thread not found: dead-thread"
        },
        activities: [{ kind: "task.state_normalized", at: "2" }]
      })
    ]
  };

  const analyzed = analyzeTaskState(state);
  assert.equal(analyzed.reopenCandidates.length, 2);
  assert.deepEqual(
    analyzed.reopenCandidates.map((item) => item.reason).sort(),
    ["missing_runtime", "thread_not_found"]
  );
});

test("analyzeTaskState flags paused tasks stuck in repeated recovery loops", () => {
  const state = {
    tasks: [
      task({
        id: "looped",
        title: "[0.1.0] 搭建服务端骨架",
        status: "PAUSED",
        runtime: {
          thread_id: "thread-1",
          last_error: "自动恢复超过 25 秒未完成，已回退到等待队列"
        },
        activities: [
          { kind: "task.watchdog_recovered", at: "1" },
          { kind: "task.auto_retry_queued", at: "2" },
          { kind: "task.watchdog_recovered", at: "3" }
        ]
      })
    ]
  };

  const analyzed = analyzeTaskState(state);
  assert.equal(analyzed.reopenCandidates.length, 1);
  assert.equal(analyzed.reopenCandidates[0].reason, "recovery_loop_exhausted");
});

test("analyzeTaskState reopens normalized paused tasks that lost runtime context", () => {
  const state = {
    tasks: [
      task({
        id: "lost-runtime",
        title: "[0.1.2.4] 真实 Codex 长会话接入",
        status: "PAUSED",
        runtime: {
          thread_id: "thread-2",
          last_error: "服务端启动时发现任务仍被标记为执行中，但本地运行会话不存在，已转为可恢复状态。"
        },
        activities: [{ kind: "task.state_normalized", at: "9" }]
      })
    ]
  };

  const analyzed = analyzeTaskState(state);
  assert.equal(analyzed.reopenCandidates.length, 1);
  assert.equal(analyzed.reopenCandidates[0].reason, "lost_runtime_context");
});

test("analyzeTaskState finds close candidates when all subtasks are terminal", () => {
  const state = {
    tasks: [
      task({ id: "parent", title: "[0.1.7] AI 总任务", status: "OPEN" }),
      task({ id: "c1", title: "[0.1.7.1] 子任务一", status: "DONE" }),
      task({ id: "c2", title: "[0.1.7.2] 子任务二", status: "ACCEPTED" })
    ]
  };

  const analyzed = analyzeTaskState(state);
  assert.equal(analyzed.closeCandidates.length, 1);
  assert.equal(analyzed.closeCandidates[0].id, "parent");
});

test("applyReassessment reopens paused tasks and strips stale progress evidence", () => {
  const state = {
    tasks: [
      task({
        id: "a",
        title: "[0.1.3.3] 工作流引擎第一版",
        status: "PAUSED",
        claimed_by: "agent-1",
        assignee_user_id: "user-1",
        runtime: {
          thread_id: "dead-thread",
          active_turn_id: "turn-1",
          last_error: "thread not found: dead-thread"
        },
        activities: [
          { kind: "task.auto_retry_queued", at: "1" },
          { kind: "task.state_normalized", at: "2" },
          { kind: "task.version_normalized", at: "3" }
        ]
      })
    ]
  };

  const analyzed = analyzeTaskState(state);
  const changed = applyReassessment(state, analyzed, "123000000");
  assert.equal(changed, 1);
  assert.equal(state.tasks[0].status, "OPEN");
  assert.equal(state.tasks[0].runtime, null);
  assert.equal(state.tasks[0].claimed_by, null);
  assert.equal(state.tasks[0].assignee_user_id, null);
  assert.deepEqual(
    state.tasks[0].activities.map((item) => item.kind),
    ["task.version_normalized", "task.reassessed_reopened"]
  );
  assert.equal(state.tasks[0].state_snapshot.last_evaluated_by, "task-state-reassess");
});
