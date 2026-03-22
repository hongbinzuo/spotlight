import test from "node:test";
import assert from "node:assert/strict";

import {
  analyzeBoard,
  analyzeHtml,
  analyzeProjectSummaries,
  computeAutonomyMetrics,
  parseTaskVersion
} from "./client-doctor.mjs";

test("parseTaskVersion extracts version from task title", () => {
  assert.equal(parseTaskVersion({ title: "[0.1.2] 接通真实 Codex 长会话运行时" }), "0.1.2");
  assert.equal(parseTaskVersion({ title: "普通任务" }), null);
});

test("analyzeBoard flags tasks that reverted to OPEN after real progress", () => {
  const report = analyzeBoard({
    agents: [{ name: "本地 Codex Agent", auto_mode: true }],
    tasks: [
      {
        title: "[0.1.2] 接通真实 Codex 长会话运行时",
        status: "OPEN",
        claimed_by: null,
        priority: null,
        activities: [
          { kind: "runtime.thread_started" },
          { kind: "task.watchdog_recovered" }
        ],
        runtime: {
          log: [{ kind: "assistant", message: "已有运行输出" }]
        }
      }
    ]
  });

  assert.equal(report.failures.some((item) => item.code === "task_progress_reverted_to_open"), true);
  assert.equal(report.warnings.some((item) => item.code === "versioned_task_missing_priority"), true);
});

test("analyzeBoard ignores historical watchdog loops when task is safely paused", () => {
  const report = analyzeBoard({
    agents: [{ name: "本地 Codex Agent", auto_mode: false }],
    tasks: [
      {
        title: "[0.1.2] 接通真实 Codex 长会话运行时",
        status: "PAUSED",
        claimed_by: null,
        priority: "HIGH",
        activities: [
          { kind: "runtime.thread_started" },
          { kind: "task.watchdog_recovered" },
          { kind: "task.watchdog_recovered" },
          { kind: "task.state_normalized" }
        ],
        runtime: {
          log: [{ kind: "assistant", message: "已有运行输出" }]
        }
      }
    ]
  });

  assert.equal(report.warnings.some((item) => item.code === "task_repeated_watchdog_recovery"), false);
});

test("analyzeBoard flags missing task state snapshots and inconsistent active turns", () => {
  const report = analyzeBoard({
    agents: [{ name: "本地 Codex Agent", auto_mode: false }],
    tasks: [
      {
        title: "[0.1.2] 接通真实 Codex 长会话运行时",
        status: "FAILED",
        claimed_by: null,
        priority: "HIGH",
        activities: [{ kind: "runtime.error" }],
        runtime: {
          active_turn_id: "turn-1",
          log: [{ kind: "assistant", message: "已有输出" }]
        }
      }
    ]
  });

  assert.equal(report.failures.some((item) => item.code === "task_state_snapshot_missing"), true);
  assert.equal(report.warnings.some((item) => item.code === "task_non_running_with_active_turn"), true);
});

test("analyzeBoard respects state snapshots and surfaces needs_attention tasks", () => {
  const report = analyzeBoard({
    agents: [{ name: "本地 Codex Agent", auto_mode: false }],
    tasks: [
      {
        title: "[0.1.0] 最小服务端",
        status: "PAUSED",
        claimed_by: null,
        priority: "HIGH",
        activities: [{ kind: "task.state_normalized" }],
        runtime: { log: [] },
        state_snapshot: {
          reason: "服务端发现状态需要人工复核",
          evidence: ["last_activity:task.state_normalized@1"],
          needs_attention: true
        }
      }
    ]
  });

  assert.equal(report.failures.some((item) => item.code === "task_state_snapshot_missing"), false);
  assert.equal(report.warnings.some((item) => item.code === "task_state_needs_attention"), true);
});

