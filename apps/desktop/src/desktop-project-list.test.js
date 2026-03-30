import test from "node:test";
import assert from "node:assert/strict";

import { deriveDesktopProjectList } from "./desktop-project-list.js";

const NOW = 1_710_000_000_000;

function secondsAgo(seconds) {
  return String(Math.floor((NOW - seconds * 1000) / 1000));
}

test("项目列表会优先固定当前焦点项目，并展示项目级运行概览", () => {
  const view = deriveDesktopProjectList(
    {
      projects: [
        {
          id: "project-1",
          name: "Spotlight 平台自身"
        },
        {
          id: "project-2",
          name: "客户项目示例"
        }
      ],
      tasks: [
        {
          id: "task-running",
          project_id: "project-1",
          title: "项目感知左栏联动",
          status: "RUNNING",
          activities: [
            {
              at: secondsAgo(18),
              message: "正在刷新项目入口"
            }
          ],
          runtime: {
            log: [
              {
                at: secondsAgo(12),
                message: "刚刚更新项目级摘要"
              }
            ],
            last_error: null
          },
          state_snapshot: {
            reason: "项目中的焦点任务仍在持续推进。",
            evidence: ["runtime.thread_started"]
          }
        },
        {
          id: "task-paused",
          project_id: "project-2",
          title: "恢复失败任务",
          status: "PAUSED",
          activities: [],
          runtime: {
            log: [],
            last_error: "watchdog 已回收，等待恢复。"
          },
          state_snapshot: {
            reason: "项目链路停在待恢复状态。",
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
        }
      ],
      pending_questions: [
        {
          id: "question-1",
          project_id: "project-1",
          status: "open"
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

  assert.equal(view.summary, "已关联 2 个项目 · 需复核 1 · 推进中 2");
  assert.equal(view.items[0].id, "project-1");
  assert.equal(view.items[0].active, true);
  assert.equal(view.items[0].tone, "busy");
  assert.equal(view.items[0].signalLabel, "刚刚更新");
  assert.equal(view.items[0].headlineTaskTitle, "项目感知左栏联动");
  assert.equal(view.items[0].countsLabel, "任务 1 · 处理中 1 · 待恢复 0 · 待答 1");
});

test("没有焦点时会把需复核的项目排到最前面", () => {
  const view = deriveDesktopProjectList(
    {
      projects: [
        {
          id: "project-1",
          name: "Spotlight 平台自身"
        },
        {
          id: "project-2",
          name: "客户项目示例"
        }
      ],
      tasks: [
        {
          id: "task-open",
          project_id: "project-1",
          title: "普通待办",
          status: "OPEN",
          activities: [],
          runtime: null,
          state_snapshot: {}
        },
        {
          id: "task-paused",
          project_id: "project-2",
          title: "恢复失败任务",
          status: "PAUSED",
          activities: [
            {
              at: secondsAgo(660),
              message: "watchdog 已回收任务"
            }
          ],
          runtime: {
            log: [],
            last_error: "本地会话已断开，等待自动恢复或人工处理。"
          },
          state_snapshot: {
            reason: "项目进入待恢复状态。",
            evidence: ["task.watchdog_recovered", "runtime.session_missing"],
            needs_attention: true
          }
        }
      ],
      agents: [],
      pending_questions: [],
      task_run_history: {}
    },
    {},
    NOW
  );

  assert.equal(view.items[0].id, "project-2");
  assert.equal(view.items[0].tone, "error");
  assert.equal(view.items[0].headlineTaskStatus, "待恢复");
  assert.equal(view.items[0].signalLabel, "链路可能停住");
  assert.match(view.items[0].reasonSummary, /等待自动恢复|待恢复/);
});

test("没有项目时也会返回可渲染的占位信息", () => {
  const view = deriveDesktopProjectList(
    {
      projects: [],
      tasks: [],
      agents: [],
      pending_questions: [],
      task_run_history: {}
    },
    {},
    NOW
  );

  assert.equal(view.summary, "服务端返回的当前看板里还没有项目。");
  assert.deepEqual(view.items, []);
});
