pub const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Spotlight 0.1.0</title>
  <style>
    :root {
      --bg: #f3efe5;
      --panel: #fffaf0;
      --panel-strong: #f7f0df;
      --border: #d8ccb6;
      --text: #241d16;
      --muted: #7b6f61;
      --accent: #c97b28;
      --success: #2a8a5c;
      --warn: #d07c32;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      font-family: "Microsoft YaHei UI", "PingFang SC", sans-serif;
      color: var(--text);
      background:
        radial-gradient(circle at top left, rgba(201, 123, 40, 0.18), transparent 22%),
        linear-gradient(180deg, #f7f1e8, #efe7da 45%, #ece3d3 100%);
    }
    header {
      padding: 20px 24px 12px;
      border-bottom: 1px solid rgba(0,0,0,0.06);
      background: rgba(255,250,240,0.75);
      backdrop-filter: blur(8px);
      position: sticky;
      top: 0;
      z-index: 1;
    }
    header h1 { margin: 0 0 6px; font-size: 24px; }
    header p { margin: 0; color: var(--muted); font-size: 14px; }
    main {
      display: grid;
      grid-template-columns: 380px minmax(0, 1fr);
      gap: 14px;
      padding: 16px 20px 24px;
      min-height: calc(100vh - 82px);
    }
    .panel {
      border: 1px solid var(--border);
      border-radius: 18px;
      background: rgba(255,250,240,0.92);
      box-shadow: 0 18px 40px rgba(81, 61, 36, 0.08);
      overflow: hidden;
    }
    .panel-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      padding: 14px 16px;
      background: rgba(201, 123, 40, 0.08);
      border-bottom: 1px solid var(--border);
    }
    .panel-header h2 { margin: 0; font-size: 16px; }
    .panel-body { padding: 14px 16px 16px; }
    .toolbar, .inline-actions { display: flex; gap: 8px; flex-wrap: wrap; }
    button, input, textarea, select { font: inherit; }
    button {
      border: 0;
      border-radius: 999px;
      padding: 9px 14px;
      cursor: pointer;
      background: var(--accent);
      color: white;
      font-weight: 700;
    }
    button.secondary {
      background: var(--panel-strong);
      color: var(--text);
      border: 1px solid var(--border);
    }
    button:disabled {
      opacity: 0.5;
      cursor: not-allowed;
    }
    button.warn { background: var(--warn); }
    button.success { background: var(--success); }
    input, textarea, select {
      width: 100%;
      border-radius: 14px;
      border: 1px solid var(--border);
      padding: 10px 12px;
      background: #fffdf9;
      color: var(--text);
    }
    textarea { min-height: 86px; resize: vertical; }
    .summary {
      display: grid;
      grid-template-columns: repeat(2, 1fr);
      gap: 8px;
      margin-bottom: 12px;
    }
    .summary-box {
      border: 1px solid var(--border);
      border-radius: 16px;
      background: #fffdf9;
      padding: 10px 12px;
    }
    .summary-box strong { display: block; font-size: 22px; }
    .summary-box span { color: var(--muted); font-size: 12px; }
    .project-card, .detail-card {
      border: 1px solid var(--border);
      border-radius: 18px;
      background: #fffdf9;
      padding: 14px;
    }
    .project-card { margin-bottom: 12px; }
    .project-card h3, .detail-card h3, .detail-card h4 { margin: 0 0 8px; }
    .notice-banner {
      margin-top: 12px;
      border-radius: 14px;
      padding: 10px 12px;
      border: 1px solid var(--border);
      background: #fff7ec;
      color: var(--text);
    }
    .notice-banner.error {
      background: #fff0eb;
      border-color: #d07c32;
    }
    .notice-banner.warn {
      background: #fff6df;
      border-color: #d8b485;
    }
    .section-head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
      flex-wrap: wrap;
      margin-bottom: 8px;
    }
    .section-head h4 { margin: 0; }
    .create-box { display: grid; gap: 8px; margin-bottom: 14px; }
    .task-list {
      display: grid;
      gap: 8px;
      max-height: calc(100vh - 420px);
      overflow: auto;
      padding-right: 4px;
    }
    .task-item {
      border: 1px solid var(--border);
      background: #fffdf9;
      border-radius: 16px;
      padding: 12px;
      cursor: pointer;
      transition: transform 120ms ease, border-color 120ms ease, background 120ms ease;
    }
    .task-item:hover { transform: translateY(-1px); border-color: #bfa37a; }
    .task-item.active { background: #fff4e7; border-color: #c97b28; }
    .task-item h3 { margin: 0 0 6px; font-size: 15px; }
    .meta {
      display: flex;
      flex-wrap: wrap;
      gap: 6px;
      margin: 6px 0 0;
      color: var(--muted);
      font-size: 12px;
    }
    .pill {
      border-radius: 999px;
      border: 1px solid var(--border);
      padding: 3px 8px;
      background: var(--panel);
    }
    .description { white-space: pre-wrap; line-height: 1.6; color: #3c3228; }
    .detail-layout { display: grid; gap: 12px; }
    .two-col {
      display: grid;
      gap: 12px;
      grid-template-columns: 320px minmax(0, 1fr);
    }
    .log {
      border: 1px solid var(--border);
      border-radius: 14px;
      background: #fffaf3;
      padding: 10px 12px;
      max-height: 420px;
      overflow: auto;
      line-height: 1.55;
      white-space: pre-wrap;
    }
    .log-textarea {
      min-height: 180px;
      resize: none;
      overflow-wrap: anywhere;
      word-break: break-word;
      white-space: pre-wrap;
      font-family: Consolas, "SFMono-Regular", monospace;
      font-size: 13px;
    }
    .flow-log {
      padding: 8px 10px;
      line-height: 1.4;
    }
    .copy-feedback {
      min-height: 18px;
      font-size: 12px;
    }
    .log-item {
      padding-bottom: 10px;
      margin-bottom: 10px;
      border-bottom: 1px dashed rgba(0,0,0,0.08);
    }
    .log-item:last-child { margin-bottom: 0; padding-bottom: 0; border-bottom: 0; }
    .muted { color: var(--muted); }
    .agents { display: grid; gap: 8px; }
    .agent-card {
      border: 1px solid var(--border);
      background: #fffaf3;
      border-radius: 14px;
      padding: 10px 12px;
    }
    .agent-actions {
      display: flex;
      gap: 8px;
      flex-wrap: wrap;
      margin-top: 8px;
    }
    .session-select {
      margin-bottom: 8px;
    }
    .conversation {
      display: grid;
      gap: 10px;
      max-height: 340px;
      overflow: auto;
    }
    .message {
      border: 1px solid var(--border);
      border-radius: 16px;
      padding: 10px 12px;
      background: #fffdf9;
    }
    .message.user {
      background: #fff1df;
      border-color: #d8b485;
    }
    .message.assistant {
      background: #f8f5ef;
    }
    .message-meta {
      display: flex;
      justify-content: space-between;
      gap: 8px;
      margin-bottom: 6px;
      color: var(--muted);
      font-size: 12px;
    }
    .task-item.running,
    .task-item.claimed {
      border-color: rgba(42, 138, 92, 0.38);
      background: #f3fbf6;
    }
    .task-item.paused {
      border-color: rgba(208, 124, 50, 0.38);
      background: #fff7ec;
    }
    .task-item.failed {
      border-color: rgba(183, 58, 58, 0.34);
      background: #fff1ef;
    }
    .task-item.canceled {
      opacity: 0.72;
      background: #f3efe8;
    }
    .task-item-summary {
      margin-top: 8px;
      color: var(--muted);
      font-size: 12px;
      line-height: 1.5;
    }
    .task-insight-grid {
      display: grid;
      gap: 10px;
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .insight-card {
      border: 1px solid var(--border);
      border-radius: 16px;
      background: #fffaf3;
      padding: 12px;
    }
    .insight-card strong {
      display: block;
      margin-bottom: 6px;
      font-size: 18px;
    }
    .insight-card .muted {
      line-height: 1.5;
    }
    .insight-card.warn {
      background: #fff4e8;
      border-color: rgba(208, 124, 50, 0.35);
    }
    .insight-card.error {
      background: #fff0ed;
      border-color: rgba(183, 58, 58, 0.3);
    }
    .stream-feed,
    .flow-feed {
      display: grid;
      gap: 8px;
    }
    .stream-feed.muted,
    .flow-feed.muted {
      color: var(--muted);
    }
    .stream-entry,
    .flow-entry {
      border: 1px solid var(--border);
      border-radius: 14px;
      background: #fffaf3;
      padding: 10px 12px;
    }
    .flow-entry {
      border-radius: 12px;
      padding: 7px 9px;
    }
    .stream-entry.assistant {
      background: #f4fbf7;
      border-color: rgba(42, 138, 92, 0.24);
    }
    .stream-entry.command,
    .stream-entry.plan {
      background: #f6f2ff;
      border-color: rgba(90, 84, 170, 0.18);
    }
    .stream-entry.stderr,
    .stream-entry.error {
      background: #fff0ed;
      border-color: rgba(183, 58, 58, 0.24);
    }
    .flow-entry.error {
      background: #fff0ed;
      border-color: rgba(183, 58, 58, 0.24);
    }
    .flow-entry.warn {
      background: #fff4e8;
      border-color: rgba(208, 124, 50, 0.32);
    }
    .flow-entry.auto {
      background: #f2f7ff;
      border-color: rgba(82, 124, 199, 0.24);
    }
    .entry-head {
      display: flex;
      justify-content: space-between;
      gap: 12px;
      align-items: center;
      margin-bottom: 6px;
      font-size: 12px;
      color: var(--muted);
    }
    .flow-entry .entry-head {
      gap: 8px;
      margin-bottom: 3px;
      font-size: 11px;
    }
    .entry-title {
      font-weight: 700;
      color: var(--text);
    }
    .entry-body {
      white-space: pre-wrap;
      line-height: 1.55;
      color: #3c3228;
      font-size: 13px;
      max-height: 180px;
      overflow: auto;
    }
    .flow-entry .entry-body {
      line-height: 1.35;
      max-height: none;
    }
    .floating-runtime-window {
      position: fixed;
      right: 18px;
      bottom: 18px;
      width: min(360px, calc(100vw - 24px));
      max-height: min(46vh, 420px);
      overflow: auto;
      z-index: 3;
      border: 1px solid var(--border);
      border-radius: 18px;
      background: rgba(255, 250, 240, 0.96);
      box-shadow: 0 18px 48px rgba(69, 51, 32, 0.22);
      backdrop-filter: blur(10px);
      padding: 14px;
    }
    .running-window-list {
      display: grid;
      gap: 8px;
      margin-top: 10px;
    }
    .running-window-item {
      border: 1px solid var(--border);
      border-radius: 14px;
      background: #fffdf9;
      padding: 10px 12px;
      cursor: pointer;
    }
    .running-window-item.active {
      border-color: var(--accent);
      background: #fff4e7;
    }
    .running-window-item:hover {
      border-color: #bfa37a;
    }
    .running-window-item strong {
      display: block;
      margin-bottom: 6px;
    }
    @media (max-width: 960px) {
      main { grid-template-columns: 1fr; }
      .task-list { max-height: none; }
      .two-col { grid-template-columns: 1fr; }
      .task-insight-grid { grid-template-columns: 1fr; }
      .floating-runtime-window {
        position: static;
        width: 100%;
        max-height: none;
        margin: 0 20px 20px;
      }
    }
  </style>
</head>
<body>
  <header>
    <div style="display:flex; align-items:flex-start; justify-content:space-between; gap:16px; flex-wrap:wrap;">
      <div>
        <h1>Spotlight 0.1.0</h1>
        <p>左侧是任务看板，右侧是 Agent 面板。当前支持项目切换、自动认领、探索目录、启动任务、暂停、补充提示词后恢复。</p>
      </div>
      <div class="detail-card" style="min-width:280px; padding:12px 14px;">
        <div class="section-head" style="margin-bottom:6px;">
          <h4>当前用户</h4>
          <span id="currentUserBadge" class="pill">未登录</span>
        </div>
        <div class="inline-actions">
          <select id="userSelect" style="min-width:160px;"></select>
          <button class="secondary" onclick="loginSelectedUser()">切换用户</button>
        </div>
      </div>
    </div>
    <div id="noticeBanner" class="notice-banner" style="display:none;"></div>
  </header>
  <main>
    <section class="panel">
      <div class="panel-header">
        <h2>任务看板</h2>
        <div class="toolbar">
          <button class="secondary" onclick="loadBoard()">刷新</button>
        </div>
      </div>
      <div class="panel-body">
        <div id="projectCard" class="project-card"></div>
        <div id="summary" class="summary"></div>
        <div class="create-box">
          <input id="title" placeholder="任务标题" />
          <textarea id="description" placeholder="请输入任务描述、范围、上下文，或者你想补充给 Agent 的说明"></textarea>
          <button onclick="createTask()">新增任务</button>
        </div>
        <div id="tasks" class="task-list"></div>
      </div>
    </section>
    <section class="panel">
      <div class="panel-header">
        <h2>Agent 面板</h2>
        <div class="toolbar">
          <button class="secondary" onclick="loadBoard()">刷新状态</button>
        </div>
      </div>
      <div class="panel-body">
        <div class="detail-layout">
          <div class="two-col">
            <div class="detail-card">
              <h4>项目目录与扫描</h4>
              <div id="projectContextCard"></div>
            </div>
            <div class="detail-card">
              <h4>项目会话</h4>
              <div id="projectSessionCard"></div>
            </div>
          </div>
          <div class="detail-card">
            <div id="taskDetail"></div>
          </div>
          <div class="two-col">
            <div class="detail-card">
              <h4>执行概览</h4>
              <div id="taskExecutionOverview" class="muted">请选择任务后查看执行状态。</div>
            </div>
            <div class="detail-card">
              <h4>最近输出</h4>
              <div id="runtimeStream" class="stream-feed muted">这里会高亮显示最新的实时输出、命令结果和错误。</div>
            </div>
          </div>
          <div class="two-col">
            <div class="detail-card">
              <h4>任务操作</h4>
              <div style="display:grid; gap:8px;">
                <select id="agentSelect"></select>
                <textarea id="promptBox" placeholder="这里可以输入启动提示词，或者在暂停后补充新的提示词再恢复"></textarea>
                <div class="inline-actions">
                  <button onclick="claimSelected()">认领</button>
                  <button class="success" onclick="startSelected()">开始执行</button>
                  <button class="warn" onclick="pauseSelected()">暂停</button>
                  <button class="warn" onclick="cancelSelected()">撤销任务</button>
                  <button class="secondary" onclick="resumeSelected()">补充后恢复</button>
                </div>
                <button class="secondary" onclick="toggleSelectedAgentAutoMode()">切换当前 Agent 自动认领</button>
              </div>
            </div>
            <div class="detail-card">
              <h4>Agent 状态</h4>
              <div id="agents" class="agents"></div>
            </div>
          </div>
          <div class="detail-card">
            <div class="section-head">
              <h4>会话日志</h4>
              <div class="inline-actions">
                <span id="runtimeLogCopyFeedback" class="copy-feedback muted" aria-live="polite"></span>
                <button id="scrollRuntimeLogTopButton" class="secondary" type="button" onclick="scrollRuntimeLogTop()" disabled>滚到顶部</button>
                <button id="scrollRuntimeLogBottomButton" class="secondary" type="button" onclick="scrollRuntimeLogBottom()" disabled>滚到底部</button>
                <button id="copyRuntimeLogButton" class="secondary" type="button" onclick="copyRuntimeLog()" disabled>复制日志</button>
              </div>
            </div>
            <textarea id="runtimeLog" class="log log-textarea muted" readonly spellcheck="false">请选择左侧任务后查看日志。</textarea>
          </div>
          <div class="detail-card">
            <h4>状态流转与活动</h4>
            <div id="activityLog" class="log flow-log muted">暂无活动。</div>
          </div>
        </div>
      </div>
    </section>
  </main>
  <aside id="runningTasksWindow" class="floating-runtime-window"></aside>
  <script>
    let board = { current_user: null, users: [], projects: [], tasks: [], agents: [], pending_questions: [] };
    let projectContext = { project_id: null, primary_workspace: null, latest_scan: null, sessions: [], chat_messages: [] };
    let selectedProjectId = null;
    let selectedTaskId = null;
    let selectedAgentIdState = null;
    let selectedProjectSessionId = null;
    let noticeState = { kind: "", message: "" };
    const workspaceDraft = { label: "", path: "", writable: true, isPrimaryDefault: true };
    const API_PREFIX = "/api/v1";
    let projectChatDraft = "";

    function statusLabel(status) {
      return {
        OPEN: "待处理",
        CLAIMED: "已认领",
        RUNNING: "运行中",
        PAUSED: "已暂停",
        DONE: "已完成",
        FAILED: "失败",
        CANCELED: "已撤销"
      }[status] || status;
    }

    function selectedProject() {
      return board.projects.find(project => project.id === selectedProjectId) || null;
    }

    function selectedProjectSession() {
      return projectContext.sessions.find(session => session.id === selectedProjectSessionId) || null;
    }

    function pendingQuestionsForCurrentProject() {
      return (board.pending_questions || []).filter(question =>
        question.project_id === selectedProjectId && question.status !== "answered"
      );
    }

    function currentUser() {
      return board.current_user || board.users[0] || null;
    }

    function userById(userId) {
      return board.users.find(user => user.id === userId) || null;
    }

    function agentById(agentId) {
      return board.agents.find(agent => agent.id === agentId) || null;
    }

    function projectById(projectId) {
      return board.projects.find(project => project.id === projectId) || null;
    }

    function taskCreatorLabel(task) {
      const creator = userById(task.creator_user_id);
      return creator ? creator.display_name : "未记录创建者";
    }

    function taskClaimLabel(task) {
      if (!task.claimed_by) {
        return "未认领";
      }

      const agent = agentById(task.claimed_by);
      if (!agent) {
        return "已认领";
      }

      const owner = userById(agent.owner_user_id);
      return owner
        ? `${agent.name} / ${owner.display_name}`
        : agent.name;
    }

    function taskPriorityLabel(priority) {
      return {
        HIGH: "高优",
        MEDIUM: "中优",
        LOW: "低优"
      }[priority] || "未标优先级";
    }

    function parseTimestamp(value) {
      const raw = Number(value);
      if (!Number.isFinite(raw) || raw <= 0) {
        return null;
      }
      if (String(Math.trunc(raw)).length >= 16) {
        return new Date(raw / 1000000);
      }
      return new Date(raw * 1000);
    }

    function formatTimestamp(value) {
      const date = parseTimestamp(value);
      return date ? date.toLocaleString() : value || "未知时间";
    }

    function relativeTime(value) {
      const date = parseTimestamp(value);
      if (!date) {
        return "未知";
      }
      const diffMs = Date.now() - date.getTime();
      const diffSeconds = Math.max(0, Math.floor(diffMs / 1000));
      if (diffSeconds < 15) return "刚刚";
      if (diffSeconds < 60) return `${diffSeconds} 秒前`;
      const diffMinutes = Math.floor(diffSeconds / 60);
      if (diffMinutes < 60) return `${diffMinutes} 分钟前`;
      const diffHours = Math.floor(diffMinutes / 60);
      if (diffHours < 24) return `${diffHours} 小时前`;
      const diffDays = Math.floor(diffHours / 24);
      return `${diffDays} 天前`;
    }

    function taskCssStatus(task) {
      return (task?.status || "").toLowerCase();
    }

    function taskRuntimeEntries(task) {
      return task?.runtime?.log || [];
    }

    function taskLastRuntimeEntry(task) {
      const entries = taskRuntimeEntries(task);
      return entries.length ? entries[entries.length - 1] : null;
    }

    function taskCurrentAgent(task) {
      return task?.claimed_by ? agentById(task.claimed_by) : null;
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
      if (["error", "stderr"].includes(kind)) return "error";
      if (["watchdog", "system"].includes(kind)) return "warn";
      if (["command", "plan"].includes(kind)) return kind;
      return "assistant";
    }

    function activityTone(kind) {
      if (!kind) return "";
      if (kind.includes("error") || kind.includes("failed")) return "error";
      if (kind.includes("watchdog") || kind.includes("pause") || kind.includes("canceled")) return "warn";
      if (kind.includes("auto") || kind.includes("retry") || kind.includes("resume")) return "auto";
      return "";
    }

    function lastInterestingActivity(task) {
      const activities = [...(task?.activities || [])].reverse();
      return activities.find(item =>
        !["task.created", "task.auto_claim_reason"].includes(item.kind)
      ) || activities[0] || null;
    }

    function taskExecutionPulse(task) {
      const lastEntry = taskLastRuntimeEntry(task);
      if (lastEntry) {
        return `${relativeTime(lastEntry.at)} 有新输出`;
      }
      const lastActivity = lastInterestingActivity(task);
      if (lastActivity) {
        return `${relativeTime(lastActivity.at)} 有状态变更`;
      }
      return "还没有执行痕迹";
    }

    function taskFlowEntries(task) {
      const flowKinds = [
        "task.auto_claimed",
        "task.auto_started",
        "task.auto_resumed",
        "task.auto_retry_queued",
        "task.watchdog_recovered",
        "task.canceled",
        "task.pause_requested",
        "task.resumed",
        "task.resume_requested",
        "agent.invoked",
        "runtime.thread_started",
        "runtime.turn_completed",
        "runtime.error",
        "runtime.exited"
      ];
      return [...(task?.activities || [])]
        .filter(item => flowKinds.includes(item.kind))
        .slice(-12)
        .reverse();
    }

    function taskLiveEntries(task) {
      return taskRuntimeEntries(task)
        .filter(item => ["assistant", "command", "plan", "stderr", "error", "watchdog", "system"].includes(item.kind))
        .slice(-10)
        .reverse();
    }

    function visibleActiveTasks() {
      const statusRank = { RUNNING: 0, CLAIMED: 1, PAUSED: 2 };
      return board.tasks
        .filter(task => ["RUNNING", "CLAIMED", "PAUSED"].includes(task.status))
        .slice()
        .sort((left, right) => {
          const statusDelta = (statusRank[left.status] ?? 99) - (statusRank[right.status] ?? 99);
          if (statusDelta !== 0) {
            return statusDelta;
          }

          const leftPulse = parseTimestamp(taskLastRuntimeEntry(left)?.at)
            || parseTimestamp(lastInterestingActivity(left)?.at);
          const rightPulse = parseTimestamp(taskLastRuntimeEntry(right)?.at)
            || parseTimestamp(lastInterestingActivity(right)?.at);
          return rightPulse - leftPulse;
        });
    }

    function tasksForCurrentProject() {
      return board.tasks.filter(task => task.project_id === selectedProjectId);
    }

    function preferredProjectId() {
      const activeTask = board.tasks.find(task =>
        ["RUNNING", "CLAIMED", "PAUSED"].includes(task.status)
      );
      if (activeTask) {
        return activeTask.project_id;
      }

      return board.projects.find(project => !project.is_spotlight_self)?.id || board.projects[0]?.id || null;
    }

    function apiUrl(url) {
      return url.startsWith("/api/") ? url.replace(/^\/api/, API_PREFIX) : url;
    }

    async function request(url, options = {}) {
      const response = await fetch(apiUrl(url), {
        headers: { "Content-Type": "application/json" },
        ...options
      });
      if (!response.ok) {
        const text = await response.text();
        throw new Error(text || "请求失败");
      }
      const type = response.headers.get("content-type") || "";
      return type.includes("application/json") ? response.json() : response.text();
    }

    async function loadBoard() {
      try {
        board = await request("/api/board");
        if (!selectedProjectId || !board.projects.some(project => project.id === selectedProjectId)) {
          selectedProjectId = preferredProjectId();
        }
        const currentTasks = tasksForCurrentProject();
        if (!selectedTaskId || !currentTasks.some(task => task.id === selectedTaskId)) {
          selectedTaskId = currentTasks[0]?.id || null;
        }
        if (!selectedAgentIdState || !board.agents.some(agent => agent.id === selectedAgentIdState)) {
          selectedAgentIdState = board.agents[0]?.id || null;
        }
        await loadProjectContext(selectedProjectId);
        render();
      } catch (error) {
        console.error(error);
        setNotice("error", error.message || "加载看板失败");
      }
    }

    async function loadProjectContext(projectId) {
      if (!projectId) {
        projectContext = { project_id: null, primary_workspace: null, latest_scan: null, sessions: [], chat_messages: [] };
        selectedProjectSessionId = null;
        return;
      }

      try {
        projectContext = await request(`/api/projects/${projectId}/context`);
        if (!selectedProjectSessionId || !projectContext.sessions.some(session => session.id === selectedProjectSessionId)) {
          selectedProjectSessionId = projectContext.sessions[0]?.id || null;
        }
      } catch (error) {
        console.error(error);
        projectContext = { project_id: projectId, primary_workspace: null, latest_scan: null, sessions: [], chat_messages: [] };
        selectedProjectSessionId = null;
        setNotice("error", error.message || "加载项目上下文失败");
      }
    }

    function setNotice(kind, message) {
      noticeState = {
        kind: kind || "warn",
        message: String(message || "").trim()
      };
      renderNotice();
    }

    function clearNotice() {
      noticeState = { kind: "", message: "" };
      renderNotice();
    }

    function renderNotice() {
      const root = document.getElementById("noticeBanner");
      if (!root) return;
      if (!noticeState.message) {
        root.style.display = "none";
        root.className = "notice-banner";
        root.textContent = "";
        return;
      }
      root.style.display = "block";
      root.className = `notice-banner ${noticeState.kind || "warn"}`;
      root.textContent = noticeState.message;
    }

    async function loginSelectedUser() {
      const select = document.getElementById("userSelect");
      const username = select?.value;
      if (!username) return;
      try {
        await request("/api/auth/login", {
          method: "POST",
          body: JSON.stringify({ username })
        });
        await loadBoard();
      } catch (error) {
        setNotice("error", error.message || "切换用户失败");
      }
    }

    async function registerWorkspaceRoot() {
      if (!selectedProjectId) return;
      const path = projectPathDraftValue();
      if (!path) return;
      try {
        await request(`/api/projects/${selectedProjectId}/workspaces`, {
          method: "POST",
          body: JSON.stringify({
            label: workspaceDraft.label,
            path,
            isPrimaryDefault: workspaceDraft.isPrimaryDefault,
            isWritable: workspaceDraft.writable
          })
        });
        workspaceDraft.label = "";
        clearNotice();
        await loadBoard();
      } catch (error) {
        setNotice("error", error.message || "接入目录失败");
      }
    }

    async function scanCurrentProject() {
      if (!selectedProjectId) return;
      try {
        await request(`/api/projects/${selectedProjectId}/scan`, { method: "POST" });
        clearNotice();
        await loadProjectContext(selectedProjectId);
        render();
      } catch (error) {
        setNotice("error", error.message || "扫描项目失败");
      }
    }

    async function startProjectSession() {
      if (!selectedProjectId) return;
      const prompt = window.prompt("请输入想发给 Agent 的项目问题：");
      if (!prompt || !prompt.trim()) return;
      const title = window.prompt("请输入本轮会话标题（可选）：", "") || "";
      try {
        await request(`/api/projects/${selectedProjectId}/sessions`, {
          method: "POST",
          body: JSON.stringify({
            title: title.trim() || null,
            prompt: prompt.trim()
          })
        });
        clearNotice();
        await loadProjectContext(selectedProjectId);
        render();
      } catch (error) {
        setNotice("error", error.message || "发起项目问答失败");
      }
    }

    async function continueProjectSession() {
      const session = selectedProjectSession();
      if (!session) return;
      const prompt = window.prompt("请输入继续追问的内容：");
      if (!prompt || !prompt.trim()) return;
      try {
        await request(`/api/project-sessions/${session.id}/turns`, {
          method: "POST",
          body: JSON.stringify({ prompt: prompt.trim() })
        });
        clearNotice();
        await loadProjectContext(selectedProjectId);
        render();
      } catch (error) {
        setNotice("error", error.message || "继续项目问答失败");
      }
    }

    async function sendProjectChatMessage() {
      if (!selectedProjectId) return;
      const message = projectChatDraft.trim();
      if (!message) return;
      try {
        projectContext = await request(`/api/projects/${selectedProjectId}/chat`, {
          method: "POST",
          body: JSON.stringify({ message })
        });
        projectChatDraft = "";
        clearNotice();
        render();
      } catch (error) {
        setNotice("error", error.message || "发送项目聊天消息失败");
      }
    }

    async function answerPendingQuestion(questionId) {
      const question = (board.pending_questions || []).find(item => item.id === questionId);
      if (!question) return;
      const answer = window.prompt(`请统一记录对这个问题的回答：\n\n${question.question}`);
      if (!answer || !answer.trim()) {
        return;
      }
      try {
        board = await request(`/api/questions/${questionId}/answer`, {
          method: "POST",
          body: JSON.stringify({ answer: answer.trim() })
        });
        clearNotice();
        render();
      } catch (error) {
        setNotice("error", error.message || "记录问题回答失败");
      }
    }

    async function createTask() {
      if (!selectedProjectId) return;
      const title = document.getElementById("title").value.trim();
      const description = document.getElementById("description").value.trim();
      if (!title || !description) return;
      await request("/api/tasks", {
        method: "POST",
        body: JSON.stringify({ project_id: selectedProjectId, title, description })
      });
      document.getElementById("title").value = "";
      document.getElementById("description").value = "";
      await loadBoard();
    }

    async function exploreProject() {
      if (!selectedProjectId) return;
      await request(`/api/projects/${selectedProjectId}/explore`, { method: "POST" });
      await loadBoard();
    }

    async function seedDocs() {
      if (!selectedProjectId) return;
      await request(`/api/projects/${selectedProjectId}/tasks/seed-docs`, { method: "POST" });
      await loadBoard();
    }

    async function bootstrapTasks() {
      if (!selectedProjectId) return;
      await request(`/api/projects/${selectedProjectId}/tasks/bootstrap`, { method: "POST" });
      await loadBoard();
    }

    async function createLocalBuildRestartTask() {
      if (!selectedProjectId) return;
      const task = await request(`/api/projects/${selectedProjectId}/tasks/local-build-restart`, {
        method: "POST"
      });
      selectedTaskId = task.id;
      await loadBoard();
    }

    async function createCloudInstallRestartTask() {
      if (!selectedProjectId) return;
      const host = window.prompt("请输入云端服务器 IP 或域名：");
      if (!host || !host.trim()) {
        return;
      }
      const username = window.prompt("请输入 SSH 用户名：", "root");
      if (!username || !username.trim()) {
        return;
      }
      const portInput = window.prompt("请输入 SSH 端口：", "22");
      if (portInput === null) {
        return;
      }
      const authMethod = window.prompt(
        "请输入认证方式（例如：SSH 证书、SSH 私钥、密码）：",
        "SSH 证书"
      );
      if (authMethod === null) {
        return;
      }
      const credentialHint = window.prompt(
        "请输入凭据说明（建议填写证书路径或凭据别名；如使用密码，建议不要在这里填写明文密码）：",
        "使用本机已配置 SSH 证书"
      );
      if (credentialHint === null) {
        return;
      }
      const deployPath = window.prompt("请输入部署目录（可选）：", "/srv/app");
      if (deployPath === null) {
        return;
      }
      const serviceHint = window.prompt("请输入服务名或重启命令（可选）：", "");
      if (serviceHint === null) {
        return;
      }

      const normalizedPort = portInput.trim() ? Number(portInput.trim()) : 22;
      if (!Number.isInteger(normalizedPort) || normalizedPort <= 0) {
        alert("SSH 端口必须是正整数。");
        return;
      }

      const task = await request(`/api/projects/${selectedProjectId}/tasks/cloud-install-restart`, {
        method: "POST",
        body: JSON.stringify({
          host: host.trim(),
          port: normalizedPort,
          username: username.trim(),
          auth_method: authMethod.trim(),
          credential_hint: credentialHint.trim(),
          deploy_path: deployPath.trim(),
          service_hint: serviceHint.trim()
        })
      });
      selectedTaskId = task.id;
      await loadBoard();
    }

    function selectedAgentId() {
      return document.getElementById("agentSelect").value;
    }

    function selectedAgentName() {
      const select = document.getElementById("agentSelect");
      return select.options[select.selectedIndex]?.dataset.name || "本地 Codex Agent";
    }

    async function claimSelected() {
      if (!selectedTaskId) return;
      await request(`/api/tasks/${selectedTaskId}/claim/${selectedAgentId()}`, { method: "POST" });
      await loadBoard();
    }

    async function startSelected() {
      if (!selectedTaskId) return;
      const prompt = document.getElementById("promptBox").value.trim();
      try {
        await request(`/api/tasks/${selectedTaskId}/start/${selectedAgentId()}`, {
          method: "POST",
          body: JSON.stringify({
            agent_name_hint: selectedAgentName(),
            prompt: prompt || null
          })
        });
        clearNotice();
        await loadBoard();
      } catch (error) {
        setNotice("error", error.message || "启动任务失败");
      }
    }

    async function pauseSelected() {
      if (!selectedTaskId) return;
      await request(`/api/tasks/${selectedTaskId}/pause`, { method: "POST" });
      await loadBoard();
    }

    async function cancelSelected() {
      if (!selectedTaskId) return;
      const reason = window.prompt("请输入撤销原因（可留空）：", "");
      if (reason === null) return;
      try {
        await request(`/api/tasks/${selectedTaskId}/cancel`, {
          method: "POST",
          body: JSON.stringify({
            reason: reason.trim() || null
          })
        });
        clearNotice();
        await loadBoard();
      } catch (error) {
        setNotice("error", error.message || "撤销任务失败");
      }
    }

    async function resumeSelected() {
      if (!selectedTaskId) return;
      const prompt = document.getElementById("promptBox").value.trim();
      if (!prompt) {
        alert("请先输入补充提示词，再恢复任务。");
        return;
      }
      await request(`/api/tasks/${selectedTaskId}/resume/${selectedAgentId()}`, {
        method: "POST",
        body: JSON.stringify({
          agent_name_hint: selectedAgentName(),
          prompt
        })
      });
      await loadBoard();
    }

    function formatRuntimeEntries(entries) {
      return entries
        .map(item => `[${item.kind}]\n${item.message}`)
        .join("\n\n");
    }

    function projectSessionLiveEntries(session) {
      return (session?.log || []).filter(item =>
        ["assistant", "command", "plan", "stderr"].includes(item.kind)
      );
    }

    function captureEditableFocus() {
      const element = document.activeElement;
      if (!element || !(element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement)) {
        return null;
      }
      if (!element.id || element.readOnly || element.disabled) {
        return null;
      }
      return {
        id: element.id,
        selectionStart: typeof element.selectionStart === "number" ? element.selectionStart : null,
        selectionEnd: typeof element.selectionEnd === "number" ? element.selectionEnd : null,
        scrollTop: element.scrollTop,
        scrollLeft: element.scrollLeft
      };
    }

    function restoreEditableFocus(focusState) {
      if (!focusState?.id) {
        return;
      }
      const element = document.getElementById(focusState.id);
      if (!element || !(element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement)) {
        return;
      }
      if (element.readOnly || element.disabled) {
        return;
      }
      element.focus({ preventScroll: true });
      if (
        typeof focusState.selectionStart === "number" &&
        typeof focusState.selectionEnd === "number" &&
        typeof element.setSelectionRange === "function"
      ) {
        element.setSelectionRange(focusState.selectionStart, focusState.selectionEnd);
      }
      if (typeof focusState.scrollTop === "number") {
        element.scrollTop = focusState.scrollTop;
      }
      if (typeof focusState.scrollLeft === "number") {
        element.scrollLeft = focusState.scrollLeft;
      }
    }

    function resizeRuntimeLog() {
      const runtimeLog = document.getElementById("runtimeLog");
      if (!runtimeLog) {
        return;
      }

      runtimeLog.style.height = "auto";
      const viewportLimit = Math.max(Math.floor(window.innerHeight * 0.52), 220);
      const nextHeight = Math.min(runtimeLog.scrollHeight, viewportLimit);
      runtimeLog.style.height = `${Math.max(nextHeight, 180)}px`;
    }

    function setRuntimeLogText(text, muted = false, copyEnabled = false) {
      const runtimeLog = document.getElementById("runtimeLog");
      const copyButton = document.getElementById("copyRuntimeLogButton");
      const scrollTopButton = document.getElementById("scrollRuntimeLogTopButton");
      const scrollBottomButton = document.getElementById("scrollRuntimeLogBottomButton");
      const feedback = document.getElementById("runtimeLogCopyFeedback");

      runtimeLog.value = text;
      runtimeLog.classList.toggle("muted", muted);
      copyButton.disabled = !copyEnabled;
      scrollTopButton.disabled = !copyEnabled;
      scrollBottomButton.disabled = !copyEnabled;
      feedback.textContent = "";
      resizeRuntimeLog();
    }

    let runtimeLogCopyFeedbackTimer = null;

    function showRuntimeLogCopyFeedback(message) {
      const feedback = document.getElementById("runtimeLogCopyFeedback");
      if (!feedback) {
        return;
      }

      feedback.textContent = message;
      if (runtimeLogCopyFeedbackTimer) {
        clearTimeout(runtimeLogCopyFeedbackTimer);
      }
      runtimeLogCopyFeedbackTimer = setTimeout(() => {
        feedback.textContent = "";
      }, 1800);
    }

    async function copyRuntimeLog() {
      const runtimeLog = document.getElementById("runtimeLog");
      const text = runtimeLog?.value?.trim() || "";

      if (!text) {
        showRuntimeLogCopyFeedback("当前没有可复制的日志。");
        return;
      }

      try {
        await navigator.clipboard.writeText(text);
        showRuntimeLogCopyFeedback("日志已复制到剪贴板。");
      } catch (error) {
        runtimeLog.focus();
        runtimeLog.select();
        const copied = document.execCommand && document.execCommand("copy");
        showRuntimeLogCopyFeedback(copied ? "日志已复制到剪贴板。" : "复制失败，请手动选择文本。");
      }
    }

    function scrollRuntimeLogTop() {
      const runtimeLog = document.getElementById("runtimeLog");
      if (!runtimeLog) return;
      runtimeLog.scrollTop = 0;
    }

    function scrollRuntimeLogBottom() {
      const runtimeLog = document.getElementById("runtimeLog");
      if (!runtimeLog) return;
      runtimeLog.scrollTop = runtimeLog.scrollHeight;
    }

    async function toggleSelectedAgentAutoMode() {
      const agentId = selectedAgentId();
      if (!agentId) return;
      await request(`/api/agents/${agentId}/auto-mode/toggle`, { method: "POST" });
      await loadBoard();
    }

    async function toggleAgentAutoMode(agentId) {
      await request(`/api/agents/${agentId}/auto-mode/toggle`, { method: "POST" });
      await loadBoard();
    }

    function renderHeaderAuth() {
      const current = currentUser();
      const badge = document.getElementById("currentUserBadge");
      const select = document.getElementById("userSelect");

      badge.textContent = current ? `${current.display_name} / ${current.role}` : "未登录";
      select.innerHTML = board.users.map(user => `
        <option value="${user.username}" ${current && user.id === current.id ? "selected" : ""}>
          ${escapeHtml(user.display_name)} / ${escapeHtml(user.role)}
        </option>
      `).join("");
    }

    function projectPathDraftValue() {
      return workspaceDraft.path.trim();
    }

    function formatUnixTime(value) {
      const seconds = Number(value);
      if (!Number.isFinite(seconds) || seconds <= 0) return value || "未知时间";
      return new Date(seconds * 1000).toLocaleString();
    }

    function sessionStatusLabel(status) {
      return {
        running: "运行中",
        completed: "已完成",
        failed: "失败",
        paused: "已暂停"
      }[status] || status || "未知";
    }

    function renderProjectContextCard() {
      const root = document.getElementById("projectContextCard");
      const project = selectedProject();
      if (!project) {
        root.innerHTML = `<div class="muted">当前还没有项目。</div>`;
        return;
      }

      const scan = projectContext.latest_scan;
      const workspace = projectContext.primary_workspace;
      const pendingQuestions = pendingQuestionsForCurrentProject();
      const scanBlock = scan ? `
        <div class="meta">
          <span class="pill">最近扫描 ${escapeHtml(formatUnixTime(scan.scanned_at))}</span>
          <span class="pill">${escapeHtml(scan.workspace_label)}</span>
        </div>
        <p class="description" style="margin:8px 0 0;">${escapeHtml(scan.stack_summary)}</p>
        <div class="meta">
          ${scan.top_level_entries.map(item => `<span class="pill">${escapeHtml(item)}</span>`).join("")}
        </div>
        <div class="muted" style="margin-top:6px;">关键文件：${escapeHtml(scan.key_files.join("、") || "未识别")}</div>
        <div class="muted" style="margin-top:4px;">文档文件：${escapeHtml(scan.document_files.join("、") || "未识别")}</div>
        <div class="muted" style="margin-top:4px;">提示：${escapeHtml(scan.notes.join("；") || "暂无")}</div>
      ` : `<div class="muted">还没有项目扫描摘要。接入目录后可以直接扫描，用于后续项目问答和任务拆解。</div>`;

      const questionsBlock = pendingQuestions.length ? `
        <div class="detail-card" style="margin-top:12px; padding:12px;">
          <div class="section-head">
            <h4>待回答问题</h4>
            <span class="pill">${pendingQuestions.length} 条</span>
          </div>
          ${pendingQuestions.map(question => `
            <article class="message" style="margin-bottom:8px;">
              <div class="message-meta">
                <strong>${escapeHtml(question.source_task_title)}</strong>
                <span>${escapeHtml(formatUnixTime(question.created_at))}</span>
              </div>
              <div class="description">${escapeHtml(question.question)}</div>
              ${question.context ? `<div class="muted" style="margin-top:6px;">上下文：${escapeHtml(question.context)}</div>` : ``}
              <div class="inline-actions" style="margin-top:8px;">
                <button class="secondary" onclick="answerPendingQuestion('${question.id}')">记录回答</button>
              </div>
            </article>
          `).join("")}
        </div>
      ` : `<div class="muted" style="margin-top:12px;">当前项目还没有待回答问题。</div>`;

      root.innerHTML = `
        <div class="muted" style="margin-bottom:8px;">
          当前主目录：${escapeHtml(workspace?.path || "尚未配置")}
        </div>
        <div class="create-box">
          <input
            placeholder="目录标签，例如 backend / docs / desktop"
            value="${escapeHtml(workspaceDraft.label)}"
            oninput="workspaceDraft.label = this.value"
          />
          <input
            placeholder="本机项目目录绝对路径"
            value="${escapeHtml(workspaceDraft.path)}"
            oninput="workspaceDraft.path = this.value"
          />
          <div class="inline-actions">
            <button class="secondary" onclick="registerWorkspaceRoot()">接入目录</button>
            <button onclick="scanCurrentProject()">扫描目录</button>
          </div>
        </div>
        ${scanBlock}
        ${questionsBlock}
      `;
    }

    function renderProjectSessionCard() {
      const root = document.getElementById("projectSessionCard");
      const session = selectedProjectSession();
      const chatMessages = projectContext.chat_messages || [];
      const sessionOptions = projectContext.sessions.map(item => `
        <option value="${item.id}" ${item.id === selectedProjectSessionId ? "selected" : ""}>
          ${escapeHtml(item.title)} / ${escapeHtml(sessionStatusLabel(item.status))}
        </option>
      `).join("");

      const conversation = session
        ? session.messages.map(message => `
            <article class="message ${message.role === "user" ? "user" : "assistant"}">
              <div class="message-meta">
                <strong>${escapeHtml(message.role === "user" ? "你" : "Agent")}</strong>
                <span>${escapeHtml(formatUnixTime(message.at))}</span>
              </div>
              <div class="description">${escapeHtml(message.content)}</div>
            </article>
          `).join("")
        : `<div class="muted">还没有项目会话。你可以直接提问项目结构、文档位置、构建入口或下一步改动建议。</div>`;
      const liveOutput = session ? formatRuntimeEntries(projectSessionLiveEntries(session)) : "";
      const projectChat = chatMessages.length
        ? chatMessages.map(message => `
            <article class="message ${message.user_id === currentUser()?.id ? "user" : "assistant"}">
              <div class="message-meta">
                <strong>${escapeHtml(message.user_display_name)}</strong>
                <span>${escapeHtml(formatUnixTime(message.at))}</span>
              </div>
              <div class="description">${escapeHtml(message.content)}</div>
            </article>
          `).join("")
        : `<div class="muted">当前项目还没有聊天室消息。你可以先发一条，后续这里也会承接项目内的人和 Agent 协作记录。</div>`;

      root.innerHTML = `
        <div class="section-head">
          <h4>项目聊天室</h4>
          <span class="pill">${chatMessages.length} 条</span>
        </div>
        <div class="conversation">${projectChat}</div>
        <div class="create-box" style="margin-top:12px;">
          <textarea
            id="projectChatInput"
            placeholder="在当前项目聊天室里发消息，例如：这个任务先别做；先看桌面端；我来补文档。"
            oninput="projectChatDraft = this.value"
          >${escapeHtml(projectChatDraft)}</textarea>
          <div class="inline-actions">
            <button class="secondary" onclick="sendProjectChatMessage()">发送聊天消息</button>
          </div>
        </div>
        <div class="section-head" style="margin-top:16px;">
          <h4>Agent 工作会话</h4>
          <span class="pill">${projectContext.sessions.length} 轮</span>
        </div>
        ${projectContext.sessions.length ? `
          <select class="session-select" onchange="changeProjectSession(this.value)">
            ${sessionOptions}
          </select>
        ` : ``}
        <div class="inline-actions" style="margin-bottom:12px;">
          <button onclick="startProjectSession()">发起项目问答</button>
          <button class="secondary" onclick="continueProjectSession()" ${session ? "" : "disabled"}>
            继续追问
          </button>
        </div>
        ${session ? `
          <div class="meta" style="margin-bottom:8px;">
            <span class="pill">${escapeHtml(sessionStatusLabel(session.status))}</span>
            <span class="pill">${escapeHtml(session.workspace_path || "未绑定目录")}</span>
            ${session.last_error ? `<span class="pill">${escapeHtml(session.last_error)}</span>` : ``}
          </div>
        ` : ``}
        <div class="conversation">${conversation}</div>
        ${session ? `
          <div class="section-head" style="margin-top:12px;">
            <h4>Agent 工作流</h4>
          </div>
          <div class="log ${liveOutput ? "" : "muted"}">${escapeHtml(liveOutput || "这里展示同一个 Agent 会话的实时工作流，包括回答增量、计划、命令输出和错误信息。")}</div>
        ` : ``}
      `;
    }

    function renderProjectCard() {
      const project = selectedProject();
      const root = document.getElementById("projectCard");
      if (!project) {
        root.innerHTML = `<div class="muted">当前还没有项目。</div>`;
        return;
      }

      const workspaceLines = project.workspace_roots.map(workspace => `
        <div class="meta">
          <span class="pill">${escapeHtml(workspace.label)}</span>
          <span class="pill">${escapeHtml(workspace.writable ? "可写" : "只读")}</span>
        </div>
        <div class="muted" style="margin-top:4px;">${escapeHtml(workspace.path)}</div>
      `).join("");

      root.innerHTML = `
        <h3>当前项目</h3>
        <select onchange="changeProject(this.value)">
          ${board.projects.map(projectItem => `
            <option value="${projectItem.id}" ${projectItem.id === selectedProjectId ? "selected" : ""}>
              ${escapeHtml(projectItem.name)}
            </option>
          `).join("")}
        </select>
        <p class="description" style="margin:10px 0 0;">${escapeHtml(project.description)}</p>
        <div style="margin-top:10px;">${workspaceLines}</div>
        <div class="toolbar" style="margin-top:12px;">
          <button onclick="exploreProject()">探索目录</button>
          ${project.is_spotlight_self ? `
            <button class="secondary" onclick="bootstrapTasks()">导入 AGENTS 任务</button>
            <button class="secondary" onclick="seedDocs()">从文档补种任务</button>
          ` : ``}
        </div>
        <div class="toolbar" style="margin-top:8px;">
          <button class="secondary" onclick="createLocalBuildRestartTask()">本地编译重启</button>
          <button class="secondary" onclick="createCloudInstallRestartTask()">云端安装重启</button>
        </div>
        <div class="muted" style="margin-top:10px;">
          ${project.is_spotlight_self
            ? "这是 Spotlight 自举项目，会自动从文档中生成版本任务；也可以直接生成本地编译或云端部署重启任务。"
            : "这个项目目录可以是空的，也可以只有文档；点击“探索目录”会创建探索任务，也可以直接生成本地编译或云端部署重启任务。"}
        </div>
      `;
    }

    function renderSummary() {
      const tasks = tasksForCurrentProject();
      const pendingQuestions = pendingQuestionsForCurrentProject();
      const counts = {
        total: tasks.length,
        open: tasks.filter(task => task.status === "OPEN").length,
        active: tasks.filter(task => ["CLAIMED", "RUNNING", "PAUSED"].includes(task.status)).length,
        done: tasks.filter(task => task.status === "DONE").length,
        canceled: tasks.filter(task => task.status === "CANCELED").length,
        questions: pendingQuestions.length
      };
      document.getElementById("summary").innerHTML = `
        <div class="summary-box"><strong>${counts.total}</strong><span>任务总数</span></div>
        <div class="summary-box"><strong>${counts.open}</strong><span>待处理</span></div>
        <div class="summary-box"><strong>${counts.active}</strong><span>处理中</span></div>
        <div class="summary-box"><strong>${counts.done}</strong><span>已完成</span></div>
        <div class="summary-box"><strong>${counts.canceled}</strong><span>已撤销</span></div>
        <div class="summary-box"><strong>${counts.questions}</strong><span>待回答问题</span></div>
      `;
    }

    function renderTaskList() {
      const tasks = tasksForCurrentProject();
      const root = document.getElementById("tasks");
      if (!tasks.length) {
        root.innerHTML = `<div class="detail-card muted">当前项目还没有任务，可以手动创建，或先点击“探索目录”。</div>`;
        return;
      }
      root.innerHTML = tasks.map(task => `
        <article class="task-item ${taskCssStatus(task)} ${task.id === selectedTaskId ? "active" : ""}" onclick="selectTask('${task.id}')">
          <h3>${escapeHtml(task.title)}</h3>
          <div class="muted">${escapeHtml(task.description.slice(0, 72))}${task.description.length > 72 ? "..." : ""}</div>
          <div class="meta">
            <span class="pill">${statusLabel(task.status)}</span>
            <span class="pill">${escapeHtml(taskPriorityLabel(task.priority))}</span>
            <span class="pill">创建者 ${escapeHtml(taskCreatorLabel(task))}</span>
            <span class="pill">${escapeHtml(taskClaimLabel(task))}</span>
          </div>
          <div class="task-item-summary">
            <div>最近脉冲：${escapeHtml(taskExecutionPulse(task))}</div>
            ${lastInterestingActivity(task) ? `<div>最近状态：${escapeHtml(lastInterestingActivity(task).message)}</div>` : ``}
          </div>
        </article>
      `).join("");
    }

    function renderDetail() {
      const task = tasksForCurrentProject().find(item => item.id === selectedTaskId);
      const taskDetail = document.getElementById("taskDetail");
      const taskExecutionOverview = document.getElementById("taskExecutionOverview");
      const runtimeStream = document.getElementById("runtimeStream");
      const activityLog = document.getElementById("activityLog");

      if (!task) {
        taskDetail.innerHTML = `<div class="muted">当前项目还没有选中的任务。</div>`;
        taskExecutionOverview.innerHTML = `<div class="muted">请选择任务后查看执行状态。</div>`;
        runtimeStream.innerHTML = "暂无输出。";
        runtimeStream.className = "stream-feed muted";
        setRuntimeLogText("暂无日志。", true, false);
        activityLog.className = "log flow-log muted";
        activityLog.textContent = "暂无活动。";
        return;
      }

      const runtimeEntries = taskRuntimeEntries(task);
      const liveEntries = taskLiveEntries(task);
      const flowEntries = taskFlowEntries(task);
      const currentAgent = taskCurrentAgent(task);
      const lastEntry = taskLastRuntimeEntry(task);
      const lastActivity = lastInterestingActivity(task);
      const lastError = task.runtime?.last_error;

      taskDetail.innerHTML = `
        <h3>${escapeHtml(task.title)}</h3>
        <div class="meta">
          <span class="pill">${statusLabel(task.status)}</span>
          <span class="pill">${escapeHtml(taskPriorityLabel(task.priority))}</span>
          <span class="pill">任务 ID ${task.id.slice(0, 8)}</span>
          <span class="pill">创建者 ${escapeHtml(taskCreatorLabel(task))}</span>
          <span class="pill">认领 ${escapeHtml(taskClaimLabel(task))}</span>
          <span class="pill">${task.runtime?.thread_id ? "长会话已建立" : "尚未建立长会话"}</span>
        </div>
        <p class="description">${escapeHtml(task.description)}</p>
      `;

      taskExecutionOverview.innerHTML = `
        <div class="task-insight-grid">
          <div class="insight-card ${task.status === "FAILED" ? "error" : ["PAUSED", "CLAIMED"].includes(task.status) ? "warn" : ""}">
            <div class="muted">当前阶段</div>
            <strong>${escapeHtml(statusLabel(task.status))}</strong>
            <div class="muted">${escapeHtml(currentAgent ? `${currentAgent.name} / ${currentAgent.status}` : "当前还没有 Agent 接手")}</div>
          </div>
          <div class="insight-card ${lastError ? "error" : ""}">
            <div class="muted">最新脉冲</div>
            <strong>${escapeHtml(taskExecutionPulse(task))}</strong>
            <div class="muted">${escapeHtml(lastEntry ? runtimeEntryLabel(lastEntry.kind) : lastActivity?.kind || "暂无")}</div>
          </div>
          <div class="insight-card ${task.runtime?.active_turn_id ? "" : "warn"}">
            <div class="muted">长会话状态</div>
            <strong>${escapeHtml(task.runtime?.active_turn_id ? "当前 turn 活跃" : task.runtime?.thread_id ? "线程已建，等待继续" : "尚未建立")}</strong>
            <div class="muted">${escapeHtml(task.runtime?.thread_id || "无 thread_id")}</div>
          </div>
          <div class="insight-card ${lastError ? "error" : flowEntries.some(item => item.kind === "task.watchdog_recovered") ? "warn" : ""}">
            <div class="muted">失败 / 回收 / 重试</div>
            <strong>${escapeHtml(lastError || "当前无错误")}</strong>
            <div class="muted">${escapeHtml(flowEntries[0]?.message || "当前没有特殊流转")}</div>
          </div>
        </div>
      `;

      runtimeStream.className = `stream-feed ${liveEntries.length ? "" : "muted"}`;
      runtimeStream.innerHTML = liveEntries.length
        ? liveEntries.map(item => `
            <div class="stream-entry ${runtimeEntryTone(item.kind)}">
              <div class="entry-head">
                <span class="entry-title">${escapeHtml(runtimeEntryLabel(item.kind))}</span>
                <span>${escapeHtml(formatTimestamp(item.at))}</span>
              </div>
              <div class="entry-body">${escapeHtml(item.message)}</div>
            </div>
          `).join("")
        : "当前还没有实时输出。任务一旦开始产生回答、命令或错误，会优先显示在这里。";

      setRuntimeLogText(
        runtimeEntries.length ? formatRuntimeEntries(runtimeEntries) : "当前任务还没有会话日志。",
        !runtimeEntries.length,
        runtimeEntries.length > 0
      );

      activityLog.innerHTML = flowEntries.length
        ? flowEntries.map(item => `
            <div class="flow-entry ${activityTone(item.kind)}">
              <div class="entry-head">
                <span class="entry-title">${escapeHtml(item.kind)}</span>
                <span>${escapeHtml(formatTimestamp(item.at))}</span>
              </div>
              <div class="entry-body">${escapeHtml(item.message)}</div>
            </div>
          `).join("")
        : "暂无活动。";
      activityLog.className = `log flow-log ${flowEntries.length ? "" : "muted"}`;
    }

    function renderRunningTasksWindow() {
      const root = document.getElementById("runningTasksWindow");
      if (!root) return;
      const activeTasks = visibleActiveTasks();
      root.innerHTML = `
        <div class="section-head">
          <h4>运行任务窗口</h4>
          <span class="pill">${activeTasks.length} 条</span>
        </div>
        <div class="muted">这里单独显示运行中、已认领、已暂停待恢复的任务。点一下可直接跳到详情。</div>
        <div class="running-window-list">
          ${activeTasks.length ? activeTasks.map(task => `
            <article class="running-window-item ${task.id === selectedTaskId ? "active" : ""}" onclick="selectTask('${task.id}')">
              <strong>${escapeHtml(task.title)}</strong>
              <div class="meta">
                <span class="pill">${statusLabel(task.status)}</span>
                <span class="pill">${escapeHtml(taskPriorityLabel(task.priority))}</span>
                <span class="pill">${escapeHtml(projectById(task.project_id)?.name || "未知项目")}</span>
              </div>
              <div class="task-item-summary">
                <div>${escapeHtml(taskClaimLabel(task))}</div>
                <div>最近脉冲：${escapeHtml(taskExecutionPulse(task))}</div>
              </div>
            </article>
          `).join("") : `<div class="muted">当前没有运行中的任务，系统会从等待队列自动接下一条。</div>`}
        </div>
      `;
    }

    function renderAgents() {
      const select = document.getElementById("agentSelect");
      select.innerHTML = board.agents.map(agent => `
        <option value="${agent.id}" data-name="${escapeHtml(agent.name)}" ${agent.id === selectedAgentIdState ? "selected" : ""}>
          ${escapeHtml(agent.name)} / ${escapeHtml(agent.status)}
        </option>
      `).join("");
      select.onchange = () => {
        selectedAgentIdState = select.value;
      };

      const root = document.getElementById("agents");
      root.innerHTML = board.agents.map(agent => `
        <article class="agent-card">
          <strong>${escapeHtml(agent.name)}</strong>
          <div class="meta">
            <span class="pill">${escapeHtml(agent.provider)}</span>
            <span class="pill">${escapeHtml(agent.status)}</span>
            <span class="pill">自动认领 ${agent.auto_mode ? "开" : "关"}</span>
          </div>
          <div class="muted" style="margin-top:6px;">${escapeHtml(agent.last_action)}</div>
          <div class="agent-actions">
            <button class="secondary" onclick="toggleAgentAutoMode('${agent.id}')">
              ${agent.auto_mode ? "关闭自动认领" : "开启自动认领"}
            </button>
          </div>
        </article>
      `).join("");
    }

    function render() {
      const focusState = captureEditableFocus();
      renderNotice();
      renderHeaderAuth();
      renderProjectContextCard();
      renderProjectSessionCard();
      renderProjectCard();
      renderSummary();
      renderTaskList();
      renderAgents();
      renderDetail();
      renderRunningTasksWindow();
      restoreEditableFocus(focusState);
    }

    async function changeProject(projectId) {
      selectedProjectId = projectId;
      const tasks = tasksForCurrentProject();
      selectedTaskId = tasks[0]?.id || null;
      await loadProjectContext(projectId);
      render();
    }

    function changeProjectSession(sessionId) {
      selectedProjectSessionId = sessionId;
      render();
    }

    function selectTask(taskId) {
      const task = board.tasks.find(item => item.id === taskId);
      if (task) {
        selectedProjectId = task.project_id;
      }
      selectedTaskId = taskId;
      render();
    }

    function escapeHtml(value) {
      return String(value || "")
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll('"', "&quot;")
        .replaceAll("'", "&#39;");
    }

    loadBoard();
    setInterval(loadBoard, 2500);
    window.addEventListener("resize", resizeRuntimeLog);
  </script>
</body>
</html>
"#;

#[cfg(test)]
mod layout_regression_tests {
    use super::INDEX_HTML;

    #[test]
    fn project_chat_layout_and_default_project_hooks_are_present() {
        assert!(INDEX_HTML.contains("preferredProjectId()"));
        assert!(INDEX_HTML.contains("sendProjectChatMessage()"));
        assert!(INDEX_HTML.contains("captureEditableFocus()"));
        assert!(INDEX_HTML.contains("restoreEditableFocus(focusState)"));
        assert!(INDEX_HTML.contains("id=\"projectChatInput\""));
        assert!(INDEX_HTML.contains("发起项目问答"));
    }
}

#[cfg(test)]
mod tests {
    use super::INDEX_HTML;

    #[test]
    fn runtime_log_uses_copyable_textarea() {
        assert!(INDEX_HTML.contains("id=\"copyRuntimeLogButton\""));
        assert!(INDEX_HTML.contains("onclick=\"copyRuntimeLog()\""));
        assert!(INDEX_HTML.contains("id=\"scrollRuntimeLogTopButton\""));
        assert!(INDEX_HTML.contains("onclick=\"scrollRuntimeLogTop()\""));
        assert!(INDEX_HTML.contains("id=\"scrollRuntimeLogBottomButton\""));
        assert!(INDEX_HTML.contains("onclick=\"scrollRuntimeLogBottom()\""));
        assert!(INDEX_HTML.contains("<textarea id=\"runtimeLog\""));
        assert!(INDEX_HTML.contains("readonly spellcheck=\"false\""));
        assert!(INDEX_HTML.contains("document.execCommand(\"copy\")"));
    }

    #[test]
    fn runtime_log_resizes_with_viewport_changes() {
        assert!(INDEX_HTML.contains("function resizeRuntimeLog()"));
        assert!(INDEX_HTML.contains("window.addEventListener(\"resize\", resizeRuntimeLog)"));
        assert!(INDEX_HTML.contains("overflow-wrap: anywhere;"));
    }

    #[test]
    fn project_card_includes_local_and_cloud_restart_actions() {
        assert!(INDEX_HTML.contains("createLocalBuildRestartTask()"));
        assert!(INDEX_HTML.contains("createCloudInstallRestartTask()"));
        assert!(INDEX_HTML.contains("本地编译重启"));
        assert!(INDEX_HTML.contains("云端安装重启"));
        assert!(INDEX_HTML.contains("证书路径或凭据别名"));
    }

    #[test]
    fn project_context_and_session_panels_are_present() {
        assert!(INDEX_HTML.contains("id=\"projectContextCard\""));
        assert!(INDEX_HTML.contains("id=\"projectSessionCard\""));
        assert!(INDEX_HTML.contains("id=\"noticeBanner\""));
        assert!(INDEX_HTML.contains("preferredProjectId()"));
        assert!(INDEX_HTML.contains("registerWorkspaceRoot()"));
        assert!(INDEX_HTML.contains("scanCurrentProject()"));
        assert!(INDEX_HTML.contains("startProjectSession()"));
        assert!(INDEX_HTML.contains("continueProjectSession()"));
        assert!(INDEX_HTML.contains("sendProjectChatMessage()"));
        assert!(INDEX_HTML.contains("answerPendingQuestion("));
        assert!(INDEX_HTML.contains("setNotice("));
        assert!(INDEX_HTML.contains("projectSessionLiveEntries(session)"));
    }

    #[test]
    fn unified_entry_page_contains_project_task_and_agent_sections() {
        assert!(INDEX_HTML.contains("当前项目"));
        assert!(INDEX_HTML.contains("任务看板"));
        assert!(INDEX_HTML.contains("Agent 面板"));
        assert!(INDEX_HTML.contains("const API_PREFIX = \"/api/v1\""));
        assert!(INDEX_HTML.contains("function apiUrl(url)"));
    }

    #[test]
    fn task_cancel_action_is_present() {
        assert!(INDEX_HTML.contains("cancelSelected()"));
        assert!(INDEX_HTML.contains("撤销任务"));
    }

    #[test]
    fn runtime_visibility_panels_are_present() {
        assert!(INDEX_HTML.contains("id=\"taskExecutionOverview\""));
        assert!(INDEX_HTML.contains("id=\"runtimeStream\""));
        assert!(INDEX_HTML.contains("id=\"runningTasksWindow\""));
        assert!(INDEX_HTML.contains("renderRunningTasksWindow()"));
        assert!(INDEX_HTML.contains("状态流转与活动"));
        assert!(INDEX_HTML.contains("运行任务窗口"));
    }
}
