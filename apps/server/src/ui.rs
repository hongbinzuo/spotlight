pub const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Spotlight 自举控制台</title>
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
      grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
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
    .summary-box small {
      display: block;
      margin-top: 6px;
      color: var(--muted);
      font-size: 11px;
      line-height: 1.4;
    }
    .summary-box.good {
      border-color: rgba(42, 138, 92, 0.34);
      background: #f3fbf6;
    }
    .summary-box.warn {
      border-color: rgba(208, 124, 50, 0.34);
      background: #fff7ec;
    }
    .summary-box.error {
      border-color: rgba(183, 58, 58, 0.3);
      background: #fff1ef;
    }
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
    .stack-flow {
      display: grid;
      gap: 12px;
    }
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
    .task-action-card {
      border: 1px solid var(--border);
      border-radius: 18px;
      background: #fffdf9;
      padding: 14px;
      margin-bottom: 12px;
    }
    .fold-panel {
      border: 1px solid var(--border);
      border-radius: 18px;
      background: #fffdf9;
      overflow: hidden;
    }
    .fold-panel summary {
      list-style: none;
      cursor: pointer;
      padding: 14px 16px;
      font-weight: 700;
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
      background: rgba(201, 123, 40, 0.06);
    }
    .fold-panel summary::-webkit-details-marker {
      display: none;
    }
    .fold-panel summary::after {
      content: "展开";
      font-size: 12px;
      color: var(--muted);
      font-weight: 600;
    }
    .fold-panel[open] summary::after {
      content: "收起";
    }
    .fold-body {
      padding: 14px 16px 16px;
      border-top: 1px solid rgba(0,0,0,0.06);
    }
    .fold-body > :last-child {
      margin-bottom: 0;
    }
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
    .detail-search {
      min-width: 220px;
      max-width: 320px;
    }
    .detail-mode-hint {
      margin: -2px 0 2px;
      font-size: 12px;
      line-height: 1.5;
    }
    .view-hidden {
      display: none !important;
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
    .status-reason {
      margin-top: 6px;
      color: #5f5246;
      font-size: 12px;
      line-height: 1.5;
    }
    .pill.attention {
      background: #fff0ed;
      border-color: rgba(183, 58, 58, 0.28);
      color: #8f2d2d;
    }
    .state-panel {
      margin-top: 10px;
      border: 1px solid var(--border);
      border-radius: 14px;
      background: #fffaf3;
      padding: 10px 12px;
    }
    .state-panel.warn {
      background: #fff4e8;
      border-color: rgba(208, 124, 50, 0.34);
    }
    .state-panel.error {
      background: #fff0ed;
      border-color: rgba(183, 58, 58, 0.28);
    }
    .evidence-list {
      display: flex;
      flex-wrap: wrap;
      gap: 6px;
      margin-top: 8px;
    }
    .evidence-pill {
      border-radius: 999px;
      border: 1px solid var(--border);
      padding: 3px 8px;
      background: #fffdf9;
      color: var(--muted);
      font-size: 11px;
      line-height: 1.4;
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
    .flow-entry.good {
      background: #f3fbf6;
      border-color: rgba(42, 138, 92, 0.28);
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
    .output-summary {
      border: 1px solid var(--border);
      border-radius: 16px;
      background: #fffdf9;
      padding: 12px;
      margin-bottom: 10px;
    }
    .output-summary.live {
      background: #f3fbf6;
      border-color: rgba(42, 138, 92, 0.32);
    }
    .output-summary.warn {
      background: #fff7ec;
      border-color: rgba(208, 124, 50, 0.32);
    }
    .output-summary.error {
      background: #fff0ed;
      border-color: rgba(183, 58, 58, 0.28);
    }
    .output-summary strong {
      display: block;
      margin-bottom: 6px;
      font-size: 16px;
    }
    .search-result-hint {
      margin-top: 8px;
      font-size: 12px;
      color: var(--muted);
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
        <h1>Spotlight 自举控制台</h1>
        <p>以任务为中心的统一入口。左侧聚焦任务队列，右侧承载 Agent、项目上下文、恢复和执行追踪。</p>
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
        <div id="decisionInbox" class="task-action-card" style="display:none;"></div>
        <div id="summary" class="summary"></div>
        <div id="taskActionPanel" class="task-action-card"></div>
        <div class="section-head" style="margin-bottom:10px;">
          <h4>任务列表</h4>
          <div class="inline-actions">
            <select id="taskStatusFilter" onchange="changeTaskStatusFilter(this.value)" style="min-width:150px;">
              <option value="ALL">全部状态</option>
              <option value="OPEN">待处理</option>
              <option value="CLAIMED">已认领</option>
              <option value="APPROVAL_REQUESTED">待审批</option>
              <option value="APPROVED">已批准</option>
              <option value="RUNNING">运行中</option>
              <option value="PAUSED">已暂停</option>
              <option value="PENDING_ACCEPTANCE">待验收</option>
              <option value="ACCEPTED">已验收</option>
              <option value="DONE">已完成</option>
              <option value="FAILED">失败</option>
              <option value="MANUAL_REVIEW">人工复核</option>
              <option value="CANCELED">已撤销</option>
            </select>
          </div>
        </div>
        <div id="taskListHint" class="muted" style="margin-bottom:10px;"></div>
        <div id="tasks" class="task-list"></div>
        <details class="fold-panel">
          <summary>新增任务与补充输入</summary>
          <div class="fold-body">
            <div class="create-box" style="margin-bottom:0;">
              <input id="title" placeholder="任务标题" />
              <textarea id="description" placeholder="请输入任务描述、范围、上下文，或者你想补充给 Agent 的说明"></textarea>
              <button onclick="createTask()">新增任务</button>
            </div>
          </div>
        </details>
      </div>
    </section>
    <section class="panel">
      <div class="panel-header">
        <h2>Agent 面板</h2>
        <div class="toolbar">
          <input
            id="detailSearchInput"
            class="detail-search"
            placeholder="搜索当前任务输出 / 活动"
            oninput="updateDetailSearch(this.value)"
          />
          <button id="simpleModeButton" class="secondary" onclick="setDetailMode('simple')">简洁模式</button>
          <button id="diagnosticModeButton" class="secondary" onclick="setDetailMode('diagnostic')">诊断模式</button>
          <button class="secondary" onclick="loadBoard()">刷新状态</button>
        </div>
      </div>
      <div class="panel-body">
        <div class="detail-layout">
          <div id="detailModeHint" class="detail-mode-hint muted"></div>
          <div class="stack-flow">
            <div class="detail-card">
              <div id="taskDetail"></div>
            </div>
            <div class="detail-card">
              <h4>执行概览</h4>
              <div id="taskExecutionOverview" class="muted">请选择任务后查看执行状态。</div>
            </div>
            <div class="detail-card">
              <h4>关键输出</h4>
              <div id="runtimeStream" class="stream-feed muted">这里会高亮显示最新的实时输出、命令结果和错误。</div>
            </div>
            <div class="detail-card">
              <h4>Agent 状态</h4>
              <div id="agents" class="agents"></div>
            </div>
            <details id="projectSessionPanel" class="fold-panel">
              <summary>项目会话与聊天室</summary>
              <div class="fold-body">
                <div id="projectSessionCard"></div>
              </div>
            </details>
            <details class="fold-panel">
              <summary>项目上下文与辅助配置</summary>
              <div class="fold-body">
                <div id="projectContextCard"></div>
              </div>
            </details>
            <details id="runtimeLogPanel" class="fold-panel diagnostic-only">
              <summary>详细日志</summary>
              <div class="fold-body">
                <div class="section-head">
                  <div class="inline-actions">
                    <span id="runtimeLogCopyFeedback" class="copy-feedback muted" aria-live="polite"></span>
                    <button id="scrollRuntimeLogTopButton" class="secondary" type="button" onclick="scrollRuntimeLogTop()" disabled>滚到顶部</button>
                    <button id="scrollRuntimeLogBottomButton" class="secondary" type="button" onclick="scrollRuntimeLogBottom()" disabled>滚到底部</button>
                    <button id="copyRuntimeLogButton" class="secondary" type="button" onclick="copyRuntimeLog()" disabled>复制日志</button>
                  </div>
                </div>
                <textarea id="runtimeLog" class="log log-textarea muted" readonly spellcheck="false">请选择左侧任务后查看日志。</textarea>
              </div>
            </details>
            <details id="activityLogPanel" class="fold-panel diagnostic-only">
              <summary>状态流转与活动</summary>
              <div class="fold-body">
                <div id="activityLog" class="log flow-log muted">暂无活动。</div>
              </div>
            </details>
          </div>
        </div>
      </div>
    </section>
  </main>
  <aside id="runningTasksWindow" class="floating-runtime-window"></aside>
  <script>
    let board = { current_user: null, users: [], projects: [], tasks: [], agents: [], pending_questions: [] };
    const emptyProjectMemory = () => ({ items: [], revisions: [], tags: [], edges: [] });
    const emptyProjectContext = (projectId = null) => ({
      project_id: projectId,
      primary_workspace: null,
      latest_scan: null,
      sessions: [],
      chat_messages: [],
      memory: emptyProjectMemory()
    });
    const emptyProjectSummary = (projectId = null) => ({
      project_id: projectId,
      project_name: "",
      generated_at: null,
      primary_workspace: null,
      latest_scan: null,
      task_counts: { open: 0, claimed: 0, running: 0, paused: 0, done: 0, failed: 0, canceled: 0 },
      agent_summary: { total: 0, auto_mode_enabled: 0, busy: 0, idle: 0 },
      session_summary: { total: 0, running: 0, paused: 0, completed: 0, failed: 0 },
      open_pending_question_count: 0,
      pending_questions: [],
      active_constraints: [],
      recent_task_summaries: []
    });
    let projectContext = emptyProjectContext();
    let projectSummary = emptyProjectSummary();
    const UI_FOCUS_STORAGE_KEY = "spotlight.ui.focus.v1";
    const UI_DETAIL_MODE_STORAGE_KEY = "spotlight.ui.detail-mode.v1";
    const UI_TASK_FILTER_STORAGE_KEY = "spotlight.ui.task-status-filter.v1";
    const ALL_TASK_STATUS_FILTER = "ALL";
    const EXPECTED_FEEDBACK_CONSTRAINTS = [
      {
        stableKey: "project_constraint/user-feedback-log-visibility",
        title: "日志展示"
      },
      {
        stableKey: "project_constraint/user-feedback-versioning",
        title: "任务编号唯一"
      },
      {
        stableKey: "project_constraint/user-feedback-client-governance",
        title: "治理状态前置暴露"
      }
    ];
    const PARENT_FOCUS_MESSAGE_SOURCE = "spotlight-board-focus";
    const initialFocusState = readInitialFocusState();
    let detailMode = readStoredDetailMode();
    let detailSearchQuery = "";
    let selectedTaskStatusFilter = readStoredTaskStatusFilter();
    let selectedProjectId = initialFocusState.projectId;
    let selectedTaskId = initialFocusState.taskId;
    let selectedAgentIdState = null;
    let selectedProjectSessionId = initialFocusState.sessionId;
    let noticeState = { kind: "", message: "" };
    const workspaceDraft = { label: "", path: "", writable: true, isPrimaryDefault: true };
    const constraintDraft = { title: "", content: "" };
    const API_PREFIX = "/api/v1";
    let projectChatDraft = "";

    function normalizedId(value) {
      return typeof value === "string" && value.trim() ? value.trim() : null;
    }

    function normalizedLabel(value) {
      return typeof value === "string" && value.trim() ? value.trim() : null;
    }

    function normalizeFocusState(raw = {}) {
      return {
        projectId: normalizedId(raw.projectId),
        taskId: normalizedId(raw.taskId),
        sessionId: normalizedId(raw.sessionId),
        projectName: normalizedLabel(raw.projectName),
        taskTitle: normalizedLabel(raw.taskTitle),
        sessionTitle: normalizedLabel(raw.sessionTitle)
      };
    }

    function readStoredFocusState() {
      try {
        return normalizeFocusState(JSON.parse(window.localStorage.getItem(UI_FOCUS_STORAGE_KEY) || "null"));
      } catch (_) {
        return normalizeFocusState();
      }
    }

    function readInitialFocusState() {
      const params = new URLSearchParams(window.location.search);
      const queryFocus = normalizeFocusState({
        projectId: params.get("project_id"),
        taskId: params.get("task_id"),
        sessionId: params.get("session_id")
      });
      const storedFocus = readStoredFocusState();

      return {
        projectId: queryFocus.projectId || storedFocus.projectId,
        taskId: queryFocus.taskId || storedFocus.taskId,
        sessionId: queryFocus.sessionId || storedFocus.sessionId,
        projectName: queryFocus.projectId
          ? (queryFocus.projectId === storedFocus.projectId ? storedFocus.projectName : null)
          : storedFocus.projectName,
        taskTitle: queryFocus.taskId
          ? (queryFocus.taskId === storedFocus.taskId ? storedFocus.taskTitle : null)
          : storedFocus.taskTitle,
        sessionTitle: queryFocus.sessionId
          ? (queryFocus.sessionId === storedFocus.sessionId ? storedFocus.sessionTitle : null)
          : storedFocus.sessionTitle
      };
    }

    function currentFocusState() {
      const project = selectedProject();
      const task = selectedTask();
      const session = selectedProjectSession();
      return normalizeFocusState({
        projectId: selectedProjectId,
        taskId: selectedTaskId,
        sessionId: selectedProjectSessionId,
        projectName: project?.name,
        taskTitle: task?.title,
        sessionTitle: session?.title
      });
    }

    function hasFocusState(focus) {
      return Boolean(focus?.projectId || focus?.taskId || focus?.sessionId);
    }

    function persistFocusState() {
      const focus = currentFocusState();
      try {
        if (hasFocusState(focus)) {
          window.localStorage.setItem(UI_FOCUS_STORAGE_KEY, JSON.stringify(focus));
        } else {
          window.localStorage.removeItem(UI_FOCUS_STORAGE_KEY);
        }
      } catch (_) {
        // 忽略页面本地状态持久化失败，保持主界面可用
      }

      if (window.parent && window.parent !== window) {
        window.parent.postMessage({
          source: PARENT_FOCUS_MESSAGE_SOURCE,
          focus
        }, "*");
      }
    }

    function readStoredDetailMode() {
      try {
        const stored = window.localStorage.getItem(UI_DETAIL_MODE_STORAGE_KEY);
        return stored === "diagnostic" ? "diagnostic" : "simple";
      } catch (_) {
        return "simple";
      }
    }

    function normalizeTaskStatusFilter(value) {
      const normalized = String(value || "").trim().toUpperCase();
      return [
        ALL_TASK_STATUS_FILTER,
        "OPEN",
        "CLAIMED",
        "APPROVAL_REQUESTED",
        "APPROVED",
        "RUNNING",
        "PAUSED",
        "PENDING_ACCEPTANCE",
        "ACCEPTED",
        "DONE",
        "FAILED",
        "MANUAL_REVIEW",
        "CANCELED"
      ].includes(normalized) ? normalized : ALL_TASK_STATUS_FILTER;
    }

    function readStoredTaskStatusFilter() {
      try {
        return normalizeTaskStatusFilter(window.localStorage.getItem(UI_TASK_FILTER_STORAGE_KEY));
      } catch (_) {
        return ALL_TASK_STATUS_FILTER;
      }
    }

    function persistTaskStatusFilter() {
      try {
        window.localStorage.setItem(UI_TASK_FILTER_STORAGE_KEY, selectedTaskStatusFilter);
      } catch (_) {
        // 忽略本地筛选偏好持久化失败
      }
    }

    function persistDetailMode() {
      try {
        window.localStorage.setItem(UI_DETAIL_MODE_STORAGE_KEY, detailMode);
      } catch (_) {
        // 忽略本地偏好持久化失败
      }
    }

    function setDetailMode(mode) {
      detailMode = mode === "diagnostic" ? "diagnostic" : "simple";
      persistDetailMode();
      render();
    }

    function isDiagnosticMode() {
      return detailMode === "diagnostic";
    }

    function updateDetailSearch(value) {
      detailSearchQuery = String(value || "").trim();
      render();
    }

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

    function statusLabel(status) {
      return {
        OPEN: "待处理",
        CLAIMED: "已认领",
        RUNNING: "运行中",
        PAUSED: "已暂停",
        DONE: "已完成",
        FAILED: "失败",
        CANCELED: "已撤销",
        ACCEPTED: "已验收"
      }[status] || status || "未知状态";
    }

    function taskStatusFilterLabel(status = selectedTaskStatusFilter) {
      return {
        ALL: "全部状态",
        OPEN: "待处理",
        CLAIMED: "已认领",
        APPROVAL_REQUESTED: "待审批",
        APPROVED: "已批准",
        RUNNING: "运行中",
        PAUSED: "已暂停",
        PENDING_ACCEPTANCE: "待验收",
        ACCEPTED: "已验收",
        DONE: "已完成",
        FAILED: "失败",
        MANUAL_REVIEW: "人工复核",
        CANCELED: "已撤销"
      }[normalizeTaskStatusFilter(status)] || "全部状态";
    }

    function selectedTask() {
      return board.tasks.find(task => task.id === selectedTaskId) || null;
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

    function runtimeFragmentIsTiny(message) {
      const text = String(message || "").trim();
      return text && text.length <= 24 && !text.includes("\n");
    }

    function joinRuntimeFragments(fragments) {
      return (fragments || []).reduce((combined, fragment) => {
        const next = String(fragment || "");
        if (!next) return combined;
        if (!combined) return next;
        if (combined.endsWith("\n") || next.startsWith("\n")) {
          return `${combined}${next}`;
        }
        if (runtimeFragmentIsTiny(fragment)) {
          return `${combined}${next}`;
        }
        return `${combined}\n${next}`;
      }, "");
    }

    function groupedRuntimeEntryLabel(kinds) {
      const labels = [...new Set((kinds || []).filter(Boolean))].map(runtimeEntryLabel);
      if (!labels.length) return "未知输出";
      if (labels.length === 1) return labels[0];
      return `连续输出（${labels.join(" / ")}）`;
    }

    function groupedRuntimeEntryTone(kinds) {
      const uniqueKinds = [...new Set((kinds || []).filter(Boolean))];
      if (uniqueKinds.some(kind => ["error", "stderr"].includes(kind))) return "error";
      if (uniqueKinds.some(kind => ["watchdog", "system"].includes(kind))) return "warn";
      if (uniqueKinds.some(kind => ["command", "plan"].includes(kind))) return "command";
      return "assistant";
    }

    function groupRuntimeEntries(entries, options = {}) {
      const burstWindowMs = Number(options.burstWindowMs || 12000);
      const maxCharsPerGroup = Number(options.maxCharsPerGroup || 1200);
      const normalizedEntries = (entries || []).filter(item =>
        item && String(item.message || "").trim()
      );
      const groups = [];

      normalizedEntries.forEach(item => {
        const kind = item.kind || "system";
        const message = String(item.message || "");
        const at = item.at || null;
        const atMs = parseTimestamp(at)?.getTime() || 0;
        const lastGroup = groups[groups.length - 1];
        const shouldStartNewGroup = !lastGroup
          || (atMs && lastGroup.lastAtMs && (atMs - lastGroup.lastAtMs) > burstWindowMs)
          || lastGroup.totalChars >= maxCharsPerGroup;

        if (shouldStartNewGroup) {
          groups.push({
            at,
            atMs,
            lastAt: at,
            lastAtMs: atMs,
            totalChars: 0,
            segments: []
          });
        }

        const currentGroup = groups[groups.length - 1];
        currentGroup.lastAt = at;
        currentGroup.lastAtMs = atMs;
        currentGroup.totalChars += message.length;

        const lastSegment = currentGroup.segments[currentGroup.segments.length - 1];
        if (lastSegment && lastSegment.kind === kind) {
          lastSegment.fragments.push(message);
          return;
        }

        currentGroup.segments.push({
          kind,
          fragments: [message]
        });
      });

      return groups.map(group => {
        const kinds = group.segments.map(segment => segment.kind);
        const singleKind = [...new Set(kinds)].length === 1;
        const message = group.segments
          .map(segment => {
            const text = joinRuntimeFragments(segment.fragments);
            return singleKind ? text : `[${runtimeEntryLabel(segment.kind)}]\n${text}`;
          })
          .join("\n\n");

        return {
          kind: singleKind ? kinds[0] : "mixed",
          kinds,
          label: groupedRuntimeEntryLabel(kinds),
          tone: groupedRuntimeEntryTone(kinds),
          message,
          at: group.lastAt,
          atMs: group.lastAtMs
        };
      });
    }

    function activityTone(kind) {
      if (!kind) return "";
      if (kind.includes("error") || kind.includes("failed")) return "error";
      if (kind.includes("done") || kind.includes("completed")) return "good";
      if (kind.includes("watchdog") || kind.includes("pause") || kind.includes("canceled")) return "warn";
      if (kind.includes("lost")) return "warn";
      if (kind.includes("auto") || kind.includes("retry") || kind.includes("resume")) return "auto";
      return "";
    }

    function activityEntryLabel(kind) {
      return {
        "task.seeded": "任务已播种",
        "task.created": "任务已创建",
        "task.auto_claimed": "系统自动认领",
        "task.auto_started": "系统自动启动",
        "task.auto_resumed": "系统自动恢复",
        "task.auto_retry_queued": "系统已排入自动重试",
        "task.watchdog_recovered": "Watchdog 已回收任务",
        "task.pause_requested": "已请求暂停",
        "task.paused": "任务已暂停",
        "task.resume_requested": "已请求恢复",
        "task.resumed": "任务已恢复",
        "task.runtime_session_lost": "运行会话已丢失",
        "task.done": "任务已完成",
        "task.canceled": "任务已取消",
        "agent.invoked": "已调用 Agent",
        "runtime.thread_started": "已建立长会话",
        "runtime.turn_completed": "当前轮次已完成",
        "runtime.error": "运行时错误",
        "runtime.exited": "运行进程已退出"
      }[kind] || kind || "未命名活动";
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

    const AUTO_ACTIVITY_KINDS = [
      "task.auto_claimed",
      "task.auto_started",
      "task.auto_resumed",
      "task.auto_retry_queued",
      "task.watchdog_recovered"
    ];

    function taskStateSnapshot(task) {
      return task?.state_snapshot || {};
    }

    function taskStateReason(task) {
      return String(taskStateSnapshot(task).reason || "").trim();
    }

    function taskStateEvidence(task) {
      return Array.isArray(taskStateSnapshot(task).evidence)
        ? taskStateSnapshot(task).evidence.filter(item => typeof item === "string" && item.trim())
        : [];
    }

    function taskNeedsAttention(task) {
      return Boolean(taskStateSnapshot(task).needs_attention);
    }

    function taskHasStateSnapshot(task) {
      return Boolean(taskStateReason(task)) && taskStateEvidence(task).length > 0;
    }

    function taskStateEvaluatedAt(task) {
      return taskStateSnapshot(task).last_evaluated_at || null;
    }

    function taskStateEvaluatedBy(task) {
      return String(taskStateSnapshot(task).last_evaluated_by || "").trim();
    }

    function formatEvidenceLabel(entry) {
      if (!entry) return "无状态证据";
      if (entry.startsWith("last_activity:")) {
        const payload = entry.slice("last_activity:".length);
        const [kind, at] = payload.split("@");
        const suffix = at ? ` / ${formatTimestamp(at)}` : "";
        return `最后活动：${kind || "unknown"}${suffix}`;
      }
      if (entry.startsWith("runtime.thread_id:")) {
        return `线程上下文：${entry.slice("runtime.thread_id:".length)}`;
      }
      if (entry.startsWith("runtime.active_turn_id:")) {
        return `活跃 turn：${entry.slice("runtime.active_turn_id:".length)}`;
      }
      if (entry.startsWith("runtime.last_error:")) {
        return `最近错误：${entry.slice("runtime.last_error:".length)}`;
      }
      if (entry.startsWith("completion.summary:")) {
        return `完成摘要：${entry.slice("completion.summary:".length)}`;
      }
      if (entry === "status.completed_evidence_mismatch") {
        return "完成证据与当前状态不一致";
      }
      if (entry === "status.done_without_strong_evidence") {
        return "已完成但缺少强证据";
      }
      return entry;
    }

    function taskAutoManaged(task) {
      const currentAgent = taskCurrentAgent(task);
      if (currentAgent?.auto_mode) {
        return true;
      }
      return (task?.activities || []).some(item => AUTO_ACTIVITY_KINDS.includes(item.kind));
    }

    function taskRecoveryConsistent(task) {
      if (!taskHasStateSnapshot(task)) {
        return false;
      }
      if (task.status === "RUNNING") {
        return Boolean(task.runtime?.active_turn_id) || !taskNeedsAttention(task);
      }
      return !task.runtime?.active_turn_id;
    }

    function taskHasEvolutionEvidence(task, summaryDigestIds = new Set()) {
      if (task.status !== "DONE") {
        return false;
      }
      if (summaryDigestIds.has(task.id)) {
        return true;
      }
      return taskStateEvidence(task).some(item =>
        item.startsWith("completion.summary:")
        || item.startsWith("last_activity:task.done@")
        || item.startsWith("last_activity:task.completed@")
      );
    }

    function ratio(numerator, denominator) {
      return denominator > 0 ? numerator / denominator : 1;
    }

    function clamp01(value) {
      return Math.max(0, Math.min(1, Number(value) || 0));
    }

    function percent(value) {
      return Math.round(clamp01(value) * 100);
    }

    function governanceTone(value) {
      const score = clamp01(value);
      if (score >= 0.9) return "good";
      if (score >= 0.7) return "warn";
      return "error";
    }

    function governanceLabel(value) {
      const score = clamp01(value);
      if (score >= 0.95) return "已接近 100% 自治";
      if (score >= 0.85) return "自动化链路较稳定";
      if (score >= 0.7) return "已有主要自治能力，但仍有缺口";
      return "仍需人工盯看与补位";
    }

    function learnedFeedbackConstraintSummary(summary) {
      const learnedKeys = new Set(
        (summary?.active_constraints || [])
          .map(item => String(item?.stable_key || ""))
          .filter(key => key.startsWith("project_constraint/user-feedback-"))
      );
      const learned = EXPECTED_FEEDBACK_CONSTRAINTS.filter(item => learnedKeys.has(item.stableKey));
      const missing = EXPECTED_FEEDBACK_CONSTRAINTS.filter(item => !learnedKeys.has(item.stableKey));
      return {
        total: EXPECTED_FEEDBACK_CONSTRAINTS.length,
        learned,
        missing,
        score: ratio(learned.length, EXPECTED_FEEDBACK_CONSTRAINTS.length)
      };
    }

    function projectGovernanceMetrics(tasks, summary) {
      const currentTasks = Array.isArray(tasks) ? tasks : [];
      const executableTasks = currentTasks.filter(task => !["DONE", "CANCELED"].includes(task.status));
      const recoverableTasks = currentTasks.filter(task => ["CLAIMED", "RUNNING", "PAUSED", "FAILED"].includes(task.status));
      const doneTasks = currentTasks.filter(task => task.status === "DONE");
      const snapshotCompleteCount = currentTasks.filter(taskHasStateSnapshot).length;
      const autoManagedCount = executableTasks.filter(taskAutoManaged).length;
      const recoveryConsistentCount = recoverableTasks.filter(taskRecoveryConsistent).length;
      const summaryDigestIds = new Set(
        (summary?.recent_task_summaries || [])
          .map(item => normalizedId(item.task_id))
          .filter(Boolean)
      );
      const evolutionCount = doneTasks.filter(task => taskHasEvolutionEvidence(task, summaryDigestIds)).length;
      const attentionCount = currentTasks.filter(taskNeedsAttention).length;
      const unmanagedOpenCount = currentTasks.filter(task =>
        task.status === "OPEN" && !taskAutoManaged(task)
      ).length;

      const stateConfidence = ratio(snapshotCompleteCount, currentTasks.length);
      const autoRunCoverage = ratio(autoManagedCount, executableTasks.length);
      const recoveryCoverage = ratio(recoveryConsistentCount, recoverableTasks.length);
      const evolutionCoverage = ratio(evolutionCount, doneTasks.length);
      const feedbackLearning = learnedFeedbackConstraintSummary(summary);
      const overall = (
        stateConfidence
        + autoRunCoverage
        + recoveryCoverage
        + evolutionCoverage
        + feedbackLearning.score
      ) / 5;

      return {
        overall,
        overall_label: governanceLabel(overall),
        counts: {
          attention: attentionCount,
          unmanaged_open: unmanagedOpenCount,
          learned_feedback: feedbackLearning.learned.length,
          missing_feedback: feedbackLearning.missing.length
        },
        metrics: [
          {
            key: "state-confidence",
            title: "状态可信度",
            score: stateConfidence,
            detail: `${snapshotCompleteCount}/${currentTasks.length || 0} 个任务有状态原因和证据`
          },
          {
            key: "auto-run",
            title: "自动运行覆盖",
            score: autoRunCoverage,
            detail: `${autoManagedCount}/${executableTasks.length || 0} 个非终态任务已纳入自动推进链路`
          },
          {
            key: "auto-recovery",
            title: "自动恢复覆盖",
            score: recoveryCoverage,
            detail: `${recoveryConsistentCount}/${recoverableTasks.length || 0} 个活跃/可恢复任务状态自洽`
          },
            {
              key: "evolution",
              title: "自动进化沉淀率",
              score: evolutionCoverage,
              detail: `${evolutionCount}/${doneTasks.length || 0} 个已完成任务已有摘要或记忆沉淀`
            },
            {
              key: "feedback-learning",
              title: "反馈学习覆盖",
              score: feedbackLearning.score,
              detail: feedbackLearning.missing.length
                ? `${feedbackLearning.learned.length}/${feedbackLearning.total} 条关键用户反馈已沉淀；仍缺 ${feedbackLearning.missing.map(item => item.title).join("、")}`
                : `已沉淀 ${feedbackLearning.learned.length}/${feedbackLearning.total} 条关键用户反馈规则`
            }
          ]
        };
    }

    function taskRecoverySuggestion(task) {
      const reason = taskStateReason(task);
      if (task.status === "RUNNING") {
        return task.runtime?.active_turn_id
          ? "长会话正在运行，继续观察实时输出和活动流转即可。"
          : "标记为 RUNNING 但缺少活跃 turn，系统应继续校正，如无恢复再人工处理。";
      }
      if (task.status === "CLAIMED") {
        return task.runtime?.thread_id
          ? "已派发且保留上下文，应优先由自动恢复继续推进。"
          : "已认领但尚未建立长会话，可先观察自动启动链路。";
      }
      if (task.status === "PAUSED") {
        if (reason.includes("会话已断开") || taskStateEvidence(task).some(item => item.startsWith("runtime.thread_id:"))) {
          return "任务已有可恢复上下文，应由系统优先进行自动恢复。";
        }
        if (taskNeedsAttention(task)) {
          return "服务端已标记需人工复核，先确认状态再恢复。";
        }
        return "当前是可等待恢复的暂停状态，留意最近活动和障碍信息。";
      }
      if (task.status === "FAILED") {
        return "先看最近错误与状态证据，再决定重试、恢复还是拆出修复任务。";
      }
      if (task.status === "DONE") {
        return "先确认是否已沉淀完成摘要和后续任务，确保系统能基于历史继续进化。";
      }
      if (reason.includes("历史执行痕迹")) {
        return "任务看起来像是回退到 OPEN，应优先检查自动认领和状态回写链路。";
      }
      return "等待自动调度或按优先级进入下一轮推进。";
    }

    function searchMatches(value, query = detailSearchQuery) {
      const text = String(value || "").toLowerCase();
      const pattern = String(query || "").trim().toLowerCase();
      if (!pattern) return true;
      return text.includes(pattern);
    }

    function filterRuntimeEntries(entries, query = detailSearchQuery) {
      return (entries || []).filter(item =>
        searchMatches(item?.kind, query) || searchMatches(item?.message, query)
      );
    }

    function filterFlowEntries(entries, query = detailSearchQuery) {
      return (entries || []).filter(item =>
        searchMatches(item?.kind, query) || searchMatches(item?.message, query)
      );
    }

    function taskPrimaryOutput(task) {
      const runtimeEntries = groupRuntimeEntries(taskRuntimeEntries(task)).reverse();
      const preferredRuntime = runtimeEntries.find(item =>
        ["assistant", "error", "stderr", "command", "plan", "mixed"].includes(item.kind)
        && String(item.message || "").trim()
      );
      if (preferredRuntime) {
        return {
          source: preferredRuntime.label || runtimeEntryLabel(preferredRuntime.kind),
          message: preferredRuntime.message,
          at: preferredRuntime.at,
          kind: preferredRuntime.kind
        };
      }
      const activity = lastInterestingActivity(task);
      if (activity) {
        return {
          source: activity.kind,
          message: activity.message,
          at: activity.at,
          kind: "activity"
        };
      }
      return null;
    }

    function taskFreshness(task) {
      const latestOutputAt = parseTimestamp(taskPrimaryOutput(task)?.at);
      const latestActivityAt = parseTimestamp(lastInterestingActivity(task)?.at);
      const latestAt = latestOutputAt || latestActivityAt;
      if (!latestAt) {
        return { tone: "warn", label: "暂无最新信号", detail: "还没有输出或活动记录。" };
      }

      const ageMs = Math.max(0, Date.now() - latestAt.getTime());
      const ageMinutes = Math.floor(ageMs / 60000);
      if (ageMinutes <= 2) {
        return { tone: "live", label: "最新输出活跃", detail: `${relativeTime(Math.floor(latestAt.getTime() / 1000))} 仍有信号` };
      }
      if (ageMinutes <= 15) {
        return { tone: "live", label: "最近刚更新", detail: `${relativeTime(Math.floor(latestAt.getTime() / 1000))} 有新信号` };
      }
      if (ageMinutes <= 60) {
        return { tone: "warn", label: "输出暂时停住", detail: `最近一次信号在 ${ageMinutes} 分钟前` };
      }
      return { tone: "error", label: "长时间无新信号", detail: `最近一次信号在 ${ageMinutes} 分钟前` };
    }

    function renderDetailToolbar() {
      const searchInput = document.getElementById("detailSearchInput");
      const simpleModeButton = document.getElementById("simpleModeButton");
      const diagnosticModeButton = document.getElementById("diagnosticModeButton");
      const hint = document.getElementById("detailModeHint");
      if (searchInput) {
        searchInput.value = detailSearchQuery;
      }
      if (simpleModeButton) {
        simpleModeButton.disabled = detailMode === "simple";
      }
      if (diagnosticModeButton) {
        diagnosticModeButton.disabled = detailMode === "diagnostic";
      }
      if (hint) {
        hint.textContent = detailMode === "diagnostic"
          ? "诊断模式：显示详细日志、状态流转与项目会话；可用搜索框筛选当前任务日志。"
          : "简洁模式：默认只看状态依据、执行概览和关键输出；详细日志与活动流转已折叠到诊断模式。";
      }
    }

    function applyDetailModeLayout() {
      document.querySelectorAll(".diagnostic-only").forEach(element => {
        element.classList.toggle("view-hidden", !isDiagnosticMode());
      });
      document.querySelectorAll(".simple-secondary").forEach(element => {
        element.classList.toggle("view-hidden", !isDiagnosticMode());
      });
      const runningWindow = document.getElementById("runningTasksWindow");
      if (runningWindow) {
        runningWindow.classList.toggle("view-hidden", !isDiagnosticMode());
      }
    }

    function importantFlowActivity(item) {
      const kind = String(item?.kind || "");
      if (!kind || kind === "task.auto_claim_reason") {
        return false;
      }
      return kind.startsWith("task.") || kind.startsWith("runtime.") || kind.startsWith("agent.");
    }

    function taskFlowEntries(task) {
      return [...(task?.activities || [])]
        .filter(importantFlowActivity)
        .slice(-12)
        .reverse();
    }

    function taskLiveEntries(task) {
      return groupRuntimeEntries(
        taskRuntimeEntries(task)
          .filter(item => ["assistant", "command", "plan", "stderr", "error", "watchdog", "system"].includes(item.kind))
          .slice(-40)
      )
        .slice(-6)
        .reverse();
    }

    function visibleActiveTasks() {
      const statusRank = { RUNNING: 0, CLAIMED: 1 };
      return board.tasks
        .filter(task => ["RUNNING", "CLAIMED"].includes(task.status))
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

    function visibleRecoverableTasks() {
      return board.tasks
        .filter(task => task.status === "PAUSED")
        .slice()
        .sort((left, right) => {
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

    function taskMatchesStatusFilter(task, status = selectedTaskStatusFilter) {
      const normalized = normalizeTaskStatusFilter(status);
      return normalized === ALL_TASK_STATUS_FILTER || task?.status === normalized;
    }

    function filteredTasksForCurrentProject(status = selectedTaskStatusFilter) {
      return tasksForCurrentProject().filter(task => taskMatchesStatusFilter(task, status));
    }

    function preferredTaskFromList(tasks) {
      return tasks.find(task => ["RUNNING", "CLAIMED"].includes(task.status))
        || tasks.find(task => task.status === "PAUSED")
        || tasks[0]
        || null;
    }

    function syncSelectedTaskForCurrentProject() {
      const visibleTasks = filteredTasksForCurrentProject();
      if (visibleTasks.some(task => task.id === selectedTaskId)) {
        return;
      }
      selectedTaskId = preferredTaskFromList(visibleTasks)?.id || null;
    }

    function syncTaskStatusFilterControl() {
      const select = document.getElementById("taskStatusFilter");
      if (select) {
        select.value = selectedTaskStatusFilter;
      }
    }

    function preferredProjectId() {
      const activeTask = board.tasks.find(task => ["RUNNING", "CLAIMED"].includes(task.status));
      if (activeTask) {
        return activeTask.project_id;
      }
      const recoverableTask = board.tasks.find(task => task.status === "PAUSED");
      if (recoverableTask) {
        return recoverableTask.project_id;
      }

      return board.projects.find(project => !project.is_spotlight_self)?.id || board.projects[0]?.id || null;
    }

    function apiUrl(url) {
      return url.startsWith("/api/") ? url.replace(/^\/api/, API_PREFIX) : url;
    }

    function normalizeRequestError(error) {
      const message = String(error?.message || error || "").trim();
      if (/failed to fetch|networkerror|load failed/i.test(message)) {
        return "本地服务暂时不可用，请稍后重试。";
      }
      return message || "请求失败";
    }

    async function request(url, options = {}) {
      let response;
      try {
        response = await fetch(apiUrl(url), {
          headers: { "Content-Type": "application/json" },
          ...options
        });
      } catch (error) {
        throw new Error(normalizeRequestError(error));
      }
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
        const focusedTask = selectedTaskId
          ? board.tasks.find(task => task.id === selectedTaskId)
          : null;
        if (focusedTask) {
          selectedProjectId = focusedTask.project_id;
        }
        if (!selectedProjectId || !board.projects.some(project => project.id === selectedProjectId)) {
          selectedProjectId = preferredProjectId();
        }
        syncSelectedTaskForCurrentProject();
        if (!selectedAgentIdState || !board.agents.some(agent => agent.id === selectedAgentIdState)) {
          selectedAgentIdState = board.agents[0]?.id || null;
        }
        await loadProjectContext(selectedProjectId);
        await loadDecisions();
        persistFocusState();
        render();
      } catch (error) {
        console.error(error);
        setNotice("error", error.message || "加载看板失败");
      }
    }

    async function loadProjectContext(projectId) {
      if (!projectId) {
        projectContext = emptyProjectContext();
        projectSummary = emptyProjectSummary();
        selectedProjectSessionId = null;
        return;
      }

      try {
        projectContext = normalizedProjectContext(await request(`/api/projects/${projectId}/context`));
        projectSummary = normalizedProjectSummary(await request(`/api/projects/${projectId}/summary`));
        if (!selectedProjectSessionId || !projectContext.sessions.some(session => session.id === selectedProjectSessionId)) {
          selectedProjectSessionId = projectContext.sessions[0]?.id || null;
        }
        persistFocusState();
      } catch (error) {
        console.error(error);
        projectContext = emptyProjectContext(projectId);
        projectSummary = emptyProjectSummary(projectId);
        selectedProjectSessionId = null;
        setNotice("error", error.message || "加载项目上下文失败");
      }
    }

    async function saveProjectConstraint() {
      const projectId = selectedProjectId;
      if (!projectId) return;
      const title = constraintDraft.title.trim();
      const content = constraintDraft.content.trim();
      if (!title || !content) {
        setNotice("warn", "请先填写约束标题和内容，再沉淀到项目记忆。");
        return;
      }

      try {
        await request(`/api/projects/${projectId}/memory/constraints`, {
          method: "POST",
          body: JSON.stringify({ title, content })
        });
        constraintDraft.title = "";
        constraintDraft.content = "";
        await loadProjectContext(projectId);
        renderProjectContextCard();
        setNotice("success", "项目约束已写入版本化记忆，可用于后续任务执行。");
      } catch (error) {
        console.error(error);
        setNotice("error", error.message || "沉淀项目约束失败");
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

    async function startProjectSession(mode = "general") {
      if (!selectedProjectId) return;
      const modeLabel = sessionModeLabel(mode);
      const prompt = window.prompt(
        mode === "planner"
          ? "请输入想交给规划器的目标："
          : mode === "evaluator"
            ? "请输入想交给评估器检查的范围："
            : "请输入想发给 Agent 的项目问题："
      );
      if (!prompt || !prompt.trim()) return;
      const title = window.prompt(`请输入本轮${modeLabel}标题（可选）：`, "") || "";
      try {
        await request(`/api/projects/${selectedProjectId}/sessions`, {
          method: "POST",
          body: JSON.stringify({
            title: title.trim() || null,
            mode: mode,
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
      return document.getElementById("agentSelect")?.value || board.agents[0]?.id || "";
    }

    function selectedAgentName() {
      const select = document.getElementById("agentSelect");
      if (!select) {
        return board.agents[0]?.name || "本地 Codex Agent";
      }
      return select.options[select.selectedIndex]?.dataset.name || "本地 Codex Agent";
    }

    async function claimSelected() {
      if (!selectedTaskId) return;
      await request(`/api/tasks/${selectedTaskId}/claim/${selectedAgentId()}`, { method: "POST" });
      await loadBoard();
    }

    async function startSelected() {
      if (!selectedTaskId) return;
      const prompt = document.getElementById("promptBox")?.value.trim() || "";
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
      const prompt = document.getElementById("promptBox")?.value.trim() || "";
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

    async function reassessSelected() {
      if (!selectedTaskId) return;
      try {
        const data = await request(`/api/v1/tasks/${selectedTaskId}/reassess`, { method: "POST" });
        if (data.action === "skip") {
          alert(`无需评估：${data.skip_reason}`);
          return;
        }
        const rd = data.rule_decision || {};
        const msg = [
          `规则引擎判断：${rd.decision || "未知"}`,
          `置信度：${rd.confidence || "?"}`,
          `原因：${rd.reason || "无"}`,
          rd.resume_hint ? `恢复建议：${rd.resume_hint}` : null,
          "",
          "完整评估提示词已生成，可复制给 Agent 执行深度评估。",
          "是否将规则引擎的判断立即应用？"
        ].filter(Boolean).join("\n");
        if (confirm(msg)) {
          await applyReassessDecision(selectedTaskId, rd.decision, rd.resume_hint);
        }
      } catch (error) {
        alert("重评估失败：" + (error.message || error));
      }
    }

    async function applyReassessDecision(taskId, decision, resumeHint) {
      if (!decision) return;
      switch (decision) {
        case "DONE":
          // 用 cancel 接口的逻辑暂时替代，后续可加专用接口
          await request(`/api/tasks/${taskId}/cancel`, {
            method: "POST",
            body: JSON.stringify({ reason: "重评估判定：代码库已体现该任务成果，标记为已完成" })
          });
          break;
        case "CANCELED":
          await request(`/api/tasks/${taskId}/cancel`, {
            method: "POST",
            body: JSON.stringify({ reason: "重评估判定：任务目标已被取消或替代" })
          });
          break;
        case "RESTART":
          if (resumeHint) {
            await request(`/api/tasks/${taskId}/resume/${selectedAgentId()}`, {
              method: "POST",
              body: JSON.stringify({
                agent_name_hint: selectedAgentName(),
                prompt: resumeHint
              })
            });
          }
          break;
        case "REOPEN":
        case "MANUAL_REVIEW":
        default:
          alert(`判定为 ${decision}，需要人工处理。`);
          break;
      }
      await loadBoard();
    }

    async function reassessProject() {
      if (!selectedProjectId) return;
      try {
        const data = await request(`/api/v1/projects/${selectedProjectId}/reassess`, { method: "POST" });
        const summary = data.summary || {};
        const lines = [`需要评估：${summary.total_needs_reassess || 0} 个任务`, `已跳过：${summary.total_skipped || 0} 个任务`];
        if (data.needs_reassess?.length) {
          lines.push("", "需评估任务：");
          for (const item of data.needs_reassess) {
            const rd = item.rule_decision || {};
            lines.push(`  - [${rd.decision}] ${item.title}：${rd.reason || ""}`);
          }
        }
        alert(lines.join("\n"));
        await loadBoard();
      } catch (error) {
        alert("项目评估失败：" + (error.message || error));
      }
    }

    function formatRuntimeEntries(entries) {
      return groupRuntimeEntries(entries, { burstWindowMs: 12000, maxCharsPerGroup: 2400 })
        .map(item => `[${item.label || runtimeEntryLabel(item.kind)}]\n${item.message}`)
        .join("\n\n");
    }

    // ─── 决策收件箱 ────────────────────────────────────────────────

    let pendingDecisions = [];

    async function loadDecisions() {
      try {
        const data = await request(`/api/v1/decisions`);
        pendingDecisions = data.pending || [];
      } catch (error) {
        pendingDecisions = [];
      }
    }

    function renderDecisionInbox() {
      const root = document.getElementById("decisionInbox");
      if (!root) return;

      if (!pendingDecisions.length) {
        root.style.display = "none";
        return;
      }

      root.style.display = "";
      const urgencyIcon = { high: "\u{1f534}", medium: "\u{1f7e1}", low: "\u{1f7e2}" };
      const cards = pendingDecisions.map(d => {
        const icon = urgencyIcon[d.urgency] || "\u{2b55}";
        const rec = d.recommended
          ? `<span class="pill" style="font-size:11px;">推荐：${escapeHtml(d.recommended)} (${(d.confidence * 100).toFixed(0)}%)</span>`
          : "";
        const buttons = (d.options || []).map(opt => {
          const cls = opt.style === "success" ? "success"
            : opt.style === "warn" ? "warn"
            : opt.style === "danger" ? "warn"
            : "secondary";
          return `<button class="${cls}" onclick="resolveDecision('${d.id}','${opt.id}')" style="font-size:12px;padding:4px 10px;">${escapeHtml(opt.label)}</button>`;
        }).join(" ");

        return `<div style="padding:8px 0;border-bottom:1px solid var(--border);">
          <div style="display:flex;align-items:center;gap:6px;">
            <span>${icon}</span>
            <strong style="flex:1;">${escapeHtml(d.title)}</strong>
            ${rec}
          </div>
          <div class="muted" style="margin:4px 0;font-size:12px;">${escapeHtml(d.context || "").slice(0,120)}</div>
          <div class="inline-actions" style="margin-top:4px;">${buttons}</div>
        </div>`;
      }).join("");

      const batchBtn = pendingDecisions.length > 1
        ? `<button class="secondary" onclick="batchResolveDecisions()" style="font-size:12px;">全部按推荐处理</button>`
        : "";

      root.innerHTML = `
        <div class="section-head">
          <h4>\u{1f4e5} 决策收件箱 (${pendingDecisions.length})</h4>
          ${batchBtn}
        </div>
        ${cards}
      `;
    }

    async function resolveDecision(decisionId, chosenOption) {
      try {
        await request(`/api/v1/decisions/${decisionId}/resolve`, {
          method: "POST",
          body: JSON.stringify({ chosen_option: chosenOption })
        });
        await loadDecisions();
        await loadBoard();
      } catch (error) {
        alert("处理决策失败：" + (error.message || error));
      }
    }

    async function batchResolveDecisions() {
      const count = pendingDecisions.filter(d => d.recommended && d.confidence >= 0.7).length;
      if (!confirm(`将按推荐自动处理 ${count} 个决策（置信度 >= 70%），确认？`)) return;
      try {
        const data = await request("/api/v1/decisions/batch-resolve", {
          method: "POST",
          body: JSON.stringify({ mode: "recommended", min_confidence: 0.7 })
        });
        alert(`已处理 ${data.resolved_count} 个，跳过 ${data.skipped_count} 个`);
        await loadDecisions();
        await loadBoard();
      } catch (error) {
        alert("批量处理失败：" + (error.message || error));
      }
    }

    // ─── 原有功能 ──────────────────────────────────────────────────

    function renderDecisionInbox() {
      const root = document.getElementById("decisionInbox");
      if (!root) return;

      if (!pendingDecisions.length) {
        root.style.display = "none";
        return;
      }

      root.style.display = "";
      const urgencyIcon = { high: "\u{1f534}", medium: "\u{1f7e1}", low: "\u{1f7e2}" };
      const cards = pendingDecisions.map(d => {
        const icon = urgencyIcon[d.urgency] || "\u{2b55}";
        const recommendation = d.recommended
          ? `<span class="pill" style="font-size:11px;">推荐：${escapeHtml(d.recommended)} (${((d.confidence || 0) * 100).toFixed(0)}%)</span>`
          : "";
        const buttons = (d.options || []).map(opt => {
          const cls = opt.style === "success" ? "success"
            : opt.style === "warn" ? "warn"
            : opt.style === "danger" ? "warn"
            : "secondary";
          return `<button class="${cls}" onclick="resolveDecision('${d.id}','${opt.id}')" style="font-size:12px;padding:4px 10px;">${escapeHtml(opt.label)}</button>`;
        }).join(" ");

        return `<div style="padding:8px 0;border-bottom:1px solid var(--border);">
          <div style="display:flex;align-items:center;gap:6px;">
            <span>${icon}</span>
            <strong style="flex:1;">${escapeHtml(d.title)}</strong>
            ${recommendation}
          </div>
          <div class="muted" style="margin:4px 0;font-size:12px;white-space:pre-wrap;">${escapeHtml(d.context || "")}</div>
          <div class="inline-actions" style="margin-top:4px;">${buttons}</div>
        </div>`;
      }).join("");

      const batchButton = pendingDecisions.length > 1
        ? `<button class="secondary" onclick="batchResolveDecisions()" style="font-size:12px;">全部按推荐处理</button>`
        : "";

      root.innerHTML = `
        <div class="section-head">
          <h4>\u{1f4e5} 决策收件箱 (${pendingDecisions.length})</h4>
          ${batchButton}
        </div>
        ${cards}
      `;
    }

    async function resolveDecision(decisionId, chosenOption) {
      try {
        await request(`/api/v1/decisions/${decisionId}/resolve`, {
          method: "POST",
          body: JSON.stringify({ chosen_option: chosenOption })
        });
        await loadDecisions();
        await loadBoard();
      } catch (error) {
        alert(`处理决策失败：${error.message || error}`);
      }
    }

    async function batchResolveDecisions() {
      const count = pendingDecisions.filter(d => d.recommended && d.confidence >= 0.7).length;
      if (!confirm(`将按推荐自动处理 ${count} 个决策（置信度 >= 70%），确认？`)) return;
      try {
        const data = await request("/api/v1/decisions/batch-resolve", {
          method: "POST",
          body: JSON.stringify({ mode: "recommended", min_confidence: 0.7 })
        });
        alert(`已处理 ${data.resolved_count} 个，跳过 ${data.skipped_count} 个`);
        await loadDecisions();
        await loadBoard();
      } catch (error) {
        alert(`批量处理失败：${error.message || error}`);
      }
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

    function previewText(value, limit = 120) {
      const text = String(value || "").replace(/\s+/g, " ").trim();
      if (!text) return "暂无内容";
      if (text.length <= limit) return text;
      return `${text.slice(0, limit)}...`;
    }

    function normalizedProjectContext(raw = {}) {
      return {
        project_id: normalizedId(raw.project_id),
        primary_workspace: raw.primary_workspace || null,
        latest_scan: raw.latest_scan || null,
        sessions: Array.isArray(raw.sessions) ? raw.sessions : [],
        chat_messages: Array.isArray(raw.chat_messages) ? raw.chat_messages : [],
        memory: {
          items: Array.isArray(raw.memory?.items) ? raw.memory.items : [],
          revisions: Array.isArray(raw.memory?.revisions) ? raw.memory.revisions : [],
          tags: Array.isArray(raw.memory?.tags) ? raw.memory.tags : [],
          edges: Array.isArray(raw.memory?.edges) ? raw.memory.edges : []
        }
      };
    }

    function normalizedProjectSummary(raw = {}) {
      return {
        project_id: normalizedId(raw.project_id),
        project_name: typeof raw.project_name === "string" ? raw.project_name : "",
        generated_at: raw.generated_at || null,
        primary_workspace: raw.primary_workspace || null,
        latest_scan: raw.latest_scan || null,
        task_counts: {
          open: Number(raw.task_counts?.open || 0),
          claimed: Number(raw.task_counts?.claimed || 0),
          running: Number(raw.task_counts?.running || 0),
          paused: Number(raw.task_counts?.paused || 0),
          done: Number(raw.task_counts?.done || 0),
          failed: Number(raw.task_counts?.failed || 0),
          canceled: Number(raw.task_counts?.canceled || 0)
        },
        agent_summary: {
          total: Number(raw.agent_summary?.total || 0),
          auto_mode_enabled: Number(raw.agent_summary?.auto_mode_enabled || 0),
          busy: Number(raw.agent_summary?.busy || 0),
          idle: Number(raw.agent_summary?.idle || 0)
        },
        session_summary: {
          total: Number(raw.session_summary?.total || 0),
          running: Number(raw.session_summary?.running || 0),
          paused: Number(raw.session_summary?.paused || 0),
          completed: Number(raw.session_summary?.completed || 0),
          failed: Number(raw.session_summary?.failed || 0)
        },
        open_pending_question_count: Number(raw.open_pending_question_count || 0),
        pending_questions: Array.isArray(raw.pending_questions) ? raw.pending_questions : [],
        active_constraints: Array.isArray(raw.active_constraints) ? raw.active_constraints : [],
        recent_task_summaries: Array.isArray(raw.recent_task_summaries) ? raw.recent_task_summaries : []
      };
    }

    function projectMemorySnapshot() {
      return normalizedProjectContext(projectContext).memory;
    }

    function activeProjectConstraints(projectId) {
      if (!projectId) return [];
      const memory = projectMemorySnapshot();
      const tagName = `project/${projectId}/active-constraints`;
      const revisionsById = new Map(memory.revisions.map(revision => [revision.id, revision]));
      return memory.items
        .filter(item =>
          item.scope_kind === "project"
          && item.scope_id === projectId
          && item.memory_kind === "project_constraint"
        )
        .map(item => {
          const tag = memory.tags.find(entry => entry.memory_item_id === item.id && entry.tag === tagName);
          const revision = tag ? revisionsById.get(tag.target_revision_id) : null;
          if (!revision) return null;
          return { item, revision };
        })
          .filter(Boolean)
          .sort((left, right) => String(left.revision.title || "").localeCompare(String(right.revision.title || ""), "zh-CN"));
    }

    function feedbackConstraintsFromEntries(entries) {
      const currentEntries = Array.isArray(entries) ? entries : [];
      const learnedKeys = new Set(
        currentEntries
          .map(entry => String(entry?.item?.stable_key || ""))
          .filter(key => key.startsWith("project_constraint/user-feedback-"))
      );
      const learned = EXPECTED_FEEDBACK_CONSTRAINTS.filter(item => learnedKeys.has(item.stableKey));
      const missing = EXPECTED_FEEDBACK_CONSTRAINTS.filter(item => !learnedKeys.has(item.stableKey));
      return {
        total: EXPECTED_FEEDBACK_CONSTRAINTS.length,
        learned,
        missing
      };
    }

    function looksCorruptedText(value) {
      const text = String(value || "").trim();
      if (!text) return false;
      const questionMarks = (text.match(/\?/g) || []).length;
      return questionMarks >= Math.max(3, Math.ceil(text.length * 0.3));
    }

    function displayConstraintTitle(entry) {
      const title = String(entry?.revision?.title || "").trim();
      return looksCorruptedText(title) ? "历史导入约束（标题待修复）" : title || "未命名约束";
    }

    function displayConstraintStableKey(entry) {
      const stableKey = String(entry?.item?.stable_key || "").trim();
      return looksCorruptedText(stableKey) ? "内部键已隐藏" : stableKey || "未命名约束";
    }

    function recentProjectTaskSummaries(projectId, limit = 4) {
      if (!projectId) return [];
      const memory = projectMemorySnapshot();
      const revisionsById = new Map(memory.revisions.map(revision => [revision.id, revision]));
      const projectTaskIds = new Set(
        board.tasks
          .filter(task => task.project_id === projectId)
          .map(task => task.id)
      );
      return memory.items
        .filter(item =>
          item.scope_kind === "task"
          && projectTaskIds.has(item.scope_id)
          && item.memory_kind === "task_summary"
        )
        .map(item => {
          const tagName = `task/${item.scope_id}/latest-summary`;
          const tag = memory.tags.find(entry => entry.memory_item_id === item.id && entry.tag === tagName);
          const revision = tag ? revisionsById.get(tag.target_revision_id) : null;
          if (!revision) return null;
          return { item, revision };
        })
        .filter(Boolean)
        .sort((left, right) => Number(right.revision.created_at || 0) - Number(left.revision.created_at || 0))
        .slice(0, limit);
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
        const constraints = activeProjectConstraints(project.id);
        const feedbackLearning = feedbackConstraintsFromEntries(constraints);
        const taskSummaries = recentProjectTaskSummaries(project.id, 4);
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

        const constraintsBlock = `
          <div class="detail-card" style="margin-top:12px; padding:12px;">
            <div class="section-head">
              <h4>当前有效项目约束</h4>
              <span class="pill">${constraints.length} 条</span>
            </div>
            <div class="state-panel ${feedbackLearning.missing.length ? "warn" : ""}" style="margin:0 0 12px;">
              <div class="meta">
                <span class="pill">用户反馈学习</span>
                <span class="pill">${feedbackLearning.learned.length}/${feedbackLearning.total}</span>
              </div>
              <div class="status-reason">
                ${escapeHtml(feedbackLearning.missing.length
                  ? `已学会 ${feedbackLearning.learned.map(item => item.title).join("、") || "暂无"}；仍缺 ${feedbackLearning.missing.map(item => item.title).join("、")} 这几类长期规则。`
                  : "三类关键用户反馈规则都已沉淀到长期约束，后续 client / agent 应默认带着这些规则工作。")}
              </div>
            </div>
            <div class="create-box" style="margin-bottom:12px;">
              <input
                placeholder="约束标题，例如：保留移动端入口"
              value="${escapeHtml(constraintDraft.title)}"
              oninput="constraintDraft.title = this.value"
            />
            <textarea
              placeholder="约束内容，例如：统一入口和 API 不能只服务桌面端。"
              oninput="constraintDraft.content = this.value"
            >${escapeHtml(constraintDraft.content)}</textarea>
            <div class="inline-actions">
              <button class="secondary" onclick="saveProjectConstraint()">沉淀项目约束</button>
            </div>
          </div>
            ${constraints.length ? constraints.map(entry => `
              <article class="message" style="margin-bottom:8px;">
                <div class="message-meta">
                  <strong>${escapeHtml(displayConstraintTitle(entry))}</strong>
                  <span>${escapeHtml(formatUnixTime(entry.revision.created_at))}</span>
                </div>
                <div class="description">${escapeHtml(entry.revision.content)}</div>
                <div class="meta">
                  <span class="pill">修订 ${escapeHtml(String(entry.revision.revision_no || 1))}</span>
                  <span class="pill">${escapeHtml(displayConstraintStableKey(entry))}</span>
                </div>
              </article>
            `).join("") : `<div class="muted">当前还没有沉淀到记忆层的长期项目约束。</div>`}
        </div>
      `;

      const taskSummaryBlock = `
        <div class="detail-card" style="margin-top:12px; padding:12px;">
          <div class="section-head">
            <h4>最近任务摘要</h4>
            <span class="pill">${taskSummaries.length} 条</span>
          </div>
          ${taskSummaries.length ? taskSummaries.map(entry => `
            <article class="message" style="margin-bottom:8px;">
              <div class="message-meta">
                <strong>${escapeHtml(entry.revision.title)}</strong>
                <span>${escapeHtml(relativeTime(entry.revision.created_at))}</span>
              </div>
              <div class="description">${escapeHtml(previewText(entry.revision.content, 160))}</div>
              <div class="meta">
                <span class="pill">${escapeHtml(formatUnixTime(entry.revision.created_at))}</span>
                <span class="pill">${escapeHtml(entry.revision.source_kind || "未知来源")}</span>
              </div>
            </article>
          `).join("") : `<div class="muted">当前项目还没有沉淀任务完成摘要。</div>`}
        </div>
      `;

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
        ${constraintsBlock}
        ${taskSummaryBlock}
        ${questionsBlock}
      `;
    }

    function sessionStatusLabel(status) {
      return {
        running: "运行中",
        completed: "已完成",
        failed: "失败",
        paused: "已暂停"
      }[status] || status || "未知状态";
    }

    function sessionModeLabel(mode) {
      return {
        planner: "规划器",
        evaluator: "评估器",
        general: "普通会话"
      }[mode] || "普通会话";
    }

    function renderProjectSessionCard() {
      const root = document.getElementById("projectSessionCard");
      const session = selectedProjectSession();
      const chatMessages = projectContext.chat_messages || [];
      const sessionOptions = projectContext.sessions.map(item => `
        <option value="${item.id}" ${item.id === selectedProjectSessionId ? "selected" : ""}>
          ${escapeHtml(item.title)} / ${escapeHtml(sessionModeLabel(item.mode))} / ${escapeHtml(sessionStatusLabel(item.status))}
        </option>
      `).join("");

      const conversation = session
        ? session.messages.map(message => `
            <article class="message ${message.role === "user" ? "user" : "assistant"}">
              <div class="message-meta">
                <strong>${escapeHtml(message.role === "user" ? "你" : sessionModeLabel(session.mode))}</strong>
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
          <button onclick="startProjectSession('general')">发起项目问答</button>
          <button class="secondary" onclick="startProjectSession('planner')">启动规划器</button>
          <button class="secondary" onclick="startProjectSession('evaluator')">启动评估器</button>
          <button class="secondary" onclick="continueProjectSession()" ${session ? "" : "disabled"}>
            继续追问
          </button>
        </div>
        ${session ? `
          <div class="meta" style="margin-bottom:8px;">
            <span class="pill">${escapeHtml(sessionModeLabel(session.mode))}</span>
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
      const projectTaskCount = board.tasks.filter(task => task.project_id === project.id).length;

      root.innerHTML = `
        <h3>当前项目</h3>
        <select onchange="changeProject(this.value)">
          ${board.projects.map(projectItem => `
            <option value="${projectItem.id}" ${projectItem.id === selectedProjectId ? "selected" : ""}>
              ${escapeHtml(projectItem.name)} · ${board.tasks.filter(task => task.project_id === projectItem.id).length} 个任务
            </option>
          `).join("")}
        </select>
        <p class="description" style="margin:10px 0 0;">${escapeHtml(project.description)}</p>
        <div class="meta" style="margin-top:10px;">
          <span class="pill">任务 ${projectTaskCount}</span>
          <span class="pill">筛选 ${escapeHtml(taskStatusFilterLabel())}</span>
        </div>
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
          <button class="secondary" onclick="reassessProject()">项目全局评估</button>
        </div>
        <div class="muted" style="margin-top:10px;">
          ${project.is_spotlight_self
            ? "这是 Spotlight 自举项目，会自动从文档中生成版本任务；也可以直接生成本地编译或云端部署重启任务。"
            : "这个项目目录可以是空的，也可以只有文档；点击\u{201c}探索目录\u{201d}会创建探索任务，也可以直接生成本地编译或云端部署重启任务。"}
        </div>
      `;
    }

    function renderSummary() {
      const tasks = tasksForCurrentProject();
      const pendingQuestions = pendingQuestionsForCurrentProject();
      const summary = projectSummary?.project_id === selectedProjectId ? projectSummary : null;
      const governance = projectGovernanceMetrics(tasks, summary);
        const counts = {
          total: summary
            ? summary.task_counts.open + summary.task_counts.claimed + summary.task_counts.running + summary.task_counts.paused + summary.task_counts.done + summary.task_counts.failed + summary.task_counts.canceled
            : tasks.length,
          open: summary ? summary.task_counts.open : tasks.filter(task => task.status === "OPEN").length,
          active: summary
            ? summary.task_counts.claimed + summary.task_counts.running
            : tasks.filter(task => ["CLAIMED", "RUNNING"].includes(task.status)).length,
          recoverable: summary ? summary.task_counts.paused : tasks.filter(task => task.status === "PAUSED").length,
          done: summary ? summary.task_counts.done : tasks.filter(task => task.status === "DONE").length,
          canceled: summary ? summary.task_counts.canceled : tasks.filter(task => task.status === "CANCELED").length,
          questions: summary ? summary.open_pending_question_count : pendingQuestions.length,
        sessions: summary ? summary.session_summary.total : projectContext.sessions.length,
        agentsBusy: summary ? summary.agent_summary.busy : board.agents.filter(agent => agent.current_task_id).length,
        agentsAuto: summary ? summary.agent_summary.auto_mode_enabled : board.agents.filter(agent => agent.auto_mode).length
      };
      const governanceBoxes = governance.metrics.map(metric => `
        <div class="summary-box ${governanceTone(metric.score)}">
          <strong>${percent(metric.score)}%</strong>
          <span>${escapeHtml(metric.title)}</span>
          <small>${escapeHtml(metric.detail)}</small>
        </div>
      `).join("");
        document.getElementById("summary").innerHTML = `
          <div class="summary-box"><strong>${counts.total}</strong><span>任务总数</span></div>
          <div class="summary-box"><strong>${counts.open}</strong><span>待处理</span></div>
          <div class="summary-box"><strong>${counts.active}</strong><span>处理中</span></div>
          <div class="summary-box"><strong>${counts.recoverable}</strong><span>待恢复</span></div>
          <div class="summary-box"><strong>${counts.done}</strong><span>已完成</span></div>
          <div class="summary-box"><strong>${counts.canceled}</strong><span>已撤销</span></div>
          <div class="summary-box"><strong>${counts.questions}</strong><span>待回答问题</span></div>
        <div class="summary-box"><strong>${counts.sessions}</strong><span>项目会话</span></div>
        <div class="summary-box"><strong>${counts.agentsBusy}/${counts.agentsAuto}</strong><span>忙碌 Agent / 自动模式</span></div>
        <div class="summary-box ${governanceTone(governance.overall)}">
          <strong>${percent(governance.overall)}%</strong>
          <span>自治指数</span>
          <small>${escapeHtml(`${governance.overall_label}；待复核 ${governance.counts.attention} 个，裸 OPEN ${governance.counts.unmanaged_open} 个，已学会 ${governance.counts.learned_feedback}/3 条关键反馈规则`)}</small>
        </div>
        ${governanceBoxes}
      `;
    }

    function renderTaskActionPanel() {
      const root = document.getElementById("taskActionPanel");
      if (!root) return;

      const task = selectedTask();
      if (!task || task.project_id !== selectedProjectId) {
        root.innerHTML = `
          <h4>当前任务操作</h4>
          <div class="muted">先从下方任务列表里选择一个任务，这里再显示认领、开始、暂停和恢复操作。</div>
        `;
        return;
      }

      root.innerHTML = `
        <div class="section-head">
          <h4>当前任务操作</h4>
          <span class="pill">${escapeHtml(statusLabel(task.status))}</span>
        </div>
        <div class="description" style="margin-bottom:8px;">${escapeHtml(task.title)}</div>
        <div class="meta">
          <span class="pill">${escapeHtml(taskPriorityLabel(task.priority))}</span>
          <span class="pill">${escapeHtml(taskClaimLabel(task))}</span>
          <span class="pill">${escapeHtml(taskExecutionPulse(task))}</span>
        </div>
        <div style="display:grid; gap:8px; margin-top:10px;">
          <select id="agentSelect"></select>
          <textarea id="promptBox" placeholder="启动任务时可选填提示词；恢复暂停任务时请补充恢复提示。"></textarea>
          <div class="inline-actions">
            <button onclick="claimSelected()">认领</button>
            <button class="success" onclick="startSelected()">开始执行</button>
            <button class="warn" onclick="pauseSelected()">暂停</button>
            <button class="warn" onclick="cancelSelected()">撤销任务</button>
            <button class="secondary" onclick="resumeSelected()">补充后恢复</button>
            <button class="secondary" onclick="reassessSelected()">重新评估</button>
          </div>
          <button class="secondary" onclick="toggleSelectedAgentAutoMode()">切换当前 Agent 自动认领</button>
        </div>
      `;
    }

    function renderTaskList() {
      syncTaskStatusFilterControl();
      const project = selectedProject();
      const allTasks = tasksForCurrentProject();
      const tasks = filteredTasksForCurrentProject();
      const root = document.getElementById("tasks");
      const hint = document.getElementById("taskListHint");

      if (!project) {
        if (hint) {
          hint.textContent = "当前还没有可切换的项目。";
        }
        root.innerHTML = `<div class="detail-card muted">当前还没有项目，因此无法显示任务列表。</div>`;
        return;
      }

      if (hint) {
        const baseHint = allTasks.length
          ? `当前筛选“${taskStatusFilterLabel()}”，显示 ${tasks.length}/${allTasks.length} 个任务。`
          : "当前项目还没有任务，可以先创建任务或探索目录。";
        const emptyWorkspaceHint = projectContext.latest_scan?.top_level_entries?.length === 0
          ? " 当前项目目录看起来是空的也没关系，仍然可以先扫描目录、探索目录，或直接创建占位任务。"
          : !project.workspace_roots.length
            ? " 当前还没有接入目录；即使目录暂时为空，也可以先接入再扫描。"
            : "";
        hint.textContent = `${baseHint}${emptyWorkspaceHint}`;
      }

      if (!allTasks.length) {
        root.innerHTML = `<div class="detail-card muted">当前项目还没有任务，可以手动创建，也可以先点击\u{201c}探索目录\u{201d}。即使项目目录暂时为空，也可以先接入目录并扫描。</div>`;
        return;
      }

      if (!tasks.length) {
        root.innerHTML = `<div class="detail-card muted">当前项目共有 ${allTasks.length} 个任务，但没有符合“${escapeHtml(taskStatusFilterLabel())}”的任务。可以切换筛选，或回到“全部状态”继续查看。</div>`;
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
          <div class="status-reason">
            <span>状态依据：${escapeHtml(taskStateReason(task) || "服务端尚未生成状态快照")}</span>
            ${taskNeedsAttention(task) ? `<span class="pill attention">需复核</span>` : ``}
          </div>
          <div class="evidence-list">
            ${taskStateEvidence(task).slice(0, 2).map(item => `
              <span class="evidence-pill">${escapeHtml(formatEvidenceLabel(item))}</span>
            `).join("") || `<span class="evidence-pill">暂无证据</span>`}
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
      const primaryOutput = taskPrimaryOutput(task);
      const freshness = taskFreshness(task);
      const filteredLiveEntries = filterRuntimeEntries(liveEntries);
      const filteredRuntimeEntries = filterRuntimeEntries(runtimeEntries);
      const filteredFlowEntries = filterFlowEntries(flowEntries);
      const stateReason = taskStateReason(task) || "服务端尚未生成状态快照。";
      const stateEvidence = taskStateEvidence(task);
      const evaluatedAt = taskStateEvaluatedAt(task);
      const evaluatedBy = taskStateEvaluatedBy(task);
      const recoverySuggestion = taskRecoverySuggestion(task);

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
        <div class="state-panel ${task.status === "FAILED" ? "error" : taskNeedsAttention(task) ? "warn" : ""}">
          <div class="meta">
            <span class="pill">状态依据</span>
            ${taskNeedsAttention(task) ? `<span class="pill attention">需复核</span>` : ``}
            ${evaluatedAt ? `<span class="pill">最近评估 ${escapeHtml(formatTimestamp(evaluatedAt))}</span>` : ``}
            ${evaluatedBy ? `<span class="pill">评估器 ${escapeHtml(evaluatedBy)}</span>` : ``}
          </div>
          <div class="status-reason">${escapeHtml(stateReason)}</div>
          <div class="evidence-list">
            ${(stateEvidence.length ? stateEvidence : ["暂无证据"]).map(item => `
              <span class="evidence-pill">${escapeHtml(item === "暂无证据" ? item : formatEvidenceLabel(item))}</span>
            `).join("")}
          </div>
          <div class="muted">恢复建议：${escapeHtml(recoverySuggestion)}</div>
        </div>
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
            <div class="muted">${escapeHtml(freshness.detail)}</div>
          </div>
          <div class="insight-card ${task.runtime?.active_turn_id ? "" : "warn"}">
            <div class="muted">长会话状态</div>
            <strong>${escapeHtml(task.runtime?.active_turn_id ? "当前 turn 活跃" : task.runtime?.thread_id ? "线程已建，等待继续" : "尚未建立")}</strong>
            <div class="muted">${escapeHtml(task.runtime?.thread_id || "无 thread_id")}</div>
          </div>
          <div class="insight-card ${taskNeedsAttention(task) ? "warn" : lastError ? "error" : flowEntries.some(item => item.kind === "task.watchdog_recovered") ? "warn" : ""}">
            <div class="muted">治理状态</div>
            <strong>${escapeHtml(taskNeedsAttention(task) ? "需复核" : "状态已纳入治理")}</strong>
            <div class="muted">${escapeHtml(stateReason)}</div>
          </div>
        </div>
      `;

      runtimeStream.className = `stream-feed ${filteredLiveEntries.length || primaryOutput ? "" : "muted"}`;
      runtimeStream.innerHTML = `
        <div class="output-summary ${freshness.tone}">
          <div class="section-head">
            <h4>${escapeHtml(freshness.label)}</h4>
            <span class="pill">${escapeHtml(primaryOutput?.at ? formatTimestamp(primaryOutput.at) : "暂无时间")}</span>
          </div>
          <strong>${escapeHtml(primaryOutput ? `${primaryOutput.source}` : "当前没有关键输出")}</strong>
          <div class="description">${escapeHtml(primaryOutput?.message || "先看状态依据和恢复建议；一旦出现新的回答、命令或错误，这里会优先显示。")}</div>
          <div class="search-result-hint">
            ${escapeHtml(detailSearchQuery
              ? `搜索\u{201c}${detailSearchQuery}\u{201d}后，关键输出区下方显示 ${filteredLiveEntries.length}/${liveEntries.length} 条匹配记录。`
              : "默认只展示最近且最重要的输出；更多原始日志请切到诊断模式。")}
          </div>
        </div>
        ${filteredLiveEntries.length
          ? filteredLiveEntries.map(item => `
            <div class="stream-entry ${item.tone || runtimeEntryTone(item.kind)}">
              <div class="entry-head">
                <span class="entry-title">${escapeHtml(item.label || runtimeEntryLabel(item.kind))}</span>
                <span>${escapeHtml(formatTimestamp(item.at))}</span>
              </div>
              <div class="entry-body">${escapeHtml(item.message)}</div>
            </div>
          `).join("")
          : `<div class="muted">${escapeHtml(detailSearchQuery ? "当前搜索条件下没有匹配输出。" : "当前还没有实时输出。任务一旦开始产生回答、命令或错误，会优先显示在这里。")}</div>`}
      `;

      setRuntimeLogText(
        filteredRuntimeEntries.length ? formatRuntimeEntries(filteredRuntimeEntries) : (detailSearchQuery ? "当前搜索条件下没有匹配日志。" : "当前任务还没有会话日志。"),
        !filteredRuntimeEntries.length,
        filteredRuntimeEntries.length > 0
      );

      activityLog.innerHTML = filteredFlowEntries.length
        ? filteredFlowEntries.map(item => `
            <div class="flow-entry ${activityTone(item.kind)}">
              <div class="entry-head">
                <span class="entry-title">${escapeHtml(activityEntryLabel(item.kind))}</span>
                <span>${escapeHtml(formatTimestamp(item.at))}</span>
              </div>
              <div class="muted" style="margin-bottom:4px;">${escapeHtml(item.kind)}</div>
              <div class="entry-body">${escapeHtml(item.message)}</div>
            </div>
          `).join("")
        : (detailSearchQuery ? "当前搜索条件下没有匹配活动。" : "暂无活动。");
      activityLog.className = `log flow-log ${filteredFlowEntries.length ? "" : "muted"}`;
    }

    function renderRunningTasksWindow() {
      const root = document.getElementById("runningTasksWindow");
      if (!root) return;
      const activeTasks = visibleActiveTasks();
      const recoverableTasks = visibleRecoverableTasks();
      root.innerHTML = `
        <div class="section-head">
          <h4>执行队列窗口</h4>
          <span class="pill">处理中 ${activeTasks.length} / 待恢复 ${recoverableTasks.length}</span>
        </div>
        <div class="muted">这里只把真正运行中的任务算进处理中；暂停任务单独归到待恢复，不再混在一起造成像并行执行。</div>
        <div class="running-window-list">
          <div class="detail-card" style="padding:10px 12px;">
            <div class="section-head">
              <h4>当前处理</h4>
              <span class="pill">${activeTasks.length} 条</span>
            </div>
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
              <div class="status-reason">
                ${escapeHtml(`状态依据：${previewText(taskStateReason(task) || "等待服务端生成状态快照", 72)}`)}
              </div>
            </article>
          `).join("") : `<div class="muted">当前没有运行中的任务，系统应只保留 0 或 1 条真正处理中任务。</div>`}
          </div>
          <div class="detail-card" style="padding:10px 12px;">
            <div class="section-head">
              <h4>待恢复</h4>
              <span class="pill">${recoverableTasks.length} 条</span>
            </div>
            ${recoverableTasks.length ? recoverableTasks.map(task => `
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
                <div class="status-reason">
                  ${escapeHtml(`恢复依据：${previewText(taskStateReason(task) || "等待服务端生成状态快照", 72)}`)}
                </div>
              </article>
            `).join("") : `<div class="muted">当前没有待恢复任务。</div>`}
          </div>
        </div>
      `;
    }

    function renderAgents() {
      const select = document.getElementById("agentSelect");
      if (select) {
        select.innerHTML = board.agents.map(agent => `
          <option value="${agent.id}" data-name="${escapeHtml(agent.name)}" ${agent.id === selectedAgentIdState ? "selected" : ""}>
            ${escapeHtml(agent.name)} / ${escapeHtml(agent.status)}
          </option>
        `).join("");
        select.onchange = () => {
          selectedAgentIdState = select.value;
        };
      }

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
      renderDetailToolbar();
      renderProjectContextCard();
      renderProjectSessionCard();
      renderProjectCard();
      renderSummary();
      renderDecisionInbox();
      renderTaskActionPanel();
      renderTaskList();
      renderAgents();
      renderDetail();
      renderRunningTasksWindow();
      applyDetailModeLayout();
      restoreEditableFocus(focusState);
    }

    async function changeProject(projectId) {
      selectedProjectId = projectId;
      syncSelectedTaskForCurrentProject();
      await loadProjectContext(projectId);
      persistFocusState();
      render();
    }

    function changeTaskStatusFilter(value) {
      selectedTaskStatusFilter = normalizeTaskStatusFilter(value);
      persistTaskStatusFilter();
      syncSelectedTaskForCurrentProject();
      persistFocusState();
      render();
    }

    function changeProjectSession(sessionId) {
      selectedProjectSessionId = sessionId;
      persistFocusState();
      render();
    }

    async function selectTask(taskId) {
      const task = board.tasks.find(item => item.id === taskId);
      selectedTaskId = taskId;
      if (task) {
        const projectChanged = task.project_id !== selectedProjectId;
        selectedProjectId = task.project_id;
        if (projectChanged) {
          await loadProjectContext(selectedProjectId);
        }
      }
      persistFocusState();
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

    window.addEventListener("beforeunload", persistFocusState);
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
        assert!(INDEX_HTML.contains("readInitialFocusState()"));
        assert!(INDEX_HTML.contains("persistFocusState()"));
        assert!(INDEX_HTML.contains("spotlight-board-focus"));
        assert!(INDEX_HTML.contains("projectName"));
        assert!(INDEX_HTML.contains("taskTitle"));
        assert!(INDEX_HTML.contains("sessionTitle"));
        assert!(INDEX_HTML.contains("function selectedTask()"));
        assert!(INDEX_HTML.contains("sendProjectChatMessage()"));
        assert!(INDEX_HTML.contains("captureEditableFocus()"));
        assert!(INDEX_HTML.contains("restoreEditableFocus(focusState)"));
        assert!(INDEX_HTML.contains("id=\"projectChatInput\""));
        assert!(INDEX_HTML.contains("发起项目问答"));
    }

    #[test]
    fn agent_panel_uses_two_column_board_with_folded_secondary_sections() {
        assert!(INDEX_HTML.contains("id=\"taskActionPanel\" class=\"task-action-card\""));
        assert!(INDEX_HTML.contains("<summary>新增任务与补充输入</summary>"));
        assert!(INDEX_HTML.contains("<summary>项目会话与聊天室</summary>"));
        assert!(INDEX_HTML.contains("<summary>项目上下文与辅助配置</summary>"));
        assert!(INDEX_HTML.contains("id=\"projectSessionPanel\" class=\"fold-panel\""));
        assert!(INDEX_HTML.contains("class=\"stack-flow\""));
        assert!(!INDEX_HTML.contains("class=\"detail-workbench\""));
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
    fn runtime_output_groups_stream_fragments_before_rendering() {
        assert!(INDEX_HTML.contains("function joinRuntimeFragments(fragments)"));
        assert!(INDEX_HTML.contains("function groupRuntimeEntries(entries, options = {})"));
        assert!(INDEX_HTML.contains("连续输出（${labels.join(\" / \")}）"));
        assert!(INDEX_HTML.contains("item.label || runtimeEntryLabel(item.kind)"));
        assert!(INDEX_HTML.contains(
            "groupRuntimeEntries(entries, { burstWindowMs: 12000, maxCharsPerGroup: 2400 })"
        ));
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
        assert!(INDEX_HTML.contains("startProjectSession(mode = \"general\")"));
        assert!(INDEX_HTML.contains("continueProjectSession()"));
        assert!(INDEX_HTML.contains("sendProjectChatMessage()"));
        assert!(INDEX_HTML.contains("answerPendingQuestion("));
        assert!(INDEX_HTML.contains("saveProjectConstraint()"));
        assert!(INDEX_HTML.contains("setNotice("));
        assert!(INDEX_HTML.contains("projectSessionLiveEntries(session)"));
        assert!(INDEX_HTML.contains("function sessionModeLabel(mode)"));
        assert!(INDEX_HTML.contains("mode: mode"));
        assert!(INDEX_HTML.contains("启动规划器"));
        assert!(INDEX_HTML.contains("启动评估器"));
    }

    #[test]
    fn project_context_panel_includes_versioned_memory_sections() {
        assert!(INDEX_HTML.contains(
            "const emptyProjectMemory = () => ({ items: [], revisions: [], tags: [], edges: [] })"
        ));
        assert!(INDEX_HTML.contains("当前有效项目约束"));
        assert!(INDEX_HTML.contains("用户反馈学习"));
        assert!(INDEX_HTML.contains("最近任务摘要"));
        assert!(INDEX_HTML.contains("project/${projectId}/active-constraints"));
        assert!(INDEX_HTML.contains("task/${item.scope_id}/latest-summary"));
        assert!(INDEX_HTML.contains("/memory/constraints"));
        assert!(INDEX_HTML.contains("项目约束已写入版本化记忆"));
    }

    #[test]
    fn task_board_supports_status_filter_state_and_empty_project_hints() {
        assert!(INDEX_HTML.contains("id=\"taskStatusFilter\""));
        assert!(INDEX_HTML.contains("id=\"taskListHint\""));
        assert!(INDEX_HTML.contains("<option value=\"APPROVAL_REQUESTED\">待审批</option>"));
        assert!(INDEX_HTML.contains("<option value=\"PENDING_ACCEPTANCE\">待验收</option>"));
        assert!(INDEX_HTML.contains("<option value=\"MANUAL_REVIEW\">人工复核</option>"));
        assert!(INDEX_HTML
            .contains("const UI_TASK_FILTER_STORAGE_KEY = \"spotlight.ui.task-status-filter.v1\""));
        assert!(INDEX_HTML.contains("function normalizeTaskStatusFilter(value)"));
        assert!(INDEX_HTML.contains("APPROVAL_REQUESTED"));
        assert!(INDEX_HTML.contains("PENDING_ACCEPTANCE"));
        assert!(INDEX_HTML.contains("MANUAL_REVIEW"));
        assert!(INDEX_HTML.contains(
            "function filteredTasksForCurrentProject(status = selectedTaskStatusFilter)"
        ));
        assert!(INDEX_HTML.contains("function syncSelectedTaskForCurrentProject()"));
        assert!(INDEX_HTML.contains("function changeTaskStatusFilter(value)"));
        assert!(INDEX_HTML.contains("当前项目目录看起来是空的也没关系"));
        assert!(INDEX_HTML.contains("没有符合“${escapeHtml(taskStatusFilterLabel())}”的任务"));
    }

    #[test]
    fn unified_entry_page_uses_task_centered_title_and_summary_model() {
        assert!(!INDEX_HTML.contains("Spotlight 0.1.0"));
        assert!(INDEX_HTML.contains("<title>Spotlight 自举控制台</title>"));
        assert!(INDEX_HTML.contains("<h1>Spotlight 自举控制台</h1>"));
        assert!(INDEX_HTML.contains("normalizedProjectSummary("));
        assert!(INDEX_HTML.contains("/summary"));
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
        assert!(INDEX_HTML.contains("执行队列窗口"));
        assert!(INDEX_HTML.contains("待恢复"));
    }

    #[test]
    fn governance_metrics_and_state_reasoning_are_present() {
        assert!(INDEX_HTML.contains("function taskStateSnapshot(task)"));
        assert!(INDEX_HTML.contains("function projectGovernanceMetrics(tasks, summary)"));
        assert!(INDEX_HTML.contains("自治指数"));
        assert!(INDEX_HTML.contains("反馈学习覆盖"));
        assert!(INDEX_HTML.contains("状态依据"));
        assert!(INDEX_HTML.contains("需复核"));
        assert!(INDEX_HTML.contains("<span>处理中</span>"));
        assert!(INDEX_HTML.contains("<span>待恢复</span>"));
        assert!(INDEX_HTML.contains("setDetailMode('simple')"));
        assert!(INDEX_HTML.contains("setDetailMode('diagnostic')"));
        assert!(INDEX_HTML.contains("id=\"detailSearchInput\""));
        assert!(INDEX_HTML.contains("默认只展示最近且最重要的输出"));
        assert!(INDEX_HTML.contains("function activityEntryLabel(kind)"));
        assert!(INDEX_HTML.contains("function importantFlowActivity(item)"));
        assert!(INDEX_HTML.contains("\"task.runtime_session_lost\": \"运行会话已丢失\""));
        assert!(INDEX_HTML.contains("\"task.done\": \"任务已完成\""));
    }
}
