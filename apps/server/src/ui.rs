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
    @media (max-width: 960px) {
      main { grid-template-columns: 1fr; }
      .task-list { max-height: none; }
      .two-col { grid-template-columns: 1fr; }
    }
  </style>
</head>
<body>
  <header>
    <h1>Spotlight 0.1.0</h1>
    <p>左侧是任务看板，右侧是 Agent 面板。当前支持项目切换、自动认领、探索目录、启动任务、暂停、补充提示词后恢复。</p>
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
          <div class="detail-card">
            <div id="taskDetail"></div>
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
                <button id="copyRuntimeLogButton" class="secondary" type="button" onclick="copyRuntimeLog()" disabled>复制日志</button>
              </div>
            </div>
            <textarea id="runtimeLog" class="log log-textarea muted" readonly spellcheck="false">请选择左侧任务后查看日志。</textarea>
          </div>
          <div class="detail-card">
            <h4>活动记录</h4>
            <div id="activityLog" class="log muted">暂无活动。</div>
          </div>
        </div>
      </div>
    </section>
  </main>
  <script>
    let board = { projects: [], tasks: [], agents: [] };
    let selectedProjectId = null;
    let selectedTaskId = null;
    let selectedAgentIdState = null;

    function statusLabel(status) {
      return {
        OPEN: "待处理",
        CLAIMED: "已认领",
        RUNNING: "运行中",
        PAUSED: "已暂停",
        DONE: "已完成",
        FAILED: "失败"
      }[status] || status;
    }

    function selectedProject() {
      return board.projects.find(project => project.id === selectedProjectId) || null;
    }

    function tasksForCurrentProject() {
      return board.tasks.filter(task => task.project_id === selectedProjectId);
    }

    async function request(url, options = {}) {
      const response = await fetch(url, {
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
          selectedProjectId = board.projects.find(project => !project.is_spotlight_self)?.id || board.projects[0]?.id || null;
        }
        const currentTasks = tasksForCurrentProject();
        if (!selectedTaskId || !currentTasks.some(task => task.id === selectedTaskId)) {
          selectedTaskId = currentTasks[0]?.id || null;
        }
        if (!selectedAgentIdState || !board.agents.some(agent => agent.id === selectedAgentIdState)) {
          selectedAgentIdState = board.agents[0]?.id || null;
        }
        render();
      } catch (error) {
        console.error(error);
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
      await request(`/api/tasks/${selectedTaskId}/start/${selectedAgentId()}`, {
        method: "POST",
        body: JSON.stringify({
          agent_name_hint: selectedAgentName(),
          prompt: prompt || null
        })
      });
      await loadBoard();
    }

    async function pauseSelected() {
      if (!selectedTaskId) return;
      await request(`/api/tasks/${selectedTaskId}/pause`, { method: "POST" });
      await loadBoard();
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
      const feedback = document.getElementById("runtimeLogCopyFeedback");

      runtimeLog.value = text;
      runtimeLog.classList.toggle("muted", muted);
      copyButton.disabled = !copyEnabled;
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
      const counts = {
        total: tasks.length,
        open: tasks.filter(task => task.status === "OPEN").length,
        active: tasks.filter(task => ["CLAIMED", "RUNNING", "PAUSED"].includes(task.status)).length,
        done: tasks.filter(task => task.status === "DONE").length
      };
      document.getElementById("summary").innerHTML = `
        <div class="summary-box"><strong>${counts.total}</strong><span>任务总数</span></div>
        <div class="summary-box"><strong>${counts.open}</strong><span>待处理</span></div>
        <div class="summary-box"><strong>${counts.active}</strong><span>处理中</span></div>
        <div class="summary-box"><strong>${counts.done}</strong><span>已完成</span></div>
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
        <article class="task-item ${task.id === selectedTaskId ? "active" : ""}" onclick="selectTask('${task.id}')">
          <h3>${escapeHtml(task.title)}</h3>
          <div class="muted">${escapeHtml(task.description.slice(0, 72))}${task.description.length > 72 ? "..." : ""}</div>
          <div class="meta">
            <span class="pill">${statusLabel(task.status)}</span>
            <span class="pill">${task.claimed_by ? "已有 Agent" : "未认领"}</span>
          </div>
        </article>
      `).join("");
    }

    function renderDetail() {
      const task = tasksForCurrentProject().find(item => item.id === selectedTaskId);
      const taskDetail = document.getElementById("taskDetail");
      const activityLog = document.getElementById("activityLog");

      if (!task) {
        taskDetail.innerHTML = `<div class="muted">当前项目还没有选中的任务。</div>`;
        setRuntimeLogText("暂无日志。", true, false);
        activityLog.textContent = "暂无活动。";
        return;
      }

      taskDetail.innerHTML = `
        <h3>${escapeHtml(task.title)}</h3>
        <div class="meta">
          <span class="pill">${statusLabel(task.status)}</span>
          <span class="pill">任务 ID ${task.id.slice(0, 8)}</span>
          <span class="pill">${task.runtime?.thread_id ? "长会话已建立" : "尚未建立长会话"}</span>
        </div>
        <p class="description">${escapeHtml(task.description)}</p>
      `;

      const runtimeEntries = task.runtime?.log || [];
      setRuntimeLogText(
        runtimeEntries.length ? formatRuntimeEntries(runtimeEntries) : "当前任务还没有会话日志。",
        !runtimeEntries.length,
        runtimeEntries.length > 0
      );

      activityLog.innerHTML = task.activities.length
        ? [...task.activities].reverse().map(item => `
            <div class="log-item">
              <strong>${escapeHtml(item.kind)}</strong>
              <div>${escapeHtml(item.message)}</div>
            </div>
          `).join("")
        : "暂无活动。";
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
      renderProjectCard();
      renderSummary();
      renderTaskList();
      renderAgents();
      renderDetail();
    }

    function changeProject(projectId) {
      selectedProjectId = projectId;
      const tasks = tasksForCurrentProject();
      selectedTaskId = tasks[0]?.id || null;
      render();
    }

    function selectTask(taskId) {
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
mod tests {
    use super::INDEX_HTML;

    #[test]
    fn runtime_log_uses_copyable_textarea() {
        assert!(INDEX_HTML.contains("id=\"copyRuntimeLogButton\""));
        assert!(INDEX_HTML.contains("onclick=\"copyRuntimeLog()\""));
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
}