test("analyzeBoard flags oversized payloads and idle auto agents", () => {
  const report = analyzeBoard(
    {
      agents: [{ name: "本地 Codex Agent", auto_mode: true }],
      tasks: [{ title: "[0.1.0] 内存版任务看板", status: "OPEN", activities: [], runtime: null }]
    },
    {
      boardBytes: 5_000_000,
      boardLatencyMs: 4_500
    }
  );

  assert.equal(report.failures.some((item) => item.code === "board_payload_too_large"), true);
  assert.equal(report.failures.some((item) => item.code === "board_latency_too_high"), true);
  assert.equal(report.failures.some((item) => item.code === "auto_agents_idle_with_pending_tasks"), true);
});

test("analyzeProjectSummaries flags misleading zero-agent summary", () => {
  const report = analyzeProjectSummaries(
    {
      agents: [{ id: "agent-1", auto_mode: true }],
      tasks: [{ id: "task-1", project_id: "project-1", status: "OPEN" }]
    },
    [
      {
        project_id: "project-1",
        project_name: "Spotlight 平台自身",
        task_counts: {
          open: 1,
          claimed: 0,
          running: 0,
          paused: 0,
          done: 0,
          failed: 0,
          canceled: 0
        },
        agent_summary: {
          total: 0,
          auto_mode_enabled: 0,
          busy: 0,
          idle: 0
        }
      }
    ]
  );

  assert.equal(report.failures.length, 1);
  assert.equal(report.failures[0].code, "project_summary_missing_agents");
});

test("computeAutonomyMetrics summarizes governance coverage", () => {
  const metrics = computeAutonomyMetrics(
    {
      agents: [{ id: "agent-1", auto_mode: true }],
      tasks: [
        {
          id: "task-open",
          title: "[0.1.0] 等待自动推进",
          status: "OPEN",
          claimed_by: null,
          activities: [],
          runtime: null,
          state_snapshot: {
            reason: "任务尚未开始执行，等待认领。",
            evidence: ["last_activity:task.created@1"],
            needs_attention: false
          }
        },
        {
          id: "task-paused",
          title: "[0.1.2] 会话断线后自动恢复",
          status: "PAUSED",
          claimed_by: "agent-1",
          activities: [{ kind: "task.auto_resumed" }],
          runtime: { thread_id: "thread-1", active_turn_id: null },
          state_snapshot: {
            reason: "本地运行会话已断开，任务处于可恢复状态。",
            evidence: ["last_activity:task.runtime_session_lost@2", "runtime.thread_id:thread-1"],
            needs_attention: false
          }
        },
        {
          id: "task-done",
          title: "[0.1.0] 最小服务端",
          status: "DONE",
          claimed_by: "agent-1",
          activities: [{ kind: "task.done" }],
          runtime: { log: [] },
          state_snapshot: {
            reason: "任务已完成：服务端已可启动。",
            evidence: ["completion.summary:服务端已可启动"],
            needs_attention: false
          }
        }
      ]
    },
    [
      {
        project_id: "project-1",
        recent_task_summaries: [{ task_id: "task-done" }]
      }
    ]
  );

  assert.equal(metrics.overallPercent, 88);
  assert.equal(metrics.metrics.find((item) => item.code === "state_confidence")?.percent, 100);
  assert.equal(metrics.metrics.find((item) => item.code === "auto_run_coverage")?.percent, 50);
  assert.equal(metrics.metrics.find((item) => item.code === "auto_recovery_coverage")?.percent, 100);
  assert.equal(metrics.metrics.find((item) => item.code === "evolution_coverage")?.percent, 100);
  assert.equal(metrics.counts.unmanagedOpen, 1);
});

test("analyzeHtml warns on stale static app version copy", () => {
  const report = analyzeHtml("<title>Spotlight 0.1.0</title><h1>Spotlight 0.1.0</h1>");
  assert.equal(report.warnings.length, 1);
  assert.equal(report.warnings[0].code, "ui_static_app_version");
});
