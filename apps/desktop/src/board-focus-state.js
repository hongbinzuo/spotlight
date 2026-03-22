export const DESKTOP_FOCUS_STORAGE_KEY = "spotlight.desktop.focus.v1";
export const BOARD_FOCUS_MESSAGE_SOURCE = "spotlight-board-focus";

function normalizedId(value) {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function normalizedLabel(value) {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

export function normalizeBoardFocus(focus) {
  return {
    projectId: normalizedId(focus?.projectId),
    taskId: normalizedId(focus?.taskId),
    sessionId: normalizedId(focus?.sessionId),
    projectName: normalizedLabel(focus?.projectName),
    taskTitle: normalizedLabel(focus?.taskTitle),
    sessionTitle: normalizedLabel(focus?.sessionTitle)
  };
}

export function hasBoardFocus(focus) {
  const normalized = normalizeBoardFocus(focus);
  return Boolean(normalized.projectId || normalized.taskId || normalized.sessionId);
}

export function readDesktopFocus(storage) {
  if (!storage?.getItem) {
    return normalizeBoardFocus();
  }

  try {
    return normalizeBoardFocus(JSON.parse(storage.getItem(DESKTOP_FOCUS_STORAGE_KEY) || "null"));
  } catch {
    return normalizeBoardFocus();
  }
}

export function writeDesktopFocus(focus, storage) {
  const normalized = normalizeBoardFocus(focus);
  if (!storage?.setItem || !storage?.removeItem) {
    return normalized;
  }

  try {
    if (hasBoardFocus(normalized)) {
      storage.setItem(DESKTOP_FOCUS_STORAGE_KEY, JSON.stringify(normalized));
    } else {
      storage.removeItem(DESKTOP_FOCUS_STORAGE_KEY);
    }
  } catch {
    // 忽略桌面端本地存储失败，避免打断主流程。
  }

  return normalized;
}

export function clearDesktopFocus(storage) {
  return writeDesktopFocus({}, storage);
}

export function buildBoardUrl(baseUrl, focus, now = Date.now()) {
  const normalized = normalizeBoardFocus(focus);
  const url = new URL(baseUrl);

  if (normalized.projectId) {
    url.searchParams.set("project_id", normalized.projectId);
  }
  if (normalized.taskId) {
    url.searchParams.set("task_id", normalized.taskId);
  }
  if (normalized.sessionId) {
    url.searchParams.set("session_id", normalized.sessionId);
  }
  url.searchParams.set("ts", String(now));

  return url.toString();
}

export function parseBoardFocusMessage(data) {
  if (!data || data.source !== BOARD_FOCUS_MESSAGE_SOURCE) {
    return null;
  }

  return normalizeBoardFocus(data.focus);
}
