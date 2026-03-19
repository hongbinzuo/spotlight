mod runtime;
mod ui;

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Json as ExtractJson, Path as AxumPath, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use platform_core::{
    merge_unique_tasks, new_activity, new_runtime_entry, seed_tasks_from_agents_markdown,
    seed_tasks_from_docs, Agent, AgentInvocationRequest, AgentResumeRequest, BoardSnapshot,
    CreateTaskRequest, Project, RuntimeLogEntry, Task, TaskRuntime, TaskStatus, User,
    WorkspaceRoot,
};
use runtime::{CodexRuntimeSession, RuntimeEvent};
use serde::{Deserialize, Serialize};
use tokio::{
    process::Command,
    sync::{mpsc, Mutex},
};
use uuid::Uuid;

type AppResult<T> = Result<T, (StatusCode, String)>;

#[derive(Clone)]
struct AppState {
    inner: Arc<Mutex<BoardState>>,
    runtime_mode: RuntimeMode,
    runtime_sessions: Arc<Mutex<HashMap<Uuid, Arc<CodexRuntimeSession>>>>,
    store_path: PathBuf,
}

#[derive(Clone)]
struct BoardState {
    users: Vec<User>,
    projects: Vec<Project>,
    tasks: Vec<Task>,
    agents: Vec<Agent>,
    project_scans: HashMap<Uuid, ProjectScanSummary>,
    project_sessions: Vec<ProjectSession>,
}

#[derive(Clone)]
struct TaskExecutionContext {
    workspace_root: PathBuf,
    prompt: String,
}

#[derive(Debug, Deserialize)]
struct CloudInstallRestartTaskRequest {
    host: String,
    port: Option<u16>,
    username: String,
    auth_method: Option<String>,
    credential_hint: Option<String>,
    deploy_path: Option<String>,
    service_hint: Option<String>,
}

#[derive(Debug, Default)]
struct StackDetection {
    stacks: Vec<&'static str>,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectScanSummary {
    project_id: Uuid,
    workspace_id: Uuid,
    workspace_label: String,
    workspace_path: String,
    scanned_at: String,
    stack_summary: String,
    detected_stacks: Vec<String>,
    top_level_entries: Vec<String>,
    key_files: Vec<String>,
    document_files: Vec<String>,
    notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectSessionMessage {
    role: String,
    content: String,
    at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectSession {
    id: Uuid,
    project_id: Uuid,
    title: String,
    status: String,
    workspace_path: Option<String>,
    thread_id: Option<String>,
    active_turn_id: Option<String>,
    messages: Vec<ProjectSessionMessage>,
    log: Vec<RuntimeLogEntry>,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
struct GitTaskBranchPlan {
    base_branch: String,
    task_branch: String,
    remote_name: Option<String>,
}

struct GitPrepareResult {
    activities: Vec<(String, String)>,
    auto_merge_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedState {
    users: Vec<User>,
    projects: Vec<Project>,
    tasks: Vec<Task>,
    agents: Vec<Agent>,
    #[serde(default)]
    project_scans: HashMap<Uuid, ProjectScanSummary>,
    #[serde(default)]
    project_sessions: Vec<ProjectSession>,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
}

#[derive(Debug, Serialize)]
struct AuthSnapshot {
    current_user: Option<User>,
    users: Vec<User>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProjectContextSnapshot {
    project_id: Uuid,
    primary_workspace: Option<WorkspaceRoot>,
    latest_scan: Option<ProjectScanSummary>,
    sessions: Vec<ProjectSession>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterWorkspaceRequest {
    label: String,
    path: String,
    is_primary_default: Option<bool>,
    is_writable: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct StartProjectSessionRequest {
    title: Option<String>,
    prompt: String,
}

#[derive(Debug, Deserialize)]
struct ContinueProjectSessionRequest {
    prompt: String,
}

#[derive(Clone, Copy)]
enum RuntimeMode {
    RealCodex,
    Stub,
}

#[tokio::main]
async fn main() {
    let workspace_root = std::env::current_dir().expect("failed to resolve current directory");
    let app = build_app(RuntimeMode::RealCodex, workspace_root);
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    println!("Spotlight server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind tcp listener");
    axum::serve(listener, app)
        .await
        .expect("failed to run axum server");
}

fn build_app(runtime_mode: RuntimeMode, workspace_root: PathBuf) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/me", get(get_me))
        .route("/api/auth/login", post(login))
        .route("/api/board", get(get_board))
        .route(
            "/api/projects/{project_id}/context",
            get(get_project_context),
        )
        .route(
            "/api/projects/{project_id}/workspaces",
            post(register_project_workspace),
        )
        .route("/api/projects/{project_id}/scan", post(scan_project))
        .route(
            "/api/projects/{project_id}/sessions",
            post(start_project_session),
        )
        .route(
            "/api/project-sessions/{session_id}/turns",
            post(continue_project_session),
        )
        .route("/api/tasks", post(create_task))
        .route(
            "/api/projects/{project_id}/tasks/bootstrap",
            post(bootstrap_tasks),
        )
        .route(
            "/api/projects/{project_id}/tasks/seed-docs",
            post(seed_doc_tasks),
        )
        .route(
            "/api/projects/{project_id}/tasks/local-build-restart",
            post(create_local_build_restart_task),
        )
        .route(
            "/api/projects/{project_id}/tasks/cloud-install-restart",
            post(create_cloud_install_restart_task),
        )
        .route("/api/projects/{project_id}/explore", post(explore_project))
        .route(
            "/api/agents/{agent_id}/auto-mode/toggle",
            post(toggle_agent_auto_mode),
        )
        .route("/api/tasks/{task_id}/claim/{agent_id}", post(claim_task))
        .route("/api/tasks/{task_id}/start/{agent_id}", post(start_task))
        .route("/api/tasks/{task_id}/pause", post(pause_task))
        .route("/api/tasks/{task_id}/resume/{agent_id}", post(resume_task))
        .with_state(default_state(runtime_mode, workspace_root))
}

fn default_state(runtime_mode: RuntimeMode, workspace_root: PathBuf) -> AppState {
    let store_path = state_store_path(&workspace_root);
    let persisted = load_or_initialize_state(&workspace_root, &store_path);

    AppState {
        inner: Arc::new(Mutex::new(BoardState {
            users: persisted.users,
            projects: persisted.projects,
            tasks: persisted.tasks,
            agents: persisted.agents,
            project_scans: persisted.project_scans,
            project_sessions: persisted.project_sessions,
        })),
        runtime_mode,
        runtime_sessions: Arc::new(Mutex::new(HashMap::new())),
        store_path,
    }
}

fn default_projects(workspace_root: &Path) -> Vec<Project> {
    let public_workspace = workspace_root.join("tmp").join("public-project-example");
    let _ = std::fs::create_dir_all(&public_workspace);

    vec![
        Project {
            id: Uuid::new_v4(),
            name: "客户项目示例".into(),
            description: "默认展示一个普通项目目录。它可能为空，也可能只有文档、Word、表格或杂项资料，不假设它一定是代码仓库。".into(),
            workspace_roots: vec![WorkspaceRoot {
                id: Uuid::new_v4(),
                label: "默认工作目录".into(),
                path: public_workspace.to_string_lossy().into_owned(),
                writable: true,
            }],
            is_spotlight_self: false,
        },
        Project {
            id: Uuid::new_v4(),
            name: "Spotlight 平台自身".into(),
            description: "用于自举 Spotlight 平台本身，默认从 docs 和 AGENTS.md 中补种任务。".into(),
            workspace_roots: vec![WorkspaceRoot {
                id: Uuid::new_v4(),
                label: "Spotlight 主工作区".into(),
                path: workspace_root.to_string_lossy().into_owned(),
                writable: true,
            }],
            is_spotlight_self: true,
        },
    ]
}

fn default_users() -> Vec<User> {
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "spotlight".into());

    vec![
        User {
            id: Uuid::new_v4(),
            username: username.clone(),
            display_name: username,
            role: "admin".into(),
        },
        User {
            id: Uuid::new_v4(),
            username: "reviewer".into(),
            display_name: "评审用户".into(),
            role: "member".into(),
        },
    ]
}

fn default_agents(users: &[User]) -> Vec<Agent> {
    let primary_user_id = users.first().map(|user| user.id);
    let reviewer_user_id = users.get(1).map(|user| user.id).or(primary_user_id);

    vec![
        Agent {
            id: Uuid::new_v4(),
            owner_user_id: primary_user_id,
            name: "本地 Codex Agent".into(),
            provider: "codex".into(),
            status: "空闲".into(),
            auto_mode: true,
            current_task_id: None,
            last_action: "等待认领或自动执行任务".into(),
        },
        Agent {
            id: Uuid::new_v4(),
            owner_user_id: reviewer_user_id,
            name: "评审助理".into(),
            provider: "codex".into(),
            status: "空闲".into(),
            auto_mode: true,
            current_task_id: None,
            last_action: "等待补充提示词、验收说明或人工协作".into(),
        },
    ]
}

fn state_store_path(workspace_root: &Path) -> PathBuf {
    #[cfg(test)]
    {
        let _ = workspace_root;
        return std::env::temp_dir().join(format!(
            "spotlight-server-state-{}.json",
            Uuid::new_v4()
        ));
    }

    #[cfg(not(test))]
    workspace_root
        .join(".spotlight")
        .join("server-state.json")
}

fn load_or_initialize_state(workspace_root: &Path, store_path: &Path) -> PersistedState {
    if let Ok(content) = std::fs::read_to_string(store_path) {
        if let Ok(state) = serde_json::from_str::<PersistedState>(&content) {
            return state;
        }
    }

    let users = default_users();
    let projects = default_projects(workspace_root);
    let mut tasks = Vec::new();
    if let Some(spotlight_project) = projects.iter().find(|project| project.is_spotlight_self) {
        merge_unique_tasks(&mut tasks, seed_tasks_from_docs(spotlight_project.id));
        merge_unique_tasks(&mut tasks, seed_tasks_from_agents_file(spotlight_project));
    }
    let primary_user_id = users.first().map(|user| user.id);
    for task in &mut tasks {
        task.creator_user_id = primary_user_id;
    }

    let state = PersistedState {
        users: users.clone(),
        projects,
        tasks,
        agents: default_agents(&users),
        project_scans: HashMap::new(),
        project_sessions: Vec::new(),
    };
    let _ = persist_state_to_path(store_path, &state);
    state
}

fn persist_state_to_path(store_path: &Path, state: &PersistedState) -> Result<(), String> {
    if let Some(parent) = store_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("创建状态目录失败：{error}"))?;
    }
    let content = serde_json::to_string_pretty(state)
        .map_err(|error| format!("序列化服务端状态失败：{error}"))?;
    std::fs::write(store_path, content).map_err(|error| format!("写入状态文件失败：{error}"))
}

fn seed_tasks_from_agents_file(project: &Project) -> Vec<Task> {
    let Some(path) = primary_workspace_path(project).ok() else {
        return Vec::new();
    };
    std::fs::read_to_string(path.join("AGENTS.md"))
        .map(|content| seed_tasks_from_agents_markdown(&content, project.id))
        .unwrap_or_default()
}

async fn index() -> Html<&'static str> {
    Html(ui::INDEX_HTML)
}

async fn get_me(State(state): State<AppState>, headers: HeaderMap) -> Json<AuthSnapshot> {
    let guard = state.inner.lock().await;
    let current_user = resolve_current_user(&guard, &headers);
    Json(AuthSnapshot {
        current_user,
        users: guard.users.clone(),
    })
}

async fn login(
    State(state): State<AppState>,
    ExtractJson(request): ExtractJson<LoginRequest>,
) -> AppResult<Response> {
    let guard = state.inner.lock().await;
    let user = guard
        .users
        .iter()
        .find(|user| user.username == request.username.trim())
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到对应用户".into()))?;
    let users = guard.users.clone();
    drop(guard);

    let cookie_value = build_auth_cookie(&user.id);
    let response = (
        [(header::SET_COOKIE, HeaderValue::from_str(&cookie_value).map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("设置登录 Cookie 失败：{error}"),
            )
        })?)],
        Json(AuthSnapshot {
            current_user: Some(user),
            users,
        }),
    )
        .into_response();
    Ok(response)
}

async fn get_board(State(state): State<AppState>, headers: HeaderMap) -> Json<BoardSnapshot> {
    let guard = state.inner.lock().await;
    Json(snapshot_from_state_with_user(
        &guard,
        resolve_current_user(&guard, &headers),
    ))
}

