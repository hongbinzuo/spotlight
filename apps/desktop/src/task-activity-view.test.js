import test from "node:test";
import assert from "node:assert/strict";

import { deriveTaskActivityView } from "./task-activity-view.js";

const NOW = 1_710_000_000_000;

function secondsAgo(seconds) {
  return String(Math.floor((NOW - seconds * 1000) / 1000));
}

test("默认模式优先展示焦点任务的最新关键输出和活动", () => {
  const view = deriveTaskActivityView(
    {
      projects: [
        {
          id: "project-1",
          name: "Spotlight 平台自身"
        }
      ],
      tasks: [
        {
          id: "task-1",
          project_id: "project-1",
          title: "补活动日志面板",
          status: "RUNNING",
          activities: [
            {
              kind: "task.auto_started",
              at: secondsAgo(45),
              message: "系统已自动启动任务"
            },
            {
              kind: "runtime.thread_started",
              at: secondsAgo(30),
              message: "已建立长会话"
            }
          ],
          runtime: {
            log: [
              {
                kind: "assistant",
                at: secondsAgo(8),
                message: "已把最新关键输出放到桌面端顶部。"
              }
            ],
            last_error: null
          },
          state_snapshot: {
            reason: "任务正在推进桌面端活动流渲染。",
            evidence: ["runtime.thread_started", "task.auto_started"],
            needs_attention: false
          }
        }
      ],
      task_run_history: {}
    },
    {
      projectId: "project-1",
      taskId: "task-1"
    },
    {
      now: NOW
    }
  );

  assert.equal(view.taskTitle, "补活动日志面板");
  assert.equal(view.statusLabel, "运行中");
  assert.equal(view.freshnessLabel, "最新输出活跃");
  assert.equal(view.primaryOutputLabel, "Agent 输出");
  assert.match(view.primaryOutputMessage, /最新关键输出/);
  assert.equal(view.items.length, 3);
  assert.equal(view.items[0].label, "Agent 输出");
  assert.equal(view.searchEnabled, false);
  assert.match(view.summaryLabel, /默认仅展示最近且最重要的/);
});

test("诊断模式支持搜索并回退到运行历史日志", () => {
  const view = deriveTaskActivityView(
    {
      projects: [
        {
          id: "project-1",
          name: "Spotlight 平台自身"
        }
      ],
      tasks: [
        {
          id: "task-1",
          project_id: "project-1",
          title: "排查恢复状态",
          status: "PAUSED",
          activities: [
            {
              kind: "task.watchdog_recovered",
              at: secondsAgo(800),
              message: "watchdog 已回收任务"
            }
          ],
          runtime: {
            log: [],
            last_error: "本地运行会话已断开，等待恢复。"
          },
          state_snapshot: {
            reason: "任务进入可恢复状态，等待恢复。",
            evidence: ["task.watchdog_recovered", "runtime.thread_id:abc"],
            needs_attention: true
          }
        }
      ],
      task_run_history: {
        "task-1": [
          {
            id: "run-1",
            log: [
              {
                kind: "command",
                at: secondsAgo(780),
                message: "rg -n activity apps/desktop/src"
              },
              {
                kind: "assistant",
                at: secondsAgo(760),
                message: "已定位到活动日志入口。"
              }
            ]
          }
        ]
      }
    },
    {
      taskId: "task-1"
    },
    {
      now: NOW,
      mode: "diagnostic",
      searchQuery: "rg -n"
    }
  );

  assert.equal(view.tone, "error");
  assert.equal(view.searchEnabled, true);
  assert.equal(view.primaryOutputLabel, "Agent 输出");
  assert.match(view.primaryOutputMessage, /活动日志入口/);
  assert.equal(view.items.length, 1);
  assert.equal(view.items[0].label, "命令输出");
  assert.match(view.items[0].message, /rg -n/);
  assert.match(view.summaryLabel, /命中 1 条/);
  assert.match(view.alert, /等待恢复/);
});

test("没有任务时返回可渲染占位信息", () => {
  const view = deriveTaskActivityView(
    {
      projects: [],
      tasks: [],
      task_run_history: {}
    },
    {
      projectName: "Spotlight 平台自身",
      taskTitle: "活动日志"
    },
    {
      now: NOW
    }
  );

  assert.equal(view.projectName, "Spotlight 平台自身");
  assert.equal(view.taskTitle, "活动日志");
  assert.equal(view.statusLabel, "暂无任务");
  assert.equal(view.items.length, 0);
  assert.match(view.hint, /详细日志/);
});

test("流式碎片会合并成连续输出块，避免一词一卡", () => {
  const view = deriveTaskActivityView(
    {
      projects: [
        {
          id: "project-1",
          name: "Spotlight 平台自身"
        }
      ],
      tasks: [
        {
          id: "task-1",
          project_id: "project-1",
          title: "收口输出聚合",
          status: "RUNNING",
          activities: [],
          runtime: {
            log: [
              {
                kind: "command",
                at: secondsAgo(8),
                message: "## 1. Architecture Summary"
              },
              {
                kind: "assistant",
                at: secondsAgo(7),
                message: "活动"
              },
              {
                kind: "command",
                at: secondsAgo(6),
                message: " return `${Math.max(1, Math.round(ageMs / (24 * 60 * 60 * 1000)))} 天前`;"
              },
              {
                kind: "assistant",
                at: secondsAgo(5),
                message: "面"
              },
              {
                kind: "command",
                at: secondsAgo(4),
                message: "# System Architecture"
              },
              {
                kind: "assistant",
                at: secondsAgo(3),
                message: "板"
              }
            ],
            last_error: null
          },
          state_snapshot: {
            reason: "正在整理长会话输出展示。",
            evidence: ["runtime.thread_started"],
            needs_attention: false
          }
        }
      ],
      task_run_history: {}
    },
    {
      taskId: "task-1"
    },
    {
      now: NOW,
      mode: "diagnostic"
    }
  );

  assert.equal(view.items.length, 1);
  assert.match(view.items[0].label, /连续输出/);
  assert.match(view.items[0].message, /\[命令输出\]/);
  assert.match(view.items[0].message, /\[Agent 输出\]/);
  assert.match(view.primaryOutputMessage, /Architecture Summary/);
});
