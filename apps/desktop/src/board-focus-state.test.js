import test from "node:test";
import assert from "node:assert/strict";

import {
  BOARD_FOCUS_MESSAGE_SOURCE,
  DESKTOP_FOCUS_STORAGE_KEY,
  buildBoardUrl,
  clearDesktopFocus,
  hasBoardFocus,
  parseBoardFocusMessage,
  readDesktopFocus,
  writeDesktopFocus
} from "./board-focus-state.js";

function createStorage(initialValue = null) {
  const store = new Map();
  if (initialValue !== null) {
    store.set(DESKTOP_FOCUS_STORAGE_KEY, initialValue);
  }
  return {
    getItem(key) {
      return store.has(key) ? store.get(key) : null;
    },
    setItem(key, value) {
      store.set(key, value);
    },
    removeItem(key) {
      store.delete(key);
    }
  };
}

test("构造看板地址时会带上恢复焦点参数", () => {
  const url = buildBoardUrl(
    "http://127.0.0.1:3000",
    {
      projectId: "project-1",
      taskId: "task-2",
      sessionId: "session-3"
    },
    123
  );

  assert.equal(
    url,
    "http://127.0.0.1:3000/?project_id=project-1&task_id=task-2&session_id=session-3&ts=123"
  );
});

test("读取桌面焦点时会忽略损坏的本地数据", () => {
  const focus = readDesktopFocus(createStorage("{bad-json"));

  assert.deepEqual(focus, {
    projectId: null,
    taskId: null,
    sessionId: null,
    projectName: null,
    taskTitle: null,
    sessionTitle: null
  });
});

test("写入桌面焦点时会标准化字段并落库", () => {
  const storage = createStorage();
  const focus = writeDesktopFocus(
    {
      projectId: " project-1 ",
      taskId: "",
      sessionId: "session-2",
      projectName: " Spotlight ",
      taskTitle: " 任务 A ",
      sessionTitle: " 会话一 "
    },
    storage
  );

  assert.deepEqual(focus, {
    projectId: "project-1",
    taskId: null,
    sessionId: "session-2",
    projectName: "Spotlight",
    taskTitle: "任务 A",
    sessionTitle: "会话一"
  });
  assert.equal(storage.getItem(DESKTOP_FOCUS_STORAGE_KEY), JSON.stringify(focus));
});

test("没有任何焦点 id 时会清理本地记录", () => {
  const storage = createStorage(JSON.stringify({
    projectId: "project-1",
    taskId: null,
    sessionId: null,
    projectName: "Spotlight",
    taskTitle: null,
    sessionTitle: null
  }));

  const focus = clearDesktopFocus(storage);

  assert.deepEqual(focus, {
    projectId: null,
    taskId: null,
    sessionId: null,
    projectName: null,
    taskTitle: null,
    sessionTitle: null
  });
  assert.equal(storage.getItem(DESKTOP_FOCUS_STORAGE_KEY), null);
});

test("hasBoardFocus 只在存在焦点 id 时返回真", () => {
  assert.equal(hasBoardFocus({}), false);
  assert.equal(hasBoardFocus({ projectName: "只有名称" }), false);
  assert.equal(hasBoardFocus({ taskId: "task-1" }), true);
});

test("解析 iframe 焦点消息时只接受约定来源", () => {
  assert.equal(parseBoardFocusMessage({ source: "other", focus: {} }), null);

  const focus = parseBoardFocusMessage({
    source: BOARD_FOCUS_MESSAGE_SOURCE,
    focus: {
      projectId: "project-1",
      taskId: "task-2",
      sessionId: "session-3",
      projectName: "项目一",
      taskTitle: "任务二",
      sessionTitle: "会话三"
    }
  });

  assert.deepEqual(focus, {
    projectId: "project-1",
    taskId: "task-2",
    sessionId: "session-3",
    projectName: "项目一",
    taskTitle: "任务二",
    sessionTitle: "会话三"
  });
});