async fn get_project_context(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<ProjectContextSnapshot>> {
    let guard = state.inner.lock().await;
    let snapshot = project_context_snapshot(&guard, project_id)?;
    Ok(Json(snapshot))
}

async fn register_project_workspace(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<RegisterWorkspaceRequest>,
) -> AppResult<Json<Project>> {
    let path = request.path.trim();
    if path.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "项目目录路径不能为空".into()));
    }

    let mut normalized_path = PathBuf::from(path);
    if !normalized_path.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("项目目录不存在或不是文件夹：{path}"),
        ));
    }
    if let Ok(canonicalized) = normalized_path.canonicalize() {
        normalized_path = canonicalized;
    }

    let label = request.label.trim();
    let workspace_label = if label.is_empty() {
        normalized_path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("项目目录")
            .to_string()
    } else {
        label.to_string()
    };

    let mut guard = state.inner.lock().await;
    let project = find_project_mut(&mut guard, project_id)?;
    let workspace_path = normalized_path.to_string_lossy().into_owned();
    let writable = request.is_writable.unwrap_or(true);

    if let Some(existing_index) = project
        .workspace_roots
        .iter()
        .position(|workspace| workspace.path == workspace_path)
    {
        let workspace = &mut project.workspace_roots[existing_index];
        workspace.label = workspace_label;
        workspace.writable = writable;
        if request.is_primary_default.unwrap_or(false) && existing_index != 0 {
            let workspace = project.workspace_roots.remove(existing_index);
            project.workspace_roots.insert(0, workspace);
        }
    } else {
        let workspace = WorkspaceRoot {
            id: Uuid::new_v4(),
            label: workspace_label,
            path: workspace_path,
            writable,
        };
        if request.is_primary_default.unwrap_or(false) {
            project.workspace_roots.insert(0, workspace);
        } else {
            project.workspace_roots.push(workspace);
        }
    }

    let updated_project = project.clone();
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(updated_project))
}

async fn scan_project(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<ProjectContextSnapshot>> {
    let project = {
        let guard = state.inner.lock().await;
        find_project(&guard, project_id)?.clone()
    };
    let workspace = project
        .primary_workspace()
        .cloned()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "当前项目还没有配置工作目录".into()))?;
    let workspace_root = PathBuf::from(&workspace.path);
    if !workspace_root.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("当前主工作目录不存在：{}", workspace.path),
        ));
    }

    let summary = build_project_scan_summary(project_id, &workspace, &workspace_root);

    let snapshot = {
        let mut guard = state.inner.lock().await;
        guard.project_scans.insert(project_id, summary);
        project_context_snapshot(&guard, project_id)?
    };
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

async fn start_project_session(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<StartProjectSessionRequest>,
) -> AppResult<Json<ProjectContextSnapshot>> {
    let prompt = request.prompt.trim();
    if prompt.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "项目会话提问不能为空".into()));
    }

    let (project, latest_scan, workspace_root) = {
        let guard = state.inner.lock().await;
        let project = find_project(&guard, project_id)?.clone();
        let latest_scan = guard.project_scans.get(&project_id).cloned();
        let workspace_root = project
            .primary_workspace()
            .map(|workspace| PathBuf::from(&workspace.path));
        (project, latest_scan, workspace_root)
    };
    if matches!(state.runtime_mode, RuntimeMode::RealCodex) && workspace_root.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "项目会话需要先为当前项目配置主工作目录".into(),
        ));
    }

    let session_id = Uuid::new_v4();
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| truncate_title(prompt));
    let workspace_path = workspace_root
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());

    let user_message = ProjectSessionMessage {
        role: "user".into(),
        content: prompt.to_string(),
        at: timestamp_string(),
    };

    {
        let mut guard = state.inner.lock().await;
        guard.project_sessions.insert(
            0,
            ProjectSession {
                id: session_id,
                project_id,
                title,
                status: "running".into(),
                workspace_path,
                thread_id: None,
                active_turn_id: None,
                messages: vec![user_message],
                log: vec![new_runtime_entry("project.session.user", prompt.to_string())],
                last_error: None,
            },
        );
    }
    persist_state(&state).await?;

    match state.runtime_mode {
        RuntimeMode::Stub => {
            complete_stub_project_session(&state, session_id, &project, latest_scan.as_ref(), prompt)
                .await?;
            let guard = state.inner.lock().await;
            return Ok(Json(project_context_snapshot(&guard, project_id)?));
        }
        RuntimeMode::RealCodex => {}
    }

    let workspace_root = workspace_root.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "项目会话需要先为当前项目配置主工作目录".into(),
        )
    })?;

    let composed_prompt = compose_project_session_prompt(&project, latest_scan.as_ref(), prompt);
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let session = match CodexRuntimeSession::spawn(workspace_root.clone(), event_tx).await {
        Ok(session) => session,
        Err(error) => {
            mark_project_session_failed(&state, session_id, &error.1).await;
            persist_state(&state).await?;
            return Err(error);
        }
    };
    let thread_id = match session
        .start_thread(&workspace_root, &project_session_developer_instructions())
        .await
    {
        Ok(thread_id) => thread_id,
        Err(error) => {
            mark_project_session_failed(&state, session_id, &error.1).await;
            persist_state(&state).await?;
            return Err(error);
        }
    };
    let turn_id = match session
        .start_turn(&workspace_root, &thread_id, &composed_prompt)
        .await
    {
        Ok(turn_id) => turn_id,
        Err(error) => {
            mark_project_session_failed(&state, session_id, &error.1).await;
            persist_state(&state).await?;
            return Err(error);
        }
    };

    {
        let mut guard = state.inner.lock().await;
        if let Some(project_session) = find_project_session_mut(&mut guard, session_id) {
            project_session.thread_id = Some(thread_id);
            project_session.active_turn_id = Some(turn_id);
        }
    }
    state
        .runtime_sessions
        .lock()
        .await
        .insert(session_id, session.clone());
    tokio::spawn(project_session_event_loop(
        state.clone(),
        session_id,
        event_rx,
    ));
    persist_state(&state).await?;

    let guard = state.inner.lock().await;
    Ok(Json(project_context_snapshot(&guard, project_id)?))
}

async fn continue_project_session(
    AxumPath(session_id): AxumPath<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<ContinueProjectSessionRequest>,
) -> AppResult<Json<ProjectContextSnapshot>> {
    let prompt = request.prompt.trim();
    if prompt.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "补充提问不能为空".into()));
    }

    let (project_id, project, latest_scan, thread_id, workspace_root) = {
        let mut guard = state.inner.lock().await;
        let (project_id, thread_id) = {
            let project_session = find_project_session_mut(&mut guard, session_id)
                .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到项目会话".into()))?;
            if project_session.active_turn_id.is_some() {
                return Err((StatusCode::CONFLICT, "当前项目会话仍在运行，请稍后再试".into()));
            }
            project_session.status = "running".into();
            project_session.last_error = None;
            project_session.messages.push(ProjectSessionMessage {
                role: "user".into(),
                content: prompt.to_string(),
                at: timestamp_string(),
            });
            project_session
                .log
                .push(new_runtime_entry("project.session.user", prompt.to_string()));
            (project_session.project_id, project_session.thread_id.clone())
        };

        let project = find_project(&guard, project_id)?.clone();
        let latest_scan = guard.project_scans.get(&project_id).cloned();
        let workspace_root = project
            .primary_workspace()
            .map(|workspace| PathBuf::from(&workspace.path));
        (
            project_id,
            project,
            latest_scan,
            thread_id,
            workspace_root,
        )
    };
    if matches!(state.runtime_mode, RuntimeMode::RealCodex) && workspace_root.is_none() {
        mark_project_session_failed(&state, session_id, "项目会话需要先为当前项目配置主工作目录").await;
        persist_state(&state).await?;
        return Err((
            StatusCode::BAD_REQUEST,
            "项目会话需要先为当前项目配置主工作目录".into(),
        ));
    }
    if matches!(state.runtime_mode, RuntimeMode::RealCodex) && thread_id.is_none() {
        mark_project_session_failed(&state, session_id, "当前项目会话缺少 thread_id，请新建一个项目会话").await;
        persist_state(&state).await?;
        return Err((
            StatusCode::CONFLICT,
            "当前项目会话缺少 thread_id，请新建一个项目会话".into(),
        ));
    }
    persist_state(&state).await?;

    match state.runtime_mode {
        RuntimeMode::Stub => {
            complete_stub_project_session(&state, session_id, &project, latest_scan.as_ref(), prompt)
                .await?;
            let guard = state.inner.lock().await;
            return Ok(Json(project_context_snapshot(&guard, project_id)?));
        }
        RuntimeMode::RealCodex => {}
    }

    let thread_id = thread_id.ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            "当前项目会话缺少 thread_id，请新建一个项目会话".into(),
        )
    })?;
    let workspace_root = workspace_root.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "项目会话需要先为当前项目配置主工作目录".into(),
        )
    })?;
    let composed_prompt = compose_project_session_prompt(&project, latest_scan.as_ref(), prompt);

    let session = state
        .runtime_sessions
        .lock()
        .await
        .get(&session_id)
        .cloned()
        .ok_or_else(|| {
            (
                StatusCode::CONFLICT,
                "当前服务进程内没有找到可恢复的项目会话，请新建一个项目会话".into(),
            )
        })?;

    let turn_id = match session
        .start_turn(&workspace_root, &thread_id, &composed_prompt)
        .await
    {
        Ok(turn_id) => turn_id,
        Err(error) => {
            mark_project_session_failed(&state, session_id, &error.1).await;
            persist_state(&state).await?;
            return Err(error);
        }
    };

    {
        let mut guard = state.inner.lock().await;
        if let Some(project_session) = find_project_session_mut(&mut guard, session_id) {
            project_session.active_turn_id = Some(turn_id);
        }
    }
    persist_state(&state).await?;

    let guard = state.inner.lock().await;
    Ok(Json(project_context_snapshot(&guard, project_id)?))
}

async fn create_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateTaskRequest>,
) -> AppResult<Json<Task>> {
    let title = request.title.trim();
    let description = request.description.trim();
    if title.is_empty() || description.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "标题和描述不能为空".into()));
    }

    let mut guard = state.inner.lock().await;
    let project = resolve_project_for_new_task(&guard, request.project_id)?.clone();
    let current_user = resolve_current_user(&guard, &headers);
    let task = Task {
        id: Uuid::new_v4(),
        project_id: project.id,
        title: title.to_string(),
        description: description.to_string(),
        status: TaskStatus::Open,
        creator_user_id: current_user.as_ref().map(|user| user.id),
        assignee_user_id: None,
        claimed_by: None,
        activities: vec![new_activity(
            "task.created",
            format!("任务由界面创建，并归属到项目“{}”", project.name),
        )],
        runtime: None,
    };
    guard.tasks.insert(0, task.clone());
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(task))
}

