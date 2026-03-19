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
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use platform_core::{
    merge_unique_tasks, new_activity, new_runtime_entry, seed_tasks_from_agents_markdown,
    seed_tasks_from_docs, Agent, AgentInvocationRequest, AgentResumeRequest, BoardSnapshot,
    CreateTaskRequest, Project, RuntimeLogEntry, Task, TaskRuntime, TaskStatus, WorkspaceRoot,
};
use runtime::{CodexRuntimeSession, RuntimeEvent};
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
}

#[derive(Clone)]
struct BoardState {
    projects: Vec<Project>,
    tasks: Vec<Task>,
    agents: Vec<Agent>,
}

#[derive(Clone)]
struct TaskExecutionContext {
    workspace_root: PathBuf,
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
        .route("/api/board", get(get_board))
        .route("/api/tasks", post(create_task))
        .route(
            "/api/projects/{project_id}/tasks/bootstrap",
            post(bootstrap_tasks),
        )
        .route(
            "/api/projects/{project_id}/tasks/seed-docs",
            post(seed_doc_tasks),
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
    let projects = default_projects(&workspace_root);
    let mut tasks = Vec::new();
    if let Some(spotlight_project) = projects.iter().find(|project| project.is_spotlight_self) {
        merge_unique_tasks(&mut tasks, seed_tasks_from_docs(spotlight_project.id));
        merge_unique_tasks(&mut tasks, seed_tasks_from_agents_file(spotlight_project));
    }

    AppState {
        inner: Arc::new(Mutex::new(BoardState {
            projects,
            tasks,
            agents: default_agents(),
        })),
        runtime_mode,
        runtime_sessions: Arc::new(Mutex::new(HashMap::new())),
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

fn default_agents() -> Vec<Agent> {
    vec![
        Agent {
            id: Uuid::new_v4(),
            name: "本地 Codex Agent".into(),
            provider: "codex".into(),
            status: "空闲".into(),
            auto_mode: true,
            current_task_id: None,
            last_action: "等待认领或自动执行任务".into(),
        },
        Agent {
            id: Uuid::new_v4(),
            name: "评审助理".into(),
            provider: "codex".into(),
            status: "空闲".into(),
            auto_mode: true,
            current_task_id: None,
            last_action: "等待补充提示词、验收说明或人工协作".into(),
        },
    ]
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

async fn get_board(State(state): State<AppState>) -> Json<BoardSnapshot> {
    let guard = state.inner.lock().await;
    Json(snapshot_from_state(&guard))
}

async fn create_task(
    State(state): State<AppState>,
    Json(request): Json<CreateTaskRequest>,
) -> AppResult<Json<Task>> {
    let title = request.title.trim();
    let description = request.description.trim();
    if title.is_empty() || description.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "标题和描述不能为空".into()));
    }

    let mut guard = state.inner.lock().await;
    let project = resolve_project_for_new_task(&guard, request.project_id)?.clone();
    let task = Task {
        id: Uuid::new_v4(),
        project_id: project.id,
        title: title.to_string(),
        description: description.to_string(),
        status: TaskStatus::Open,
        claimed_by: None,
        activities: vec![new_activity(
            "task.created",
            format!("任务由界面创建，并归属到项目“{}”", project.name),
        )],
        runtime: None,
    };
    guard.tasks.insert(0, task.clone());
    Ok(Json(task))
}

async fn seed_doc_tasks(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<BoardSnapshot>> {
    let mut guard = state.inner.lock().await;
    ensure_project_exists(&guard, project_id)?;
    merge_unique_tasks(&mut guard.tasks, seed_tasks_from_docs(project_id));
    Ok(Json(snapshot_from_state(&guard)))
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
    Ok(Json(snapshot_from_state(&guard)))
}

async fn explore_project(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<BoardSnapshot>> {
    let project = {
        let guard = state.inner.lock().await;
        find_project(&guard, project_id)?.clone()
    };
    if let Ok(path) = primary_workspace_path(&project) {
        let _ = std::fs::create_dir_all(&path);
    }
    let task = build_exploration_task(&project);
    let mut guard = state.inner.lock().await;
    guard.tasks.insert(0, task);
    Ok(Json(snapshot_from_state(&guard)))
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
    Ok(Json(snapshot_from_state(&guard)))
}

async fn claim_task(
    AxumPath((task_id, agent_id)): AxumPath<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> AppResult<Json<BoardSnapshot>> {
    let mut guard = state.inner.lock().await;
    let agent_name = guard
        .agents
        .iter()
        .find(|agent| agent.id == agent_id)
        .map(|agent| agent.name.clone())
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
    task.status = TaskStatus::Claimed;
    task.activities.push(new_activity(
        "task.claimed",
        format!("任务已由 {} 认领", agent_name),
    ));
    Ok(Json(snapshot_from_state(&guard)))
}

async fn start_task(
    AxumPath((task_id, agent_id)): AxumPath<(Uuid, Uuid)>,
    State(state): State<AppState>,
    Json(request): Json<AgentInvocationRequest>,
) -> AppResult<Json<BoardSnapshot>> {
    let context = resolve_task_execution_context(&state, task_id, request.prompt.clone()).await?;
    let _ = std::fs::create_dir_all(&context.workspace_root);
    apply_git_snapshot(&context.workspace_root, task_id, &state).await;

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
            )?;
            Ok(Json(snapshot_from_state(&guard)))
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
            Ok(Json(snapshot_from_state(&guard)))
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
            return Ok(Json(snapshot_from_state(&guard)));
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
    Ok(Json(snapshot_from_state(&guard)))
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
            return Ok(Json(snapshot_from_state(&guard)));
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
    Ok(Json(snapshot_from_state(&guard)))
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
        let task_is_running;

        {
            let task = &mut guard.tasks[task_index];
            let runtime = task.runtime.get_or_insert_with(|| TaskRuntime {
                provider: "codex".into(),
                thread_id: None,
                active_turn_id: None,
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

        if remove_runtime {
            state.runtime_sessions.lock().await.remove(&task_id);
            break;
        }
    }
}

fn snapshot_from_state(state: &BoardState) -> BoardSnapshot {
    BoardSnapshot {
        projects: state.projects.clone(),
        tasks: state.tasks.clone(),
        agents: state.agents.clone(),
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
            log: Vec::new(),
            last_error: None,
        });
        runtime.thread_id = thread_id;
        runtime.active_turn_id = turn_id;
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
    ]
    .join(" ")
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

    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workspace_root)
        .output()
        .await
        .ok()
        .map(|output| !String::from_utf8_lossy(&output.stdout).trim().is_empty())
        .unwrap_or(false);

    let tag = format!("task/{task_id}/pre-run/{}", timestamp_compact());
    let result = Command::new("git")
        .args(["tag", &tag])
        .current_dir(workspace_root)
        .output()
        .await;

    let message = match result {
        Ok(output) if output.status.success() => format!("已创建预执行 tag：{tag}，dirty={dirty}"),
        Ok(output) => format!(
            "创建预执行 tag 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(error) => format!("创建预执行 tag 失败：{error}"),
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
    use super::{build_app, RuntimeMode};
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use platform_core::{
        AgentInvocationRequest, AgentResumeRequest, BoardSnapshot, CreateTaskRequest,
    };
    use std::path::PathBuf;
    use tower::util::ServiceExt;

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
}
