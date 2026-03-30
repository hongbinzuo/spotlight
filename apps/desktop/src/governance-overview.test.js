import test from "node:test";
import assert from "node:assert/strict";

import { deriveGovernanceOverview } from "./governance-overview.js";

const NOW = 1_710_000_000_000;

function secondsAgo(seconds) {
  return String(Math.floor((NOW - seconds * 1000) / 1000));
}

test("治理概览优先展示焦点任务的状态依据和自动运行信号", () => {
  const board = {
    projects: [
      {
        id: "project-1",
        name: "Spotlight 平台自身"
      }
    ],
    tasks: [
      {
        id: "task-running",
        project_id: "project-1",
        title: "补桌面壳治理概览",
        status: "RUNNING",
        activities: [
          {
            at: secondsAgo(32),
            message: "任务已进入运行中"
          }
        ],
        runtime: {
          log: [
            {
              at: secondsAgo(12),
              message: "已展示状态依据和关键输出"
            }
          ],
          last_error: null
        },
        state_snapshot: {
          reason: "任务正在执行最近一轮桌面壳补丁。",
          evidence: ["runtime.thread_started", "task.activity_updated"],
          needs_attention: false
        }
      },
      {
        id: "task-paused",
        project_id: "project-1",
        title: "恢复桌面会话",
        status: "PAUSED",
        activities: [],
        runtime: {
          log: [],
          last_error: "本地运行会话已断开，任务已转为可恢复状态"
        },
        state_snapshot: {
          reason: "任务已进入可恢复状态。",
          evidence: ["task.watchdog_recovered"],
          needs_attention: true
        }
      }
    ],
    agents: [
      {
        id: "agent-1",
        auto_mode: true,
        current_task_id: "task-running"
      },
      {
        id: "agent-2",
        auto_mode: true,
        current_task_id: null
      }
    ],
    pending_questions: [
      {
        id: "question-1",
        project_id: "project-1"
      }
    ],
    task_run_history: {}
  };

  const view = deriveGovernanceOverview(
    board,
    {
      projectId: "project-1",
      taskId: "task-running"
    },
    NOW
  );

  assert.equal(view.tone, "ready");
  assert.equal(view.projectName, "Spotlight 平台自身");
  assert.equal(view.taskTitle, "补桌面壳治理概览");
  assert.equal(view.taskStatusLabel, "运行中");
  assert.equal(view.freshnessLabel, "最新输出活跃");
  assert.match(view.recoveryLabel, /无需恢复/);
  assert.match(view.automationLabel, /2 个自动运行 Agent/);
  assert.match(view.countsLabel, /待恢复 1/);
  assert.match(view.stateReason, /桌面壳补丁/);
  assert.match(view.primaryOutput, /状态依据和关键输出/);
});

test("治理概览会把可恢复但停住的任务标记为需复核", () => {
  const board = {
    projects: [
      {
        id: "project-1",
        name: "Spotlight 平台自身"
      }
    ],
    tasks: [
      {
        id: "task-paused",
        project_id: "project-1",
        title: "恢复桌面会话",
        status: "PAUSED",
        activities: [
          {
            at: secondsAgo(660),
            message: "watchdog 已回收任务"
          }
        ],
        runtime: {
          log: [],
          last_error: "本地运行会话已断开，任务已转为可恢复状态，等待自动恢复或人工继续"
        },
        state_snapshot: {
          reason: "任务已进入可恢复状态，等待自动恢复。",
          evidence: ["task.watchdog_recovered", "runtime.session_missing"],
          needs_attention: true
        }
      },
      {
        id: "task-open",
        project_id: "project-1",
        title: "普通待办",
        status: "OPEN",
        activities: [],
        runtime: null,
        state_snapshot: {}
      }
    ],
    agents: [
      {
        id: "agent-1",
        auto_mode: true,
        current_task_id: null
      }
    ],
    pending_questions: [],
    task_run_history: {}
  };

  const view = deriveGovernanceOverview(
    board,
    {
      projectId: "project-1"
    },
    NOW
  );

  assert.equal(view.taskTitle, "恢复桌面会话");
  assert.equal(view.tone, "error");
  assert.equal(view.taskStatusLabel, "待恢复");
  assert.equal(view.freshnessLabel, "链路可能停住");
  assert.match(view.recoveryLabel, /自动恢复/);
  assert.match(view.automationLabel, /自动运行 Agent/);
  assert.match(view.alert, /可恢复状态/);
});

test("没有任务时也会返回可渲染的治理占位信息", () => {
  const view = deriveGovernanceOverview(
    {
      projects: [],
      tasks: [],
      agents: [],
      pending_questions: [],
      task_run_history: {}
    },
    {
      projectName: "Spotlight 平台自身",
      taskTitle: "桌面壳任务"
    },
    NOW
  );

  assert.equal(view.tone, "idle");
  assert.equal(view.projectName, "Spotlight 平台自身");
  assert.equal(view.taskTitle, "桌面壳任务");
  assert.equal(view.taskStatusLabel, "暂无任务");
});