async fn seed_doc_tasks(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<BoardSnapshot>> {
    let mut guard = state.inner.lock().await;
    ensure_project_exists(&guard, project_id)?;
    merge_unique_tasks(&mut guard.tasks, seed_tasks_from_docs(project_id));
    let snapshot = snapshot_from_state(&guard);
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

async fn create_local_build_restart_task(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Task>> {
    let (project, current_user) = {
        let guard = state.inner.lock().await;
        (
            find_project(&guard, project_id)?.clone(),
            resolve_current_user(&guard, &headers),
        )
    };
    let mut task = build_local_build_restart_task(&project);
    task.creator_user_id = current_user.as_ref().map(|user| user.id);
    let mut guard = state.inner.lock().await;
    guard.tasks.insert(0, task.clone());
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(task))
}

async fn create_cloud_install_restart_task(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CloudInstallRestartTaskRequest>,
) -> AppResult<Json<Task>> {
    let (project, current_user) = {
        let guard = state.inner.lock().await;
        (
            find_project(&guard, project_id)?.clone(),
            resolve_current_user(&guard, &headers),
        )
    };
    let mut task = build_cloud_install_restart_task(&project, request)?;
    task.creator_user_id = current_user.as_ref().map(|user| user.id);
    let mut guard = state.inner.lock().await;
    guard.tasks.insert(0, task.clone());
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(task))
}

async fn bootstrap_tasks(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<BoardSnapshot>> {
    let project = {
        let guard = state.inner.lock().await;
        find_project(&guard, project_id)?.clone()
    };
    let incoming = seed_tasks_from_agents_file(&project);
    let mut guard = state.inner.lock().await;
    merge_unique_tasks(&mut guard.tasks, incoming);
    let snapshot = snapshot_from_state(&guard);
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

async fn explore_project(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<BoardSnapshot>> {
    let (project, current_user) = {
        let guard = state.inner.lock().await;
        (
            find_project(&guard, project_id)?.clone(),
            resolve_current_user(&guard, &headers),
        )
    };
    if let Ok(path) = primary_workspace_path(&project) {
        let _ = std::fs::create_dir_all(&path);
    }
    let mut task = build_exploration_task(&project);
    task.creator_user_id = current_user.as_ref().map(|user| user.id);
    let mut guard = state.inner.lock().await;
    guard.tasks.insert(0, task);
    let snapshot = snapshot_from_state(&guard);
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

async fn toggle_agent_auto_mode(
    AxumPath(agent_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<BoardSnapshot>> {
    let mut guard = state.inner.lock().await;
    let agent = guard
        .agents
        .iter_mut()
        .find(|agent| agent.id == agent_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到 Agent".into()))?;
    agent.auto_mode = !agent.auto_mode;
    agent.last_action = if agent.auto_mode {
        "已开启自动认领，可自动接手队列任务".into()
    } else {
        "已关闭自动认领，只接受手动分配".into()
    };
    let snapshot = snapshot_from_state(&guard);
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

async fn claim_task(
    AxumPath((task_id, agent_id)): AxumPath<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> AppResult<Json<BoardSnapshot>> {
    let mut guard = state.inner.lock().await;
    let (agent_name, owner_user_id) = guard
        .agents
        .iter()
        .find(|agent| agent.id == agent_id)
        .map(|agent| (agent.name.clone(), agent.owner_user_id))
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到 Agent".into()))?;
    let task = find_task_mut(&mut guard, task_id)?;
    if let Some(claimed_by) = task.claimed_by {
        if claimed_by != agent_id {
            return Err((StatusCode::CONFLICT, "任务已被其他 Agent 认领".into()));
        }
    }
    if matches!(task.status, TaskStatus::Running | TaskStatus::Done) {
        return Err((StatusCode::CONFLICT, "当前任务状态不允许重新认领".into()));
    }
    task.claimed_by = Some(agent_id);
    task.assignee_user_id = owner_user_id;
    task.status = TaskStatus::Claimed;
    task.activities.push(new_activity(
        "task.claimed",
        format!("任务已由 {} 认领", agent_name),
    ));
    let snapshot = snapshot_from_state(&guard);
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

async fn start_task(
    AxumPath((task_id, agent_id)): AxumPath<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(request): Json<AgentInvocationRequest>,
) -> AppResult<Json<BoardSnapshot>> {
    let context = resolve_task_execution_context(&state, task_id, request.prompt.clone()).await?;
    let _ = std::fs::create_dir_all(&context.workspace_root);
    let mut git_auto_merge_enabled = false;
    if matches!(state.runtime_mode, RuntimeMode::RealCodex) {
        let git_prepare = prepare_git_task_branch_in_repo(&context.workspace_root, task_id).await?;
        git_auto_merge_enabled = git_prepare.auto_merge_enabled;
        for (kind, message) in git_prepare.activities {
                    record_task_activity(&state, task_id, kind, message).await;
        }
        apply_git_snapshot(&context.workspace_root, task_id, &state).await;
    }

    match state.runtime_mode {
        RuntimeMode::Stub => {
            let mut guard = state.inner.lock().await;
            mark_task_running(
                &mut guard,
                task_id,
                agent_id,
                &request.agent_name_hint,
                &context.prompt,
                Some("stub-thread".into()),
                Some("stub-turn".into()),
                false,
            )?;
            let snapshot = snapshot_from_state(&guard);
            drop(guard);
            persist_state(&state).await?;
            Ok(Json(snapshot))
        }
        RuntimeMode::RealCodex => {
            let (event_tx, event_rx) = mpsc::unbounded_channel();
            let session =
                CodexRuntimeSession::spawn(context.workspace_root.clone(), event_tx).await?;
            let thread_id = session
                .start_thread(&context.workspace_root, &task_developer_instructions())
                .await?;
            let turn_id = session
                .start_turn(&context.workspace_root, &thread_id, &context.prompt)
                .await?;
            {
                let mut guard = state.inner.lock().await;
                mark_task_running(
                    &mut guard,
                    task_id,
                    agent_id,
                    &request.agent_name_hint,
                    &context.prompt,
                    Some(thread_id),
                    Some(turn_id),
                    git_auto_merge_enabled,
                )?;
            }
            state.runtime_sessions.lock().await.insert(task_id, session);
            tokio::spawn(runtime_event_loop(
                state.clone(),
                task_id,
                agent_id,
                event_rx,
            ));
            let guard = state.inner.lock().await;
            let snapshot = snapshot_from_state(&guard);
            drop(guard);
            persist_state(&state).await?;
            Ok(Json(snapshot))
        }
    }
}

async fn pause_task(
    AxumPath(task_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<BoardSnapshot>> {
    match state.runtime_mode {
        RuntimeMode::Stub => {
            let mut guard = state.inner.lock().await;
            {
                let task = find_task_mut(&mut guard, task_id)?;
                task.status = TaskStatus::Paused;
                task.activities
                    .push(new_activity("task.paused", "任务已暂停，等待补充提示词"));
                if let Some(runtime) = task.runtime.as_mut() {
                    runtime
                        .log
                        .push(new_runtime_entry("system", "已暂停当前任务"));
                }
            }
            reset_agent_if_needed(&mut guard, task_id, "任务已暂停，等待恢复");
            let snapshot = snapshot_from_state(&guard);
            drop(guard);
            persist_state(&state).await?;
            return Ok(Json(snapshot));
        }
        RuntimeMode::RealCodex => {}
    }

    let (thread_id, turn_id) = {
        let mut guard = state.inner.lock().await;
        let task = find_task_mut(&mut guard, task_id)?;
        task.status = TaskStatus::Paused;
        task.activities
            .push(new_activity("task.pause_requested", "已发送暂停请求"));
        let runtime = task
            .runtime
            .as_mut()
            .ok_or_else(|| (StatusCode::CONFLICT, "当前任务没有活动会话".into()))?;
        runtime
            .log
            .push(new_runtime_entry("user", "请求暂停当前运行"));
        let thread_id = runtime
            .thread_id
            .clone()
            .ok_or_else(|| (StatusCode::CONFLICT, "缺少 thread_id，无法暂停".into()))?;
        let turn_id = runtime
            .active_turn_id
            .clone()
            .ok_or_else(|| (StatusCode::CONFLICT, "缺少活动 turn_id，无法暂停".into()))?;
        (thread_id, turn_id)
    };

    let sessions = state.runtime_sessions.lock().await;
    let session = sessions
        .get(&task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到运行时会话".into()))?;
    drop(sessions);
    session.interrupt_turn(&thread_id, &turn_id).await?;

    let guard = state.inner.lock().await;
    let snapshot = snapshot_from_state(&guard);
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

async fn resume_task(
    AxumPath((task_id, agent_id)): AxumPath<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(request): Json<AgentResumeRequest>,
) -> AppResult<Json<BoardSnapshot>> {
    if request.prompt.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "恢复时必须补充提示词".into()));
    }

    match state.runtime_mode {
        RuntimeMode::Stub => {
            let mut guard = state.inner.lock().await;
            {
                let task = find_task_mut(&mut guard, task_id)?;
                task.status = TaskStatus::Done;
                task.claimed_by = Some(agent_id);
                task.activities.push(new_activity(
                    "task.resumed",
                    format!("补充提示词后恢复执行：{}", request.prompt.trim()),
                ));
                let runtime = task.runtime.get_or_insert_with(|| TaskRuntime {
                    provider: "stub-codex".into(),
                    thread_id: Some("stub-thread".into()),
                    active_turn_id: Some("stub-turn-2".into()),
                    git_auto_merge_enabled: false,
                    log: Vec::new(),
                    last_error: None,
                });
                runtime
                    .log
                    .push(new_runtime_entry("user", request.prompt.trim().to_string()));
                runtime
                    .log
                    .push(new_runtime_entry("assistant", "Stub 会话已完成任务"));
                runtime.active_turn_id = None;
            }
            reset_agent_if_needed(&mut guard, task_id, "最近一次任务已完成");
            let snapshot = snapshot_from_state(&guard);
            drop(guard);
            persist_state(&state).await?;
            return Ok(Json(snapshot));
        }
        RuntimeMode::RealCodex => {}
    }

    let workspace_root = resolve_workspace_for_task(&state, task_id).await?;
    let thread_id = {
        let mut guard = state.inner.lock().await;
        let task = find_task_mut(&mut guard, task_id)?;
        task.status = TaskStatus::Running;
        task.claimed_by = Some(agent_id);
        task.activities.push(new_activity(
            "task.resume_requested",
            format!("已补充提示词并恢复：{}", request.prompt.trim()),
        ));
        let runtime = task
            .runtime
            .as_mut()
            .ok_or_else(|| (StatusCode::CONFLICT, "当前任务没有可恢复的会话".into()))?;
        runtime
            .log
            .push(new_runtime_entry("user", request.prompt.trim().to_string()));
        runtime
            .thread_id
            .clone()
            .ok_or_else(|| (StatusCode::CONFLICT, "缺少 thread_id，无法恢复".into()))?
    };

    {
        let mut guard = state.inner.lock().await;
        assign_agent_running(&mut guard, agent_id, task_id, "继续执行当前任务".into());
    }

    let sessions = state.runtime_sessions.lock().await;
    let session = sessions
        .get(&task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到运行时会话".into()))?;
    drop(sessions);

    let turn_id = session
        .start_turn(&workspace_root, &thread_id, request.prompt.trim())
        .await?;

    let mut guard = state.inner.lock().await;
    let task = find_task_mut(&mut guard, task_id)?;
    if let Some(runtime) = task.runtime.as_mut() {
        runtime.active_turn_id = Some(turn_id);
    }
    let snapshot = snapshot_from_state(&guard);
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

async fn runtime_event_loop(
    state: AppState,
    task_id: Uuid,
    agent_id: Uuid,
    mut event_rx: mpsc::UnboundedReceiver<RuntimeEvent>,
) {
    while let Some(event) = event_rx.recv().await {
        let mut guard = state.inner.lock().await;
        let Some(task_index) = guard.tasks.iter().position(|task| task.id == task_id) else {
            break;
        };

        let mut reset_action: Option<&'static str> = None;
        let mut remove_runtime = false;
        let mut should_finalize_git_merge = false;
        let task_is_running;

        {
            let task = &mut guard.tasks[task_index];
            let runtime = task.runtime.get_or_insert_with(|| TaskRuntime {
                provider: "codex".into(),
                thread_id: None,
                active_turn_id: None,
                git_auto_merge_enabled: false,
                log: Vec::new(),
                last_error: None,
            });

            match event {
                RuntimeEvent::ThreadStarted { thread_id } => {
                    runtime.thread_id = Some(thread_id.clone());
                    task.activities.push(new_activity(
                        "runtime.thread_started",
                        format!("Codex 线程已启动：{thread_id}"),
                    ));
                }
                RuntimeEvent::TurnStarted { turn_id } => {
                    runtime.active_turn_id = Some(turn_id.clone());
                    runtime.log.push(new_runtime_entry(
                        "system",
                        format!("Turn 已启动：{turn_id}"),
                    ));
                    task.status = TaskStatus::Running;
                }
                RuntimeEvent::AgentDelta { delta } => {
                    push_runtime_delta(&mut runtime.log, "assistant", &delta);
                }
                RuntimeEvent::CommandDelta { delta } => {
                    push_runtime_delta(&mut runtime.log, "command", &delta);
                }
                RuntimeEvent::PlanDelta { delta } => {
                    push_runtime_delta(&mut runtime.log, "plan", &delta);
                }
                RuntimeEvent::TurnCompleted { turn_id, status } => {
                    runtime.active_turn_id = None;
                    task.activities.push(new_activity(
                        "runtime.turn_completed",
                        format!("Turn {turn_id} 已结束，状态：{status}"),
                    ));
                    match status.as_str() {
                        "completed" => {
                            task.status = TaskStatus::Done;
                            reset_action = Some("最近一次任务已完成");
                            should_finalize_git_merge = runtime.git_auto_merge_enabled;
                        }
                        "interrupted" => {
                            task.status = TaskStatus::Paused;
                            reset_action = Some("任务已暂停，等待恢复");
                        }
                        _ => {
                            task.status = TaskStatus::Failed;
                            runtime.last_error = Some(format!("Turn 结束状态异常：{status}"));
                            reset_action = Some("最近一次任务执行失败");
                        }
                    }
                }
                RuntimeEvent::Error { message } => {
                    task.status = TaskStatus::Failed;
                    runtime.last_error = Some(message.clone());
                    runtime
                        .log
                        .push(new_runtime_entry("error", message.clone()));
                    task.activities.push(new_activity(
                        "runtime.error",
                        format!("运行失败：{message}"),
                    ));
                    reset_action = Some("最近一次任务执行失败");
                }
                RuntimeEvent::Stderr { message } => {
                    runtime.log.push(new_runtime_entry("stderr", message));
                }
                RuntimeEvent::Exited { message } => {
                    if matches!(task.status, TaskStatus::Running) {
                        task.status = TaskStatus::Failed;
                    }
                    runtime.last_error = Some(message.clone());
                    task.activities
                        .push(new_activity("runtime.exited", message));
                    reset_action = Some("运行时已退出");
                    remove_runtime = true;
                }
            }

            task_is_running = matches!(task.status, TaskStatus::Running);
        }

        if let Some(action) = reset_action {
            reset_agent_if_needed(&mut guard, task_id, action);
        }

        if task_is_running {
            if let Some(agent) = guard.agents.iter_mut().find(|agent| agent.id == agent_id) {
                agent.status = "运行中".into();
            }
        }

        let workspace_root = if should_finalize_git_merge {
            let project_id = guard.tasks[task_index].project_id;
            find_project(&guard, project_id)
                .ok()
                .and_then(|project| primary_workspace_path(project).ok())
        } else {
            None
        };

        drop(guard);

        if let Some(workspace_root) = workspace_root {
            let merge_activities = finalize_git_task_branch_in_repo(&workspace_root, task_id).await;
            for (kind, message) in merge_activities {
                record_task_activity(&state, task_id, kind, message).await;
            }
        }

        let _ = persist_state(&state).await;

        if remove_runtime {
            state.runtime_sessions.lock().await.remove(&task_id);
            break;
        }
    }
}

fn snapshot_from_state(state: &BoardState) -> BoardSnapshot {
    snapshot_from_state_with_user(state, state.users.first().cloned())
}

fn snapshot_from_state_with_user(state: &BoardState, current_user: Option<User>) -> BoardSnapshot {
    BoardSnapshot {
        current_user,
        users: state.users.clone(),
        projects: state.projects.clone(),
        tasks: state.tasks.clone(),
        agents: state.agents.clone(),
    }
}

fn build_auth_cookie(user_id: &Uuid) -> String {
    format!("spotlight_user_id={user_id}; Path=/; HttpOnly; SameSite=Lax")
}

fn cookie_value<'a>(headers: &'a HeaderMap, key: &str) -> Option<&'a str> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie_header.split(';').find_map(|part| {
        let (cookie_key, cookie_value) = part.trim().split_once('=')?;
        (cookie_key == key).then_some(cookie_value.trim())
    })
}

fn resolve_current_user(state: &BoardState, headers: &HeaderMap) -> Option<User> {
    if let Some(raw_user_id) = cookie_value(headers, "spotlight_user_id") {
        if let Ok(user_id) = Uuid::parse_str(raw_user_id) {
            if let Some(user) = state.users.iter().find(|user| user.id == user_id) {
                return Some(user.clone());
            }
        }
    }

    state.users.first().cloned()
}

async fn persist_state(state: &AppState) -> AppResult<()> {
    let (persisted, store_path) = {
        let guard = state.inner.lock().await;
        (
            PersistedState {
                users: guard.users.clone(),
                projects: guard.projects.clone(),
                tasks: guard.tasks.clone(),
                agents: guard.agents.clone(),
                project_scans: guard.project_scans.clone(),
                project_sessions: guard.project_sessions.clone(),
            },
            state.store_path.clone(),
        )
    };

    persist_state_to_path(&store_path, &persisted)
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))
}

fn project_context_snapshot(
    state: &BoardState,
    project_id: Uuid,
) -> AppResult<ProjectContextSnapshot> {
    let project = find_project(state, project_id)?;
    let sessions = state
        .project_sessions
        .iter()
        .filter(|session| session.project_id == project_id)
        .cloned()
        .collect::<Vec<_>>();

    Ok(ProjectContextSnapshot {
        project_id,
        primary_workspace: project.primary_workspace().cloned(),
        latest_scan: state.project_scans.get(&project_id).cloned(),
        sessions,
    })
}

fn truncate_title(input: &str) -> String {
    let compact = input.replace('\n', " ").trim().to_string();
    let mut chars = compact.chars();
    let preview = chars.by_ref().take(24).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}...")
    } else if preview.is_empty() {
        "项目会话".into()
    } else {
        preview
    }
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

fn build_project_scan_summary(
    project_id: Uuid,
    workspace: &WorkspaceRoot,
    workspace_root: &Path,
) -> ProjectScanSummary {
    let detection = detect_project_stack(workspace_root);
    let files = collect_workspace_files(workspace_root, 2);
    let top_level_entries = collect_top_level_entries(workspace_root, 10);
    let key_files = files
        .iter()
        .filter_map(|path| {
            is_key_project_file(path)
                .then(|| display_relative_path(workspace_root, path))
        })
        .take(12)
        .collect::<Vec<_>>();
    let document_files = files
        .iter()
        .filter_map(|path| {
            is_document_file(path)
                .then(|| display_relative_path(workspace_root, path))
        })
        .take(12)
        .collect::<Vec<_>>();

    let mut notes = Vec::new();
    if top_level_entries.is_empty() {
        notes.push("当前目录看起来几乎为空，建议先确认是否打开了正确目录。".into());
    }
    if detection.stacks.is_empty() {
        notes.push("暂未识别到常见构建清单，当前目录可能更偏文档、交付物或资料目录。".into());
    }
    if !document_files.is_empty() {
        notes.push("目录中存在文档类文件，适合先做项目扫描和问答，再拆成正式任务。".into());
    }
    if !workspace_root.join(".git").exists() {
        notes.push("当前主目录没有发现 .git，后续代码改动前需要确认真实仓库边界。".into());
    }

    ProjectScanSummary {
        project_id,
        workspace_id: workspace.id,
        workspace_label: workspace.label.clone(),
        workspace_path: workspace.path.clone(),
        scanned_at: timestamp_string(),
        stack_summary: detection.summary(),
        detected_stacks: detection.stacks.iter().map(|stack| stack.to_string()).collect(),
        top_level_entries,
        key_files,
        document_files,
        notes,
    }
}

fn collect_top_level_entries(base: &Path, limit: usize) -> Vec<String> {
    let entries = match std::fs::read_dir(base) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut items = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.to_string();
            if path.is_dir() {
                Some(format!("{name}/"))
            } else {
                Some(name)
            }
        })
        .collect::<Vec<_>>();
    items.sort();
    items.into_iter().take(limit).collect()
}

fn is_key_project_file(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if [
        "Cargo.toml",
        "package.json",
        "pnpm-workspace.yaml",
        "pyproject.toml",
        "requirements.txt",
        "README.md",
        "AGENTS.md",
        "deploy.md",
        "Dockerfile",
        "docker-compose.yml",
        "docker-compose.yaml",
        "tauri.conf.json",
    ]
    .contains(&file_name)
    {
        return true;
    }

    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "md" | "toml" | "json" | "yaml" | "yml")
    )
}

fn is_document_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("md" | "txt" | "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx")
    )
}

fn compose_project_session_prompt(
    project: &Project,
    latest_scan: Option<&ProjectScanSummary>,
    user_prompt: &str,
) -> String {
    let workspace_list = project
        .workspace_roots
        .iter()
        .map(|workspace| {
            format!(
                "- {}: {}（{}）",
                workspace.label,
                workspace.path,
                if workspace.writable { "可写" } else { "只读" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let scan_summary = latest_scan
        .map(|scan| {
            format!(
                "最近扫描摘要：{}\n顶层目录：{}\n关键文件：{}\n文档文件：{}\n提示：{}",
                scan.stack_summary,
                if scan.top_level_entries.is_empty() {
                    "无".into()
                } else {
                    scan.top_level_entries.join("、")
                },
                if scan.key_files.is_empty() {
                    "无".into()
                } else {
                    scan.key_files.join("、")
                },
                if scan.document_files.is_empty() {
                    "无".into()
                } else {
                    scan.document_files.join("、")
                },
                if scan.notes.is_empty() {
                    "无".into()
                } else {
                    scan.notes.join("；")
                }
            )
        })
        .unwrap_or_else(|| "最近还没有项目扫描摘要；如果需要判断目录结构、文档和构建入口，请先建议用户执行项目扫描。".into());

    format!(
        "你正在进行 Spotlight 的项目级问答会话，而不是直接执行一个任务。\n\
项目名称：{}\n\
项目说明：{}\n\
可见工作目录：\n{}\n\
{}\n\
\n\
当前用户问题：{}\n\
\n\
回答要求：\n\
1. 优先用中文回答。\n\
2. 先基于目录结构、代码和文档做判断，不要臆造未读取到的内容。\n\
3. 如果信息不足，要明确指出缺口，并给出下一步建议。\n\
4. 如果适合拆成任务，请顺手给出建议任务标题和说明。\n\
5. 若涉及实际改动，先说明影响范围和风险，再给方案。",
        project.name, project.description, workspace_list, scan_summary, user_prompt
    )
}

fn project_session_developer_instructions() -> String {
    [
        "你是 Spotlight 的项目协作 Agent。",
        "当前会话的目标是帮助用户理解项目目录、文档、代码结构和下一步改动方向。",
        "默认优先做分析、解释、风险提示和任务拆解，而不是直接执行破坏性修改。",
        "当需要代码改动时，要先说清楚影响范围、依赖条件和建议步骤。",
        "回答尽量用中文，结构清晰，可读性高。",
    ]
    .join(" ")
}

fn build_stub_project_session_reply(
    project: &Project,
    latest_scan: Option<&ProjectScanSummary>,
    prompt: &str,
) -> String {
    let scan_line = latest_scan
        .map(|scan| format!("最近扫描结果：{}。", scan.stack_summary))
        .unwrap_or_else(|| "当前还没有扫描摘要，建议先对项目目录做一次扫描。".into());

    format!(
        "这是项目会话的本地 Stub 回复。\n\
项目：{}\n\
{}\n\
你刚刚的问题：{}\n\
\n\
建议下一步：\n\
1. 先确认项目目录和关键 workspace 是否已经接入。\n\
2. 如果需要判断代码入口、文档分布或构建方式，先执行项目扫描。\n\
3. 如果已经足够明确，就把结论继续追问，或拆成正式任务推进。",
        project.name, scan_line, prompt
    )
}

async fn complete_stub_project_session(
    state: &AppState,
    session_id: Uuid,
    project: &Project,
    latest_scan: Option<&ProjectScanSummary>,
    prompt: &str,
) -> AppResult<()> {
    let reply = build_stub_project_session_reply(project, latest_scan, prompt);
    let mut guard = state.inner.lock().await;
    let Some(project_session) = find_project_session_mut(&mut guard, session_id) else {
        return Err((StatusCode::NOT_FOUND, "未找到项目会话".into()));
    };
    project_session.status = "completed".into();
    project_session.thread_id.get_or_insert_with(|| "stub-project-thread".into());
    project_session.active_turn_id = None;
    project_session.messages.push(ProjectSessionMessage {
        role: "assistant".into(),
        content: reply.clone(),
        at: timestamp_string(),
    });
    project_session
        .log
        .push(new_runtime_entry("assistant", reply));
    drop(guard);
    persist_state(state).await
}

async fn mark_project_session_failed(state: &AppState, session_id: Uuid, message: &str) {
    let mut guard = state.inner.lock().await;
    if let Some(project_session) = find_project_session_mut(&mut guard, session_id) {
        project_session.status = "failed".into();
        project_session.active_turn_id = None;
        project_session.last_error = Some(message.to_string());
        project_session
            .log
            .push(new_runtime_entry("project.session.error", message.to_string()));
    }
}

async fn project_session_event_loop(
    state: AppState,
    session_id: Uuid,
    mut event_rx: mpsc::UnboundedReceiver<RuntimeEvent>,
) {
    let mut assistant_buffer = String::new();

    while let Some(event) = event_rx.recv().await {
        let mut remove_runtime = false;
        {
            let mut guard = state.inner.lock().await;
            let Some(project_session) = find_project_session_mut(&mut guard, session_id) else {
                break;
            };

            match event {
                RuntimeEvent::ThreadStarted { thread_id } => {
                    project_session.thread_id = Some(thread_id.clone());
                    project_session.log.push(new_runtime_entry(
                        "project.session.thread_started",
                        format!("项目会话线程已启动：{thread_id}"),
                    ));
                }
                RuntimeEvent::TurnStarted { turn_id } => {
                    assistant_buffer.clear();
                    project_session.active_turn_id = Some(turn_id.clone());
                    project_session.status = "running".into();
                    project_session.log.push(new_runtime_entry(
                        "project.session.turn_started",
                        format!("项目会话轮次已启动：{turn_id}"),
                    ));
                }
                RuntimeEvent::AgentDelta { delta } => {
                    assistant_buffer.push_str(&delta);
                    push_runtime_delta(&mut project_session.log, "assistant", &delta);
                }
                RuntimeEvent::CommandDelta { delta } => {
                    push_runtime_delta(&mut project_session.log, "command", &delta);
                }
                RuntimeEvent::PlanDelta { delta } => {
                    push_runtime_delta(&mut project_session.log, "plan", &delta);
                }
                RuntimeEvent::TurnCompleted { status, .. } => {
                    project_session.active_turn_id = None;
                    project_session.status = if status == "completed" {
                        "completed".into()
                    } else if status == "interrupted" {
                        "paused".into()
                    } else {
                        "failed".into()
                    };
                    if !assistant_buffer.trim().is_empty() {
                        project_session.messages.push(ProjectSessionMessage {
                            role: "assistant".into(),
                            content: assistant_buffer.trim().to_string(),
                            at: timestamp_string(),
                        });
                    }
                    assistant_buffer.clear();
                    if project_session.status == "failed" {
                        project_session.last_error =
                            Some(format!("项目会话以异常状态结束：{status}"));
                    }
                    project_session.log.push(new_runtime_entry(
                        "project.session.turn_completed",
                        format!("项目会话轮次结束：{status}"),
                    ));
                }
                RuntimeEvent::Error { message } => {
                    project_session.active_turn_id = None;
                    project_session.status = "failed".into();
                    project_session.last_error = Some(message.clone());
                    if !assistant_buffer.trim().is_empty() {
                        project_session.messages.push(ProjectSessionMessage {
                            role: "assistant".into(),
                            content: assistant_buffer.trim().to_string(),
                            at: timestamp_string(),
                        });
                    }
                    assistant_buffer.clear();
                    project_session
                        .log
                        .push(new_runtime_entry("project.session.error", message));
                }
                RuntimeEvent::Stderr { message } => {
                    push_runtime_delta(&mut project_session.log, "stderr", &message);
                }
                RuntimeEvent::Exited { message } => {
                    remove_runtime = true;
                    if project_session.status == "running" {
                        project_session.status = "failed".into();
                        project_session.last_error = Some(message.clone());
                    }
                    project_session
                        .log
                        .push(new_runtime_entry("project.session.exited", message));
                }
            }
        }

        let _ = persist_state(&state).await;

        if remove_runtime {
            state.runtime_sessions.lock().await.remove(&session_id);
            break;
        }
    }
}

fn resolve_project_for_new_task(
    state: &BoardState,
    project_id: Option<Uuid>,
) -> AppResult<&Project> {
    match project_id {
        Some(project_id) => find_project(state, project_id),
        None => state
            .projects
            .first()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "当前没有可用项目".into())),
    }
}

fn find_project(state: &BoardState, project_id: Uuid) -> AppResult<&Project> {
    state
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到项目".into()))
}

fn find_project_mut(state: &mut BoardState, project_id: Uuid) -> AppResult<&mut Project> {
    state
        .projects
        .iter_mut()
        .find(|project| project.id == project_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到项目".into()))
}

fn ensure_project_exists(state: &BoardState, project_id: Uuid) -> AppResult<()> {
    find_project(state, project_id).map(|_| ())
}

fn find_task_mut(state: &mut BoardState, task_id: Uuid) -> AppResult<&mut Task> {
    state
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到任务".into()))
}

fn find_project_session_mut(
    state: &mut BoardState,
    session_id: Uuid,
) -> Option<&mut ProjectSession> {
    state
        .project_sessions
        .iter_mut()
        .find(|session| session.id == session_id)
}

fn primary_workspace_path(project: &Project) -> AppResult<PathBuf> {
    project
        .primary_workspace()
        .map(|workspace| PathBuf::from(&workspace.path))
        .ok_or_else(|| {
            (
                StatusCode::FAILED_DEPENDENCY,
                "项目还没有绑定工作目录".into(),
            )
        })
}

async fn resolve_workspace_for_task(state: &AppState, task_id: Uuid) -> AppResult<PathBuf> {
    let guard = state.inner.lock().await;
    let task = guard
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到任务".into()))?;
    let project = find_project(&guard, task.project_id)?;
    primary_workspace_path(project)
}

async fn resolve_task_execution_context(
    state: &AppState,
    task_id: Uuid,
    prompt_override: Option<String>,
) -> AppResult<TaskExecutionContext> {
    let guard = state.inner.lock().await;
    let task = guard
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到任务".into()))?;
    let project = find_project(&guard, task.project_id)?.clone();
    let workspace_root = primary_workspace_path(&project)?;
    let prompt = compose_task_prompt(&task, &project, prompt_override);
    Ok(TaskExecutionContext {
        workspace_root,
        prompt,
    })
}

fn compose_task_prompt(task: &Task, project: &Project, prompt_override: Option<String>) -> String {
    let workspace_list = project
        .workspace_roots
        .iter()
        .map(|workspace| {
            format!(
                "- {}: {}（{}）",
                workspace.label,
                workspace.path,
                if workspace.writable {
                    "可写"
                } else {
                    "只读"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut prompt = format!(
        "你正在执行 Spotlight 项目任务。\n\
项目名称：{}\n\
项目说明：{}\n\
工作目录：\n{}\n\
任务标题：{}\n\
任务描述：{}\n\
\n\
执行要求：\n\
1. 先分析再行动，给出清晰的执行步骤。\n\
2. 不要假设当前目录一定是代码仓库；它可能为空，也可能只有 Word、PDF、表格、图片或其他资料。\n\
3. 如果遇到 Office 或二进制文件，不要臆造内容，可以基于文件名、目录结构、相邻文本和可读元数据给出判断。\n\
4. 项目外目录允许读取，但不要做破坏性修改。\n\
5. 输出时尽量用中文，结论、风险和建议都要清楚可读。",
        project.name, project.description, workspace_list, task.title, task.description
    );

    if let Some(extra_prompt) = prompt_override {
        let extra_prompt = extra_prompt.trim();
        if !extra_prompt.is_empty() {
            prompt.push_str("\n\n用户补充提示词：\n");
            prompt.push_str(extra_prompt);
        }
    }

    prompt
}

impl StackDetection {
    fn summary(&self) -> String {
        if self.stacks.is_empty() {
            "未识别到常见构建清单；当前目录可能是文档目录、交付目录，或需要在更深层继续探索。"
                .into()
        } else if self.evidence.is_empty() {
            format!("{}", self.stacks.join("、"))
        } else {
            format!(
                "{}（线索：{}）",
                self.stacks.join("、"),
                self.evidence.join("，")
            )
        }
    }
}

fn build_local_build_restart_task(project: &Project) -> Task {
    let workspace_root = primary_workspace_path(project).ok();
    let stack_detection = workspace_root
        .as_deref()
        .map(detect_project_stack)
        .unwrap_or_default();
    let workspace_path = workspace_root
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "未配置主工作目录".into());

    Task {
        id: Uuid::new_v4(),
        project_id: project.id,
        title: "本地编译重启".into(),
        description: format!(
            "请在当前项目工作目录中完成一次“本地编译重启”尝试，并输出中文结论。\n\
主工作目录：{}\n\
初步识别：{}\n\
\n\
执行目标：\n\
1. 先识别项目类型与主要语言，优先判断 Rust、C++、Python、JavaScript / TypeScript，也可补充其他语言或运行时。\n\
2. 判断当前目录是否具备可执行的依赖安装、构建、打包、启动或重启入口。\n\
3. 若具备条件，安装缺失依赖、完成本地编译或打包，并重启相关服务；若存在多个服务，要明确说明本次处理范围。\n\
4. 若缺少入口、配置、环境变量、二进制依赖或启动参数，要明确列出阻塞点与人工确认项。\n\
5. 若项目根目录没有 deploy.md，请新增；若已有 deploy.md，请补充本地编译、启动、重启、校验和回滚说明。\n\
\n\
执行约束：\n\
- 不要假设当前目录一定是完整代码仓库。\n\
- 如遇 Word、PDF、图片或其他二进制文件，不要臆造内容，可基于文件名和目录结构做谨慎判断。\n\
- 不要对项目外目录做破坏性修改。\n\
- 对需要管理员权限、系统级安装、覆盖已有进程或危险重启的动作，要先说明风险再执行。\n\
\n\
交付内容：\n\
- 识别出的技术栈与关键入口\n\
- 依赖安装、编译/打包、重启结果\n\
- deploy.md 的新增或更新说明\n\
- 风险、阻塞项与下一步建议",
            workspace_path,
            stack_detection.summary(),
        ),
        status: TaskStatus::Open,
        creator_user_id: None,
        assignee_user_id: None,
        claimed_by: None,
        activities: vec![new_activity(
            "task.local_build_restart_created",
            format!("已为项目“{}”创建本地编译重启任务", project.name),
        )],
        runtime: None,
    }
}

fn build_cloud_install_restart_task(
    project: &Project,
    request: CloudInstallRestartTaskRequest,
) -> AppResult<Task> {
    let host = request.host.trim();
    let username = request.username.trim();
    if host.is_empty() || username.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "云端服务器地址和 SSH 用户名不能为空".into(),
        ));
    }

    let auth_method = request
        .auth_method
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("SSH 证书");
    let credential_hint = sanitize_credential_hint(auth_method, request.credential_hint.as_deref());
    let deploy_path = request
        .deploy_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("待确认部署目录");
    let service_hint = request
        .service_hint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("待确认服务名或重启命令");
    let workspace_root = primary_workspace_path(project).ok();
    let stack_detection = workspace_root
        .as_deref()
        .map(detect_project_stack)
        .unwrap_or_default();

    Ok(Task {
        id: Uuid::new_v4(),
        project_id: project.id,
        title: "云端安装重启".into(),
        description: format!(
            "请为当前项目执行一次“云端安装重启”任务，并输出中文结论。\n\
\n\
远端信息：\n\
- 主机/IP：{}\n\
- 端口：{}\n\
- SSH 用户：{}\n\
- 认证方式：{}\n\
- 凭据说明：{}\n\
- 部署目录：{}\n\
- 服务信息：{}\n\
\n\
本地初步识别：{}\n\
\n\
执行目标：\n\
1. 结合本地项目内容识别主要技术栈，优先判断 Rust、C++、Python、JavaScript / TypeScript，也可补充其他语言或运行时。\n\
2. 规划并执行远端依赖安装、构建/打包、发布、服务重启与可用性校验步骤。\n\
3. 若凭据和网络条件具备，可尝试通过 SSH 登录并执行；若不具备，要明确说明阻塞点和所需人工补充信息。\n\
4. 若项目根目录没有 deploy.md，请新增；若已有 deploy.md，请补充远端部署、重启、回滚和校验方法。\n\
5. 对覆盖发布、系统级安装、服务停机、数据迁移等高风险操作，要先记录风险与回滚方案。\n\
\n\
安全要求：\n\
- 不要把明文密码写入 deploy.md、任务结论或长期日志。\n\
- 优先使用已配置 SSH 证书、私钥路径或凭据别名。\n\
- 如需临时补充密码，建议在任务启动前通过提示词补充，不要长期保存在任务描述。\n\
\n\
交付内容：\n\
- 远端连通性与认证结果\n\
- 依赖安装、构建/部署、重启与校验结果\n\
- deploy.md 的新增或更新说明\n\
- 风险、阻塞项与下一步建议",
            host,
            request.port.unwrap_or(22),
            username,
            auth_method,
            credential_hint,
            deploy_path,
            service_hint,
            stack_detection.summary(),
        ),
        status: TaskStatus::Open,
        creator_user_id: None,
        assignee_user_id: None,
        claimed_by: None,
        activities: vec![new_activity(
            "task.cloud_install_restart_created",
            format!("已为项目“{}”创建云端安装重启任务", project.name),
        )],
        runtime: None,
    })
}

fn sanitize_credential_hint(auth_method: &str, credential_hint: Option<&str>) -> String {
    let trimmed = credential_hint
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if auth_method.contains("密码") {
        if trimmed.is_some() {
            "已收到密码类凭据，但为安全起见不在任务描述中回显；请在启动任务前临时补充。".into()
        } else {
            "未记录明文密码；如需密码登录，请在启动任务前临时补充。".into()
        }
    } else {
        trimmed
            .map(|value| value.to_string())
            .unwrap_or_else(|| "建议使用已配置 SSH 证书、私钥路径或系统凭据别名".into())
    }
}

fn detect_project_stack(workspace_root: &Path) -> StackDetection {
    let files = collect_workspace_files(workspace_root, 2);
    let rules = [
        ("Rust", ["Cargo.toml"].as_slice()),
        (
            "JavaScript / TypeScript",
            [
                "package.json",
                "pnpm-workspace.yaml",
                "package-lock.json",
                "yarn.lock",
                "tsconfig.json",
            ]
            .as_slice(),
        ),
        (
            "Python",
            ["pyproject.toml", "requirements.txt", "Pipfile", "setup.py"].as_slice(),
        ),
        (
            "C++",
            [
                "CMakeLists.txt",
                "meson.build",
                "conanfile.txt",
                "conanfile.py",
                "Makefile",
            ]
            .as_slice(),
        ),
    ];

    let mut detection = StackDetection::default();
    for (stack, file_names) in rules {
        let matches = files
            .iter()
            .filter_map(|path| {
                let file_name = path.file_name()?.to_str()?;
                file_names
                    .contains(&file_name)
                    .then(|| display_relative_path(workspace_root, path))
            })
            .take(2)
            .collect::<Vec<_>>();
        if !matches.is_empty() {
            detection.stacks.push(stack);
            detection.evidence.extend(matches);
        }
    }

    if detection.stacks.is_empty() {
        let extension_rules = [
            ("Rust", ["rs"].as_slice()),
            (
                "JavaScript / TypeScript",
                ["js", "jsx", "ts", "tsx"].as_slice(),
            ),
            ("Python", ["py"].as_slice()),
            ("C++", ["cpp", "cc", "cxx", "hpp", "hh", "h"].as_slice()),
        ];
        for (stack, extensions) in extension_rules {
            let matches = files
                .iter()
                .filter_map(|path| {
                    let extension = path.extension()?.to_str()?;
                    extensions
                        .contains(&extension)
                        .then(|| display_relative_path(workspace_root, path))
                })
                .take(2)
                .collect::<Vec<_>>();
            if !matches.is_empty() {
                detection.stacks.push(stack);
                detection.evidence.extend(matches);
            }
        }
    }

    detection
}

fn collect_workspace_files(base: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_workspace_files_inner(base, base, 0, max_depth, &mut files);
    files
}

fn collect_workspace_files_inner(
    base: &Path,
    current: &Path,
    depth: usize,
    max_depth: usize,
    files: &mut Vec<PathBuf>,
) {
    if depth > max_depth {
        return;
    }

    let entries = match std::fs::read_dir(current) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            files.push(path);
            continue;
        }

        if !path.is_dir() || depth == max_depth {
            continue;
        }

        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if should_skip_workspace_dir(base, &path, name) {
            continue;
        }

        collect_workspace_files_inner(base, &path, depth + 1, max_depth, files);
    }
}

fn should_skip_workspace_dir(base: &Path, path: &Path, name: &str) -> bool {
    [
        ".git",
        "target",
        "node_modules",
        "dist",
        "build",
        ".next",
        ".turbo",
        ".venv",
        "venv",
        "__pycache__",
    ]
    .contains(&name)
        || path == base.join("tmp")
}

fn display_relative_path(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn build_exploration_task(project: &Project) -> Task {
    let workspace_list = project
        .workspace_roots
        .iter()
        .map(|workspace| format!("- {}：{}", workspace.label, workspace.path))
        .collect::<Vec<_>>()
        .join("\n");

    Task {
        id: Uuid::new_v4(),
        project_id: project.id,
        title: "探索当前目录并生成建议任务".into(),
        description: format!(
            "请先探索当前项目目录，再输出一份中文结论。\n\
输出至少包含：\n\
1. 当前目录内容摘要\n\
2. 可识别的技术栈、文档类型或交付物\n\
3. 主要风险、信息缺口和需要人工确认的地方\n\
4. 建议的任务列表（按优先级排序，标题和说明都用中文）\n\
\n\
特别要求：\n\
- 不要假设这里一定有源码仓库\n\
- 目录可能为空，也可能只有 Word、Excel、PDF、图片或零散资料\n\
- 如遇不可直接读取的文件，请基于文件名、目录结构和周边材料做谨慎判断\n\
- 如果目录几乎为空，要明确说明现状，并给出下一步建议\n\
\n\
当前工作目录：\n{}",
            workspace_list
        ),
        status: TaskStatus::Open,
        creator_user_id: None,
        assignee_user_id: None,
        claimed_by: None,
        activities: vec![new_activity(
            "task.explore_created",
            format!("已为项目“{}”创建探索任务", project.name),
        )],
        runtime: None,
    }
}

fn mark_task_running(
    state: &mut BoardState,
    task_id: Uuid,
    agent_id: Uuid,
    agent_name: &str,
    prompt: &str,
    thread_id: Option<String>,
    turn_id: Option<String>,
    git_auto_merge_enabled: bool,
) -> AppResult<()> {
    let task_title = {
        let task = find_task_mut(state, task_id)?;
        if let Some(claimed_by) = task.claimed_by {
            if claimed_by != agent_id {
                return Err((StatusCode::CONFLICT, "任务已被其他 Agent 认领".into()));
            }
        }
        if matches!(task.status, TaskStatus::Running | TaskStatus::Done) {
            return Err((StatusCode::CONFLICT, "当前任务状态不允许启动".into()));
        }

        task.status = TaskStatus::Running;
        task.claimed_by = Some(agent_id);
        task.activities.push(new_activity(
            "agent.invoked",
            format!("已由 {agent_name} 开始执行"),
        ));
        let runtime = task.runtime.get_or_insert_with(|| TaskRuntime {
            provider: "codex".into(),
            thread_id: None,
            active_turn_id: None,
            git_auto_merge_enabled: false,
            log: Vec::new(),
            last_error: None,
        });
        runtime.thread_id = thread_id;
        runtime.active_turn_id = turn_id;
        runtime.git_auto_merge_enabled = git_auto_merge_enabled;
        runtime
            .log
            .push(new_runtime_entry("user", prompt.to_string()));
        task.title.clone()
    };

    assign_agent_running(
        state,
        agent_id,
        task_id,
        format!("正在执行任务：{task_title}"),
    );
    Ok(())
}

fn assign_agent_running(state: &mut BoardState, agent_id: Uuid, task_id: Uuid, action: String) {
    if let Some(agent) = state.agents.iter_mut().find(|agent| agent.id == agent_id) {
        agent.status = "运行中".into();
        agent.current_task_id = Some(task_id);
        agent.last_action = action;
    }
}

fn reset_agent_if_needed(state: &mut BoardState, task_id: Uuid, action: &str) {
    if let Some(agent) = state
        .agents
        .iter_mut()
        .find(|agent| agent.current_task_id == Some(task_id))
    {
        agent.status = "空闲".into();
        agent.current_task_id = None;
        agent.last_action = action.into();
    }
}

fn push_runtime_delta(log: &mut Vec<RuntimeLogEntry>, kind: &str, delta: &str) {
    if delta.trim().is_empty() {
        return;
    }
    if let Some(last) = log.last_mut() {
        if last.kind == kind {
            last.message.push_str(delta);
            return;
        }
    }
    log.push(new_runtime_entry(kind, delta.to_string()));
}

fn task_developer_instructions() -> String {
    [
        "你是 Spotlight 的本地工程 Agent。",
        "你需要在当前工作目录内完成软件任务，并保持结果可回顾。",
        "优先输出清晰的计划、执行过程、命令结果、风险判断和最终结论。",
        "如果用户暂停后补充提示词，要在同一线程里继续推进，不要丢失上下文。",
        "平台会在任务启动前自动从主分支切出任务分支，并在任务完成后尝试按门禁规则合并回主分支。",
        "除非任务明确要求，不要自行执行危险 Git 历史改写，也不要跳过测试就声称可以合并。",
    ]
    .join(" ")
}

async fn record_task_activity(
    state: &AppState,
    task_id: Uuid,
    kind: impl Into<String>,
    message: impl Into<String>,
) {
    let mut guard = state.inner.lock().await;
    if let Ok(task) = find_task_mut(&mut guard, task_id) {
        task.activities.push(new_activity(kind, message));
    }
}

async fn git_command_output(
    workspace_root: &Path,
    args: &[&str],
) -> Result<std::process::Output, String> {
    Command::new("git")
        .args(args)
        .current_dir(workspace_root)
        .output()
        .await
        .map_err(|error| format!("执行 git {:?} 失败：{error}", args))
}

fn git_stderr_message(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }

    "git 命令失败，但没有返回详细输出".into()
}

async fn git_stdout_trimmed(workspace_root: &Path, args: &[&str]) -> Option<String> {
    let output = git_command_output(workspace_root, args).await.ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!stdout.is_empty()).then_some(stdout)
}

async fn git_ref_exists(workspace_root: &Path, reference: &str) -> bool {
    git_command_output(
        workspace_root,
        &["show-ref", "--verify", "--quiet", reference],
    )
    .await
    .map(|output| output.status.success())
    .unwrap_or(false)
}

async fn is_git_repo(workspace_root: &Path) -> bool {
    git_command_output(workspace_root, &["rev-parse", "--is-inside-work-tree"])
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

async fn git_worktree_dirty(workspace_root: &Path) -> Result<bool, String> {
    let output = git_command_output(workspace_root, &["status", "--porcelain"]).await?;
    if !output.status.success() {
        return Err(git_stderr_message(&output));
    }

    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

async fn detect_primary_remote(workspace_root: &Path) -> Option<String> {
    if git_command_output(workspace_root, &["remote", "get-url", "origin"])
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
    {
        return Some("origin".into());
    }

    git_stdout_trimmed(workspace_root, &["remote"])
        .await
        .and_then(|stdout| stdout.lines().map(str::trim).find(|line| !line.is_empty()).map(str::to_string))
}

async fn detect_base_branch(workspace_root: &Path, remote_name: Option<&str>) -> String {
    if let Some(remote) = remote_name {
        let remote_head = format!("refs/remotes/{remote}/HEAD");
        if let Some(symbolic_ref) =
            git_stdout_trimmed(workspace_root, &["symbolic-ref", &remote_head]).await
        {
            if let Some((_, branch)) = symbolic_ref.rsplit_once('/') {
                if !branch.trim().is_empty() {
                    return branch.trim().to_string();
                }
            }
        }
    }

    for candidate in ["main", "master"] {
        let local_ref = format!("refs/heads/{candidate}");
        if git_ref_exists(workspace_root, &local_ref).await {
            return candidate.to_string();
        }
    }

    git_stdout_trimmed(workspace_root, &["branch", "--show-current"])
        .await
        .filter(|branch| !branch.trim().is_empty())
        .unwrap_or_else(|| "main".into())
}

async fn detect_git_task_branch_plan(workspace_root: &Path, task_id: Uuid) -> GitTaskBranchPlan {
    let remote_name = detect_primary_remote(workspace_root).await;
    let base_branch = detect_base_branch(workspace_root, remote_name.as_deref()).await;

    GitTaskBranchPlan {
        base_branch,
        task_branch: format!("task/{task_id}"),
        remote_name,
    }
}

async fn prepare_git_task_branch_in_repo(
    workspace_root: &Path,
    task_id: Uuid,
) -> AppResult<GitPrepareResult> {
    let mut activities = Vec::new();

    if !is_git_repo(workspace_root).await {
        activities.push((
            "git.branch_prepare_skipped".into(),
            "当前工作目录不是 Git 仓库，跳过任务分支预处理。".into(),
        ));
        return Ok(GitPrepareResult {
            activities,
            auto_merge_enabled: false,
        });
    }

    if git_worktree_dirty(workspace_root)
        .await
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message.clone()))?
    {
        let _message =
            "任务启动前检测到 Git 工作区存在未提交改动，已阻止自动切换主分支并创建任务分支。".to_string();
        activities.push((
            "git.branch_prepare_skipped".into(),
            "任务启动前检测到 Git 工作区存在未提交改动，本次跳过自动切换主分支、任务分支创建和后续自动合并；任务仍会继续执行。".into(),
        ));
        return Ok(GitPrepareResult {
            activities,
            auto_merge_enabled: false,
        });
    }

    let plan = detect_git_task_branch_plan(workspace_root, task_id).await;
    activities.push((
        "git.branch_plan".into(),
        format!(
            "任务 Git 计划：主分支={}，任务分支={}，远端={}",
            plan.base_branch,
            plan.task_branch,
            plan.remote_name.as_deref().unwrap_or("无")
        ),
    ));

    if let Some(remote) = plan.remote_name.as_deref() {
        let fetch = git_command_output(workspace_root, &["fetch", remote]).await;
        match fetch {
            Ok(output) if output.status.success() => activities.push((
                "git.remote_fetched".into(),
                format!("已获取远端 {remote} 的最新引用。"),
            )),
            Ok(output) => {
                let message = format!("获取远端 {remote} 失败：{}", git_stderr_message(&output));
                activities.push(("git.branch_prepare_failed".into(), message.clone()));
                return Err((StatusCode::BAD_GATEWAY, message));
            }
            Err(error) => {
                activities.push(("git.branch_prepare_failed".into(), error.clone()));
                return Err((StatusCode::BAD_GATEWAY, error));
            }
        }
    }

    let checkout_base =
        git_command_output(workspace_root, &["checkout", &plan.base_branch]).await;
    match checkout_base {
        Ok(output) if output.status.success() => activities.push((
            "git.base_checked_out".into(),
            format!("已切换到主分支 {}。", plan.base_branch),
        )),
        Ok(output) => {
            let message = format!(
                "切换到主分支 {} 失败：{}",
                plan.base_branch,
                git_stderr_message(&output)
            );
            activities.push(("git.branch_prepare_failed".into(), message.clone()));
            return Err((StatusCode::CONFLICT, message));
        }
        Err(error) => {
            activities.push(("git.branch_prepare_failed".into(), error.clone()));
            return Err((StatusCode::CONFLICT, error));
        }
    }

    if let Some(remote) = plan.remote_name.as_deref() {
        let remote_branch_ref = format!("refs/remotes/{remote}/{}", plan.base_branch);
        if git_ref_exists(workspace_root, &remote_branch_ref).await {
            let upstream = format!("{remote}/{}", plan.base_branch);
            let update_main =
                git_command_output(workspace_root, &["merge", "--ff-only", &upstream]).await;
            match update_main {
                Ok(output) if output.status.success() => activities.push((
                    "git.base_updated".into(),
                    format!("已使用 {upstream} 快进更新本地主分支。"),
                )),
                Ok(output) => {
                    let message = format!(
                        "主分支在任务启动前无法快进到 {upstream}：{}",
                        git_stderr_message(&output)
                    );
                    activities.push(("git.branch_prepare_failed".into(), message.clone()));
                    return Err((StatusCode::CONFLICT, message));
                }
                Err(error) => {
                    activities.push(("git.branch_prepare_failed".into(), error.clone()));
                    return Err((StatusCode::CONFLICT, error));
                }
            }
        } else {
            activities.push((
                "git.base_update_skipped".into(),
                format!("远端 {remote} 上未找到 {}/{}，跳过主分支快进。", remote, plan.base_branch),
            ));
        }
    }

    let task_branch_ref = format!("refs/heads/{}", plan.task_branch);
    let branch_exists = git_ref_exists(workspace_root, &task_branch_ref).await;
    let checkout_task_args = if branch_exists {
        vec!["checkout", plan.task_branch.as_str()]
    } else {
        vec!["checkout", "-b", plan.task_branch.as_str()]
    };
    let checkout_task = git_command_output(workspace_root, &checkout_task_args).await;
    match checkout_task {
        Ok(output) if output.status.success() => activities.push((
            if branch_exists {
                "git.task_branch_reused".into()
            } else {
                "git.task_branch_created".into()
            },
            if branch_exists {
                format!("已切换到已存在的任务分支 {}。", plan.task_branch)
            } else {
                format!(
                    "已基于主分支 {} 创建任务分支 {}。",
                    plan.base_branch, plan.task_branch
                )
            },
        )),
        Ok(output) => {
            let message = format!(
                "切换到任务分支 {} 失败：{}",
                plan.task_branch,
                git_stderr_message(&output)
            );
            activities.push(("git.branch_prepare_failed".into(), message.clone()));
            return Err((StatusCode::CONFLICT, message));
        }
        Err(error) => {
            activities.push(("git.branch_prepare_failed".into(), error.clone()));
            return Err((StatusCode::CONFLICT, error));
        }
    }

    Ok(GitPrepareResult {
        activities,
        auto_merge_enabled: true,
    })
}

async fn finalize_git_task_branch_in_repo(
    workspace_root: &Path,
    task_id: Uuid,
) -> Vec<(String, String)> {
    let mut activities = Vec::new();

    if !is_git_repo(workspace_root).await {
        activities.push((
            "git.merge_skipped".into(),
            "当前工作目录不是 Git 仓库，跳过任务完成后的自动合并。".into(),
        ));
        return activities;
    }

    let plan = detect_git_task_branch_plan(workspace_root, task_id).await;
    activities.push((
        "git.merge_plan".into(),
        format!(
            "任务完成后的 Git 合并计划：主分支={}，任务分支={}，远端={}",
            plan.base_branch,
            plan.task_branch,
            plan.remote_name.as_deref().unwrap_or("无")
        ),
    ));

    let current_branch = git_stdout_trimmed(workspace_root, &["branch", "--show-current"])
        .await
        .unwrap_or_default();
    if current_branch.trim() != plan.task_branch {
        let task_branch_ref = format!("refs/heads/{}", plan.task_branch);
        if !git_ref_exists(workspace_root, &task_branch_ref).await {
            activities.push((
                "git.merge_skipped".into(),
                format!("未找到任务分支 {}，跳过自动合并。", plan.task_branch),
            ));
            return activities;
        }

        match git_command_output(workspace_root, &["checkout", &plan.task_branch]).await {
            Ok(output) if output.status.success() => activities.push((
                "git.task_branch_checked_out".into(),
                format!("自动合并前已切换回任务分支 {}。", plan.task_branch),
            )),
            Ok(output) => {
                activities.push((
                    "git.merge_blocked".into(),
                    format!(
                        "自动合并前无法切换到任务分支 {}：{}",
                        plan.task_branch,
                        git_stderr_message(&output)
                    ),
                ));
                return activities;
            }
            Err(error) => {
                activities.push(("git.merge_blocked".into(), error));
                return activities;
            }
        }
    }

    match git_worktree_dirty(workspace_root).await {
        Ok(true) => {
            match git_command_output(workspace_root, &["add", "-A"]).await {
                Ok(output) if output.status.success() => {}
                Ok(output) => {
                    activities.push((
                        "git.merge_blocked".into(),
                        format!("自动提交前执行 git add -A 失败：{}", git_stderr_message(&output)),
                    ));
                    return activities;
                }
                Err(error) => {
                    activities.push(("git.merge_blocked".into(), error));
                    return activities;
                }
            }

            let cached_clean = git_command_output(
                workspace_root,
                &["diff", "--cached", "--quiet", "--exit-code"],
            )
            .await;
            let needs_commit = match cached_clean {
                Ok(output) => !output.status.success(),
                Err(error) => {
                    activities.push(("git.merge_blocked".into(), error));
                    return activities;
                }
            };

            if needs_commit {
                let commit_message = format!("chore(task): 完成任务 {task_id}");
                match git_command_output(
                    workspace_root,
                    &["commit", "-m", commit_message.as_str()],
                )
                .await
                {
                    Ok(output) if output.status.success() => activities.push((
                        "git.task_branch_committed".into(),
                        format!("已自动提交任务分支 {} 上的改动。", plan.task_branch),
                    )),
                    Ok(output) => {
                        activities.push((
                            "git.merge_blocked".into(),
                            format!(
                                "任务分支自动提交失败，已保留分支 {}：{}",
                                plan.task_branch,
                                git_stderr_message(&output)
                            ),
                        ));
                        return activities;
                    }
                    Err(error) => {
                        activities.push(("git.merge_blocked".into(), error));
                        return activities;
                    }
                }
            }
        }
        Ok(false) => activities.push((
            "git.task_branch_clean".into(),
            format!("任务分支 {} 没有额外未提交改动。", plan.task_branch),
        )),
        Err(error) => {
            activities.push(("git.merge_blocked".into(), error));
            return activities;
        }
    }

    if let Some(remote) = plan.remote_name.as_deref() {
        match git_command_output(workspace_root, &["fetch", remote]).await {
            Ok(output) if output.status.success() => activities.push((
                "git.remote_refetched".into(),
                format!("自动合并前已重新获取远端 {remote} 的最新引用。"),
            )),
            Ok(output) => {
                activities.push((
                    "git.merge_blocked".into(),
                    format!("自动合并前获取远端 {remote} 失败：{}", git_stderr_message(&output)),
                ));
                return activities;
            }
            Err(error) => {
                activities.push(("git.merge_blocked".into(), error));
                return activities;
            }
        }
    }

    match git_command_output(workspace_root, &["checkout", &plan.base_branch]).await {
        Ok(output) if output.status.success() => activities.push((
            "git.base_checked_out".into(),
            format!("自动合并前已切换回主分支 {}。", plan.base_branch),
        )),
        Ok(output) => {
            activities.push((
                "git.merge_blocked".into(),
                format!(
                    "自动合并前无法切换到主分支 {}：{}",
                    plan.base_branch,
                    git_stderr_message(&output)
                ),
            ));
            let _ = git_command_output(workspace_root, &["checkout", &plan.task_branch]).await;
            return activities;
        }
        Err(error) => {
            activities.push(("git.merge_blocked".into(), error));
            let _ = git_command_output(workspace_root, &["checkout", &plan.task_branch]).await;
            return activities;
        }
    }

    if let Some(remote) = plan.remote_name.as_deref() {
        let remote_branch_ref = format!("refs/remotes/{remote}/{}", plan.base_branch);
        if git_ref_exists(workspace_root, &remote_branch_ref).await {
            let upstream = format!("{remote}/{}", plan.base_branch);
            match git_command_output(workspace_root, &["merge", "--ff-only", &upstream]).await {
                Ok(output) if output.status.success() => activities.push((
                    "git.base_updated".into(),
                    format!("自动合并前已使用 {upstream} 快进更新主分支。"),
                )),
                Ok(output) => {
                    activities.push((
                        "git.merge_blocked".into(),
                        format!(
                            "自动合并前无法先快进主分支到 {upstream}：{}",
                            git_stderr_message(&output)
                        ),
                    ));
                    let _ = git_command_output(workspace_root, &["checkout", &plan.task_branch]).await;
                    return activities;
                }
                Err(error) => {
                    activities.push(("git.merge_blocked".into(), error));
                    let _ = git_command_output(workspace_root, &["checkout", &plan.task_branch]).await;
                    return activities;
                }
            }
        }
    }

    let merge_message = format!("merge task branch {} for {}", plan.task_branch, task_id);
    match git_command_output(
        workspace_root,
        &["merge", "--no-ff", "-m", merge_message.as_str(), &plan.task_branch],
    )
    .await
    {
        Ok(output) if output.status.success() => activities.push((
            "git.merge_completed".into(),
            format!(
                "已将任务分支 {} 合并回主分支 {}。",
                plan.task_branch, plan.base_branch
            ),
        )),
        Ok(output) => {
            let details = git_stderr_message(&output);
            let _ = git_command_output(workspace_root, &["merge", "--abort"]).await;
            let _ = git_command_output(workspace_root, &["checkout", &plan.task_branch]).await;
            activities.push((
                "git.merge_blocked".into(),
                format!(
                    "任务分支 {} 未能自动合并回主分支 {}，已保留任务分支等待人工处理：{}",
                    plan.task_branch, plan.base_branch, details
                ),
            ));
        }
        Err(error) => {
            let _ = git_command_output(workspace_root, &["merge", "--abort"]).await;
            let _ = git_command_output(workspace_root, &["checkout", &plan.task_branch]).await;
            activities.push((
                "git.merge_blocked".into(),
                format!(
                    "任务分支 {} 自动合并失败，已保留任务分支：{}",
                    plan.task_branch, error
                ),
            ));
        }
    }

    activities
}

async fn apply_git_snapshot(workspace_root: &Path, task_id: Uuid, state: &AppState) {
    let repo_check = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(workspace_root)
        .output()
        .await;
    let Ok(repo_check) = repo_check else {
        return;
    };
    if !repo_check.status.success() {
        return;
    }

    let dirty = git_worktree_dirty(workspace_root).await.unwrap_or(false);
    let branch = git_stdout_trimmed(workspace_root, &["branch", "--show-current"])
        .await
        .unwrap_or_else(|| "unknown".into());
    let head = git_stdout_trimmed(workspace_root, &["rev-parse", "HEAD"])
        .await
        .unwrap_or_else(|| "unknown".into());

    let tag = format!("task/{task_id}/pre-run/{}", timestamp_compact());
    let result = Command::new("git")
        .args(["tag", &tag])
        .current_dir(workspace_root)
        .output()
        .await;

    let message = match result {
        Ok(output) if output.status.success() => {
            format!("已创建预执行 tag：{tag}，branch={branch}，HEAD={head}，dirty={dirty}")
        }
        Ok(output) => format!(
            "创建预执行 tag 失败：branch={branch}，HEAD={head}，dirty={dirty}，{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(error) => format!(
            "创建预执行 tag 失败：branch={branch}，HEAD={head}，dirty={dirty}，{error}"
        ),
    };

    let mut guard = state.inner.lock().await;
    if let Ok(task) = find_task_mut(&mut guard, task_id) {
        task.activities
            .push(new_activity("git.pre_run_snapshot", message));
    }
}

fn timestamp_compact() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        build_app, detect_project_stack, finalize_git_task_branch_in_repo,
        prepare_git_task_branch_in_repo, sanitize_credential_hint, ProjectContextSnapshot,
        RuntimeMode,
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use platform_core::{
        AgentInvocationRequest, AgentResumeRequest, BoardSnapshot, CreateTaskRequest, Project,
        Task,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command as StdCommand,
        time::{SystemTime, UNIX_EPOCH},
    };
    use tower::util::ServiceExt;
    use uuid::Uuid;

    fn test_app() -> axum::Router {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("failed to resolve workspace root")
            .to_path_buf();
        build_app(RuntimeMode::Stub, workspace_root)
    }

    async fn read_snapshot(response: axum::response::Response) -> BoardSnapshot {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    async fn read_task(response: axum::response::Response) -> Task {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    async fn read_json<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn unique_temp_path(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}"))
    }

    fn run_git_sync(workspace_root: &Path, args: &[&str]) -> std::process::Output {
        StdCommand::new("git")
            .args(args)
            .current_dir(workspace_root)
            .output()
            .unwrap()
    }

    fn ensure_git_ok(workspace_root: &Path, args: &[&str]) -> String {
        let output = run_git_sync(workspace_root, args);
        if !output.status.success() {
            panic!(
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init_git_test_workspace() -> PathBuf {
        let root = unique_temp_path("spotlight-git-workflow");
        let remote = root.join("remote.git");
        let local = root.join("local");

        fs::create_dir_all(&root).unwrap();
        ensure_git_ok(&root, &["init", "--bare", "--initial-branch=main", remote.to_str().unwrap()]);
        ensure_git_ok(&root, &["clone", remote.to_str().unwrap(), local.to_str().unwrap()]);
        ensure_git_ok(&local, &["config", "user.name", "Spotlight Test"]);
        ensure_git_ok(&local, &["config", "user.email", "spotlight@example.com"]);

        fs::write(local.join("README.md"), "hello\n").unwrap();
        ensure_git_ok(&local, &["add", "README.md"]);
        ensure_git_ok(&local, &["commit", "-m", "init"]);
        ensure_git_ok(&local, &["push", "-u", "origin", "main"]);

        local
    }

    #[tokio::test]
    async fn board_returns_projects_tasks_and_agents() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let snapshot = read_snapshot(response).await;
        assert!(snapshot.projects.len() >= 2);
        assert!(!snapshot.projects[0].is_spotlight_self);
        assert!(!snapshot.tasks.is_empty());
        assert!(!snapshot.agents.is_empty());
    }

    #[tokio::test]
    async fn bootstrap_endpoint_reads_agents_plan_for_spotlight_project() {
        let app = test_app();
        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        let spotlight_project = snapshot
            .projects
            .iter()
            .find(|project| project.is_spotlight_self)
            .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/projects/{}/tasks/bootstrap",
                        spotlight_project.id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let updated_snapshot = read_snapshot(response).await;
        assert!(updated_snapshot
            .tasks
            .iter()
            .any(|task| task.title.contains("[0.1.0]")));
    }

    #[tokio::test]
    async fn create_task_adds_new_item_to_selected_project() {
        let app = test_app();
        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        let project_id = snapshot.projects.first().unwrap().id;

        let payload = serde_json::to_vec(&CreateTaskRequest {
            project_id: Some(project_id),
            title: "补充回归测试".into(),
            description: "把暂停恢复和长会话状态流转补到测试里。".into(),
        })
        .unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let board_response = app
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        assert!(snapshot
            .tasks
            .iter()
            .any(|task| { task.project_id == project_id && task.title == "补充回归测试" }));
    }

    #[tokio::test]
    async fn login_and_me_endpoints_return_current_user() {
        let app = test_app();
        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        let username = snapshot.users.first().unwrap().username.clone();

        let login_payload = serde_json::json!({ "username": username });
        let login_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(login_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login_response.status(), StatusCode::OK);
        assert!(login_response.headers().contains_key("set-cookie"));

        let me_response = app
            .oneshot(
                Request::builder()
                    .uri("/api/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(me_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn created_task_records_creator_user() {
        let app = test_app();
        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        let project_id = snapshot.projects.first().unwrap().id;
        let creator_id = snapshot.current_user.as_ref().unwrap().id;

        let payload = serde_json::to_vec(&CreateTaskRequest {
            project_id: Some(project_id),
            title: "记录创建者".into(),
            description: "验证新建任务会记录当前用户".into(),
        })
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let task = read_task(response).await;
        assert_eq!(task.creator_user_id, Some(creator_id));
    }

    #[tokio::test]
    async fn register_workspace_updates_project_primary_directory() {
        let app = test_app();
        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        let project_id = snapshot.projects.first().unwrap().id;

        let workspace_root = unique_temp_path("spotlight-project-workspace");
        fs::create_dir_all(&workspace_root).unwrap();

        let payload = serde_json::json!({
            "label": "backend",
            "path": workspace_root.to_string_lossy(),
            "isPrimaryDefault": true,
            "isWritable": true
        });
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/projects/{project_id}/workspaces"))
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let updated_project: Project = read_json(response).await;
        assert_eq!(updated_project.workspace_roots.first().unwrap().label, "backend");
        assert!(
            updated_project.workspace_roots.first().unwrap().path.contains("spotlight-project-workspace")
        );
    }

    #[tokio::test]
    async fn scan_endpoint_returns_workspace_summary() {
        let app = test_app();
        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        let project_id = snapshot.projects.first().unwrap().id;

        let workspace_root = unique_temp_path("spotlight-scan-workspace");
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        fs::write(workspace_root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        fs::write(workspace_root.join("README.md"), "# demo\n").unwrap();
        fs::write(workspace_root.join("src").join("main.rs"), "fn main() {}\n").unwrap();

        let register_payload = serde_json::json!({
            "label": "demo-root",
            "path": workspace_root.to_string_lossy(),
            "isPrimaryDefault": true,
            "isWritable": true
        });
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/projects/{project_id}/workspaces"))
                    .header("content-type", "application/json")
                    .body(Body::from(register_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/projects/{project_id}/scan"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let context: ProjectContextSnapshot = read_json(response).await;
        let scan = context.latest_scan.expect("expected latest scan summary");
        assert!(scan.detected_stacks.iter().any(|stack| stack == "Rust"));
        assert!(scan.key_files.iter().any(|path| path.ends_with("Cargo.toml")));
    }

    #[tokio::test]
    async fn project_session_stub_flow_records_messages() {
        let app = test_app();
        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        let project_id = snapshot.projects.first().unwrap().id;

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/projects/{project_id}/sessions"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "title": "项目问答",
                            "prompt": "这个项目现在最缺哪一块最小可用能力？"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let context: ProjectContextSnapshot = read_json(response).await;
        let session = context.sessions.first().expect("expected project session");
        assert_eq!(session.status, "completed");
        assert_eq!(session.messages.first().unwrap().role, "user");
        assert_eq!(session.messages.last().unwrap().role, "assistant");
        assert!(!session.log.is_empty());
    }

    #[tokio::test]
    async fn local_build_restart_endpoint_creates_task_with_stack_hint() {
        let app = test_app();
        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        let spotlight_project = snapshot
            .projects
            .iter()
            .find(|project| project.is_spotlight_self)
            .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/projects/{}/tasks/local-build-restart",
                        spotlight_project.id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let task = read_task(response).await;
        assert_eq!(task.project_id, spotlight_project.id);
        assert_eq!(task.title, "本地编译重启");
        assert!(task.description.contains("deploy.md"));
        assert!(task.description.contains("Rust"));
    }

    #[tokio::test]
    async fn cloud_install_restart_endpoint_redacts_password_like_hint() {
        let app = test_app();
        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        let project_id = snapshot.projects.first().unwrap().id;

        let payload = serde_json::json!({
            "host": "10.0.0.8",
            "port": 22,
            "username": "deploy",
            "auth_method": "密码",
            "credential_hint": "super-secret",
            "deploy_path": "/srv/spotlight",
            "service_hint": "systemctl restart spotlight"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/projects/{}/tasks/cloud-install-restart",
                        project_id
                    ))
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let task = read_task(response).await;
        assert_eq!(task.title, "云端安装重启");
        assert!(task.description.contains("10.0.0.8"));
        assert!(task.description.contains("已收到密码类凭据"));
        assert!(!task.description.contains("super-secret"));
    }

    #[tokio::test]
    async fn explore_endpoint_creates_canned_task_for_project() {
        let app = test_app();
        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        let project_id = snapshot.projects.first().unwrap().id;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/projects/{project_id}/explore"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let snapshot = read_snapshot(response).await;
        assert!(snapshot.tasks.iter().any(|task| {
            task.project_id == project_id && task.title == "探索当前目录并生成建议任务"
        }));
    }

    #[tokio::test]
    async fn toggle_agent_auto_mode_updates_board_state() {
        let app = test_app();
        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        let agent = snapshot.agents.first().unwrap().clone();

        let toggle_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/agents/{}/auto-mode/toggle", agent.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(toggle_response.status(), StatusCode::OK);

        let toggled_snapshot = read_snapshot(toggle_response).await;
        let updated_agent = toggled_snapshot
            .agents
            .iter()
            .find(|candidate| candidate.id == agent.id)
            .unwrap();
        assert_ne!(updated_agent.auto_mode, agent.auto_mode);
    }

    #[tokio::test]
    async fn claim_start_pause_resume_flow_works_in_stub_mode() {
        let app = test_app();
        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        let project_id = snapshot.projects.first().unwrap().id;
        let agent = snapshot.agents.first().unwrap().clone();

        let create_payload = serde_json::to_vec(&CreateTaskRequest {
            project_id: Some(project_id),
            title: "验证暂停恢复流程".into(),
            description: "启动后暂停，再补充提示词并恢复，最后验证状态流转。".into(),
        })
        .unwrap();
        let create_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(create_payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_response.status(), StatusCode::OK);
        let created_task = {
            let body = axum::body::to_bytes(create_response.into_body(), usize::MAX)
                .await
                .unwrap();
            serde_json::from_slice::<platform_core::Task>(&body).unwrap()
        };

        let start_payload = serde_json::to_vec(&AgentInvocationRequest {
            agent_name_hint: agent.name.clone(),
            prompt: Some("请先输出计划，再模拟开始执行。".into()),
        })
        .unwrap();
        let start_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tasks/{}/start/{}", created_task.id, agent.id))
                    .header("content-type", "application/json")
                    .body(Body::from(start_payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::OK);

        let pause_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tasks/{}/pause", created_task.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pause_response.status(), StatusCode::OK);

        let resume_payload = serde_json::to_vec(&AgentResumeRequest {
            agent_name_hint: agent.name.clone(),
            prompt: "我补充了一段提示词，请继续完成任务。".into(),
        })
        .unwrap();
        let resume_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/tasks/{}/resume/{}",
                        created_task.id, agent.id
                    ))
                    .header("content-type", "application/json")
                    .body(Body::from(resume_payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resume_response.status(), StatusCode::OK);

        let final_board = app
            .oneshot(
                Request::builder()
                    .uri("/api/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let final_snapshot = read_snapshot(final_board).await;
        let updated_task = final_snapshot
            .tasks
            .iter()
            .find(|candidate| candidate.id == created_task.id)
            .unwrap();
        assert_eq!(updated_task.status.as_str(), "DONE");
        assert!(updated_task.runtime.is_some());
        assert!(updated_task
            .runtime
            .as_ref()
            .unwrap()
            .log
            .iter()
            .any(|entry| entry.message.contains("补充")));
        let updated_agent = final_snapshot
            .agents
            .iter()
            .find(|candidate| candidate.id == agent.id)
            .unwrap();
        assert_eq!(updated_agent.status, "空闲");
    }

    #[test]
    fn detect_project_stack_can_find_nested_python_workspace() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("spotlight-stack-test-{unique}"));
        let nested_dir = temp_root.join("services").join("api");
        std::fs::create_dir_all(&nested_dir).unwrap();
        std::fs::write(
            nested_dir.join("pyproject.toml"),
            "[project]\nname='demo'\n",
        )
        .unwrap();

        let detection = detect_project_stack(&temp_root);

        assert!(detection.stacks.contains(&"Python"));
        assert!(detection
            .evidence
            .iter()
            .any(|item| item.contains("pyproject.toml")));

        let _ = std::fs::remove_dir_all(temp_root);
    }

    #[test]
    fn sanitize_credential_hint_redacts_plaintext_passwords() {
        let hint = sanitize_credential_hint("密码", Some("plain-text-password"));
        assert!(hint.contains("不在任务描述中回显"));
        assert!(!hint.contains("plain-text-password"));
    }

    #[tokio::test]
    async fn prepare_git_task_branch_creates_branch_from_updated_main() {
        let workspace_root = init_git_test_workspace();
        let task_id = Uuid::from_u128(42);

        let prepare = prepare_git_task_branch_in_repo(&workspace_root, task_id)
            .await
            .unwrap();
        let activities = prepare.activities;

        let current_branch = ensure_git_ok(&workspace_root, &["branch", "--show-current"]);
        assert_eq!(current_branch, format!("task/{task_id}"));
        assert!(activities
            .iter()
            .any(|(kind, _)| kind == "git.task_branch_created"));

        let _ = fs::remove_dir_all(
            workspace_root
                .parent()
                .expect("local clone should have parent directory"),
        );
    }

    #[tokio::test]
    async fn finalize_git_task_branch_commits_and_merges_back_to_main() {
        let workspace_root = init_git_test_workspace();
        let task_id = Uuid::from_u128(84);

        prepare_git_task_branch_in_repo(&workspace_root, task_id)
            .await
            .unwrap();
        fs::write(workspace_root.join("feature.txt"), "from task branch\n").unwrap();

        let activities = finalize_git_task_branch_in_repo(&workspace_root, task_id).await;

        let current_branch = ensure_git_ok(&workspace_root, &["branch", "--show-current"]);
        assert_eq!(current_branch, "main");
        assert!(workspace_root.join("feature.txt").exists());
        assert!(activities
            .iter()
            .any(|(kind, _)| kind == "git.merge_completed"));

        let merged_branches = ensure_git_ok(
            &workspace_root,
            &["branch", "--merged", "main"],
        );
        assert!(merged_branches.contains(&format!("task/{task_id}")));

        let _ = fs::remove_dir_all(
            workspace_root
                .parent()
                .expect("local clone should have parent directory"),
        );
    }

    #[tokio::test]
    async fn prepare_git_task_branch_skips_auto_branching_when_worktree_is_dirty() {
        let workspace_root = init_git_test_workspace();
        let task_id = Uuid::from_u128(126);

        fs::write(workspace_root.join("dirty.txt"), "local change\n").unwrap();

        let prepare = prepare_git_task_branch_in_repo(&workspace_root, task_id)
            .await
            .unwrap();

        let current_branch = ensure_git_ok(&workspace_root, &["branch", "--show-current"]);
        assert_eq!(current_branch, "main");
        assert!(!prepare.auto_merge_enabled);
        assert!(prepare
            .activities
            .iter()
            .any(|(kind, _)| kind == "git.branch_prepare_skipped"));

        let _ = fs::remove_dir_all(
            workspace_root
                .parent()
                .expect("local clone should have parent directory"),
        );
    }
}
