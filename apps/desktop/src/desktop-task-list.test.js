import test from "node:test";
import assert from "node:assert/strict";

import { deriveDesktopTaskList } from "./desktop-task-list.js";

const NOW = 1_710_000_000_000;

function secondsAgo(seconds) {
  return String(Math.floor((NOW - seconds * 1000) / 1000));
}

test("桌面任务列表会优先固定当前焦点任务，并保留状态摘要", () => {
  const view = deriveDesktopTaskList(
    {
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
          title: "治理概览联动",
          status: "RUNNING",
          priority: "MEDIUM",
          activities: [
            {
              at: secondsAgo(20),
              message: "任务正在推进"
            }
          ],
          runtime: {
            log: [
              {
                at: secondsAgo(12),
                message: "刚刚完成一轮左栏刷新"
              }
            ],
            last_error: null
          },
          state_snapshot: {
            reason: "任务运行中，链路正常。",
            evidence: ["runtime.thread_started"]
          }
        },
        {
          id: "task-paused",
          project_id: "project-1",
          title: "恢复桌面会话",
          status: "PAUSED",
          priority: "HIGH",
          activities: [],
          runtime: {
            log: [],
            last_error: "watchdog 已回收，等待恢复。"
          },
          state_snapshot: {
            reason: "任务进入待恢复状态。",
            evidence: ["task.watchdog_recovered"],
            needs_attention: true
          }
        }
      ],
      task_run_history: {}
    },
    {
      projectId: "project-1",
      taskId: "task-running"
    },
    NOW
  );

  assert.equal(view.scopeLabel, "Spotlight 平台自身");
  assert.equal(view.summary, "处理中 1 · 待恢复 1 · 需复核 1");
  assert.equal(view.items[0].id, "task-running");
  assert.equal(view.items[0].active, true);
  assert.equal(view.items[0].statusLabel, "运行中");
  assert.equal(view.items[0].signalLabel, "刚刚更新");
  assert.match(view.items[0].reasonSummary, /链路正常|左栏刷新/);
});

test("没有焦点时会先展示需要复核和待恢复的任务", () => {
  const view = deriveDesktopTaskList(
    {
      projects: [
        {
          id: "project-1",
          name: "Spotlight 平台自身"
        }
      ],
      tasks: [
        {
          id: "task-open",
          project_id: "project-1",
          title: "普通待办",
          status: "OPEN",
          priority: "LOW",
          activities: [],
          runtime: null,
          state_snapshot: {}
        },
        {
          id: "task-paused",
          project_id: "project-1",
          title: "恢复桌面会话",
          status: "PAUSED",
          priority: "HIGH",
          activities: [
            {
              at: secondsAgo(720),
              message: "watchdog 已回收任务"
            }
          ],
          runtime: {
            log: [],
            last_error: "运行会话已断开，等待恢复。"
          },
          state_snapshot: {
            reason: "任务进入待恢复状态。",
            evidence: ["task.watchdog_recovered", "runtime.session_missing"],
            needs_attention: true
          }
        }
      ],
      task_run_history: {}
    },
    {},
    NOW
  );

  assert.equal(view.items[0].id, "task-paused");
  assert.equal(view.items[0].tone, "error");
  assert.equal(view.items[0].statusLabel, "待恢复");
  assert.equal(view.items[0].signalLabel, "链路可能停住");
  assert.match(view.items[0].reasonSummary, /等待恢复|断开/);
  assert.match(view.items[0].evidenceSummary, /watchdog/);
});

test("空任务列表也会返回可渲染占位信息", () => {
  const view = deriveDesktopTaskList(
    {
      projects: [],
      tasks: [],
      task_run_history: {}
    },
    {
      projectName: "Spotlight 平台自身"
    },
    NOW
  );

  assert.equal(view.scopeLabel, "Spotlight 平台自身");
  assert.equal(view.summary, "服务端返回的当前看板里还没有任务。");
  assert.deepEqual(view.items, []);
});
