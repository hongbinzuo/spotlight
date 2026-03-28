mod automation;
mod completion;
mod git_ops;
mod handlers;
mod models;
mod prompt;
mod runtime;
mod server;
mod snapshot;
mod state;
mod task_ops;
mod ui;

use automation::{
    reconcile_parallel_active_tasks, reconcile_watchdog_state, run_automation_cycle_once,
    select_next_auto_resume_task_id, start_background_automation,
};
use completion::*;
use git_ops::*;
use models::*;
#[cfg(test)]
use server::{build_api_router, parse_server_port};
use server::{build_app, server_listen_addr};
use state::{
    default_agents, default_projects, default_state, default_users, load_or_initialize_state,
    normalize_persisted_state,
};

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Json as ExtractJson, Path as AxumPath, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use platform_core::{
    merge_unique_tasks, new_activity, new_runtime_entry, seed_tasks_from_agents_markdown,
    seed_tasks_from_docs, Agent, AgentInvocationRequest, AgentResumeRequest, BoardSnapshot,
    CreateTaskRequest, DecisionCard, PendingQuestion, Project, RuntimeLogEntry, Task, TaskActivity,
    TaskAssignmentMode, TaskPriority, TaskRunAttemptRecord, TaskRunRecord, TaskRuntime,
    TaskStateSnapshot, TaskStatus, User, WorkspaceRoot,
};
use runtime::{ProviderRuntimeSession, RuntimeEvent};
use tokio::{
    sync::{mpsc, Mutex},
    time::{timeout, Duration},
};
use uuid::Uuid;

type AppResult<T> = Result<T, (StatusCode, String)>;
const AUTO_MAINTENANCE_INTERVAL_SECS: u64 = 5;
const TASK_STALE_TIMEOUT_SECS: u64 = 300;
const RUNTIME_OPERATION_TIMEOUT_SECS: u64 = 25;
const BOARD_TASK_ACTIVITY_LIMIT: usize = 24;
const BOARD_TASK_RUNTIME_LOG_LIMIT: usize = 24;
const BOARD_MESSAGE_CHAR_LIMIT: usize = 2_000;

#[derive(Clone)]
struct AppState {
    inner: Arc<Mutex<BoardState>>,
    runtime_mode: RuntimeMode,
    runtime_sessions: Arc<Mutex<HashMap<Uuid, Arc<ProviderRuntimeSession>>>>,
    store_path: PathBuf,
}

#[derive(Clone)]
struct BoardState {
    users: Vec<User>,
    projects: Vec<Project>,
    tasks: Vec<Task>,
    agents: Vec<Agent>,
    task_run_history: HashMap<Uuid, Vec<TaskRunRecord>>,
    pending_questions: Vec<PendingQuestion>,
    project_scans: HashMap<Uuid, ProjectScanSummary>,
    project_sessions: Vec<ProjectSession>,
    project_chat_messages: Vec<ProjectChatMessage>,
    memory_items: Vec<MemoryItem>,
    memory_revisions: Vec<MemoryRevision>,
    memory_tags: Vec<MemoryTag>,
    memory_edges: Vec<MemoryEdge>,
    decisions: Vec<DecisionCard>,
}

#[tokio::main]
async fn main() {
    let workspace_root = std::env::current_dir().expect("failed to resolve current directory");
    let app = build_app(RuntimeMode::RealCodex, workspace_root);
    let addr = server_listen_addr();

    println!("Spotlight server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind tcp listener");
    axum::serve(listener, app)
        .await
        .expect("failed to run axum server");
}

 fn runtime_operation_timeout_secs() -> u64 {
     std::env::var("SPOTLIGHT_RUNTIME_OP_TIMEOUT_SECS")
         .ok()
         .and_then(|value| value.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(RUNTIME_OPERATION_TIMEOUT_SECS)
}

fn runtime_operation_timeout_error(action: &str) -> (StatusCode, String) {
    (
        StatusCode::GATEWAY_TIMEOUT,
        format!(
            "{action}超过 {} 秒仍未完成",
            runtime_operation_timeout_secs()
        ),
    )
}

async fn spawn_runtime_session_with_timeout(
    workspace_root: PathBuf,
    event_tx: mpsc::UnboundedSender<RuntimeEvent>,
    action: &str,
) -> AppResult<Arc<ProviderRuntimeSession>> {
    match timeout(
        Duration::from_secs(runtime_operation_timeout_secs()),
        ProviderRuntimeSession::spawn(workspace_root, event_tx),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(runtime_operation_timeout_error(action)),
    }
}

async fn start_runtime_thread_with_timeout(
    session: &Arc<ProviderRuntimeSession>,
    cwd: &Path,
    developer_instructions: &str,
    action: &str,
) -> AppResult<String> {
    match timeout(
        Duration::from_secs(runtime_operation_timeout_secs()),
        session.start_thread(cwd, developer_instructions),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            session.shutdown().await;
            Err(runtime_operation_timeout_error(action))
        }
    }
}

async fn resume_runtime_thread_with_timeout(
    session: &Arc<ProviderRuntimeSession>,
    thread_id: &str,
    action: &str,
) -> AppResult<String> {
    match timeout(
        Duration::from_secs(runtime_operation_timeout_secs()),
        session.resume_thread(thread_id),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            session.shutdown().await;
            Err(runtime_operation_timeout_error(action))
        }
    }
}

async fn start_runtime_turn_with_timeout(
    session: &Arc<ProviderRuntimeSession>,
    cwd: &Path,
    thread_id: &str,
    prompt: &str,
    action: &str,
) -> AppResult<String> {
    match timeout(
        Duration::from_secs(runtime_operation_timeout_secs()),
        session.start_turn(cwd, thread_id, prompt),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            session.shutdown().await;
            Err(runtime_operation_timeout_error(action))
        }
    }
}

fn current_time_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

fn stale_timeout_nanos() -> u128 {
    TASK_STALE_TIMEOUT_SECS as u128 * 1_000_000_000
}

fn task_last_touch_nanos(task: &Task) -> Option<u128> {
    task.runtime
        .as_ref()
        .and_then(|runtime| runtime.log.last())
        .and_then(|entry| entry.at.parse::<u128>().ok())
        .or_else(|| {
            task.activities
                .last()
                .and_then(|activity| activity.at.parse::<u128>().ok())
        })
}

fn task_status_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Open => "待处理",
        TaskStatus::Claimed => "已认领",
        TaskStatus::ApprovalRequested => "待审批",
        TaskStatus::Approved => "已审批",
        TaskStatus::Running => "运行中",
        TaskStatus::Paused => "已暂停",
        TaskStatus::PendingAcceptance => "待验收",
        TaskStatus::Accepted => "已验收",
        TaskStatus::Done => "已完成",
        TaskStatus::Failed => "失败",
        TaskStatus::ManualReview => "人工复核",
        TaskStatus::Canceled => "已撤销",
    }
}

fn task_is_serialized_active(task: &Task) -> bool {
    matches!(task.status, TaskStatus::Claimed | TaskStatus::Running)
}

fn normalize_serialization_workspace_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

fn task_serialization_lane_key(projects: &[Project], task: &Task) -> String {
    if let Some(workspace_path) = projects
        .iter()
        .find(|project| project.id == task.project_id)
        .and_then(Project::primary_workspace)
        .map(|workspace| normalize_serialization_workspace_path(&workspace.path))
    {
        return format!("workspace:{workspace_path}");
    }

    format!("project:{}", task.project_id)
}

fn tasks_share_serialization_lane(projects: &[Project], left: &Task, right: &Task) -> bool {
    task_serialization_lane_key(projects, left) == task_serialization_lane_key(projects, right)
}

fn active_task_conflict<'a>(
    projects: &[Project],
    tasks: &'a [Task],
    task_id: Uuid,
    exclude_task_id: Option<Uuid>,
) -> Option<&'a Task> {
    let target_task = tasks.iter().find(|task| task.id == task_id)?;
    tasks.iter().find(|task| {
        task_is_serialized_active(task)
            && exclude_task_id.is_none_or(|excluded_task_id| task.id != excluded_task_id)
            && tasks_share_serialization_lane(projects, target_task, task)
    })
}

fn active_task_conflict_message(task: &Task) -> String {
    format!(
        "任务《{}》当前已在同一工作区处于{}，同一工作区一次只允许一个活跃任务",
        task.title,
        task_status_label(task.status)
    )
}

pub(crate) fn task_has_progress_evidence(task: &Task) -> bool {
    let has_runtime_log = task
        .runtime
        .as_ref()
        .is_some_and(|runtime| !runtime.log.is_empty());
    let has_progress_activity = task.activities.iter().any(|activity| {
        matches!(
            activity.kind.as_str(),
            "runtime.thread_started"
                | "runtime.turn_started"
                | "runtime.turn_completed"
                | "runtime.error"
                | "task.watchdog_recovered"
                | "task.auto_retry_queued"
                | "task.runtime_session_lost"
        )
    });

    has_runtime_log || has_progress_activity
}

pub(crate) fn task_has_completion_evidence(task: &Task) -> bool {
    task.activities.iter().rev().any(|activity| {
        activity.kind == "task.done"
            || (activity.kind == "runtime.turn_completed" && activity.message.contains("completed"))
            || activity.kind == "task.completion_summary"
    }) || task
        .runtime
        .as_ref()
        .and_then(|runtime| extract_task_completion_report(&runtime.log))
        .and_then(|report| report.result)
        .is_some_and(|result| result.eq_ignore_ascii_case("done"))
}

fn task_state_snapshot_needs_refresh(task: &Task) -> bool {
    task.state_snapshot.reason.is_none()
        || task.state_snapshot.evidence.is_empty()
        || task.state_snapshot.last_evaluated_by.is_none()
}

fn refresh_task_state_snapshot(task: &mut Task, evaluator: &str) {
    task.state_snapshot = evaluate_task_state_snapshot(task, evaluator);
}

fn evaluate_task_state_snapshot(task: &Task, evaluator: &str) -> TaskStateSnapshot {
    let completion_evidence = task_has_completion_evidence(task);
    let has_runtime_thread = task
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.thread_id.as_deref())
        .is_some();
    let running_without_turn = matches!(task.status, TaskStatus::Running)
        && task
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.active_turn_id.is_none());
    let done_without_completion_evidence =
        matches!(task.status, TaskStatus::Done) && !completion_evidence;
    let inconsistent_completion =
        !matches!(task.status, TaskStatus::Done | TaskStatus::Canceled) && completion_evidence;

    let reason = match task.status {
        TaskStatus::Open => {
            if task_has_progress_evidence(task) {
                "任务存在历史执行痕迹，但当前已回到等待队列。".to_string()
            } else {
                "任务尚未开始执行，等待认领。".to_string()
            }
        }
        TaskStatus::Claimed => {
            if has_runtime_thread {
                "任务已被认领，并保留了可继续的运行上下文。".to_string()
            } else {
                "任务已被认领，等待正式启动。".to_string()
            }
        }
        TaskStatus::ApprovalRequested => "任务正在等待审批确认。".to_string(),
        TaskStatus::Approved => "任务已通过审批，等待启动执行。".to_string(),
        TaskStatus::Running => {
            if running_without_turn {
                "任务被标记为 RUNNING，但缺少活跃 turn，状态需要继续校正。".to_string()
            } else {
                "任务正在由 Agent 执行。".to_string()
            }
        }
        TaskStatus::Paused => match task
            .activities
            .last()
            .map(|activity| activity.kind.as_str())
        {
            Some("task.runtime_session_lost") => {
                "本地运行会话已断开，任务处于可恢复状态。".to_string()
            }
            Some("task.watchdog_recovered") => {
                "系统检测到任务长时间无进展，已自动暂停等待恢复。".to_string()
            }
            Some("task.paused") | Some("task.pause_requested") => {
                "任务已人工暂停，等待补充信息或恢复执行。".to_string()
            }
            Some("task.state_normalized") => {
                "服务端启动时校正了任务状态，当前等待恢复或人工处理。".to_string()
            }
            _ => "任务当前处于暂停状态。".to_string(),
        },
        TaskStatus::Done => task_completion_summary(task)
            .map(|summary| format!("任务已完成：{summary}"))
            .unwrap_or_else(|| "任务已完成。".to_string()),
        TaskStatus::Failed => task
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.last_error.clone())
            .unwrap_or_else(|| "任务执行失败，需要人工处理。".to_string()),
        TaskStatus::PendingAcceptance => "任务已执行完成，等待验收确认。".to_string(),
        TaskStatus::Accepted => "任务已通过验收。".to_string(),
        TaskStatus::ManualReview => "任务需要人工复核后决定下一步。".to_string(),
        TaskStatus::Canceled => "任务已取消。".to_string(),
    };

    let mut evidence = Vec::new();
    if let Some(last_activity) = task.activities.last() {
        evidence.push(format!(
            "last_activity:{}@{}",
            last_activity.kind, last_activity.at
        ));
    }
    if let Some(runtime) = task.runtime.as_ref() {
        if let Some(thread_id) = runtime.thread_id.as_deref() {
            evidence.push(format!("runtime.thread_id:{thread_id}"));
        }
        if let Some(turn_id) = runtime.active_turn_id.as_deref() {
            evidence.push(format!("runtime.active_turn_id:{turn_id}"));
        }
        if let Some(last_error) = runtime
            .last_error
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            evidence.push(format!("runtime.last_error:{last_error}"));
        }
    }
    if let Some(summary) = task_completion_summary(task) {
        evidence.push(format!("completion.summary:{summary}"));
    }
    if inconsistent_completion {
        evidence.push("status.completed_evidence_mismatch".into());
    }
    if done_without_completion_evidence {
        evidence.push("status.done_without_strong_evidence".into());
    }
    evidence.truncate(6);

    TaskStateSnapshot {
        reason: Some(reason),
        evidence,
        last_evaluated_at: Some(current_time_nanos().to_string()),
        last_evaluated_by: Some(evaluator.to_string()),
        needs_attention: inconsistent_completion
            || done_without_completion_evidence
            || running_without_turn,
    }
}

fn task_completion_summary(task: &Task) -> Option<String> {
    task.runtime
        .as_ref()
        .and_then(|runtime| extract_task_completion_report(&runtime.log))
        .and_then(|report| report.summary)
        .map(|summary| summary.trim().to_string())
        .filter(|summary| !summary.is_empty())
}

fn priority_label(priority: TaskPriority) -> &'static str {
    match priority {
        TaskPriority::High => "高优先级",
        TaskPriority::Medium => "中优先级",
        TaskPriority::Low => "低优先级",
    }
}

fn persist_state_to_path(store_path: &Path, state: &PersistedState) -> Result<(), String> {
    if let Some(parent) = store_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建状态目录失败：{error}"))?;
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
        [(
            header::SET_COOKIE,
            HeaderValue::from_str(&cookie_value).map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("设置登录 Cookie 失败：{error}"),
                )
            })?,
        )],
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

async fn list_projects(State(state): State<AppState>) -> Json<Vec<Project>> {
    let guard = state.inner.lock().await;
    Json(guard.projects.clone())
}

async fn list_project_tasks(
    AxumPath(project_id): AxumPath<Uuid>,
    Query(query): Query<ListProjectTasksQuery>,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<Task>>> {
    let guard = state.inner.lock().await;
    ensure_project_exists(&guard, project_id)?;
    let status_filter = query.status.as_deref().map(parse_task_status).transpose()?;

    let tasks = guard
        .tasks
        .iter()
        .filter(|task| {
            task.project_id == project_id
                && status_filter
                    .map(|expected_status| task.status == expected_status)
                    .unwrap_or(true)
        })
        .map(|task| task_with_state_snapshot(task, "server.project_tasks"))
        .collect();
    Ok(Json(tasks))
}

async fn list_agents(State(state): State<AppState>) -> Json<Vec<Agent>> {
    let guard = state.inner.lock().await;
    Json(guard.agents.clone())
}

fn parse_task_status(raw: &str) -> AppResult<TaskStatus> {
    TaskStatus::parse_filter(raw).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("不支持的任务状态筛选：{raw}"),
        )
    })
}

async fn answer_pending_question(
    AxumPath(question_id): AxumPath<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<AnswerPendingQuestionRequest>,
) -> AppResult<Json<BoardSnapshot>> {
    let answer = request.answer.trim();
    if answer.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "回答内容不能为空".into()));
    }

    let mut guard = state.inner.lock().await;
    let question = find_pending_question_mut(&mut guard, question_id)?;
    question.status = "answered".into();
    question.answer = Some(answer.to_string());
    question.answered_at = Some(timestamp_string());
    let source_task_id = question.source_task_id;
    let question_text = question.question.clone();
    if let Ok(task) = find_task_mut(&mut guard, source_task_id) {
        task.activities.push(new_activity(
            "task.question_answered",
            format!("已统一记录问题回答：{}\n回答：{}", question_text, answer),
        ));
    }
    let snapshot = snapshot_from_state(&guard);
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

async fn get_project_context(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<ProjectContextSnapshot>> {
    let guard = state.inner.lock().await;
    let snapshot = project_context_snapshot(&guard, project_id)?;
    Ok(Json(snapshot))
}

async fn get_project_summary(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<ProjectSummarySnapshot>> {
    let guard = state.inner.lock().await;
    let snapshot = project_summary_snapshot(&guard, project_id)?;
    Ok(Json(snapshot))
}

async fn get_project_memory(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<ProjectMemorySnapshot>> {
    let guard = state.inner.lock().await;
    ensure_project_exists(&guard, project_id)?;
    Ok(Json(project_memory_snapshot(&guard, project_id)))
}

async fn upsert_project_constraint(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<UpsertProjectConstraintRequest>,
) -> AppResult<Json<ProjectMemorySnapshot>> {
    let title = request.title.trim();
    let content = request.content.trim();
    if title.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "约束标题不能为空".into()));
    }
    if content.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "约束内容不能为空".into()));
    }

    let mut guard = state.inner.lock().await;
    find_project(&guard, project_id)?;
    let current_user_id = resolve_current_user(&guard, &headers)
        .as_ref()
        .map(|user| user.id);
    let stable_key = normalized_constraint_stable_key(request.stable_key.as_deref(), title);
    write_memory_revision(
        &mut guard,
        MemoryWriteSpec {
            scope_kind: "project",
            scope_id: project_id,
            memory_kind: "project_constraint",
            stable_key,
            tag: format!("project/{project_id}/active-constraints"),
            title: title.to_string(),
            content: content.to_string(),
            structured_payload: Some(serde_json::json!({
                "kind": "project_constraint",
                "title": title,
                "content": content,
            })),
            source_kind: "manual_constraint",
            source_id: None,
            confidence: Some(1.0),
            created_by: current_user_id,
        },
    );
    let snapshot = project_memory_snapshot(&guard, project_id);
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

async fn post_project_chat_message(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProjectChatRequest>,
) -> AppResult<Json<ProjectContextSnapshot>> {
    let message = request.message.trim();
    if message.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "消息内容不能为空".into()));
    }

    let mut guard = state.inner.lock().await;
    find_project(&guard, project_id)?;
    let current_user = resolve_current_user(&guard, &headers);
    let display_name = current_user
        .as_ref()
        .map(|user| user.display_name.clone())
        .unwrap_or_else(|| "未知用户".into());

    guard.project_chat_messages.push(ProjectChatMessage {
        id: Uuid::new_v4(),
        project_id,
        user_id: current_user.as_ref().map(|user| user.id),
        user_display_name: display_name,
        content: message.to_string(),
        at: timestamp_string(),
    });

    let snapshot = project_context_snapshot(&guard, project_id)?;
    drop(guard);
    persist_state(&state).await?;
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
    let mode = normalize_project_session_mode(request.mode.as_deref()).to_string();
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            let preview = truncate_title(prompt);
            if mode == "general" {
                preview
            } else {
                format!("{}：{preview}", project_session_mode_label(&mode))
            }
        });
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
                mode: mode.clone(),
                status: "running".into(),
                workspace_path,
                thread_id: None,
                active_turn_id: None,
                messages: vec![user_message],
                log: vec![new_runtime_entry(
                    "project.session.user",
                    prompt.to_string(),
                )],
                last_error: None,
            },
        );
    }
    persist_state(&state).await?;

    match state.runtime_mode {
        RuntimeMode::Stub => {
            complete_stub_project_session(
                &state,
                session_id,
                &project,
                latest_scan.as_ref(),
                &mode,
                prompt,
            )
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

    let composed_prompt = prompt::compose_project_session_prompt_for_mode(
        &project,
        latest_scan.as_ref(),
        prompt,
        &mode,
    );
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let session = match spawn_runtime_session_with_timeout(
        workspace_root.clone(),
        event_tx,
        "启动项目会话时创建运行时会话",
    )
    .await
    {
        Ok(session) => session,
        Err(error) => {
            mark_project_session_failed(&state, session_id, &error.1).await;
            persist_state(&state).await?;
            return Err(error);
        }
    };
    let thread_id = match start_runtime_thread_with_timeout(
        &session,
        &workspace_root,
        &prompt::project_session_developer_instructions_for_mode(&mode),
        "启动项目会话时创建线程",
    )
    .await
    {
        Ok(thread_id) => thread_id,
        Err(error) => {
            mark_project_session_failed(&state, session_id, &error.1).await;
            persist_state(&state).await?;
            return Err(error);
        }
    };
    let turn_id = match start_runtime_turn_with_timeout(
        &session,
        &workspace_root,
        &thread_id,
        &composed_prompt,
        "启动项目会话时启动会话轮次",
    )
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

    let (project_id, project, latest_scan, thread_id, workspace_root, mode) = {
        let mut guard = state.inner.lock().await;
        let (project_id, thread_id, mode) = {
            let project_session = find_project_session_mut(&mut guard, session_id)
                .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到项目会话".into()))?;
            if project_session.active_turn_id.is_some() {
                return Err((
                    StatusCode::CONFLICT,
                    "当前项目会话仍在运行，请稍后再试".into(),
                ));
            }
            let mode = normalize_project_session_mode(Some(&project_session.mode)).to_string();
            project_session.mode = mode.clone();
            project_session.status = "running".into();
            project_session.last_error = None;
            project_session.messages.push(ProjectSessionMessage {
                role: "user".into(),
                content: prompt.to_string(),
                at: timestamp_string(),
            });
            project_session.log.push(new_runtime_entry(
                "project.session.user",
                prompt.to_string(),
            ));
            (
                project_session.project_id,
                project_session.thread_id.clone(),
                mode,
            )
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
            mode,
        )
    };
    if matches!(state.runtime_mode, RuntimeMode::RealCodex) && workspace_root.is_none() {
        mark_project_session_failed(&state, session_id, "项目会话需要先为当前项目配置主工作目录")
            .await;
        persist_state(&state).await?;
        return Err((
            StatusCode::BAD_REQUEST,
            "项目会话需要先为当前项目配置主工作目录".into(),
        ));
    }
    if matches!(state.runtime_mode, RuntimeMode::RealCodex) && thread_id.is_none() {
        mark_project_session_failed(
            &state,
            session_id,
            "当前项目会话缺少 thread_id，请新建一个项目会话",
        )
        .await;
        persist_state(&state).await?;
        return Err((
            StatusCode::CONFLICT,
            "当前项目会话缺少 thread_id，请新建一个项目会话".into(),
        ));
    }
    persist_state(&state).await?;

    match state.runtime_mode {
        RuntimeMode::Stub => {
            complete_stub_project_session(
                &state,
                session_id,
                &project,
                latest_scan.as_ref(),
                &mode,
                prompt,
            )
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
    let composed_prompt = prompt::compose_project_session_prompt_for_mode(
        &project,
        latest_scan.as_ref(),
        prompt,
        &mode,
    );

    let resolved_session =
        resolve_project_runtime_session(&state, session_id, workspace_root.clone(), &thread_id)
            .await?;

    let turn_id = match start_runtime_turn_with_timeout(
        &resolved_session.session,
        &workspace_root,
        &resolved_session.thread_id,
        &composed_prompt,
        "恢复项目会话时启动会话轮次",
    )
    .await
    {
        Ok(turn_id) => turn_id,
        Err(error) => {
            if resolved_session.event_rx.is_some() {
                resolved_session.session.shutdown().await;
            }
            mark_project_session_failed(&state, session_id, &error.1).await;
            persist_state(&state).await?;
            return Err(error);
        }
    };

    register_project_runtime_session(
        &state,
        session_id,
        resolved_session.session.clone(),
        resolved_session.event_rx,
    )
    .await;

    {
        let mut guard = state.inner.lock().await;
        if let Some(project_session) = find_project_session_mut(&mut guard, session_id) {
            project_session.thread_id = Some(resolved_session.thread_id);
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
        priority: request.priority,
        labels: request.labels,
        creator_user_id: current_user.as_ref().map(|user| user.id),
        assignee_user_id: None,
        assignment_mode: TaskAssignmentMode::PublicQueue,
        requested_agent_id: None,
        source_task_id: None,
        claimed_by: None,
        activities: vec![new_activity(
            "task.created",
            format!(
                "任务由界面创建，并归属到项目\u{201c}{}\u{201d}",
                project.name
            ),
        )],
        runtime: None,
        approval: Default::default(),
        acceptance: Default::default(),
        state_snapshot: TaskStateSnapshot::default(),
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

async fn pull_next_task(
    AxumPath(agent_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<PullNextResponse>> {
    let mut guard = state.inner.lock().await;
    let task = auto_claim_next_task(&mut guard, agent_id)?;
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(PullNextResponse { task }))
}

async fn claim_task(
    AxumPath((task_id, agent_id)): AxumPath<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> AppResult<Json<BoardSnapshot>> {
    let mut guard = state.inner.lock().await;
    if let Some(conflict) =
        active_task_conflict(&guard.projects, &guard.tasks, task_id, Some(task_id))
    {
        return Err((StatusCode::CONFLICT, active_task_conflict_message(conflict)));
    }
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
    if matches!(
        task.status,
        TaskStatus::Running | TaskStatus::Done | TaskStatus::Canceled
    ) {
        return Err((StatusCode::CONFLICT, "当前任务状态不允许重新认领".into()));
    }
    task.claimed_by = Some(agent_id);
    task.assignee_user_id = owner_user_id;
    task.status = TaskStatus::Claimed;
    task.activities.push(new_activity(
        "task.claimed",
        format!("任务已由 {} 认领", agent_name),
    ));
    let task_title = task.title.clone();
    assign_agent_claimed(
        &mut guard,
        agent_id,
        task_id,
        format!("claim acquired: {task_title}"),
    );
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
    {
        let mut guard = state.inner.lock().await;
        if let Some(conflict) =
            active_task_conflict(&guard.projects, &guard.tasks, task_id, Some(task_id))
        {
            return Err((StatusCode::CONFLICT, active_task_conflict_message(conflict)));
        }
        let task = find_task_mut(&mut guard, task_id)?;
        if matches!(task.status, TaskStatus::Canceled) {
            return Err((StatusCode::CONFLICT, "已撤销的任务不能启动".into()));
        }
    }

    let context = prompt::resolve_task_execution_context(&state, task_id, request.prompt.clone())
        .await?;
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
                "stub-codex",
                &context.prompt,
                Some("stub-thread".into()),
                Some("stub-turn".into()),
                false,
            )?;
            record_task_run_start(
                &mut guard,
                task_id,
                agent_id,
                "stub-codex",
                &context.prompt,
                Some(context.workspace_root.display().to_string()),
                Some("stub-thread".into()),
                Some("stub-turn".into()),
                "start",
            );
            let snapshot = snapshot_from_state(&guard);
            drop(guard);
            persist_state(&state).await?;
            Ok(Json(snapshot))
        }
        RuntimeMode::RealCodex => {
            let (event_tx, event_rx) = mpsc::unbounded_channel();
            let session = spawn_runtime_session_with_timeout(
                context.workspace_root.clone(),
                event_tx,
                "启动任务时创建运行时会话",
            )
            .await?;
            let provider_id = session.provider_id().to_string();
                let thread_id = start_runtime_thread_with_timeout(
                    &session,
                    &context.workspace_root,
                    &prompt::task_developer_instructions(),
                    "启动任务时创建线程",
                )
                .await?;
            let turn_id = start_runtime_turn_with_timeout(
                &session,
                &context.workspace_root,
                &thread_id,
                &context.prompt,
                "启动任务时启动会话轮次",
            )
            .await?;
            let run_thread_id = thread_id.clone();
            let run_turn_id = turn_id.clone();
            {
                let mut guard = state.inner.lock().await;
                mark_task_running(
                    &mut guard,
                    task_id,
                    agent_id,
                    &request.agent_name_hint,
                    &provider_id,
                    &context.prompt,
                    Some(thread_id),
                    Some(turn_id),
                    git_auto_merge_enabled,
                )?;
                record_task_run_start(
                    &mut guard,
                    task_id,
                    agent_id,
                    &provider_id,
                    &context.prompt,
                    Some(context.workspace_root.display().to_string()),
                    Some(run_thread_id),
                    Some(run_turn_id),
                    "start",
                );
            }
            state.runtime_sessions.lock().await.insert(task_id, session);
            tokio::spawn(runtime_event_loop(
                state.clone(),
                task_id,
                agent_id,
                event_rx,
                provider_id,
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
            record_task_run_transition(
                &mut guard,
                task_id,
                "interrupted",
                Some("task paused by user"),
                None,
                None,
            );
            reset_agent_if_needed(&mut guard, task_id, "任务已暂停，等待恢复");
            let snapshot = snapshot_from_state(&guard);
            drop(guard);
            persist_state(&state).await?;
            return Ok(Json(snapshot));
        }
        RuntimeMode::RealCodex => {}
    }

    let (thread_id, turn_id) = {
        let guard = state.inner.lock().await;
        let task = find_task(&guard, task_id)?;
        task_pause_runtime_ids(task)?
    };

    let sessions = state.runtime_sessions.lock().await;
    let session = sessions
        .get(&task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到运行时会话".into()))?;
    drop(sessions);
    session.interrupt_turn(&thread_id, &turn_id).await?;

    let snapshot = {
        let mut guard = state.inner.lock().await;
        {
            let task = find_task_mut(&mut guard, task_id)?;
            task.status = TaskStatus::Paused;
            task.activities
                .push(new_activity("task.paused", "任务已暂停，等待补充提示词"));
            if let Some(runtime) = task.runtime.as_mut() {
                runtime.active_turn_id = None;
                runtime
                    .log
                    .push(new_runtime_entry("system", "已暂停当前任务"));
            }
        }
        record_task_run_transition(
            &mut guard,
            task_id,
            "interrupted",
            Some("task paused by user"),
            Some(&thread_id),
            Some(&turn_id),
        );
        reset_agent_if_needed(&mut guard, task_id, "任务已暂停，等待恢复");
        snapshot_from_state(&guard)
    };
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

async fn cancel_task(
    AxumPath(task_id): AxumPath<Uuid>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CancelTaskRequest>,
) -> AppResult<Json<BoardSnapshot>> {
    {
        let mut guard = state.inner.lock().await;
        let current_user = resolve_current_user(&guard, &headers);
        let current_user_label = current_user
            .as_ref()
            .map(|user| user.display_name.clone())
            .unwrap_or_else(|| "未知用户".into());
        let reason = request
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("用户确认当前任务不再继续");

        let task = find_task_mut(&mut guard, task_id)?;
        if matches!(
            task.status,
            TaskStatus::Done | TaskStatus::Failed | TaskStatus::Canceled
        ) {
            return Err((StatusCode::CONFLICT, "当前任务已结束，不能重复撤销".into()));
        }
        if matches!(task.status, TaskStatus::Running) {
            return Err((
                StatusCode::CONFLICT,
                "运行中的任务请先暂停，再撤销为不做状态".into(),
            ));
        }

        task.status = TaskStatus::Canceled;
        task.claimed_by = None;
        if let Some(runtime) = task.runtime.as_mut() {
            runtime.active_turn_id = None;
            runtime
                .log
                .push(new_runtime_entry("system", format!("任务已撤销: {reason}")));
        }
        task.activities.push(new_activity(
            "task.canceled",
            format!("任务已撤销。操作人: {current_user_label}；原因: {reason}"),
        ));
        record_task_run_transition(&mut guard, task_id, "aborted", Some(reason), None, None);
        reset_agent_if_needed(&mut guard, task_id, "最近一次任务已撤销");
    }

    let guard = state.inner.lock().await;
    let snapshot = snapshot_from_state_with_user(&guard, resolve_current_user(&guard, &headers));
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

    {
        let mut guard = state.inner.lock().await;
        if let Some(conflict) =
            active_task_conflict(&guard.projects, &guard.tasks, task_id, Some(task_id))
        {
            return Err((StatusCode::CONFLICT, active_task_conflict_message(conflict)));
        }
        let task = find_task_mut(&mut guard, task_id)?;
        if matches!(task.status, TaskStatus::Canceled) {
            return Err((StatusCode::CONFLICT, "已撤销的任务不能恢复执行".into()));
        }
    }

    match state.runtime_mode {
        RuntimeMode::Stub => {
            let mut guard = state.inner.lock().await;
            let prompt_for_history = request.prompt.trim().to_string();
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
                runtime.last_error = None;
            }
            record_task_run_start(
                &mut guard,
                task_id,
                agent_id,
                "stub-codex",
                &prompt_for_history,
                None,
                Some("stub-thread".into()),
                Some("stub-turn-2".into()),
                "resume",
            );
            record_task_run_transition(
                &mut guard,
                task_id,
                "completed",
                Some("stub resume completed"),
                Some("stub-thread"),
                Some("stub-turn-2"),
            );
            process_task_completion_outputs(&mut guard, task_id);
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
        let guard = state.inner.lock().await;
        let task = find_task(&guard, task_id)?;
        task_resume_thread_id(task)?
    };

    let resolved_session = resolve_task_runtime_session(
        &state,
        task_id,
        agent_id,
        workspace_root.clone(),
        &thread_id,
    )
    .await?;

    let turn_id = match start_runtime_turn_with_timeout(
        &resolved_session.session,
        &workspace_root,
        &resolved_session.thread_id,
        request.prompt.trim(),
        "恢复任务时启动会话轮次",
    )
    .await
    {
        Ok(turn_id) => turn_id,
        Err(error) => {
            if resolved_session.event_rx.is_some() {
                resolved_session.session.shutdown().await;
            }
            return Err(error);
        }
    };

    register_task_runtime_session(
        &state,
        task_id,
        agent_id,
        resolved_session.session.clone(),
        resolved_session.event_rx,
    )
    .await;

    let snapshot = {
        let mut guard = state.inner.lock().await;
        {
            let task = find_task_mut(&mut guard, task_id)?;
            task.status = TaskStatus::Running;
            task.claimed_by = Some(agent_id);
            task.activities.push(new_activity(
                "task.resumed",
                format!("已补充提示词并恢复：{}", request.prompt.trim()),
            ));
            if let Some(runtime) = task.runtime.as_mut() {
                runtime
                    .log
                    .push(new_runtime_entry("user", request.prompt.trim().to_string()));
                runtime.provider = resolved_session.session.provider_id().to_string();
                runtime.thread_id = Some(resolved_session.thread_id.clone());
                runtime.active_turn_id = Some(turn_id.clone());
            }
        }
        record_task_run_start(
            &mut guard,
            task_id,
            agent_id,
            resolved_session.session.provider_id(),
            request.prompt.trim(),
            Some(workspace_root.display().to_string()),
            Some(resolved_session.thread_id.clone()),
            Some(turn_id.clone()),
            "resume",
        );
        assign_agent_running(&mut guard, agent_id, task_id, "继续执行当前任务".into());
        snapshot_from_state(&guard)
    };
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

async fn resolve_task_runtime_session(
    state: &AppState,
    task_id: Uuid,
    agent_id: Uuid,
    workspace_root: PathBuf,
    thread_id: &str,
) -> AppResult<ResolvedProviderRuntimeSession> {
    let existing_session = {
        let sessions = state.runtime_sessions.lock().await;
        sessions.get(&task_id).cloned()
    };

    if let Some(session) = existing_session {
        return Ok(ResolvedProviderRuntimeSession {
            session,
            thread_id: thread_id.to_string(),
            event_rx: None,
        });
    }

    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let session =
        spawn_runtime_session_with_timeout(workspace_root, event_tx, "恢复任务时创建运行时会话")
            .await?;
    let resumed_thread_id =
        match resume_runtime_thread_with_timeout(&session, thread_id, "恢复任务时恢复线程").await
        {
            Ok(thread_id) => thread_id,
            Err(error) => {
                session.shutdown().await;
                return Err(error);
            }
        };

    let _ = agent_id;
    Ok(ResolvedProviderRuntimeSession {
        session,
        thread_id: resumed_thread_id,
        event_rx: Some(event_rx),
    })
}

async fn register_task_runtime_session(
    state: &AppState,
    task_id: Uuid,
    agent_id: Uuid,
    session: Arc<ProviderRuntimeSession>,
    event_rx: Option<mpsc::UnboundedReceiver<RuntimeEvent>>,
) {
    let Some(event_rx) = event_rx else {
        return;
    };

    let provider_id = session.provider_id().to_string();
    state.runtime_sessions.lock().await.insert(task_id, session);
    tokio::spawn(runtime_event_loop(
        state.clone(),
        task_id,
        agent_id,
        event_rx,
        provider_id,
    ));
}

async fn resolve_project_runtime_session(
    state: &AppState,
    session_id: Uuid,
    workspace_root: PathBuf,
    thread_id: &str,
) -> AppResult<ResolvedProviderRuntimeSession> {
    let existing_session = {
        let sessions = state.runtime_sessions.lock().await;
        sessions.get(&session_id).cloned()
    };

    if let Some(session) = existing_session {
        return Ok(ResolvedProviderRuntimeSession {
            session,
            thread_id: thread_id.to_string(),
            event_rx: None,
        });
    }

    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let session = spawn_runtime_session_with_timeout(
        workspace_root,
        event_tx,
        "恢复项目会话时创建运行时会话",
    )
    .await?;
    let resumed_thread_id =
        match resume_runtime_thread_with_timeout(&session, thread_id, "恢复项目会话时恢复线程")
            .await
        {
            Ok(thread_id) => thread_id,
            Err(error) => {
                session.shutdown().await;
                return Err(error);
            }
        };

    Ok(ResolvedProviderRuntimeSession {
        session,
        thread_id: resumed_thread_id,
        event_rx: Some(event_rx),
    })
}

async fn register_project_runtime_session(
    state: &AppState,
    session_id: Uuid,
    session: Arc<ProviderRuntimeSession>,
    event_rx: Option<mpsc::UnboundedReceiver<RuntimeEvent>>,
) {
    let Some(event_rx) = event_rx else {
        return;
    };

    state
        .runtime_sessions
        .lock()
        .await
        .insert(session_id, session);
    tokio::spawn(project_session_event_loop(
        state.clone(),
        session_id,
        event_rx,
    ));
}

async fn runtime_event_loop(
    state: AppState,
    task_id: Uuid,
    agent_id: Uuid,
    mut event_rx: mpsc::UnboundedReceiver<RuntimeEvent>,
    provider_id: String,
) {
    while let Some(event) = event_rx.recv().await {
        let mut guard = state.inner.lock().await;
        let Some(task_index) = guard.tasks.iter().position(|task| task.id == task_id) else {
            break;
        };

        let mut reset_action: Option<&'static str> = None;
        let mut remove_runtime = false;
        let mut should_finalize_git_merge = false;
        let mut should_process_completion_outputs = false;
        let task_is_running;

        {
            let task = &mut guard.tasks[task_index];
            let runtime = task.runtime.get_or_insert_with(|| TaskRuntime {
                provider: provider_id.clone(),
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
                            should_process_completion_outputs = true;
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
                    runtime.active_turn_id = None;
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
                    runtime.active_turn_id = None;
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

        if should_process_completion_outputs {
            process_task_completion_outputs(&mut guard, task_id);
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

    state.runtime_sessions.lock().await.remove(&task_id);
    reconcile_task_runtime_session_lost(&state, task_id).await;
}

async fn reconcile_task_runtime_session_lost(state: &AppState, task_id: Uuid) {
    let mut guard = state.inner.lock().await;
    let Some(task) = guard.tasks.iter_mut().find(|task| task.id == task_id) else {
        return;
    };

    if !matches!(task.status, TaskStatus::Running | TaskStatus::Claimed) {
        return;
    }

    let message = "本地运行会话已断开，任务已转为可恢复状态，等待自动恢复或人工继续";
    task.status = TaskStatus::Paused;
    task.claimed_by = None;
    task.activities
        .push(new_activity("task.runtime_session_lost", message));
    if let Some(runtime) = task.runtime.as_mut() {
        runtime.active_turn_id = None;
        runtime.last_error = Some(message.into());
        runtime
            .log
            .push(new_runtime_entry("system", message.to_string()));
    }
    reset_agent_if_needed(&mut guard, task_id, "本地运行会话已断开，等待恢复");
    drop(guard);
    let _ = persist_state(state).await;
}

fn snapshot_from_state(state: &BoardState) -> BoardSnapshot {
    snapshot_from_state_with_user(state, state.users.first().cloned())
}

fn snapshot_from_state_with_user(state: &BoardState, current_user: Option<User>) -> BoardSnapshot {
    let mut task_run_history = state.task_run_history.clone();
    let _ = normalize_task_run_history_entries(&state.tasks, &mut task_run_history);

    BoardSnapshot {
        current_user,
        users: state.users.clone(),
        projects: state.projects.clone(),
        tasks: state.tasks.iter().map(board_snapshot_task).collect(),
        agents: state.agents.clone(),
        task_run_history,
        pending_questions: state.pending_questions.clone(),
    }
}

fn board_snapshot_task(task: &Task) -> Task {
    let mut snapshot = task_with_state_snapshot(task, "server.board_snapshot");
    snapshot.activities = trim_task_activities(&task.activities, BOARD_TASK_ACTIVITY_LIMIT);
    snapshot.runtime = task.runtime.as_ref().map(board_snapshot_runtime);
    snapshot
}

fn task_with_state_snapshot(task: &Task, evaluator: &str) -> Task {
    let mut snapshot = task.clone();
    refresh_task_state_snapshot(&mut snapshot, evaluator);
    snapshot
}

fn board_snapshot_runtime(runtime: &TaskRuntime) -> TaskRuntime {
    let mut snapshot = runtime.clone();
    snapshot.log = trim_runtime_entries(&runtime.log, BOARD_TASK_RUNTIME_LOG_LIMIT);
    snapshot.last_error = runtime
        .last_error
        .as_deref()
        .map(|message| truncate_message(message, BOARD_MESSAGE_CHAR_LIMIT));
    snapshot
}

fn trim_task_activities(items: &[TaskActivity], limit: usize) -> Vec<TaskActivity> {
    trim_tail(items, limit)
        .into_iter()
        .map(|item| TaskActivity {
            message: truncate_message(&item.message, BOARD_MESSAGE_CHAR_LIMIT),
            ..item
        })
        .collect()
}

fn trim_runtime_entries(items: &[RuntimeLogEntry], limit: usize) -> Vec<RuntimeLogEntry> {
    trim_tail(items, limit)
        .into_iter()
        .map(|item| RuntimeLogEntry {
            message: truncate_message(&item.message, BOARD_MESSAGE_CHAR_LIMIT),
            ..item
        })
        .collect()
}

fn trim_tail<T: Clone>(items: &[T], limit: usize) -> Vec<T> {
    if items.len() <= limit {
        return items.to_vec();
    }
    items[items.len() - limit..].to_vec()
}

fn truncate_message(message: &str, limit: usize) -> String {
    let total_chars = message.chars().count();
    if total_chars <= limit {
        return message.to_string();
    }
    let truncated = message.chars().take(limit).collect::<String>();
    format!("{truncated}...(已截断，共 {total_chars} 字符)")
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
    let (mut persisted, store_path) = {
        let guard = state.inner.lock().await;
        (
            PersistedState {
                users: guard.users.clone(),
                projects: guard.projects.clone(),
                tasks: guard.tasks.clone(),
                agents: guard.agents.clone(),
                task_run_history: guard.task_run_history.clone(),
                pending_questions: guard.pending_questions.clone(),
                project_scans: guard.project_scans.clone(),
                project_sessions: guard.project_sessions.clone(),
                project_chat_messages: guard.project_chat_messages.clone(),
                memory_items: guard.memory_items.clone(),
                memory_revisions: guard.memory_revisions.clone(),
                memory_tags: guard.memory_tags.clone(),
                memory_edges: guard.memory_edges.clone(),
                decisions: guard.decisions.clone(),
            },
            state.store_path.clone(),
        )
    };

    for task in &mut persisted.tasks {
        refresh_task_state_snapshot(task, "server.persist");
    }
    let _ = normalize_task_run_history(&mut persisted);

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
    let chat_messages = state
        .project_chat_messages
        .iter()
        .filter(|message| message.project_id == project_id)
        .cloned()
        .collect::<Vec<_>>();

    Ok(ProjectContextSnapshot {
        project_id,
        primary_workspace: project.primary_workspace().cloned(),
        latest_scan: state.project_scans.get(&project_id).cloned(),
        sessions,
        chat_messages,
        memory: project_memory_snapshot(state, project_id),
    })
}

fn project_summary_snapshot(
    state: &BoardState,
    project_id: Uuid,
) -> AppResult<ProjectSummarySnapshot> {
    let project = find_project(state, project_id)?;
    let project_tasks = state
        .tasks
        .iter()
        .filter(|task| task.project_id == project_id)
        .collect::<Vec<_>>();
    let project_task_ids = project_tasks
        .iter()
        .map(|task| task.id)
        .collect::<HashSet<_>>();
    let project_agents = state.agents.iter().collect::<Vec<_>>();
    let project_sessions = state
        .project_sessions
        .iter()
        .filter(|session| session.project_id == project_id)
        .collect::<Vec<_>>();
    let open_pending_questions = state
        .pending_questions
        .iter()
        .filter(|question| question.project_id == project_id && question.status != "answered")
        .collect::<Vec<_>>();
    let memory = project_memory_snapshot(state, project_id);

    Ok(ProjectSummarySnapshot {
        project_id,
        project_name: project.name.clone(),
        generated_at: timestamp_string(),
        primary_workspace: project.primary_workspace().cloned(),
        latest_scan: state.project_scans.get(&project_id).cloned(),
        task_counts: project_task_status_counts(&project_tasks),
        agent_summary: project_agent_summary(&project_agents, &project_task_ids),
        session_summary: project_session_summary(&project_sessions),
        open_pending_question_count: open_pending_questions.len(),
        pending_questions: open_pending_questions
            .into_iter()
            .take(5)
            .map(|question| ProjectPendingQuestionDigest {
                id: question.id,
                source_task_id: question.source_task_id,
                source_task_title: question.source_task_title.clone(),
                question: question.question.clone(),
                created_at: question.created_at.clone(),
            })
            .collect(),
        active_constraints: active_project_constraint_digests(&memory, project_id),
        recent_task_summaries: recent_task_summary_digests(&memory, &project_tasks, 5),
    })
}

fn project_task_status_counts(tasks: &[&Task]) -> ProjectTaskStatusCounts {
    let mut counts = ProjectTaskStatusCounts::default();
    for task in tasks {
        match task.status {
            TaskStatus::Open => counts.open += 1,
            TaskStatus::Claimed => counts.claimed += 1,
            TaskStatus::Running => counts.running += 1,
            TaskStatus::Paused => counts.paused += 1,
            TaskStatus::Done => counts.done += 1,
            TaskStatus::Failed => counts.failed += 1,
            TaskStatus::Canceled => counts.canceled += 1,
            _ => {}
        }
    }
    counts
}

fn project_agent_summary(
    agents: &[&Agent],
    project_task_ids: &HashSet<Uuid>,
) -> ProjectAgentSummary {
    let total = agents.len();
    let auto_mode_enabled = agents.iter().filter(|agent| agent.auto_mode).count();
    let busy = agents
        .iter()
        .filter(|agent| {
            agent
                .current_task_id
                .is_some_and(|task_id| project_task_ids.contains(&task_id))
        })
        .count();
    ProjectAgentSummary {
        total,
        auto_mode_enabled,
        busy,
        idle: total.saturating_sub(busy),
    }
}

fn project_session_summary(sessions: &[&ProjectSession]) -> ProjectSessionSummary {
    let mut summary = ProjectSessionSummary {
        total: sessions.len(),
        ..ProjectSessionSummary::default()
    };
    for session in sessions {
        match session.status.as_str() {
            "running" => summary.running += 1,
            "paused" => summary.paused += 1,
            "failed" => summary.failed += 1,
            "completed" => summary.completed += 1,
            _ => {}
        }
    }
    summary
}

fn active_project_constraint_digests(
    memory: &ProjectMemorySnapshot,
    project_id: Uuid,
) -> Vec<ProjectConstraintDigest> {
    let tag_name = format!("project/{project_id}/active-constraints");
    let revisions_by_id = memory
        .revisions
        .iter()
        .map(|revision| (revision.id, revision))
        .collect::<HashMap<_, _>>();

    let mut entries = memory
        .items
        .iter()
        .filter(|item| {
            item.scope_kind == "project"
                && item.scope_id == project_id
                && item.memory_kind == "project_constraint"
        })
        .filter_map(|item| {
            let tag = memory
                .tags
                .iter()
                .find(|tag| tag.memory_item_id == item.id && tag.tag == tag_name)?;
            let revision = revisions_by_id.get(&tag.target_revision_id)?;
            Some(ProjectConstraintDigest {
                stable_key: item.stable_key.clone(),
                title: revision.title.clone(),
                content: revision.content.clone(),
                revision_no: revision.revision_no,
                updated_at: revision.created_at.clone(),
            })
        })
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| left.stable_key.cmp(&right.stable_key));
    entries
}

fn recent_task_summary_digests(
    memory: &ProjectMemorySnapshot,
    project_tasks: &[&Task],
    limit: usize,
) -> Vec<ProjectTaskSummaryDigest> {
    let revisions_by_id = memory
        .revisions
        .iter()
        .map(|revision| (revision.id, revision))
        .collect::<HashMap<_, _>>();
    let tasks_by_id = project_tasks
        .iter()
        .map(|task| (task.id, *task))
        .collect::<HashMap<_, _>>();

    let mut entries = memory
        .items
        .iter()
        .filter(|item| item.scope_kind == "task" && item.memory_kind == "task_summary")
        .filter_map(|item| {
            let task = tasks_by_id.get(&item.scope_id)?;
            let tag_name = format!("task/{}/latest-summary", item.scope_id);
            let tag = memory
                .tags
                .iter()
                .find(|tag| tag.memory_item_id == item.id && tag.tag == tag_name)?;
            let revision = revisions_by_id.get(&tag.target_revision_id)?;
            Some(ProjectTaskSummaryDigest {
                task_id: task.id,
                task_title: task.title.clone(),
                summary: revision.content.clone(),
                created_at: revision.created_at.clone(),
                source_kind: revision.source_kind.clone(),
            })
        })
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    entries.truncate(limit);
    entries
}

fn project_memory_snapshot(state: &BoardState, project_id: Uuid) -> ProjectMemorySnapshot {
    let task_ids = state
        .tasks
        .iter()
        .filter(|task| task.project_id == project_id)
        .map(|task| task.id)
        .collect::<HashSet<_>>();

    let items = state
        .memory_items
        .iter()
        .filter(|item| {
            (item.scope_kind == "project" && item.scope_id == project_id)
                || (item.scope_kind == "task" && task_ids.contains(&item.scope_id))
        })
        .cloned()
        .collect::<Vec<_>>();
    let item_ids = items.iter().map(|item| item.id).collect::<HashSet<_>>();

    let revisions = state
        .memory_revisions
        .iter()
        .filter(|revision| item_ids.contains(&revision.memory_item_id))
        .cloned()
        .collect::<Vec<_>>();
    let revision_ids = revisions
        .iter()
        .map(|revision| revision.id)
        .collect::<HashSet<_>>();

    let tags = state
        .memory_tags
        .iter()
        .filter(|tag| item_ids.contains(&tag.memory_item_id))
        .cloned()
        .collect::<Vec<_>>();

    let edges = state
        .memory_edges
        .iter()
        .filter(|edge| {
            revision_ids.contains(&edge.from_revision_id)
                || revision_ids.contains(&edge.to_revision_id)
        })
        .cloned()
        .collect::<Vec<_>>();

    ProjectMemorySnapshot {
        items,
        revisions,
        tags,
        edges,
    }
}

fn normalized_constraint_stable_key(raw: Option<&str>, title: &str) -> String {
    if let Some(key) = raw.map(str::trim).filter(|value| !value.is_empty()) {
        return format!("project_constraint/{key}");
    }

    let normalized = title
        .chars()
        .map(|ch| if ch.is_whitespace() { '-' } else { ch })
        .collect::<String>();
    format!("project_constraint/{normalized}")
}

fn write_memory_revision(state: &mut BoardState, spec: MemoryWriteSpec) -> Uuid {
    let timestamp = timestamp_string();
    let item_id = state
        .memory_items
        .iter()
        .find(|item| {
            item.scope_kind == spec.scope_kind
                && item.scope_id == spec.scope_id
                && item.memory_kind == spec.memory_kind
                && item.stable_key == spec.stable_key
        })
        .map(|item| item.id)
        .unwrap_or_else(|| {
            let id = Uuid::new_v4();
            state.memory_items.push(MemoryItem {
                id,
                scope_kind: spec.scope_kind.into(),
                scope_id: spec.scope_id,
                memory_kind: spec.memory_kind.into(),
                stable_key: spec.stable_key.clone(),
                created_at: timestamp.clone(),
                created_by: spec.created_by,
            });
            id
        });

    let current_tag = state
        .memory_tags
        .iter()
        .find(|tag| tag.memory_item_id == item_id && tag.tag == spec.tag)
        .cloned();
    let current_revision = current_tag.as_ref().and_then(|tag| {
        state
            .memory_revisions
            .iter()
            .find(|revision| revision.id == tag.target_revision_id)
            .cloned()
    });

    if current_revision.as_ref().is_some_and(|revision| {
        revision.title == spec.title
            && revision.content == spec.content
            && revision.structured_payload.as_ref() == spec.structured_payload.as_ref()
            && revision.source_kind == spec.source_kind
            && revision.source_id.as_ref() == spec.source_id.as_ref()
    }) {
        return current_revision.unwrap().id;
    }

    let revision_no = state
        .memory_revisions
        .iter()
        .filter(|revision| revision.memory_item_id == item_id)
        .map(|revision| revision.revision_no)
        .max()
        .unwrap_or(0)
        + 1;
    let revision_id = Uuid::new_v4();
    let supersedes_revision_id = current_tag.as_ref().map(|tag| tag.target_revision_id);
    state.memory_revisions.push(MemoryRevision {
        id: revision_id,
        memory_item_id: item_id,
        revision_no,
        status: "active".into(),
        title: spec.title,
        content: spec.content,
        structured_payload: spec.structured_payload,
        source_kind: spec.source_kind.into(),
        source_id: spec.source_id,
        confidence: spec.confidence,
        supersedes_revision_id,
        created_at: timestamp.clone(),
        created_by: spec.created_by,
    });

    if let Some(previous_revision_id) = supersedes_revision_id {
        state.memory_edges.push(MemoryEdge {
            id: Uuid::new_v4(),
            from_revision_id: revision_id,
            to_revision_id: previous_revision_id,
            edge_kind: "supersedes".into(),
            created_at: timestamp.clone(),
        });
    }

    if let Some(tag) = state
        .memory_tags
        .iter_mut()
        .find(|tag| tag.memory_item_id == item_id && tag.tag == spec.tag)
    {
        tag.target_revision_id = revision_id;
        tag.updated_at = timestamp;
        tag.updated_by = spec.created_by;
    } else {
        state.memory_tags.push(MemoryTag {
            id: Uuid::new_v4(),
            memory_item_id: item_id,
            tag: spec.tag,
            target_revision_id: revision_id,
            updated_at: timestamp,
            updated_by: spec.created_by,
        });
    }

    revision_id
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
            is_key_project_file(path).then(|| display_relative_path(workspace_root, path))
        })
        .take(12)
        .collect::<Vec<_>>();
    let document_files = files
        .iter()
        .filter_map(|path| {
            is_document_file(path).then(|| display_relative_path(workspace_root, path))
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
        detected_stacks: detection
            .stacks
            .iter()
            .map(|stack| stack.to_string())
            .collect(),
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

fn build_stub_project_session_reply(
    project: &Project,
    latest_scan: Option<&ProjectScanSummary>,
    mode: &str,
    prompt: &str,
) -> String {
    let scan_line = latest_scan
        .map(|scan| format!("最近扫描结果：{}。", scan.stack_summary))
        .unwrap_or_else(|| "当前还没有扫描摘要，建议先对项目目录做一次扫描。".into());

    match normalize_project_session_mode(Some(mode)) {
        "planner" => format!(
            "这是规划器会话的本地 Stub 回复。\n\
项目：{}\n\
{}\n\
用户目标：{}\n\
\n\
建议先形成最小规格、里程碑和验收口径，再让生成器沿着同一目标推进。\n\
```json\n\
{{\n\
  \"result\": \"plan_ready\",\n\
  \"summary\": \"已整理出最小可用规划，建议先补规划与验收基线，再进入实现。\",\n\
  \"questions\": [\n\
    {{\n\
      \"question\": \"是否先把桌面端客户端稳定性作为第一优先级？\",\n\
      \"context\": \"当前用户更关注客户端真实可用，而不是纯后端能力堆叠\"\n\
    }}\n\
  ],\n\
  \"follow_ups\": [\n\
    {{\n\
      \"kind\": \"follow_up_task\",\n\
      \"title\": \"梳理客户端 Harness 最小闭环规格\",\n\
      \"description\": \"明确 planner / generator / evaluator 在当前版本中的最小落地点、入口和验收标准。\",\n\
      \"priority\": \"P1\",\n\
      \"can_auto_create_task\": true,\n\
      \"can_auto_apply\": false\n\
    }},\n\
    {{\n\
      \"kind\": \"test_gap\",\n\
      \"title\": \"补项目会话模式回归测试\",\n\
      \"description\": \"覆盖 planner / evaluator 会话完成后自动沉淀记忆和拆任务的行为。\",\n\
      \"priority\": \"P1\",\n\
      \"can_auto_create_task\": true,\n\
      \"can_auto_apply\": false\n\
    }}\n\
  ],\n\
  \"risks\": [\"如果没有统一验收标准，后续多轮会话容易继续碎片化推进\"]\n\
}}\n\
```",
            project.name, scan_line, prompt
        ),
        "evaluator" => format!(
            "这是评估器会话的本地 Stub 回复。\n\
项目：{}\n\
{}\n\
当前评估目标：{}\n\
\n\
初步判断：需要优先检查客户端是否真的能看到运行中任务、实时日志和失败原因。\n\
```json\n\
{{\n\
  \"result\": \"needs_fix\",\n\
  \"summary\": \"发现客户端可观测性仍是主阻塞，建议先修日志和状态可见性。\",\n\
  \"questions\": [],\n\
  \"follow_ups\": [\n\
    {{\n\
      \"kind\": \"bug_fix\",\n\
      \"title\": \"修复项目会话与任务运行日志可见性\",\n\
      \"description\": \"确保客户端能稳定看到运行中任务、Agent 输出和 failed to fetch 的真实原因。\",\n\
      \"priority\": \"P1\",\n\
      \"can_auto_create_task\": true,\n\
      \"can_auto_apply\": false\n\
    }}\n\
  ],\n\
  \"risks\": [\"如果评估只停留在状态字段而没有真实输出链路，客户端仍会表现为像死了一样\"]\n\
}}\n\
```",
            project.name, scan_line, prompt
        ),
        _ => format!(
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
        ),
    }
}

async fn complete_stub_project_session(
    state: &AppState,
    session_id: Uuid,
    project: &Project,
    latest_scan: Option<&ProjectScanSummary>,
    mode: &str,
    prompt: &str,
) -> AppResult<()> {
    let reply = build_stub_project_session_reply(project, latest_scan, mode, prompt);
    let mut guard = state.inner.lock().await;
    let Some(project_session) = find_project_session_mut(&mut guard, session_id) else {
        return Err((StatusCode::NOT_FOUND, "未找到项目会话".into()));
    };
    project_session.status = "completed".into();
    project_session
        .thread_id
        .get_or_insert_with(|| "stub-project-thread".into());
    project_session.active_turn_id = None;
    project_session.messages.push(ProjectSessionMessage {
        role: "assistant".into(),
        content: reply.clone(),
        at: timestamp_string(),
    });
    project_session
        .log
        .push(new_runtime_entry("assistant", reply));
    process_project_session_completion_outputs(&mut guard, session_id);
    drop(guard);
    persist_state(state).await
}

async fn mark_project_session_failed(state: &AppState, session_id: Uuid, message: &str) {
    let mut guard = state.inner.lock().await;
    if let Some(project_session) = find_project_session_mut(&mut guard, session_id) {
        project_session.status = "failed".into();
        project_session.active_turn_id = None;
        project_session.last_error = Some(message.to_string());
        project_session.log.push(new_runtime_entry(
            "project.session.error",
            message.to_string(),
        ));
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
        let mut should_process_completion_outputs = false;
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
                    should_process_completion_outputs = status == "completed";
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

        if should_process_completion_outputs {
            let mut guard = state.inner.lock().await;
            process_project_session_completion_outputs(&mut guard, session_id);
        }

        let _ = persist_state(&state).await;

        if remove_runtime {
            state.runtime_sessions.lock().await.remove(&session_id);
            break;
        }
    }

    state.runtime_sessions.lock().await.remove(&session_id);
    reconcile_project_session_runtime_lost(&state, session_id).await;
}

async fn reconcile_project_session_runtime_lost(state: &AppState, session_id: Uuid) {
    let mut guard = state.inner.lock().await;
    let Some(project_session) = find_project_session_mut(&mut guard, session_id) else {
        return;
    };

    if project_session.status != "running" {
        return;
    }

    let message = "本地项目会话已断开，当前会话已暂停，可继续沿用原 thread 恢复";
    project_session.status = "paused".into();
    project_session.active_turn_id = None;
    project_session.last_error = Some(message.into());
    project_session
        .log
        .push(new_runtime_entry("project.session.runtime_lost", message));
    drop(guard);
    let _ = persist_state(state).await;
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

fn find_task(state: &BoardState, task_id: Uuid) -> AppResult<&Task> {
    state
        .tasks
        .iter()
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

fn find_pending_question_mut(
    state: &mut BoardState,
    question_id: Uuid,
) -> AppResult<&mut PendingQuestion> {
    state
        .pending_questions
        .iter_mut()
        .find(|question| question.id == question_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到待回答问题".into()))
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

fn prompt_timestamp_key(value: &str) -> u128 {
    value.parse::<u128>().unwrap_or_default()
}

fn prompt_preview(input: &str, max_chars: usize) -> String {
    let compact = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "无".into();
    }

    let mut chars = compact.chars();
    let preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

fn prompt_section_or_default(lines: Vec<String>, empty_message: &str) -> String {
    if lines.is_empty() {
        empty_message.into()
    } else {
        lines.join("\n")
    }
}

fn project_scan_summary_for_prompt(latest_scan: Option<&ProjectScanSummary>) -> String {
    latest_scan
        .map(|scan| {
            format!(
                "工作区：{}（{}）\n技术栈摘要：{}\n顶层目录：{}\n关键文件：{}\n文档文件：{}\n提示：{}",
                scan.workspace_label,
                scan.workspace_path,
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
        .unwrap_or_else(|| {
            "最近还没有项目扫描摘要；执行前要先结合目录实际情况判断，不要把旧认知当成当前事实。"
                .into()
        })
}

fn recent_task_activity_lines(task: &Task, limit: usize) -> Vec<String> {
    let mut activities = task.activities.iter().collect::<Vec<_>>();
    activities.sort_by_key(|activity| prompt_timestamp_key(&activity.at));
    let skip = activities.len().saturating_sub(limit);
    activities
        .into_iter()
        .skip(skip)
        .map(|activity| {
            format!(
                "- [{}] {}：{}",
                activity.at,
                activity.kind,
                prompt_preview(&activity.message, 120)
            )
        })
        .collect()
}

fn recent_task_runtime_lines(task: &Task, limit: usize) -> Vec<String> {
    let Some(runtime) = task.runtime.as_ref() else {
        return Vec::new();
    };

    let mut entries = runtime.log.iter().collect::<Vec<_>>();
    entries.sort_by_key(|entry| prompt_timestamp_key(&entry.at));
    let skip = entries.len().saturating_sub(limit);
    entries
        .into_iter()
        .skip(skip)
        .map(|entry| {
            format!(
                "- [{}] {}：{}",
                entry.at,
                entry.kind,
                prompt_preview(&entry.message, 120)
            )
        })
        .collect()
}

fn recent_project_chat_messages(
    messages: &[ProjectChatMessage],
    project_id: Uuid,
    limit: usize,
) -> Vec<ProjectChatMessage> {
    let mut relevant = messages
        .iter()
        .filter(|message| message.project_id == project_id)
        .cloned()
        .collect::<Vec<_>>();
    relevant.sort_by_key(|message| prompt_timestamp_key(&message.at));
    let skip = relevant.len().saturating_sub(limit);
    relevant.into_iter().skip(skip).collect()
}

fn recent_project_chat_lines(messages: &[ProjectChatMessage]) -> Vec<String> {
    messages
        .iter()
        .map(|message| {
            format!(
                "- [{}] {}：{}",
                message.at,
                message.user_display_name,
                prompt_preview(&message.content, 120)
            )
        })
        .collect()
}

fn contains_scope_change_signal(text: &str) -> bool {
    let lowered = text.to_lowercase();
    [
        "取消",
        "撤销",
        "不做",
        "先不做",
        "去掉",
        "移除",
        "删除",
        "关闭",
        "废弃",
        "放弃",
        "不用做",
        "不要做",
        "不需要",
        "终止",
        "搁置",
        "撤回",
        "去除",
        "cancel",
        "drop",
        "remove",
        "skip",
        "disable",
    ]
    .iter()
    .any(|keyword| lowered.contains(keyword))
}

fn recent_scope_signal_lines(
    task: &Task,
    project_chat_messages: &[ProjectChatMessage],
) -> Vec<String> {
    let mut signals = task
        .activities
        .iter()
        .filter(|activity| {
            activity.kind == "task.canceled" || contains_scope_change_signal(&activity.message)
        })
        .map(|activity| {
            (
                prompt_timestamp_key(&activity.at),
                format!(
                    "- [任务活动 {}] {}：{}",
                    activity.at,
                    activity.kind,
                    prompt_preview(&activity.message, 120)
                ),
            )
        })
        .collect::<Vec<_>>();
    signals.extend(project_chat_messages.iter().filter_map(|message| {
        contains_scope_change_signal(&message.content).then(|| {
            (
                prompt_timestamp_key(&message.at),
                format!(
                    "- [项目聊天室 {}] {}：{}",
                    message.at,
                    message.user_display_name,
                    prompt_preview(&message.content, 120)
                ),
            )
        })
    }));
    signals.sort_by_key(|(at, _)| *at);
    let skip = signals.len().saturating_sub(6);
    signals
        .into_iter()
        .skip(skip)
        .map(|(_, line)| line)
        .collect()
}

fn active_project_constraint_lines(state: &BoardState, project_id: Uuid) -> Vec<String> {
    let tag_name = format!("project/{project_id}/active-constraints");
    let mut lines = state
        .memory_items
        .iter()
        .filter(|item| {
            item.scope_kind == "project"
                && item.scope_id == project_id
                && item.memory_kind == "project_constraint"
        })
        .filter_map(|item| {
            let tag = state
                .memory_tags
                .iter()
                .find(|tag| tag.memory_item_id == item.id && tag.tag == tag_name)?;
            let revision = state
                .memory_revisions
                .iter()
                .find(|revision| revision.id == tag.target_revision_id)?;
            Some(format!(
                "- {}：{}",
                revision.title,
                prompt_preview(&revision.content, 120)
            ))
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines
}

fn recent_project_task_summary_lines(
    state: &BoardState,
    project_id: Uuid,
    limit: usize,
) -> Vec<String> {
    let project_tasks = state
        .tasks
        .iter()
        .filter(|task| task.project_id == project_id)
        .collect::<Vec<_>>();
    let memory = project_memory_snapshot(state, project_id);
    recent_task_summary_digests(&memory, &project_tasks, limit)
        .into_iter()
        .map(|entry| {
            format!(
                "- {}：{}",
                entry.task_title,
                prompt_preview(&entry.summary, 120)
            )
        })
        .collect()
}

fn open_pending_question_lines(state: &BoardState, project_id: Uuid, limit: usize) -> Vec<String> {
    let mut questions = state
        .pending_questions
        .iter()
        .filter(|question| question.project_id == project_id && question.status != "answered")
        .collect::<Vec<_>>();
    questions.sort_by_key(|question| prompt_timestamp_key(&question.created_at));
    let skip = questions.len().saturating_sub(limit);
    questions
        .into_iter()
        .skip(skip)
        .map(|question| {
            format!(
                "- {}：{}",
                question.source_task_title,
                prompt_preview(&question.question, 120)
            )
        })
        .collect()
}

fn compose_task_context_snapshot(
    task: &Task,
    project: &Project,
    latest_scan: Option<&ProjectScanSummary>,
    project_constraint_lines: &[String],
    recent_task_summary_lines: &[String],
    pending_question_lines: &[String],
    project_chat_messages: &[ProjectChatMessage],
    recent_activity_lines: &[String],
    recent_runtime_lines: &[String],
    scope_signal_lines: &[String],
) -> String {
    let priority = task
        .priority
        .map(|priority| priority_label(priority).to_string())
        .unwrap_or_else(|| "未设置".into());
    let snapshot = serde_json::json!({
        "project": {
            "name": project.name,
            "description": prompt_preview(&project.description, 160),
            "workspace_roots": project.workspace_roots.iter().map(|workspace| serde_json::json!({
                "label": workspace.label,
                "path": workspace.path,
                "writable": workspace.writable,
            })).collect::<Vec<_>>(),
        },
        "task": {
            "title": task.title,
            "description": prompt_preview(&task.description, 240),
            "status": task.status.as_str(),
            "priority": priority,
            "labels": task.labels,
            "thread_id": task.runtime.as_ref().and_then(|runtime| runtime.thread_id.as_deref()),
            "last_error": task.runtime.as_ref().and_then(|runtime| runtime.last_error.as_deref()),
        },
        "scan": latest_scan.map(|scan| serde_json::json!({
            "stack_summary": scan.stack_summary,
            "top_level_entries": scan.top_level_entries,
            "key_files": scan.key_files,
            "document_files": scan.document_files,
            "notes": scan.notes,
        })),
        "constraints": project_constraint_lines,
        "recent_task_summaries": recent_task_summary_lines,
        "pending_questions": pending_question_lines,
        "scope_signals": scope_signal_lines,
        "recent_activity": recent_activity_lines,
        "recent_runtime": recent_runtime_lines,
        "recent_project_chat": project_chat_messages
            .iter()
            .map(|message| {
                format!(
                    "[{}] {}: {}",
                    message.at,
                    message.user_display_name,
                    prompt_preview(&message.content, 120)
                )
            })
            .collect::<Vec<_>>(),
    });

    serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".into())
}

#[allow(unreachable_code)]
fn compose_task_prompt(
    task: &Task,
    project: &Project,
    latest_scan: Option<&ProjectScanSummary>,
    project_constraint_lines: &[String],
    recent_task_summary_lines: &[String],
    pending_question_lines: &[String],
    project_chat_messages: &[ProjectChatMessage],
    prompt_override: Option<String>,
) -> String {
    return crate::prompt::compose_task_prompt_with_snapshot(
        task,
        project,
        latest_scan,
        project_constraint_lines,
        recent_task_summary_lines,
        pending_question_lines,
        project_chat_messages,
        prompt_override.clone(),
    );

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
    let scan_summary = project_scan_summary_for_prompt(latest_scan);
    let recent_activity_lines = recent_task_activity_lines(task, 8);
    let recent_activity_summary =
        prompt_section_or_default(recent_activity_lines.clone(), "最近还没有任务活动记录。");
    let recent_runtime_lines = recent_task_runtime_lines(task, 6);
    let recent_runtime_summary =
        prompt_section_or_default(recent_runtime_lines.clone(), "最近还没有运行输出。");
    let recent_chat_summary = prompt_section_or_default(
        recent_project_chat_lines(project_chat_messages),
        "最近还没有项目聊天室消息。",
    );
    let project_constraint_summary = prompt_section_or_default(
        project_constraint_lines.to_vec(),
        "当前还没有沉淀到记忆层的项目长期约束；若最近聊天或任务活动出现明确约束，执行时仍要优先遵守。",
    );
    let recent_task_summary = prompt_section_or_default(
        recent_task_summary_lines.to_vec(),
        "当前还没有沉淀到记忆层的最近任务摘要。",
    );
    let pending_question_summary = prompt_section_or_default(
        pending_question_lines.to_vec(),
        "当前没有未回答的项目问题。",
    );
    let scope_signal_lines = recent_scope_signal_lines(task, project_chat_messages);
    let scope_signal_summary = if scope_signal_lines.is_empty() {
        "最近未检测到明确的撤销/不做信号；但执行前仍要核对最新活动、运行输出和项目聊天，不要机械照搬旧描述。".into()
    } else {
        format!(
            "最近检测到以下范围收缩或取消信号，请优先遵守这些更近的明确决策，不要继续实现对应子需求：\n{}",
            scope_signal_lines.join("\n")
        )
    };
    let context_snapshot = compose_task_context_snapshot(
        task,
        project,
        latest_scan,
        project_constraint_lines,
        recent_task_summary_lines,
        pending_question_lines,
        project_chat_messages,
        &recent_activity_lines,
        &recent_runtime_lines,
        &scope_signal_lines,
    );

    let mut prompt = format!(
        "你正在执行 Spotlight 项目任务。\n\
项目名称：{}\n\
项目说明：{}\n\
工作目录：\n{}\n\
任务标题：{}\n\
任务描述：{}\n\
\n\
最近扫描摘要：\n{}\n\
\n\
最近任务活动：\n{}\n\
\n\
最近运行输出：\n{}\n\
\n\
当前有效项目约束：\n{}\n\
\n\
最近项目聊天室：\n{}\n\
\n\
范围提醒：\n{}\n\
\n\
执行要求：\n\
1. 先分析再行动，给出清晰的执行步骤。\n\
2. 不要假设当前目录一定是代码仓库；它可能为空，也可能只有 Word、PDF、表格、图片或其他资料。\n\
3. 如果遇到 Office 或二进制文件，不要臆造内容，可以基于文件名、目录结构、相邻文本和可读元数据给出判断。\n\
4. 修改前要先核对上面的最近活动、最近运行输出和项目聊天室，避免重复劳动或继续做过期需求。\n\
5. 对\u{201c}当前有效项目约束\u{201d}要视为跨会话仍然有效的长期规则，除非有更新、更明确的近因决策覆盖它。\n\
6. 如果最近活动、运行输出或项目聊天表明某个子需求已撤销、先不做、去掉或删除，必须将其视为当前范围外，并在结论里说明你如何收敛范围。\n\
7. 如果任务标题或旧描述与最近明确决策冲突，以时间更近、表达更明确的决策为准；必要时先做最小安全收口，再提出后续任务。\n\
8. 项目外目录允许读取，但不要做破坏性修改。\n\
9. 输出时尽量用中文，结论、风险和建议都要清楚可读。\n\
10. 任务结束时，请在最后附加一个 ```json 代码块，字段至少包含 result、summary、questions、follow_ups、risks；如果没有内容也要给空数组。",
        project.name,
        project.description,
        workspace_list,
        task.title,
        task.description,
        scan_summary,
        recent_activity_summary,
        recent_runtime_summary,
        project_constraint_summary,
        recent_chat_summary,
        scope_signal_summary
    );

    prompt.push_str("\n\n最近任务摘要：\n");
    prompt.push_str(&recent_task_summary);
    prompt.push_str("\n\n仍待回答的项目问题：\n");
    prompt.push_str(&pending_question_summary);
    prompt.push_str("\n\n任务上下文快照（机器可读）：\n");
    prompt.push_str(&context_snapshot);

    if let Some(extra_prompt) = prompt_override {
        let extra_prompt = extra_prompt.trim();
        if !extra_prompt.is_empty() {
            prompt.push_str("\n\n用户补充提示词：\n");
            prompt.push_str(extra_prompt);
        }
    }

    prompt
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
            "请在当前项目工作目录中完成一次\u{201c}本地编译重启\u{201d}尝试，并输出中文结论。\n\
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
        priority: None,
        labels: Vec::new(),
        creator_user_id: None,
        assignee_user_id: None,
        assignment_mode: TaskAssignmentMode::PublicQueue,
        requested_agent_id: None,
        source_task_id: None,
        claimed_by: None,
        activities: vec![new_activity(
            "task.local_build_restart_created",
            format!("已为项目\u{201c}{}\u{201d}创建本地编译重启任务", project.name),
        )],
        runtime: None,
        approval: Default::default(),
        acceptance: Default::default(),
        state_snapshot: TaskStateSnapshot::default(),
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
            "请为当前项目执行一次\u{201c}云端安装重启\u{201d}任务，并输出中文结论。\n\
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
        priority: None,
        labels: Vec::new(),
        creator_user_id: None,
        assignee_user_id: None,
        assignment_mode: TaskAssignmentMode::PublicQueue,
        requested_agent_id: None,
        source_task_id: None,
        claimed_by: None,
        activities: vec![new_activity(
            "task.cloud_install_restart_created",
            format!("已为项目\u{201c}{}\u{201d}创建云端安装重启任务", project.name),
        )],
        runtime: None,
        approval: Default::default(),
        acceptance: Default::default(),
        state_snapshot: TaskStateSnapshot::default(),
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
        priority: None,
        labels: Vec::new(),
        creator_user_id: None,
        assignee_user_id: None,
        assignment_mode: TaskAssignmentMode::PublicQueue,
        requested_agent_id: None,
        source_task_id: None,
        claimed_by: None,
        activities: vec![new_activity(
            "task.explore_created",
            format!("已为项目\u{201c}{}\u{201d}创建探索任务", project.name),
        )],
        runtime: None,
        approval: Default::default(),
        acceptance: Default::default(),
        state_snapshot: TaskStateSnapshot::default(),
    }
}

fn task_run_state_for_task_status(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Done | TaskStatus::Accepted | TaskStatus::PendingAcceptance => "completed",
        TaskStatus::Failed => "failed",
        TaskStatus::Canceled => "aborted",
        TaskStatus::Running | TaskStatus::Claimed => "executing",
        _ => "interrupted",
    }
}

fn task_run_is_terminal(state: &str) -> bool {
    matches!(state, "completed" | "failed" | "aborted")
}

fn append_task_run_thread(run: &mut TaskRunRecord, thread_id: Option<&str>) {
    let Some(thread_id) = thread_id.filter(|value| !value.is_empty()) else {
        return;
    };
    if !run
        .session_threads
        .iter()
        .any(|existing| existing == thread_id)
    {
        run.session_threads.push(thread_id.to_string());
    }
}

fn build_legacy_task_run_record(task: &Task, run_number: u32) -> TaskRunRecord {
    let started_at = task
        .activities
        .first()
        .map(|activity| activity.at.clone())
        .unwrap_or_else(timestamp_string);
    let ended_at = (!matches!(task.status, TaskStatus::Running | TaskStatus::Claimed)).then(|| {
        task.activities
            .last()
            .map(|activity| activity.at.clone())
            .unwrap_or_else(timestamp_string)
    });
    let run_state = task_run_state_for_task_status(task.status).to_string();
    let runtime = task.runtime.as_ref();
    let thread_id = runtime.and_then(|item| item.thread_id.clone());
    let turn_id = runtime.and_then(|item| item.active_turn_id.clone());
    let last_error = runtime.and_then(|item| item.last_error.clone());

    TaskRunRecord {
        id: Uuid::new_v4(),
        task_id: task.id,
        run_number,
        state: run_state.clone(),
        provider: runtime
            .map(|item| item.provider.clone())
            .unwrap_or_else(|| "codex".into()),
        started_by_agent_id: task.claimed_by,
        started_at: started_at.clone(),
        ended_at,
        retry_count: 0,
        primary_workspace_path: None,
        session_threads: thread_id.iter().cloned().collect(),
        attempts: vec![TaskRunAttemptRecord {
            id: Uuid::new_v4(),
            attempt_number: 1,
            trigger_kind: "legacy_backfill".into(),
            status: run_state,
            prompt: task.description.clone(),
            started_at,
            ended_at: task
                .activities
                .last()
                .map(|activity| activity.at.clone())
                .or_else(|| Some(timestamp_string())),
            thread_id,
            turn_id,
            error_summary: last_error.clone(),
        }],
        log: runtime.map(|item| item.log.clone()).unwrap_or_default(),
        last_error,
    }
}

fn record_task_run_start(
    state: &mut BoardState,
    task_id: Uuid,
    agent_id: Uuid,
    provider_id: &str,
    prompt: &str,
    primary_workspace_path: Option<String>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    trigger_kind: &str,
) {
    let runs = state.task_run_history.entry(task_id).or_default();
    let now = timestamp_string();

    if trigger_kind == "resume" {
        if let Some(run) = runs
            .last_mut()
            .filter(|run| !task_run_is_terminal(&run.state))
        {
            run.state = "executing".into();
            run.provider = provider_id.to_string();
            run.started_by_agent_id = Some(agent_id);
            if run.primary_workspace_path.is_none() {
                run.primary_workspace_path = primary_workspace_path;
            }
            append_task_run_thread(run, thread_id.as_deref());
            run.last_error = None;
            run.retry_count = run.attempts.len() as u32;
            run.attempts.push(TaskRunAttemptRecord {
                id: Uuid::new_v4(),
                attempt_number: run.attempts.len() as u32 + 1,
                trigger_kind: trigger_kind.into(),
                status: "executing".into(),
                prompt: prompt.to_string(),
                started_at: now.clone(),
                ended_at: None,
                thread_id,
                turn_id,
                error_summary: None,
            });
            run.log.push(new_runtime_entry(
                "system",
                format!("task run resumed via {trigger_kind}"),
            ));
            return;
        }
    }

    let run_number = runs.last().map(|run| run.run_number + 1).unwrap_or(1);
    let log_message = if trigger_kind == "resume" {
        "task run created from resume"
    } else {
        "task run created"
    };
    runs.push(TaskRunRecord {
        id: Uuid::new_v4(),
        task_id,
        run_number,
        state: "executing".into(),
        provider: provider_id.to_string(),
        started_by_agent_id: Some(agent_id),
        started_at: now.clone(),
        ended_at: None,
        retry_count: 0,
        primary_workspace_path,
        session_threads: thread_id.iter().cloned().collect(),
        attempts: vec![TaskRunAttemptRecord {
            id: Uuid::new_v4(),
            attempt_number: 1,
            trigger_kind: trigger_kind.into(),
            status: "executing".into(),
            prompt: prompt.to_string(),
            started_at: now.clone(),
            ended_at: None,
            thread_id,
            turn_id,
            error_summary: None,
        }],
        log: vec![new_runtime_entry("system", log_message)],
        last_error: None,
    });
}

fn record_task_run_transition(
    state: &mut BoardState,
    task_id: Uuid,
    next_state: &str,
    detail: Option<&str>,
    thread_id: Option<&str>,
    turn_id: Option<&str>,
) {
    let Some(run) = state
        .task_run_history
        .entry(task_id)
        .or_default()
        .last_mut()
    else {
        return;
    };

    let now = timestamp_string();
    run.state = next_state.to_string();
    append_task_run_thread(run, thread_id);
    if task_run_is_terminal(next_state) {
        run.ended_at = Some(now.clone());
    }

    if next_state == "completed" {
        run.last_error = None;
    } else if let Some(detail) = detail {
        run.last_error = Some(detail.to_string());
    }

    if let Some(attempt) = run.attempts.last_mut() {
        attempt.status = next_state.to_string();
        if let Some(thread_id) = thread_id {
            attempt.thread_id = Some(thread_id.to_string());
        }
        if let Some(turn_id) = turn_id {
            attempt.turn_id = Some(turn_id.to_string());
        }
        if next_state != "executing" {
            attempt.ended_at = Some(now.clone());
        }
        attempt.error_summary = match next_state {
            "completed" => None,
            _ => detail.map(ToOwned::to_owned),
        };
    }

    run.retry_count = run.attempts.len().saturating_sub(1) as u32;
    if let Some(detail) = detail {
        run.log
            .push(new_runtime_entry("system", detail.to_string()));
    }
}

fn normalize_task_run_history_entries(
    tasks: &[Task],
    task_run_history: &mut HashMap<Uuid, Vec<TaskRunRecord>>,
) -> bool {
    let mut changed = false;
    let tasks = tasks.to_vec();

    for task in tasks {
        let runs = task_run_history.entry(task.id).or_default();
        if runs.is_empty() && task_has_progress_evidence(&task) {
            runs.push(build_legacy_task_run_record(&task, 1));
            changed = true;
        }

        let Some(run) = runs.last_mut() else {
            continue;
        };

        let expected_state = task_run_state_for_task_status(task.status);
        if run.state != expected_state {
            run.state = expected_state.to_string();
            changed = true;
        }

        let runtime = task.runtime.as_ref();
        append_task_run_thread(run, runtime.and_then(|item| item.thread_id.as_deref()));
        if runtime
            .and_then(|item| item.thread_id.as_deref())
            .is_some_and(|thread_id| !run.session_threads.iter().any(|item| item == thread_id))
        {
            changed = true;
        }

        if task_run_is_terminal(expected_state) && run.ended_at.is_none() {
            run.ended_at = task.activities.last().map(|activity| activity.at.clone());
            if run.ended_at.is_none() {
                run.ended_at = Some(timestamp_string());
            }
            changed = true;
        }

        if let Some(attempt) = run.attempts.last_mut() {
            if attempt.status != expected_state {
                attempt.status = expected_state.to_string();
                changed = true;
            }
            if let Some(runtime) = runtime {
                if attempt.thread_id != runtime.thread_id {
                    attempt.thread_id = runtime.thread_id.clone();
                    changed = true;
                }
                if runtime.active_turn_id.is_some() && attempt.turn_id != runtime.active_turn_id {
                    attempt.turn_id = runtime.active_turn_id.clone();
                    changed = true;
                }
                if runtime.last_error.is_some() && attempt.error_summary != runtime.last_error {
                    attempt.error_summary = runtime.last_error.clone();
                    changed = true;
                }
            }
            if task_run_is_terminal(expected_state) && attempt.ended_at.is_none() {
                attempt.ended_at = run.ended_at.clone().or_else(|| Some(timestamp_string()));
                changed = true;
            }
        }
    }

    changed
}

fn normalize_task_run_history(state: &mut PersistedState) -> bool {
    normalize_task_run_history_entries(&state.tasks, &mut state.task_run_history)
}

fn mark_task_running(
    state: &mut BoardState,
    task_id: Uuid,
    agent_id: Uuid,
    agent_name: &str,
    provider_id: &str,
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
        if matches!(
            task.status,
            TaskStatus::Running | TaskStatus::Done | TaskStatus::Canceled
        ) {
            return Err((StatusCode::CONFLICT, "当前任务状态不允许启动".into()));
        }

        task.status = TaskStatus::Running;
        task.claimed_by = Some(agent_id);
        task.activities.push(new_activity(
            "task.started",
            format!("已由 {agent_name} 开始执行"),
        ));
        let runtime = task.runtime.get_or_insert_with(|| TaskRuntime {
            provider: provider_id.to_string(),
            thread_id: None,
            active_turn_id: None,
            git_auto_merge_enabled: false,
            log: Vec::new(),
            last_error: None,
        });
        runtime.provider = provider_id.to_string();
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
        format!("running task: {task_title}"),
    );
    Ok(())
}

fn task_pause_runtime_ids(task: &Task) -> AppResult<(String, String)> {
    if !matches!(task.status, TaskStatus::Running) {
        return Err((
            StatusCode::CONFLICT,
            "当前任务不处于可暂停的运行状态".into(),
        ));
    }

    let runtime = task
        .runtime
        .as_ref()
        .ok_or_else(|| (StatusCode::CONFLICT, "当前任务没有活动会话".into()))?;
    let thread_id = runtime
        .thread_id
        .clone()
        .ok_or_else(|| (StatusCode::CONFLICT, "缺少 thread_id，无法暂停".into()))?;
    let turn_id = runtime
        .active_turn_id
        .clone()
        .ok_or_else(|| (StatusCode::CONFLICT, "缺少活动 turn_id，无法暂停".into()))?;
    Ok((thread_id, turn_id))
}

fn task_resume_thread_id(task: &Task) -> AppResult<String> {
    if !matches!(task.status, TaskStatus::Paused) {
        return Err((
            StatusCode::CONFLICT,
            "当前任务不处于可恢复的暂停状态".into(),
        ));
    }

    let runtime = task
        .runtime
        .as_ref()
        .ok_or_else(|| (StatusCode::CONFLICT, "当前任务没有可恢复的会话".into()))?;
    runtime
        .thread_id
        .clone()
        .ok_or_else(|| (StatusCode::CONFLICT, "缺少 thread_id，无法恢复".into()))
}

fn assign_agent_running(state: &mut BoardState, agent_id: Uuid, task_id: Uuid, action: String) {
    assign_agent_task(state, agent_id, task_id, "RUNNING", action);
}

fn assign_agent_claimed(state: &mut BoardState, agent_id: Uuid, task_id: Uuid, action: String) {
    assign_agent_task(state, agent_id, task_id, "CLAIMED", action);
}

fn assign_agent_task(
    state: &mut BoardState,
    agent_id: Uuid,
    task_id: Uuid,
    status: &str,
    action: String,
) {
    if let Some(agent) = state.agents.iter_mut().find(|agent| agent.id == agent_id) {
        agent.status = status.into();
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

#[cfg(test)]
mod tests {
    use super::{
        active_task_conflict, auto_claim_next_task, build_api_router, build_app, default_agents,
        default_projects, default_state, default_users, detect_project_stack,
        finalize_git_task_branch_in_repo, mark_task_running, prepare_git_task_branch_in_repo,
        reconcile_parallel_active_tasks, reconcile_watchdog_state, run_automation_cycle_once,
        runtime_event_loop, sanitize_credential_hint, select_next_auto_resume_task_id,
        write_memory_revision, AppState, BoardState, MemoryWriteSpec, PersistedState,
        ProjectChatMessage, ProjectContextSnapshot, ProjectMemorySnapshot, ProjectScanSummary,
        ProjectSession, ProjectSummarySnapshot, PullNextResponse, RuntimeEvent, RuntimeMode,
        BOARD_MESSAGE_CHAR_LIMIT, BOARD_TASK_ACTIVITY_LIMIT, BOARD_TASK_RUNTIME_LOG_LIMIT,
        TASK_STALE_TIMEOUT_SECS,
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        Router,
    };
    use platform_core::{
        new_activity, new_runtime_entry, AgentInvocationRequest, AgentResumeRequest, BoardSnapshot,
        CreateTaskRequest, PendingQuestion, Project, RuntimeLogEntry, Task, TaskActivity,
        TaskAssignmentMode, TaskPriority, TaskRuntime, TaskStateSnapshot, TaskStatus,
        WorkspaceRoot,
    };
    use serde_json::Value;
    use std::{
        collections::{HashMap, HashSet},
        ffi::OsString,
        fs,
        path::{Path, PathBuf},
        process::Command as StdCommand,
        sync::OnceLock,
        time::{SystemTime, UNIX_EPOCH},
    };
    use tokio::{
        sync::mpsc,
        time::{sleep, Duration},
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

    async fn test_app_with_state(
        runtime_mode: RuntimeMode,
        workspace_root: PathBuf,
    ) -> (Router, AppState) {
        let state = default_state(runtime_mode, workspace_root);
        {
            let mut guard = state.inner.lock().await;
            for agent in &mut guard.agents {
                agent.auto_mode = false;
            }
        }
        let api = build_api_router();
        let app = Router::new()
            .nest("/api", api.clone())
            .nest("/api/v1", api)
            .with_state(state.clone());
        (app, state)
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

    async fn read_text(response: axum::response::Response) -> String {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(body.to_vec()).unwrap()
    }

    fn test_project(name: &str, workspace_path: PathBuf) -> Project {
        Project {
            id: Uuid::new_v4(),
            name: name.into(),
            description: format!("{name} project"),
            workspace_roots: vec![WorkspaceRoot {
                id: Uuid::new_v4(),
                label: format!("{name} workspace"),
                path: workspace_path.to_string_lossy().into_owned(),
                writable: true,
            }],
            is_spotlight_self: false,
        }
    }

    fn test_task(
        project_id: Uuid,
        title: &str,
        status: TaskStatus,
        priority: Option<TaskPriority>,
    ) -> Task {
        Task {
            id: Uuid::new_v4(),
            project_id,
            title: title.into(),
            description: format!("{title} description"),
            status,
            priority,
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![TaskActivity {
                kind: "task.created".into(),
                message: format!("{title} created"),
                at: "1".into(),
            }],
            runtime: None,
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        }
    }

    fn codex_stub_env_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    struct TestCodexStubEnvironment {
        root: PathBuf,
        log_path: PathBuf,
        original_path: Option<OsString>,
        original_log_path: Option<OsString>,
        original_hang_method: Option<OsString>,
        original_runtime_timeout_secs: Option<OsString>,
    }

    impl TestCodexStubEnvironment {
        fn install() -> Self {
            Self::install_with_options(None, None)
        }

        fn install_with_options(
            hang_method: Option<&str>,
            runtime_timeout_secs: Option<&str>,
        ) -> Self {
            let root = unique_temp_path("spotlight-codex-stub");
            fs::create_dir_all(&root).unwrap();

            let log_path = root.join("requests.jsonl");
            let command_path = root.join("codex.cmd");
            let script_path = root.join("codex-app-server-stub.js");

            fs::write(
                &command_path,
                "@echo off\r\nnode \"%~dp0codex-app-server-stub.js\" %*\r\n",
            )
            .unwrap();
            fs::write(
                &script_path,
                r#"const fs = require('fs');

const logPath = process.env.SPOTLIGHT_CODEX_STUB_LOG;
const hangMethod = process.env.SPOTLIGHT_CODEX_STUB_HANG_METHOD || '';
let turnCounter = 0;
let buffer = '';

function send(payload) {
  process.stdout.write(`${JSON.stringify(payload)}\n`);
}

function handleRequest(request) {
  if (hangMethod && request.method === hangMethod) {
    return;
  }

  switch (request.method) {
    case 'initialize':
      send({ id: request.id, result: {} });
      break;
    case 'thread/start': {
      const threadId = 'stub-thread-1';
      send({ method: 'thread/started', params: { thread: { id: threadId } } });
      send({ id: request.id, result: { thread: { id: threadId } } });
      break;
    }
    case 'thread/resume': {
      const threadId = request.params?.threadId || 'stub-thread-resumed';
      send({ method: 'thread/started', params: { thread: { id: threadId } } });
      send({ id: request.id, result: { thread: { id: threadId } } });
      break;
    }
    case 'turn/start': {
      turnCounter += 1;
      const turnId = `stub-turn-${turnCounter}`;
      send({ method: 'turn/started', params: { turn: { id: turnId } } });
      send({ id: request.id, result: { turn: { id: turnId } } });
      break;
    }
    case 'turn/interrupt': {
      const turnId = request.params?.turnId || 'stub-turn-unknown';
      send({ method: 'turn/completed', params: { turn: { id: turnId, status: 'interrupted' } } });
      send({ id: request.id, result: {} });
      break;
    }
    default:
      send({ id: request.id, error: { message: `unsupported method: ${request.method}` } });
      break;
  }
}

process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => {
  buffer += chunk;
  let newlineIndex = buffer.indexOf('\n');
  while (newlineIndex >= 0) {
    const line = buffer.slice(0, newlineIndex).trim();
    buffer = buffer.slice(newlineIndex + 1);
    newlineIndex = buffer.indexOf('\n');

    if (!line) {
      continue;
    }

    fs.appendFileSync(logPath, `${line}\n`);
    handleRequest(JSON.parse(line));
  }
});
"#,
            )
            .unwrap();

            let original_path = std::env::var_os("PATH");
            let original_log_path = std::env::var_os("SPOTLIGHT_CODEX_STUB_LOG");
            let original_hang_method = std::env::var_os("SPOTLIGHT_CODEX_STUB_HANG_METHOD");
            let original_runtime_timeout_secs =
                std::env::var_os("SPOTLIGHT_RUNTIME_OP_TIMEOUT_SECS");
            let mut updated_path = OsString::from(root.as_os_str());
            if let Some(path) = original_path.as_ref() {
                updated_path.push(";");
                updated_path.push(path);
            }

            std::env::set_var("PATH", updated_path);
            std::env::set_var("SPOTLIGHT_CODEX_STUB_LOG", &log_path);
            std::env::set_var("SPOTLIGHT_PROVIDER", "codex");
            if let Some(hang_method) = hang_method {
                std::env::set_var("SPOTLIGHT_CODEX_STUB_HANG_METHOD", hang_method);
            } else {
                std::env::remove_var("SPOTLIGHT_CODEX_STUB_HANG_METHOD");
            }
            if let Some(runtime_timeout_secs) = runtime_timeout_secs {
                std::env::set_var("SPOTLIGHT_RUNTIME_OP_TIMEOUT_SECS", runtime_timeout_secs);
            } else {
                std::env::remove_var("SPOTLIGHT_RUNTIME_OP_TIMEOUT_SECS");
            }

            Self {
                root,
                log_path,
                original_path,
                original_log_path,
                original_hang_method,
                original_runtime_timeout_secs,
            }
        }

        fn read_requests(&self) -> Vec<Value> {
            if !self.log_path.exists() {
                return Vec::new();
            }

            fs::read_to_string(&self.log_path)
                .unwrap_or_default()
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .collect()
        }
    }

    impl Drop for TestCodexStubEnvironment {
        fn drop(&mut self) {
            if let Some(path) = self.original_path.as_ref() {
                std::env::set_var("PATH", path);
            } else {
                std::env::remove_var("PATH");
            }

            if let Some(log_path) = self.original_log_path.as_ref() {
                std::env::set_var("SPOTLIGHT_CODEX_STUB_LOG", log_path);
            } else {
                std::env::remove_var("SPOTLIGHT_CODEX_STUB_LOG");
            }
            if let Some(hang_method) = self.original_hang_method.as_ref() {
                std::env::set_var("SPOTLIGHT_CODEX_STUB_HANG_METHOD", hang_method);
            } else {
                std::env::remove_var("SPOTLIGHT_CODEX_STUB_HANG_METHOD");
            }
            if let Some(timeout_secs) = self.original_runtime_timeout_secs.as_ref() {
                std::env::set_var("SPOTLIGHT_RUNTIME_OP_TIMEOUT_SECS", timeout_secs);
            } else {
                std::env::remove_var("SPOTLIGHT_RUNTIME_OP_TIMEOUT_SECS");
            }

            std::env::remove_var("SPOTLIGHT_PROVIDER");

            let _ = fs::remove_dir_all(&self.root);
        }
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
        ensure_git_ok(
            &root,
            &[
                "init",
                "--bare",
                "--initial-branch=main",
                remote.to_str().unwrap(),
            ],
        );
        ensure_git_ok(
            &root,
            &["clone", remote.to_str().unwrap(), local.to_str().unwrap()],
        );
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
        assert!(snapshot
            .tasks
            .first()
            .and_then(|task| task.state_snapshot.reason.as_deref())
            .is_some());
        assert!(!snapshot.agents.is_empty());
    }

    #[tokio::test]
    async fn board_snapshot_trims_large_runtime_logs_for_polling_clients() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("failed to resolve workspace root")
            .to_path_buf();
        let (app, state) = test_app_with_state(RuntimeMode::Stub, workspace_root).await;

        {
            let mut guard = state.inner.lock().await;
            let task = guard.tasks.first_mut().expect("expected task");
            task.activities = (0..40)
                .map(|index| TaskActivity {
                    kind: format!("task.test.{index}"),
                    message: "a".repeat(BOARD_MESSAGE_CHAR_LIMIT + 128),
                    at: format!("17100000{index:02}"),
                })
                .collect();
            task.runtime = Some(TaskRuntime {
                provider: "stub-codex".into(),
                thread_id: Some("stub-thread".into()),
                active_turn_id: Some("stub-turn".into()),
                git_auto_merge_enabled: false,
                log: (0..40)
                    .map(|index| RuntimeLogEntry {
                        kind: "assistant".into(),
                        message: "b".repeat(BOARD_MESSAGE_CHAR_LIMIT + 256),
                        at: format!("17110000{index:02}"),
                    })
                    .collect(),
                last_error: Some("c".repeat(BOARD_MESSAGE_CHAR_LIMIT + 64)),
            });
        }

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let snapshot = read_snapshot(response).await;
        let task = snapshot.tasks.first().expect("expected snapshot task");
        let runtime = task.runtime.as_ref().expect("expected runtime");

        assert_eq!(task.activities.len(), BOARD_TASK_ACTIVITY_LIMIT);
        assert_eq!(runtime.log.len(), BOARD_TASK_RUNTIME_LOG_LIMIT);
        assert!(task
            .activities
            .iter()
            .all(|item| item.message.contains("已截断")));
        assert!(runtime
            .log
            .iter()
            .all(|item| item.message.contains("已截断")));
        assert!(runtime
            .last_error
            .as_deref()
            .is_some_and(|message| message.contains("已截断")));
    }

    #[tokio::test]
    async fn index_route_serves_unified_entry_page() {
        let app = test_app();
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let html = read_text(response).await;

        assert!(content_type.starts_with("text/html"));
        assert!(html.contains("当前项目"));
        assert!(html.contains("任务看板"));
        assert!(html.contains("Agent 面板"));
        assert!(html.contains("const API_PREFIX = \"/api/v1\""));
    }

    #[tokio::test]
    async fn service_shell_exposes_0_1_0_core_surfaces() {
        let app = test_app();

        let index_response = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(index_response.status(), StatusCode::OK);

        let index_html = read_text(index_response).await;
        assert!(index_html.contains("Spotlight"));
        assert!(index_html.contains("const API_PREFIX = \"/api/v1\""));

        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(board_response.status(), StatusCode::OK);
        let board_snapshot = read_snapshot(board_response).await;
        assert!(!board_snapshot.projects.is_empty());
        assert!(!board_snapshot.agents.is_empty());

        let project_id = board_snapshot.projects[0].id;

        let projects_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(projects_response.status(), StatusCode::OK);
        let projects: Vec<Project> = read_json(projects_response).await;
        assert!(projects.iter().any(|project| project.id == project_id));

        let agents_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(agents_response.status(), StatusCode::OK);
        let agents: Vec<platform_core::Agent> = read_json(agents_response).await;
        assert!(!agents.is_empty());

        let summary_response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{project_id}/summary"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(summary_response.status(), StatusCode::OK);
        let summary: ProjectSummarySnapshot = read_json(summary_response).await;
        assert_eq!(summary.project_id, project_id);
        assert_eq!(summary.agent_summary.total, agents.len());
    }

    #[tokio::test]
    async fn versioned_projects_route_returns_visible_projects() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let projects: Vec<Project> = read_json(response).await;
        assert!(projects.len() >= 2);
        assert!(projects.iter().any(|project| project.is_spotlight_self));
    }

    #[tokio::test]
    async fn versioned_project_tasks_route_can_filter_by_status() {
        let app = test_app();
        let board_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/board")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let snapshot = read_snapshot(board_response).await;
        let project_id = snapshot.projects[0].id;
        let expected_open_count = snapshot
            .tasks
            .iter()
            .filter(|task| task.project_id == project_id && task.status == TaskStatus::Open)
            .count();

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{project_id}/tasks?status=OPEN"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let tasks: Vec<Task> = read_json(response).await;
        assert_eq!(tasks.len(), expected_open_count);
        assert!(tasks
            .iter()
            .all(|task| task.project_id == project_id && task.status == TaskStatus::Open));
    }

    #[tokio::test]
    async fn versioned_project_tasks_route_accepts_manual_review_status_filter() {
        let workspace_root = unique_temp_path("spotlight-task-status-filter");
        let (app, state) = test_app_with_state(RuntimeMode::Stub, workspace_root).await;

        let (project_id, manual_review_task_id) = {
            let mut guard = state.inner.lock().await;
            let task = guard.tasks.first_mut().expect("expected task in board");
            let project_id = task.project_id;
            task.status = TaskStatus::ManualReview;
            (project_id, task.id)
        };

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/projects/{project_id}/tasks?status=manual_review"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let tasks: Vec<Task> = read_json(response).await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, manual_review_task_id);
        assert_eq!(tasks[0].status, TaskStatus::ManualReview);
    }

    #[tokio::test]
    async fn versioned_agents_route_returns_agent_panel_snapshot() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let agents: Vec<platform_core::Agent> = read_json(response).await;
        assert!(!agents.is_empty());
        assert!(agents.iter().all(|agent| !agent.name.trim().is_empty()));
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
    async fn seed_doc_tasks_endpoint_populates_missing_doc_seed_tasks_without_duplicates() {
        let workspace_root = unique_temp_path("spotlight-seed-docs");
        let (app, state) = test_app_with_state(RuntimeMode::Stub, workspace_root.clone()).await;
        let project_id = {
            let mut guard = state.inner.lock().await;
            let project_id = guard
                .projects
                .iter()
                .find(|project| project.is_spotlight_self)
                .map(|project| project.id)
                .expect("expected spotlight self project");
            guard.tasks.clear();
            project_id
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/projects/{project_id}/tasks/seed-docs"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let seeded_snapshot = read_snapshot(response).await;
        let seeded_count = seeded_snapshot
            .tasks
            .iter()
            .filter(|task| task.project_id == project_id)
            .count();
        assert!(seeded_count > 0);
        assert!(seeded_snapshot
            .tasks
            .iter()
            .any(|task| task.project_id == project_id && task.title.starts_with("[0.1.0]")));

        let second_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/projects/{project_id}/tasks/seed-docs"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second_response.status(), StatusCode::OK);

        let second_snapshot = read_snapshot(second_response).await;
        let second_count = second_snapshot
            .tasks
            .iter()
            .filter(|task| task.project_id == project_id)
            .count();
        assert_eq!(second_count, seeded_count);

        let _ = fs::remove_dir_all(workspace_root);
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
            priority: None,
            labels: Vec::new(),
            requested_agent_id: None,
            approval_required: false,
            acceptance_owner_user_id: None,
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
    async fn created_task_records_creator_user_and_metadata() {
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
            priority: Some(TaskPriority::High),
            labels: vec!["backend".into(), "queue".into()],
            requested_agent_id: None,
            approval_required: false,
            acceptance_owner_user_id: None,
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
        assert_eq!(task.priority, Some(TaskPriority::High));
        assert_eq!(task.labels, vec!["backend", "queue"]);
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
        assert_eq!(
            updated_project.workspace_roots.first().unwrap().label,
            "backend"
        );
        assert!(updated_project
            .workspace_roots
            .first()
            .unwrap()
            .path
            .contains("spotlight-project-workspace"));
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
        fs::write(
            workspace_root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\n",
        )
        .unwrap();
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
        assert!(scan
            .key_files
            .iter()
            .any(|path| path.ends_with("Cargo.toml")));
    }

    #[tokio::test]
    async fn scan_endpoint_reports_empty_workspace_hint() {
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

        let workspace_root = unique_temp_path("spotlight-empty-scan-workspace");
        fs::create_dir_all(&workspace_root).unwrap();

        let register_payload = serde_json::json!({
            "label": "empty-root",
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
        assert!(scan.top_level_entries.is_empty());
        assert!(scan
            .notes
            .iter()
            .any(|note| note.contains("当前目录看起来几乎为空")));
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
    async fn project_chat_message_roundtrip_updates_project_context() {
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
                    .uri(format!("/api/projects/{project_id}/chat"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "message": "请把当前最小可用能力缺口梳理成队列"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let context: ProjectContextSnapshot = read_json(response).await;
        let message = context
            .chat_messages
            .last()
            .expect("expected persisted project chat message");
        assert_eq!(message.project_id, project_id);
        assert_eq!(message.content, "请把当前最小可用能力缺口梳理成队列");

        let context_response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/projects/{project_id}/context"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(context_response.status(), StatusCode::OK);

        let refreshed_context: ProjectContextSnapshot = read_json(context_response).await;
        assert!(refreshed_context
            .chat_messages
            .iter()
            .any(|item| item.content == "请把当前最小可用能力缺口梳理成队列"));
    }

    #[tokio::test]
    async fn project_constraint_memory_creates_revisions_and_updates_tag() {
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

        for content in [
            "保留移动端入口，统一入口不能做成桌面专属。",
            "继续保留移动端入口，同时允许桌面恢复最近焦点。",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/projects/{project_id}/memory/constraints"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "stableKey": "mobile-entry",
                                "title": "保留移动端入口",
                                "content": content
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let memory_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/projects/{project_id}/memory"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(memory_response.status(), StatusCode::OK);

        let memory: ProjectMemorySnapshot = read_json(memory_response).await;
        assert_eq!(memory.items.len(), 1);
        assert_eq!(memory.revisions.len(), 2);
        assert_eq!(memory.tags.len(), 1);
        assert_eq!(memory.edges.len(), 1);
        assert_eq!(memory.items[0].memory_kind, "project_constraint");
        assert_eq!(
            memory.items[0].stable_key,
            "project_constraint/mobile-entry"
        );
        assert_eq!(
            memory.tags[0].tag,
            format!("project/{project_id}/active-constraints")
        );

        let tagged_revision = memory
            .revisions
            .iter()
            .find(|revision| revision.id == memory.tags[0].target_revision_id)
            .expect("expected active revision");
        assert_eq!(
            tagged_revision.content,
            "继续保留移动端入口，同时允许桌面恢复最近焦点。"
        );

        let context_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/projects/{project_id}/context"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(context_response.status(), StatusCode::OK);
        let context: ProjectContextSnapshot = read_json(context_response).await;
        assert_eq!(context.memory.revisions.len(), 2);
    }

    #[tokio::test]
    async fn project_summary_endpoint_aggregates_memory_and_statuses() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("failed to resolve workspace root")
            .to_path_buf();
        let (app, state) = test_app_with_state(RuntimeMode::Stub, workspace_root).await;

        let (project_id, running_task_id, paused_task_id) = {
            let mut guard = state.inner.lock().await;
            let project_id = guard.projects.first().expect("expected project").id;
            while guard
                .tasks
                .iter()
                .filter(|task| task.project_id == project_id)
                .count()
                < 2
            {
                guard.tasks.push(Task {
                    id: Uuid::new_v4(),
                    project_id,
                    title: "测试任务".into(),
                    description: "用于项目摘要接口回归测试".into(),
                    status: TaskStatus::Open,
                    priority: Some(TaskPriority::Medium),
                    labels: vec!["summary-test".into()],
                    creator_user_id: None,
                    assignee_user_id: None,
                    assignment_mode: TaskAssignmentMode::PublicQueue,
                    requested_agent_id: None,
                    source_task_id: None,
                    claimed_by: None,
                    activities: Vec::new(),
                    runtime: None,
                    approval: Default::default(),
                    acceptance: Default::default(),
                    state_snapshot: TaskStateSnapshot::default(),
                });
            }
            let project_task_ids = guard
                .tasks
                .iter()
                .filter(|task| task.project_id == project_id)
                .map(|task| task.id)
                .take(2)
                .collect::<Vec<_>>();
            let running_task_id = *project_task_ids.first().expect("expected running task");
            let paused_task_id = *project_task_ids.get(1).expect("expected paused task");

            let running_task = guard
                .tasks
                .iter_mut()
                .find(|task| task.id == running_task_id)
                .expect("expected mutable running task");
            running_task.status = TaskStatus::Running;
            running_task.activities.push(TaskActivity {
                kind: "task.auto_started".into(),
                message: "自动认领后启动执行".into(),
                at: "1710000001".into(),
            });

            let paused_task = guard
                .tasks
                .iter_mut()
                .find(|task| task.id == paused_task_id)
                .expect("expected mutable paused task");
            paused_task.status = TaskStatus::Paused;

            let agent = guard.agents.first_mut().expect("expected agent");
            agent.auto_mode = true;
            agent.current_task_id = Some(running_task_id);

            guard.pending_questions.push(PendingQuestion {
                id: Uuid::new_v4(),
                project_id,
                source_task_id: running_task_id,
                source_task_title: "统一入口恢复联调".into(),
                question: "移动端是否也需要展示最近任务摘要？".into(),
                context: Some("当前只在统一入口页展示".into()),
                status: "open".into(),
                answer: None,
                created_at: "1710000002".into(),
                answered_at: None,
            });

            write_memory_revision(
                &mut guard,
                MemoryWriteSpec {
                    scope_kind: "project",
                    scope_id: project_id,
                    memory_kind: "project_constraint",
                    stable_key: "project_constraint/mobile-entry".into(),
                    tag: format!("project/{project_id}/active-constraints"),
                    title: "保留移动端入口".into(),
                    content: "统一入口和摘要接口不能只服务桌面端。".into(),
                    structured_payload: Some(serde_json::json!({
                        "kind": "project_constraint",
                        "title": "保留移动端入口"
                    })),
                    source_kind: "manual_constraint",
                    source_id: None,
                    confidence: Some(1.0),
                    created_by: None,
                },
            );

            write_memory_revision(
                &mut guard,
                MemoryWriteSpec {
                    scope_kind: "task",
                    scope_id: running_task_id,
                    memory_kind: "task_summary",
                    stable_key: format!("task_summary/{running_task_id}"),
                    tag: format!("task/{running_task_id}/latest-summary"),
                    title: "任务摘要：统一入口恢复联调".into(),
                    content: "已打通最近恢复位置，并保持移动端入口不被桌面端分叉。".into(),
                    structured_payload: Some(serde_json::json!({
                        "taskId": running_task_id,
                        "summary": "已打通最近恢复位置"
                    })),
                    source_kind: "task_completion_report",
                    source_id: Some(running_task_id.to_string()),
                    confidence: Some(0.9),
                    created_by: None,
                },
            );

            (project_id, running_task_id, paused_task_id)
        };

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{project_id}/summary"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let summary: ProjectSummarySnapshot = read_json(response).await;
        assert_eq!(summary.project_id, project_id);
        assert_eq!(summary.task_counts.running, 1);
        assert_eq!(summary.task_counts.paused, 1);
        assert_eq!(summary.agent_summary.total, 2);
        assert_eq!(summary.agent_summary.auto_mode_enabled, 1);
        assert_eq!(summary.agent_summary.busy, 1);
        assert_eq!(summary.agent_summary.idle, 1);
        assert_eq!(summary.open_pending_question_count, 1);
        assert_eq!(summary.pending_questions.len(), 1);
        assert_eq!(summary.active_constraints.len(), 1);
        assert_eq!(
            summary.active_constraints[0].stable_key,
            "project_constraint/mobile-entry"
        );
        assert_eq!(summary.recent_task_summaries.len(), 1);
        assert_eq!(summary.recent_task_summaries[0].task_id, running_task_id);
        assert!(summary.recent_task_summaries[0]
            .summary
            .contains("保持移动端入口"));
        assert_ne!(paused_task_id, running_task_id);
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

    #[test]
    fn auto_claim_next_task_picks_oldest_open_task_and_skips_done_items() {
        let workspace_root = unique_temp_path("spotlight-auto-claim");
        let users = default_users();
        let projects = default_projects(&workspace_root);
        let agents = default_agents(&users);
        let agent = agents.first().unwrap().clone();
        let project_id = projects.first().unwrap().id;

        let newest_open = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "较新的待处理任务".into(),
            description: "newer open".into(),
            status: TaskStatus::Open,
            priority: None,
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![TaskActivity {
                kind: "task.created".into(),
                message: "new".into(),
                at: "300".into(),
            }],
            runtime: None,
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };
        let oldest_open = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "较早的待处理任务".into(),
            description: "older open".into(),
            status: TaskStatus::Open,
            priority: None,
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![TaskActivity {
                kind: "task.created".into(),
                message: "old".into(),
                at: "200".into(),
            }],
            runtime: None,
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };
        let oldest_done = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "最早但已完成的任务".into(),
            description: "done".into(),
            status: TaskStatus::Done,
            priority: None,
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![TaskActivity {
                kind: "task.created".into(),
                message: "done".into(),
                at: "100".into(),
            }],
            runtime: None,
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };

        let mut state = BoardState {
            users,
            projects,
            tasks: vec![newest_open.clone(), oldest_open.clone(), oldest_done],
            agents,
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: Vec::new(),
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };

        let claimed = auto_claim_next_task(&mut state, agent.id)
            .unwrap()
            .expect("should auto claim oldest open task");

        assert_eq!(claimed.id, oldest_open.id);
        let updated = state
            .tasks
            .iter()
            .find(|task| task.id == oldest_open.id)
            .unwrap();
        assert_eq!(updated.status, TaskStatus::Claimed);
        assert_eq!(updated.claimed_by, Some(agent.id));
    }

    #[test]
    fn auto_claim_next_task_prefers_priority_queue_and_skips_other_users_assignments() {
        let workspace_root = unique_temp_path("spotlight-auto-claim-priority");
        let users = default_users();
        let projects = default_projects(&workspace_root);
        let agents = default_agents(&users);
        let agent = agents.first().unwrap().clone();
        let owner_user_id = agent.owner_user_id.expect("agent should have owner");
        let other_user_id = users
            .iter()
            .find(|user| user.id != owner_user_id)
            .map(|user| user.id)
            .expect("expected another user");
        let project_id = projects.first().unwrap().id;

        let older_unprioritized = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "older-open".into(),
            description: "older open".into(),
            status: TaskStatus::Open,
            priority: None,
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![TaskActivity {
                kind: "task.created".into(),
                message: "older".into(),
                at: "100".into(),
            }],
            runtime: None,
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };
        let owner_medium = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "owner-medium".into(),
            description: "owner medium".into(),
            status: TaskStatus::Open,
            priority: Some(TaskPriority::Medium),
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: Some(owner_user_id),
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![TaskActivity {
                kind: "task.created".into(),
                message: "owner medium".into(),
                at: "200".into(),
            }],
            runtime: None,
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };
        let shared_high = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "shared-high".into(),
            description: "shared high".into(),
            status: TaskStatus::Open,
            priority: Some(TaskPriority::High),
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![TaskActivity {
                kind: "task.created".into(),
                message: "shared high".into(),
                at: "300".into(),
            }],
            runtime: None,
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };
        let other_users_high = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "other-users-high".into(),
            description: "other users high".into(),
            status: TaskStatus::Open,
            priority: Some(TaskPriority::High),
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: Some(other_user_id),
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![TaskActivity {
                kind: "task.created".into(),
                message: "other users high".into(),
                at: "50".into(),
            }],
            runtime: None,
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };

        let mut state = BoardState {
            users,
            projects,
            tasks: vec![
                older_unprioritized,
                owner_medium,
                shared_high.clone(),
                other_users_high,
            ],
            agents,
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: Vec::new(),
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };

        let claimed = auto_claim_next_task(&mut state, agent.id)
            .unwrap()
            .expect("should auto claim the highest-priority eligible task");

        assert_eq!(claimed.id, shared_high.id);
        assert_eq!(claimed.status, TaskStatus::Claimed);
        assert!(
            claimed
                .activities
                .iter()
                .any(|item| item.kind == "task.auto_claim_reason"
                    && item.message.contains("高优先级"))
        );
    }

    #[test]
    fn select_next_auto_resume_task_prefers_high_priority_paused_task_for_same_owner() {
        let workspace_root = unique_temp_path("spotlight-auto-resume-priority");
        let users = default_users();
        let projects = default_projects(&workspace_root);
        let agents = default_agents(&users);
        let agent = agents.first().unwrap().clone();
        let owner_user_id = agent.owner_user_id.expect("agent should have owner");
        let project_id = projects.first().unwrap().id;

        let low_priority = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "paused-low".into(),
            description: "paused low".into(),
            status: TaskStatus::Paused,
            priority: Some(TaskPriority::Low),
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: Some(owner_user_id),
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![
                TaskActivity {
                    kind: "task.created".into(),
                    message: "low".into(),
                    at: "100".into(),
                },
                TaskActivity {
                    kind: "task.watchdog_recovered".into(),
                    message: "recovered".into(),
                    at: "101".into(),
                },
            ],
            runtime: Some(platform_core::TaskRuntime {
                provider: "codex".into(),
                thread_id: Some("thread-low".into()),
                active_turn_id: None,
                git_auto_merge_enabled: false,
                log: Vec::new(),
                last_error: None,
            }),
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };
        let high_priority = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "paused-high".into(),
            description: "paused high".into(),
            status: TaskStatus::Paused,
            priority: Some(TaskPriority::High),
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: Some(owner_user_id),
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![
                TaskActivity {
                    kind: "task.created".into(),
                    message: "high".into(),
                    at: "200".into(),
                },
                TaskActivity {
                    kind: "task.watchdog_recovered".into(),
                    message: "recovered".into(),
                    at: "201".into(),
                },
            ],
            runtime: Some(platform_core::TaskRuntime {
                provider: "codex".into(),
                thread_id: Some("thread-high".into()),
                active_turn_id: None,
                git_auto_merge_enabled: false,
                log: Vec::new(),
                last_error: None,
            }),
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };

        let selected = select_next_auto_resume_task_id(
            &[low_priority, high_priority.clone()],
            Some(owner_user_id),
        )
        .expect("expected paused task to be selected for auto resume");

        assert_eq!(selected, high_priority.id);
    }

    #[test]
    fn active_task_conflict_only_blocks_same_workspace() {
        let workspace_root = unique_temp_path("spotlight-workspace-serialization-lanes");
        let project_a = test_project("alpha", workspace_root.join("alpha"));
        let project_b = test_project("beta", workspace_root.join("beta"));

        let active_alpha = test_task(project_a.id, "alpha-running", TaskStatus::Running, None);
        let queued_alpha = test_task(project_a.id, "alpha-open", TaskStatus::Open, None);
        let queued_beta = test_task(project_b.id, "beta-open", TaskStatus::Open, None);
        let projects = vec![project_a, project_b];
        let tasks = vec![
            active_alpha.clone(),
            queued_alpha.clone(),
            queued_beta.clone(),
        ];

        let same_workspace_conflict =
            active_task_conflict(&projects, &tasks, queued_alpha.id, Some(queued_alpha.id));
        assert_eq!(
            same_workspace_conflict.map(|task| task.id),
            Some(active_alpha.id)
        );

        let other_workspace_conflict =
            active_task_conflict(&projects, &tasks, queued_beta.id, Some(queued_beta.id));
        assert!(other_workspace_conflict.is_none());
    }

    #[test]
    fn active_task_conflict_blocks_projects_sharing_same_workspace() {
        let workspace_root = unique_temp_path("spotlight-shared-workspace-lane");
        let shared_workspace = workspace_root.join("shared");
        let project_a = test_project("alpha", shared_workspace.clone());
        let project_b = test_project("beta", shared_workspace);

        let active_alpha = test_task(project_a.id, "alpha-running", TaskStatus::Running, None);
        let queued_beta = test_task(project_b.id, "beta-open", TaskStatus::Open, None);
        let projects = vec![project_a, project_b];
        let tasks = vec![active_alpha.clone(), queued_beta.clone()];

        let conflict =
            active_task_conflict(&projects, &tasks, queued_beta.id, Some(queued_beta.id));
        assert_eq!(conflict.map(|task| task.id), Some(active_alpha.id));
    }

    #[test]
    fn auto_claim_next_task_skips_busy_workspace_and_claims_other_workspace() {
        let workspace_root = unique_temp_path("spotlight-auto-claim-workspace-lanes");
        let users = default_users();
        let agents = default_agents(&users);
        let agent = agents.first().unwrap().clone();

        let project_a = test_project("alpha", workspace_root.join("alpha"));
        let project_b = test_project("beta", workspace_root.join("beta"));

        let mut active_alpha = test_task(
            project_a.id,
            "alpha-running",
            TaskStatus::Running,
            Some(TaskPriority::Low),
        );
        active_alpha.claimed_by = Some(Uuid::new_v4());

        let queued_alpha = test_task(
            project_a.id,
            "alpha-open",
            TaskStatus::Open,
            Some(TaskPriority::High),
        );
        let queued_beta = test_task(
            project_b.id,
            "beta-open",
            TaskStatus::Open,
            Some(TaskPriority::Medium),
        );

        let mut state = BoardState {
            users,
            projects: vec![project_a, project_b],
            tasks: vec![active_alpha, queued_alpha, queued_beta.clone()],
            agents,
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: Vec::new(),
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };
        if let Some(agent_state) = state.agents.iter_mut().find(|item| item.id == agent.id) {
            agent_state.auto_mode = true;
            agent_state.current_task_id = None;
        }

        let claimed = auto_claim_next_task(&mut state, agent.id)
            .unwrap()
            .expect("should claim a task from another workspace");

        assert_eq!(claimed.id, queued_beta.id);
        let queued_alpha_state = state
            .tasks
            .iter()
            .find(|task| task.title == "alpha-open")
            .expect("expected alpha open task");
        assert_eq!(queued_alpha_state.status, TaskStatus::Open);
    }

    #[test]
    fn reconcile_parallel_active_tasks_keeps_different_workspaces_running() {
        let workspace_root = unique_temp_path("spotlight-parallel-reconcile-lanes");
        let project_a = test_project("alpha", workspace_root.join("alpha"));
        let project_b = test_project("beta", workspace_root.join("beta"));

        let mut running_alpha = test_task(project_a.id, "alpha-running", TaskStatus::Running, None);
        running_alpha.claimed_by = Some(Uuid::new_v4());
        let mut running_beta = test_task(project_b.id, "beta-running", TaskStatus::Running, None);
        running_beta.claimed_by = Some(Uuid::new_v4());

        let users = default_users();
        let mut state = BoardState {
            users: users.clone(),
            projects: vec![project_a, project_b],
            tasks: vec![running_alpha.clone(), running_beta.clone()],
            agents: default_agents(&users),
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: Vec::new(),
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };

        let sessions_to_stop = reconcile_parallel_active_tasks(&mut state);

        assert!(sessions_to_stop.is_empty());
        assert_eq!(
            state
                .tasks
                .iter()
                .find(|task| task.id == running_alpha.id)
                .map(|task| task.status),
            Some(TaskStatus::Running)
        );
        assert_eq!(
            state
                .tasks
                .iter()
                .find(|task| task.id == running_beta.id)
                .map(|task| task.status),
            Some(TaskStatus::Running)
        );
    }

    #[test]
    fn reconcile_parallel_active_tasks_requeues_only_conflicting_workspace() {
        let workspace_root = unique_temp_path("spotlight-parallel-reconcile-same-workspace");
        let shared_workspace = workspace_root.join("shared");
        let isolated_workspace = workspace_root.join("isolated");
        let project_a = test_project("alpha", shared_workspace.clone());
        let project_b = test_project("beta", shared_workspace);
        let project_c = test_project("gamma", isolated_workspace);

        let mut running_alpha = test_task(project_a.id, "alpha-running", TaskStatus::Running, None);
        running_alpha.claimed_by = Some(Uuid::new_v4());

        let mut claimed_beta = test_task(project_b.id, "beta-claimed", TaskStatus::Claimed, None);
        claimed_beta.claimed_by = Some(Uuid::new_v4());

        let mut running_gamma = test_task(project_c.id, "gamma-running", TaskStatus::Running, None);
        running_gamma.claimed_by = Some(Uuid::new_v4());

        let users = default_users();
        let mut state = BoardState {
            users: users.clone(),
            projects: vec![project_a, project_b, project_c],
            tasks: vec![
                running_alpha.clone(),
                claimed_beta.clone(),
                running_gamma.clone(),
            ],
            agents: default_agents(&users),
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: Vec::new(),
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };

        let sessions_to_stop = reconcile_parallel_active_tasks(&mut state);

        assert_eq!(sessions_to_stop, vec![claimed_beta.id]);
        let requeued_beta = state
            .tasks
            .iter()
            .find(|task| task.id == claimed_beta.id)
            .expect("expected claimed beta task");
        assert_eq!(requeued_beta.status, TaskStatus::Open);
        assert!(requeued_beta.claimed_by.is_none());
        assert!(requeued_beta
            .activities
            .iter()
            .any(|activity| activity.kind == "task.parallel_requeued"));

        let kept_alpha = state
            .tasks
            .iter()
            .find(|task| task.id == running_alpha.id)
            .expect("expected alpha task");
        assert_eq!(kept_alpha.status, TaskStatus::Running);

        let untouched_gamma = state
            .tasks
            .iter()
            .find(|task| task.id == running_gamma.id)
            .expect("expected gamma task");
        assert_eq!(untouched_gamma.status, TaskStatus::Running);
    }

    #[test]
    fn select_next_auto_resume_task_skips_non_resumable_thread_not_found_tasks() {
        let workspace_root = unique_temp_path("spotlight-auto-resume-filter");
        let users = default_users();
        let projects = default_projects(&workspace_root);
        let agents = default_agents(&users);
        let owner_user_id = agents
            .first()
            .and_then(|agent| agent.owner_user_id)
            .expect("agent should have owner");
        let project_id = projects.first().unwrap().id;

        let non_resumable = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "paused-thread-missing".into(),
            description: "non resumable".into(),
            status: TaskStatus::Paused,
            priority: Some(TaskPriority::High),
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: Some(owner_user_id),
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![TaskActivity {
                kind: "task.watchdog_recovered".into(),
                message: "watchdog".into(),
                at: "1".into(),
            }],
            runtime: Some(platform_core::TaskRuntime {
                provider: "codex".into(),
                thread_id: Some("missing-thread".into()),
                active_turn_id: None,
                git_auto_merge_enabled: false,
                log: Vec::new(),
                last_error: Some("thread not found: missing-thread".into()),
            }),
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };

        assert_eq!(
            select_next_auto_resume_task_id(&[non_resumable], Some(owner_user_id)),
            None
        );
    }

    #[test]
    fn select_next_auto_resume_task_skips_non_resumable_rollout_not_found_tasks() {
        let workspace_root = unique_temp_path("spotlight-auto-resume-rollout-filter");
        let users = default_users();
        let projects = default_projects(&workspace_root);
        let agents = default_agents(&users);
        let owner_user_id = agents
            .first()
            .and_then(|agent| agent.owner_user_id)
            .expect("agent should have owner");
        let project_id = projects.first().unwrap().id;

        let non_resumable = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "paused-rollout-missing".into(),
            description: "non resumable".into(),
            status: TaskStatus::Paused,
            priority: Some(TaskPriority::High),
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: Some(owner_user_id),
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![TaskActivity {
                kind: "task.watchdog_recovered".into(),
                message: "watchdog".into(),
                at: "1".into(),
            }],
            runtime: Some(platform_core::TaskRuntime {
                provider: "codex".into(),
                thread_id: Some("missing-rollout-thread".into()),
                active_turn_id: None,
                git_auto_merge_enabled: false,
                log: Vec::new(),
                last_error: Some("no rollout found for thread id missing-rollout-thread".into()),
            }),
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };

        assert_eq!(
            select_next_auto_resume_task_id(&[non_resumable], Some(owner_user_id)),
            None
        );
    }

    #[test]
    fn select_next_auto_resume_task_accepts_runtime_session_lost_marker() {
        let workspace_root = unique_temp_path("spotlight-auto-resume-runtime-lost");
        let users = default_users();
        let projects = default_projects(&workspace_root);
        let agents = default_agents(&users);
        let owner_user_id = agents
            .first()
            .and_then(|agent| agent.owner_user_id)
            .expect("agent should have owner");
        let project_id = projects.first().unwrap().id;
        let task_id = Uuid::new_v4();

        let resumable = Task {
            id: task_id,
            project_id,
            title: "paused-runtime-lost".into(),
            description: "resumable".into(),
            status: TaskStatus::Paused,
            priority: Some(TaskPriority::High),
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: Some(owner_user_id),
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![TaskActivity {
                kind: "task.runtime_session_lost".into(),
                message: "runtime lost".into(),
                at: "1".into(),
            }],
            runtime: Some(platform_core::TaskRuntime {
                provider: "codex".into(),
                thread_id: Some("resumable-thread".into()),
                active_turn_id: None,
                git_auto_merge_enabled: false,
                log: Vec::new(),
                last_error: Some(
                    "本地运行会话已断开，任务已转为可恢复状态，等待自动恢复或人工继续".into(),
                ),
            }),
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };

        assert_eq!(
            select_next_auto_resume_task_id(&[resumable], Some(owner_user_id)),
            Some(task_id)
        );
    }

    #[tokio::test]
    async fn runtime_event_loop_reconciles_running_task_when_session_channel_closes() {
        let workspace_root = unique_temp_path("spotlight-runtime-session-lost");
        let state = default_state(RuntimeMode::Stub, workspace_root);
        let agent_id;
        let task_id;

        {
            let mut guard = state.inner.lock().await;
            agent_id = guard.agents[0].id;
            task_id = guard.tasks[0].id;
            mark_task_running(
                &mut guard,
                task_id,
                agent_id,
                "本地 Codex Agent",
                "codex",
                "继续执行当前任务",
                Some("thread-1".into()),
                Some("turn-1".into()),
                false,
            )
            .expect("expected task to enter running state");
        }

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        drop(event_tx);
        runtime_event_loop(state.clone(), task_id, agent_id, event_rx, "codex".into()).await;

        let guard = state.inner.lock().await;
        let task = guard
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .expect("expected task");
        let runtime = task.runtime.as_ref().expect("expected runtime");
        assert_eq!(task.status, TaskStatus::Paused);
        assert_eq!(task.claimed_by, None);
        assert_eq!(runtime.active_turn_id, None);
        assert_eq!(
            runtime.last_error.as_deref(),
            Some("本地运行会话已断开，任务已转为可恢复状态，等待自动恢复或人工继续")
        );
        assert!(task
            .activities
            .iter()
            .any(|item| item.kind == "task.runtime_session_lost"));
        assert!(guard
            .agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .is_some_and(|agent| agent.current_task_id.is_none()));
    }

    #[tokio::test]
    async fn runtime_event_loop_clears_active_turn_when_runtime_errors() {
        let workspace_root = unique_temp_path("spotlight-runtime-error");
        let state = default_state(RuntimeMode::Stub, workspace_root);
        let agent_id;
        let task_id;

        {
            let mut guard = state.inner.lock().await;
            agent_id = guard.agents[0].id;
            task_id = guard.tasks[0].id;
            mark_task_running(
                &mut guard,
                task_id,
                agent_id,
                "本地 Codex Agent",
                "codex",
                "继续执行当前任务",
                Some("thread-1".into()),
                Some("turn-1".into()),
                false,
            )
            .expect("expected task to enter running state");
        }

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        event_tx
            .send(RuntimeEvent::Error {
                message: "runtime boom".into(),
            })
            .expect("expected event to be delivered");
        drop(event_tx);
        runtime_event_loop(state.clone(), task_id, agent_id, event_rx, "codex".into()).await;

        let guard = state.inner.lock().await;
        let task = guard
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .expect("expected task");
        let runtime = task.runtime.as_ref().expect("expected runtime");
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(runtime.active_turn_id, None);
        assert_eq!(runtime.last_error.as_deref(), Some("runtime boom"));
    }

    #[test]
    fn reconcile_watchdog_state_recovers_stale_running_task_and_releases_agent() {
        let workspace_root = unique_temp_path("spotlight-watchdog");
        let users = default_users();
        let projects = default_projects(&workspace_root);
        let mut agents = default_agents(&users);
        let agent_id = agents.first().unwrap().id;
        let task_id = Uuid::new_v4();
        agents.first_mut().unwrap().current_task_id = Some(task_id);
        agents.first_mut().unwrap().status = "运行中".into();
        let project_id = projects.first().unwrap().id;

        let mut state = BoardState {
            users,
            projects,
            tasks: vec![Task {
                id: task_id,
                project_id,
                title: "stale-running".into(),
                description: "stale running".into(),
                status: TaskStatus::Running,
                priority: Some(TaskPriority::High),
                labels: Vec::new(),
                creator_user_id: None,
                assignee_user_id: None,
                assignment_mode: TaskAssignmentMode::PublicQueue,
                requested_agent_id: None,
                source_task_id: None,
                claimed_by: Some(agent_id),
                activities: vec![TaskActivity {
                    kind: "task.started".into(),
                    message: "running".into(),
                    at: "1".into(),
                }],
                runtime: Some(platform_core::TaskRuntime {
                    provider: "codex".into(),
                    thread_id: Some("thread-1".into()),
                    active_turn_id: Some("turn-1".into()),
                    git_auto_merge_enabled: false,
                    log: vec![RuntimeLogEntry {
                        kind: "assistant".into(),
                        message: "still running".into(),
                        at: "1".into(),
                    }],
                    last_error: None,
                }),
                approval: Default::default(),
                acceptance: Default::default(),
                state_snapshot: TaskStateSnapshot::default(),
            }],
            agents,
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: Vec::new(),
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };

        let sessions_to_stop = reconcile_watchdog_state(
            &mut state,
            &HashSet::from([task_id]),
            (TASK_STALE_TIMEOUT_SECS as u128 + 10) * 1_000_000_000,
        );

        assert_eq!(sessions_to_stop, vec![task_id]);
        let task = state.tasks.first().unwrap();
        assert_eq!(task.status, TaskStatus::Paused);
        assert_eq!(task.claimed_by, None);
        assert!(task
            .activities
            .iter()
            .any(|item| item.kind == "task.watchdog_recovered"));
        assert_eq!(
            task.runtime
                .as_ref()
                .and_then(|runtime| runtime.active_turn_id.as_deref()),
            None
        );
        let agent = state.agents.first().unwrap();
        assert_eq!(agent.current_task_id, None);
        assert_eq!(agent.status, "空闲");
    }

    #[test]
    fn normalize_persisted_state_backfills_priority_and_repairs_reverted_open_tasks() {
        let workspace_root = unique_temp_path("spotlight-normalize");
        let users = default_users();
        let projects = default_projects(&workspace_root);
        let mut agents = default_agents(&users);
        let task_id = Uuid::new_v4();
        agents.first_mut().unwrap().current_task_id = Some(task_id);
        agents.first_mut().unwrap().status = "运行中".into();

        let mut state = PersistedState {
            users,
            projects: projects.clone(),
            tasks: vec![Task {
                id: task_id,
                project_id: projects[1].id,
                title: "[0.1.2] 接通真实 Codex 长会话运行时".into(),
                description: "修复跨重启后的恢复状态".into(),
                status: TaskStatus::Open,
                priority: None,
                labels: Vec::new(),
                creator_user_id: None,
                assignee_user_id: None,
                assignment_mode: TaskAssignmentMode::PublicQueue,
                requested_agent_id: None,
                source_task_id: None,
                claimed_by: None,
                activities: vec![new_activity(
                    "runtime.thread_started",
                    "线程已启动，稍后因会话丢失而中断",
                )],
                runtime: Some(TaskRuntime {
                    provider: "codex".into(),
                    thread_id: Some("thread-1".into()),
                    active_turn_id: None,
                    git_auto_merge_enabled: false,
                    log: vec![new_runtime_entry("assistant", "已经产出过执行日志")],
                    last_error: None,
                }),
                approval: Default::default(),
                acceptance: Default::default(),
                state_snapshot: TaskStateSnapshot::default(),
            }],
            agents,
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: Vec::new(),
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };

        assert!(super::normalize_persisted_state(&mut state));

        let task = state.tasks.first().unwrap();
        assert_eq!(task.status, TaskStatus::Paused);
        assert_eq!(task.priority, Some(TaskPriority::High));
        assert_eq!(task.claimed_by, None);
        assert!(task.state_snapshot.reason.is_some());
        assert!(!task.state_snapshot.evidence.is_empty());
        assert_eq!(
            task.state_snapshot.last_evaluated_by.as_deref(),
            Some("server.load.normalize")
        );
        assert!(task
            .activities
            .iter()
            .any(|item| item.kind == "task.state_normalized"));
        assert!(task
            .activities
            .iter()
            .any(|item| item.kind == "task.priority_inferred"));
        assert_eq!(
            task.runtime
                .as_ref()
                .and_then(|runtime| runtime.last_error.as_deref()),
            Some("服务端启动时检测到任务已有运行痕迹却回退为 OPEN，已自动归一化为 PAUSED。")
        );

        let agent = state.agents.first().unwrap();
        assert_eq!(agent.current_task_id, None);
        assert_eq!(agent.status, "空闲");
        assert_eq!(agent.last_action, "服务启动时释放了失效的任务占用");
    }

    #[test]
    fn normalize_persisted_state_keeps_reassessed_open_tasks_open() {
        let workspace_root = unique_temp_path("spotlight-normalize-reopened");
        let users = default_users();
        let projects = default_projects(&workspace_root);

        let mut state = PersistedState {
            users: users.clone(),
            projects: projects.clone(),
            tasks: vec![Task {
                id: Uuid::new_v4(),
                project_id: projects[1].id,
                title: "[0.1.2.4] 真实 Codex 长会话接入".into(),
                description: "已经被状态复核重新开放，不应在重启时再次压回暂停".into(),
                status: TaskStatus::Open,
                priority: None,
                labels: Vec::new(),
                creator_user_id: None,
                assignee_user_id: None,
                assignment_mode: TaskAssignmentMode::PublicQueue,
                requested_agent_id: None,
                source_task_id: None,
                claimed_by: None,
                activities: vec![
                    new_activity("runtime.thread_started", "线程已启动，稍后失去上下文"),
                    new_activity(
                        "task.reassessed_reopened",
                        "系统重新评估后将任务重新开放，等待再次认领",
                    ),
                ],
                runtime: None,
                approval: Default::default(),
                acceptance: Default::default(),
                state_snapshot: TaskStateSnapshot::default(),
            }],
            agents: default_agents(&users),
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: Vec::new(),
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };

        assert!(super::normalize_persisted_state(&mut state));

        let task = state.tasks.first().unwrap();
        assert_eq!(task.status, TaskStatus::Open);
        assert!(task
            .activities
            .iter()
            .all(|item| item.kind != "task.state_normalized"));
    }

    #[test]
    fn normalize_persisted_state_marks_done_when_completion_report_exists() {
        let workspace_root = unique_temp_path("spotlight-normalize-done");
        let users = default_users();
        let projects = default_projects(&workspace_root);

        let mut state = PersistedState {
            users: users.clone(),
            projects: projects.clone(),
            tasks: vec![Task {
                id: Uuid::new_v4(),
                project_id: projects[1].id,
                title: "[0.1.0] 最小服务端".into(),
                description: "补状态归一化".into(),
                status: TaskStatus::Paused,
                priority: Some(TaskPriority::High),
                labels: Vec::new(),
                creator_user_id: None,
                assignee_user_id: None,
                assignment_mode: TaskAssignmentMode::PublicQueue,
                requested_agent_id: None,
                source_task_id: None,
                claimed_by: None,
                activities: vec![new_activity("task.runtime_session_lost", "runtime lost")],
                runtime: Some(TaskRuntime {
                    provider: "codex".into(),
                    thread_id: Some("thread-done".into()),
                    active_turn_id: None,
                    git_auto_merge_enabled: false,
                    log: vec![new_runtime_entry(
                        "assistant",
                        "```json\n{\n  \"result\": \"done\",\n  \"summary\": \"已经完成状态硬化\"\n}\n```",
                    )],
                    last_error: Some("thread not found: thread-done".into()),
                }),
                approval: Default::default(),
                acceptance: Default::default(),
                state_snapshot: TaskStateSnapshot::default(),
            }],
            agents: default_agents(&users),
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: Vec::new(),
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };

        assert!(super::normalize_persisted_state(&mut state));

        let task = state.tasks.first().unwrap();
        assert_eq!(task.status, TaskStatus::Done);
        assert_eq!(
            task.runtime
                .as_ref()
                .and_then(|runtime| runtime.last_error.as_deref()),
            None
        );
        assert_eq!(
            task.state_snapshot.reason.as_deref(),
            Some("任务已完成：已经完成状态硬化")
        );
        assert!(!task.state_snapshot.needs_attention);
    }

    #[test]
    fn normalize_persisted_state_clears_inactive_turn_ids() {
        let workspace_root = unique_temp_path("spotlight-normalize-turn");
        let users = default_users();
        let projects = default_projects(&workspace_root);

        let mut state = PersistedState {
            users,
            projects: projects.clone(),
            tasks: vec![Task {
                id: Uuid::new_v4(),
                project_id: projects[1].id,
                title: "[0.1.3] 抽取 Provider 运行时协议".into(),
                description: "清理残留 active_turn_id".into(),
                status: TaskStatus::Failed,
                priority: Some(TaskPriority::High),
                labels: Vec::new(),
                creator_user_id: None,
                assignee_user_id: None,
                assignment_mode: TaskAssignmentMode::PublicQueue,
                requested_agent_id: None,
                source_task_id: None,
                claimed_by: None,
                activities: vec![new_activity("runtime.error", "runtime failed")],
                runtime: Some(TaskRuntime {
                    provider: "codex".into(),
                    thread_id: Some("thread-failed".into()),
                    active_turn_id: Some("turn-stale".into()),
                    git_auto_merge_enabled: false,
                    log: Vec::new(),
                    last_error: Some("Reconnecting... 1/5".into()),
                }),
                approval: Default::default(),
                acceptance: Default::default(),
                state_snapshot: TaskStateSnapshot::default(),
            }],
            agents: default_agents(&default_users()),
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: Vec::new(),
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };

        assert!(super::normalize_persisted_state(&mut state));

        let task = state.tasks.first().unwrap();
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(
            task.runtime
                .as_ref()
                .and_then(|runtime| runtime.active_turn_id.as_deref()),
            None
        );
        assert!(task
            .activities
            .iter()
            .any(|item| item.kind == "task.state_normalized"));
    }

    #[test]
    fn normalize_persisted_state_restores_primary_auto_agent_when_all_agents_are_disabled() {
        let workspace_root = unique_temp_path("spotlight-normalize-auto-mode");
        let users = default_users();
        let projects = default_projects(&workspace_root);
        let mut agents = default_agents(&users);
        for agent in &mut agents {
            agent.auto_mode = false;
            agent.last_action = "手动关闭自动模式".into();
        }

        let mut state = PersistedState {
            users,
            projects: projects.clone(),
            tasks: vec![Task {
                id: Uuid::new_v4(),
                project_id: projects[1].id,
                title: "[0.1.2] 接通真实 Codex 长会话运行时".into(),
                description: "等待自动执行恢复主流程".into(),
                status: TaskStatus::Open,
                priority: Some(TaskPriority::High),
                labels: Vec::new(),
                creator_user_id: None,
                assignee_user_id: None,
                assignment_mode: TaskAssignmentMode::PublicQueue,
                requested_agent_id: None,
                source_task_id: None,
                claimed_by: None,
                activities: vec![new_activity("task.created", "等待自动执行")],
                runtime: None,
                approval: Default::default(),
                acceptance: Default::default(),
                state_snapshot: TaskStateSnapshot::default(),
            }],
            agents,
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: Vec::new(),
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };

        assert!(super::normalize_persisted_state(&mut state));
        assert!(state.agents.iter().any(|agent| agent.auto_mode));
        assert_eq!(state.agents[0].status, "空闲");
        assert_eq!(
            state.agents[0].last_action,
            "服务启动时检测到无人自动执行，已恢复主 Agent 的自动模式"
        );
    }

    #[test]
    fn parse_server_port_falls_back_to_default_when_env_is_invalid() {
        assert_eq!(super::parse_server_port(Some("3001")), 3001);
        assert_eq!(super::parse_server_port(Some("not-a-port")), 3000);
        assert_eq!(super::parse_server_port(None), 3000);
    }

    #[tokio::test]
    async fn automation_cycle_auto_claims_open_task_without_ui_polling() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("failed to resolve workspace root")
            .to_path_buf();
        let state = default_state(RuntimeMode::Stub, workspace_root);

        run_automation_cycle_once(&state)
            .await
            .expect("automation cycle should succeed");

        let guard = state.inner.lock().await;
        assert!(guard
            .agents
            .iter()
            .any(|agent| agent.auto_mode && agent.current_task_id.is_some()));
        assert!(guard
            .tasks
            .iter()
            .any(|task| matches!(task.status, TaskStatus::Running | TaskStatus::Claimed)));
    }

    #[tokio::test]
    async fn automation_cycle_auto_start_records_run_history() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("failed to resolve workspace root")
            .to_path_buf();
        let state = default_state(RuntimeMode::Stub, workspace_root);

        run_automation_cycle_once(&state)
            .await
            .expect("automation cycle should succeed");

        let guard = state.inner.lock().await;
        let auto_started_task = guard
            .tasks
            .iter()
            .find(|task| task.activities.iter().any(|item| item.kind == "task.auto_started"))
            .expect("expected one auto started task");
        let latest_run = guard
            .task_run_history
            .get(&auto_started_task.id)
            .and_then(|runs| runs.last())
            .expect("expected task run history for auto started task");

        assert_eq!(latest_run.provider, "stub-codex");
        assert_eq!(
            latest_run
                .attempts
                .last()
                .map(|attempt| attempt.trigger_kind.as_str()),
            Some("auto_start")
        );
    }

    #[tokio::test]
    async fn automation_cycle_auto_resume_records_run_history() {
        let workspace_root = unique_temp_path("spotlight-auto-resume-history");
        fs::create_dir_all(&workspace_root).unwrap();
        let state = default_state(RuntimeMode::Stub, workspace_root.clone());

        let resumable_task_id = {
            let mut guard = state.inner.lock().await;
            let project_id = guard
                .projects
                .iter()
                .find(|project| project.is_spotlight_self)
                .map(|project| project.id)
                .expect("expected spotlight project");
            let owner_user_id = {
                let agent = guard.agents.first_mut().expect("expected agent");
                let owner_user_id = agent.owner_user_id.expect("agent should have owner");
                agent.auto_mode = true;
                agent.current_task_id = None;
                owner_user_id
            };
            let resumable_task_id = Uuid::new_v4();
            guard.tasks = vec![Task {
                id: resumable_task_id,
                project_id,
                title: "可自动恢复任务".into(),
                description: "验证 auto_resume 会留下 run history".into(),
                status: TaskStatus::Paused,
                priority: Some(TaskPriority::High),
                labels: vec!["regression".into(), "auto-resume".into()],
                creator_user_id: None,
                assignee_user_id: Some(owner_user_id),
                assignment_mode: TaskAssignmentMode::PublicQueue,
                requested_agent_id: None,
                source_task_id: None,
                claimed_by: None,
                activities: vec![new_activity("task.watchdog_recovered", "watchdog recovered")],
                runtime: Some(TaskRuntime {
                    provider: "codex".into(),
                    thread_id: Some("resumable-thread".into()),
                    active_turn_id: None,
                    git_auto_merge_enabled: false,
                    log: Vec::new(),
                    last_error: None,
                }),
                approval: Default::default(),
                acceptance: Default::default(),
                state_snapshot: TaskStateSnapshot::default(),
            }];
            resumable_task_id
        };

        run_automation_cycle_once(&state)
            .await
            .expect("automation cycle should resume paused task");

        let guard = state.inner.lock().await;
        let resumed_task = guard
            .tasks
            .iter()
            .find(|task| task.id == resumable_task_id)
            .expect("expected resumable task");
        assert_eq!(resumed_task.status, TaskStatus::Done);

        let latest_run = guard
            .task_run_history
            .get(&resumable_task_id)
            .and_then(|runs| runs.last())
            .expect("expected task run history for resumed task");

        assert_eq!(latest_run.provider, "stub-codex");
        assert_eq!(latest_run.state, "completed");
        assert_eq!(
            latest_run
                .attempts
                .last()
                .map(|attempt| attempt.trigger_kind.as_str()),
            Some("auto_resume")
        );
    }

    #[tokio::test]
    async fn automation_cycle_skips_timed_out_auto_resume_and_moves_on_to_open_task() {
        let _env_lock = codex_stub_env_lock().lock().await;
        let stub_env =
            TestCodexStubEnvironment::install_with_options(Some("thread/resume"), Some("1"));
        let workspace_root = unique_temp_path("spotlight-auto-resume-timeout");
        fs::create_dir_all(&workspace_root).unwrap();
        let state = default_state(RuntimeMode::RealCodex, workspace_root);

        let (paused_task_id, open_task_id, agent_id) = {
            let mut guard = state.inner.lock().await;
            let project_id = guard
                .projects
                .iter()
                .find(|project| project.is_spotlight_self)
                .map(|project| project.id)
                .expect("expected spotlight project");
            let (agent_id, owner_user_id) = {
                let agent = guard.agents.first_mut().expect("expected agent");
                let owner_user_id = agent.owner_user_id.expect("agent should have owner");
                agent.auto_mode = true;
                agent.current_task_id = None;
                (agent.id, owner_user_id)
            };

            let paused_task_id = Uuid::new_v4();
            let open_task_id = Uuid::new_v4();
            guard.tasks = vec![
                Task {
                    id: paused_task_id,
                    project_id,
                    title: "坏恢复任务".into(),
                    description: "用于验证自动恢复超时不会堵住后续调度".into(),
                    status: TaskStatus::Paused,
                    priority: Some(TaskPriority::High),
                    labels: vec!["regression".into(), "auto-resume".into()],
                    creator_user_id: None,
                    assignee_user_id: Some(owner_user_id),
                    assignment_mode: TaskAssignmentMode::PublicQueue,
                    requested_agent_id: None,
                    source_task_id: None,
                    claimed_by: None,
                    activities: vec![new_activity("task.watchdog_recovered", "超时回收")],
                    runtime: Some(TaskRuntime {
                        provider: "codex".into(),
                        thread_id: Some("stuck-thread".into()),
                        active_turn_id: None,
                        git_auto_merge_enabled: false,
                        log: Vec::new(),
                        last_error: None,
                    }),
                    approval: Default::default(),
                    acceptance: Default::default(),
                    state_snapshot: TaskStateSnapshot::default(),
                },
                Task {
                    id: open_task_id,
                    project_id,
                    title: "后续开放任务".into(),
                    description: "用于验证自动循环会继续拉起新的开放任务".into(),
                    status: TaskStatus::Open,
                    priority: Some(TaskPriority::Medium),
                    labels: vec!["regression".into(), "auto-start".into()],
                    creator_user_id: None,
                    assignee_user_id: None,
                    assignment_mode: TaskAssignmentMode::PublicQueue,
                    requested_agent_id: None,
                    source_task_id: None,
                    claimed_by: None,
                    activities: vec![new_activity("task.created", "等待自动执行")],
                    runtime: None,
                    approval: Default::default(),
                    acceptance: Default::default(),
                    state_snapshot: TaskStateSnapshot::default(),
                },
            ];

            (paused_task_id, open_task_id, agent_id)
        };

        run_automation_cycle_once(&state)
            .await
            .expect("automation cycle should recover from timed out auto resume");

        {
            let guard = state.inner.lock().await;
            let paused_task = guard
                .tasks
                .iter()
                .find(|task| task.id == paused_task_id)
                .expect("expected paused task");
            assert_eq!(paused_task.status, TaskStatus::Paused);
            assert_eq!(paused_task.claimed_by, None);
            assert!(paused_task
                .activities
                .iter()
                .any(|entry| entry.kind == "task.auto_retry_queued"));
            assert!(paused_task.runtime.as_ref().is_some_and(|runtime| {
                runtime.active_turn_id.is_none() && runtime.last_error.is_some()
            }));
            assert!(guard
                .agents
                .iter()
                .find(|agent| agent.id == agent_id)
                .is_some_and(|agent| agent.current_task_id.is_none()));
        }
        assert!(state.runtime_sessions.lock().await.is_empty());

        std::env::remove_var("SPOTLIGHT_CODEX_STUB_HANG_METHOD");
        run_automation_cycle_once(&state)
            .await
            .expect("next automation cycle should move on to open work");
        sleep(Duration::from_millis(80)).await;

        let guard = state.inner.lock().await;
        let open_task = guard
            .tasks
            .iter()
            .find(|task| task.id == open_task_id)
            .expect("expected open task");
        assert_ne!(open_task.status, TaskStatus::Open);
        assert!(open_task
            .activities
            .iter()
            .any(|entry| entry.kind == "task.auto_started"));
        assert_eq!(
            guard
                .task_run_history
                .get(&open_task_id)
                .and_then(|runs| runs.last())
                .and_then(|run| run.attempts.last())
                .map(|attempt| attempt.trigger_kind.as_str()),
            Some("auto_start")
        );
        drop(guard);

        let requests = stub_env.read_requests();
        assert!(requests.iter().any(|request| {
            request.get("method").and_then(Value::as_str) == Some("thread/resume")
                && request
                    .get("params")
                    .and_then(|params| params.get("threadId"))
                    .and_then(Value::as_str)
                    == Some("stuck-thread")
        }));
        assert!(requests
            .iter()
            .any(|request| request.get("method").and_then(Value::as_str) == Some("thread/start")));
        assert!(requests
            .iter()
            .any(|request| request.get("method").and_then(Value::as_str) == Some("turn/start")));
    }

    #[tokio::test]
    async fn pull_next_route_returns_a_claimed_task() {
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

        for title in ["第一条待处理任务", "第二条待处理任务"] {
            let payload = serde_json::to_vec(&CreateTaskRequest {
                project_id: Some(project_id),
                title: title.into(),
                description: format!("为 {title} 生成测试数据"),
                priority: None,
                labels: Vec::new(),
                requested_agent_id: None,
                approval_required: false,
                acceptance_owner_user_id: None,
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
        }

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/agents/{}/pull-next", agent.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let allocation: PullNextResponse = read_json(response).await;
        let task = allocation.task.expect("should allocate one pending task");
        assert_eq!(task.status, TaskStatus::Claimed);
        assert_eq!(task.claimed_by, Some(agent.id));
    }

    #[tokio::test]
    async fn pull_next_route_marks_agent_busy_for_claimed_task() {
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
            title: "验证自动认领同步 Agent 占用".into(),
            description: "pull-next 之后 Agent 应立即绑定当前任务".into(),
            priority: Some(TaskPriority::High),
            labels: vec!["claim".into()],
            requested_agent_id: None,
            approval_required: false,
            acceptance_owner_user_id: None,
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
        let _created_task = read_task(create_response).await;

        let pull_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/agents/{}/pull-next", agent.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pull_response.status(), StatusCode::OK);

        let allocation: PullNextResponse = read_json(pull_response).await;
        let claimed_task = allocation.task.expect("should auto claim one task");
        assert_eq!(claimed_task.status, TaskStatus::Claimed);

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
        let board_snapshot = read_snapshot(board_response).await;
        let updated_agent = board_snapshot
            .agents
            .iter()
            .find(|candidate| candidate.id == agent.id)
            .unwrap();
        assert_eq!(updated_agent.current_task_id, Some(claimed_task.id));
        assert_eq!(updated_agent.status, "CLAIMED");

        let summary_response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/projects/{}/summary",
                        claimed_task.project_id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(summary_response.status(), StatusCode::OK);
        let summary: ProjectSummarySnapshot = read_json(summary_response).await;
        assert_eq!(summary.task_counts.claimed, 1);
        assert_eq!(summary.agent_summary.busy, 1);
    }

    #[tokio::test]
    async fn claim_route_marks_agent_busy_before_start() {
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
            title: "验证手动认领同步 Agent 占用".into(),
            description: "手动 claim 后不需要 start 也应反映为已绑定".into(),
            priority: Some(TaskPriority::Medium),
            labels: vec!["claim".into()],
            requested_agent_id: None,
            approval_required: false,
            acceptance_owner_user_id: None,
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
        let created_task = read_task(create_response).await;

        let claim_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tasks/{}/claim/{}", created_task.id, agent.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(claim_response.status(), StatusCode::OK);
        let claim_snapshot = read_snapshot(claim_response).await;

        let updated_task = claim_snapshot
            .tasks
            .iter()
            .find(|task| task.id == created_task.id)
            .unwrap();
        assert_eq!(updated_task.status, TaskStatus::Claimed);
        assert_eq!(updated_task.claimed_by, Some(agent.id));

        let updated_agent = claim_snapshot
            .agents
            .iter()
            .find(|candidate| candidate.id == agent.id)
            .unwrap();
        assert_eq!(updated_agent.current_task_id, Some(created_task.id));
        assert_eq!(updated_agent.status, "CLAIMED");
    }

    #[tokio::test]
    async fn claim_route_allows_approved_tasks() {
        let workspace_root = unique_temp_path("spotlight-claim-approved");
        fs::create_dir_all(&workspace_root).unwrap();
        let (app, state) = test_app_with_state(RuntimeMode::Stub, workspace_root).await;

        let (project_id, agent) = {
            let guard = state.inner.lock().await;
            (
                guard.projects.first().unwrap().id,
                guard.agents.first().unwrap().clone(),
            )
        };
        let task_id = Uuid::new_v4();
        {
            let mut guard = state.inner.lock().await;
            guard.tasks.insert(
                0,
                Task {
                    id: task_id,
                    project_id,
                    title: "审批通过后允许认领".into(),
                    description: "验证 APPROVED 任务可被手动认领".into(),
                    status: TaskStatus::Approved,
                    priority: Some(TaskPriority::Medium),
                    labels: vec!["claim".into()],
                    creator_user_id: None,
                    assignee_user_id: None,
                    assignment_mode: TaskAssignmentMode::PublicQueue,
                    requested_agent_id: None,
                    source_task_id: None,
                    claimed_by: None,
                    activities: vec![new_activity("task.approved", "任务已通过审批，等待认领")],
                    runtime: None,
                    approval: Default::default(),
                    acceptance: Default::default(),
                    state_snapshot: TaskStateSnapshot::default(),
                },
            );
        }

        let claim_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tasks/{task_id}/claim/{}", agent.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(claim_response.status(), StatusCode::OK);

        let claim_snapshot = read_snapshot(claim_response).await;
        let updated_task = claim_snapshot
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .unwrap();
        assert_eq!(updated_task.status, TaskStatus::Claimed);
        assert_eq!(updated_task.claimed_by, Some(agent.id));
    }

    #[tokio::test]
    async fn claim_route_rejects_non_claimable_statuses() {
        let cases = [
            ("approval", TaskStatus::ApprovalRequested),
            ("paused", TaskStatus::Paused),
            ("acceptance", TaskStatus::PendingAcceptance),
            ("manual-review", TaskStatus::ManualReview),
            ("accepted", TaskStatus::Accepted),
            ("failed", TaskStatus::Failed),
        ];

        for (suffix, status) in cases {
            let workspace_root = unique_temp_path(&format!("spotlight-claim-blocked-{suffix}"));
            fs::create_dir_all(&workspace_root).unwrap();
            let (app, state) = test_app_with_state(RuntimeMode::Stub, workspace_root).await;

            let (project_id, agent) = {
                let guard = state.inner.lock().await;
                (
                    guard.projects.first().unwrap().id,
                    guard.agents.first().unwrap().clone(),
                )
            };
            let task_id = Uuid::new_v4();
            {
                let mut guard = state.inner.lock().await;
                guard.tasks.insert(
                    0,
                    Task {
                        id: task_id,
                        project_id,
                        title: format!("不可认领状态-{suffix}"),
                        description: "验证手动认领只允许 OPEN 和 APPROVED".into(),
                        status,
                        priority: Some(TaskPriority::Medium),
                        labels: vec!["claim".into()],
                        creator_user_id: None,
                        assignee_user_id: None,
                        assignment_mode: TaskAssignmentMode::PublicQueue,
                        requested_agent_id: None,
                        source_task_id: None,
                        claimed_by: None,
                        activities: vec![new_activity("task.seeded", "测试任务")],
                        runtime: None,
                        approval: Default::default(),
                        acceptance: Default::default(),
                        state_snapshot: TaskStateSnapshot::default(),
                    },
                );
            }

            let claim_response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/tasks/{task_id}/claim/{}", agent.id))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                claim_response.status(),
                StatusCode::CONFLICT,
                "status {status:?} should not be claimable"
            );
        }
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
            priority: None,
            labels: Vec::new(),
            requested_agent_id: None,
            approval_required: false,
            acceptance_owner_user_id: None,
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
        assert!(updated_task
            .activities
            .iter()
            .any(|entry| entry.kind == "task.started"));
        assert!(updated_task
            .activities
            .iter()
            .any(|entry| entry.kind == "task.paused"));
        assert!(updated_task
            .activities
            .iter()
            .any(|entry| entry.kind == "task.resumed"));
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
        let task_runs = final_snapshot
            .task_run_history
            .get(&created_task.id)
            .expect("expected task run history");
        assert_eq!(task_runs.len(), 1);
        assert_eq!(task_runs[0].state, "completed");
        assert_eq!(task_runs[0].attempts.len(), 2);
        assert_eq!(task_runs[0].attempts[0].trigger_kind, "start");
        assert_eq!(task_runs[0].attempts[1].trigger_kind, "resume");
    }

    #[tokio::test]
    async fn persisted_state_reloads_task_run_history_from_state_file() {
        let workspace_root = unique_temp_path("spotlight-task-run-persistence");
        fs::create_dir_all(&workspace_root).unwrap();
        let (app, state) = test_app_with_state(RuntimeMode::Stub, workspace_root.clone()).await;

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
            title: "验证任务运行历史持久化".into(),
            description:
                "启动、暂停、恢复后重新读取 server-state.json，确认 task_run_history 仍存在".into(),
            priority: None,
            labels: Vec::new(),
            requested_agent_id: None,
            approval_required: false,
            acceptance_owner_user_id: None,
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
        let created_task = read_task(create_response).await;

        let start_payload = serde_json::to_vec(&AgentInvocationRequest {
            agent_name_hint: agent.name.clone(),
            prompt: Some("请先开始执行，再等待恢复。".into()),
        })
        .unwrap();
        let _ = app
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

        let _ = app
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

        let resume_payload = serde_json::to_vec(&AgentResumeRequest {
            agent_name_hint: agent.name.clone(),
            prompt: "我补充了恢复提示，请继续完成。".into(),
        })
        .unwrap();
        let _ = app
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

        let persisted = super::load_or_initialize_state(&workspace_root, &state.store_path);
        let task_runs = persisted
            .task_run_history
            .get(&created_task.id)
            .expect("expected persisted task run history");
        assert_eq!(task_runs.len(), 1);
        assert_eq!(task_runs[0].state, "completed");
        assert_eq!(task_runs[0].attempts.len(), 2);
        assert_eq!(task_runs[0].attempts[0].trigger_kind, "start");
        assert_eq!(task_runs[0].attempts[1].trigger_kind, "resume");
    }

    #[tokio::test]
    async fn pause_task_rejects_missing_runtime_without_mutating_task_state() {
        let workspace_root = unique_temp_path("spotlight-pause-guard");
        fs::create_dir_all(&workspace_root).unwrap();
        let (app, state) = test_app_with_state(RuntimeMode::RealCodex, workspace_root).await;

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

        let create_payload = serde_json::to_vec(&CreateTaskRequest {
            project_id: Some(project_id),
            title: "验证暂停失败不污染状态".into(),
            description: "未启动时直接暂停，状态不应被改成 PAUSED。".into(),
            priority: None,
            labels: Vec::new(),
            requested_agent_id: None,
            approval_required: false,
            acceptance_owner_user_id: None,
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
        let created_task = read_task(create_response).await;

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
        assert_eq!(pause_response.status(), StatusCode::CONFLICT);

        let guard = state.inner.lock().await;
        let task = guard
            .tasks
            .iter()
            .find(|candidate| candidate.id == created_task.id)
            .expect("expected task to remain");
        assert_eq!(task.status, TaskStatus::Open);
        assert!(task
            .activities
            .iter()
            .all(|entry| entry.kind != "task.paused"));
        assert!(task.runtime.is_none());
    }

    #[tokio::test]
    async fn resume_task_rejects_missing_runtime_without_mutating_task_state() {
        let workspace_root = unique_temp_path("spotlight-resume-guard");
        fs::create_dir_all(&workspace_root).unwrap();
        let (app, state) = test_app_with_state(RuntimeMode::RealCodex, workspace_root).await;

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
        let agent = snapshot
            .agents
            .first()
            .cloned()
            .expect("expected default agent");

        let create_payload = serde_json::to_vec(&CreateTaskRequest {
            project_id: Some(project_id),
            title: "验证恢复失败不污染状态".into(),
            description: "缺少运行时上下文时恢复请求不应把任务改成 RUNNING。".into(),
            priority: None,
            labels: Vec::new(),
            requested_agent_id: None,
            approval_required: false,
            acceptance_owner_user_id: None,
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
        let created_task = read_task(create_response).await;

        {
            let mut guard = state.inner.lock().await;
            let task = guard
                .tasks
                .iter_mut()
                .find(|candidate| candidate.id == created_task.id)
                .expect("expected task to exist");
            task.status = TaskStatus::Paused;
        }

        let resume_payload = serde_json::to_vec(&AgentResumeRequest {
            agent_name_hint: agent.name.clone(),
            prompt: "请继续执行".into(),
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
        assert_eq!(resume_response.status(), StatusCode::CONFLICT);

        let guard = state.inner.lock().await;
        let task = guard
            .tasks
            .iter()
            .find(|candidate| candidate.id == created_task.id)
            .expect("expected task to remain");
        assert_eq!(task.status, TaskStatus::Paused);
        assert_eq!(task.claimed_by, None);
        assert!(task
            .activities
            .iter()
            .all(|entry| entry.kind != "task.resumed"));
        assert!(task.runtime.is_none());
    }

    #[tokio::test]
    async fn real_codex_pause_task_interrupts_active_turn() {
        let _env_lock = codex_stub_env_lock().lock().await;
        let stub_env = TestCodexStubEnvironment::install();
        let workspace_root = unique_temp_path("spotlight-real-codex-pause-task");
        fs::create_dir_all(&workspace_root).unwrap();
        let (app, state) = test_app_with_state(RuntimeMode::RealCodex, workspace_root).await;

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
        let project_id = snapshot
            .projects
            .iter()
            .find(|project| project.is_spotlight_self)
            .map(|project| project.id)
            .expect("expected spotlight project");
        let agent = snapshot
            .agents
            .first()
            .cloned()
            .expect("expected default agent");

        let create_payload = serde_json::to_vec(&CreateTaskRequest {
            project_id: Some(project_id),
            title: "验证真实 Codex 暂停".into(),
            description: "启动真实 Codex 会话后暂停，确认会发送 turn/interrupt。".into(),
            priority: None,
            labels: Vec::new(),
            requested_agent_id: None,
            approval_required: false,
            acceptance_owner_user_id: None,
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
            prompt: Some("请先给出一个最小计划，然后保持线程等待后续指令。".into()),
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
        sleep(Duration::from_millis(80)).await;

        let guard = state.inner.lock().await;
        let updated_task = guard
            .tasks
            .iter()
            .find(|candidate| candidate.id == created_task.id)
            .expect("expected updated task");
        assert_eq!(updated_task.status, TaskStatus::Paused);
        assert!(updated_task
            .activities
            .iter()
            .any(|entry| entry.kind == "task.started"));
        assert!(updated_task
            .activities
            .iter()
            .any(|entry| entry.kind == "task.paused"));
        let runtime = updated_task.runtime.as_ref().expect("expected runtime");
        assert_eq!(runtime.thread_id.as_deref(), Some("stub-thread-1"));
        assert!(runtime.active_turn_id.is_none());
        drop(guard);

        let requests = stub_env.read_requests();
        assert!(requests.iter().any(|request| {
            request.get("method").and_then(Value::as_str) == Some("turn/interrupt")
                && request
                    .get("params")
                    .and_then(|params| params.get("threadId"))
                    .and_then(Value::as_str)
                    == Some("stub-thread-1")
                && request
                    .get("params")
                    .and_then(|params| params.get("turnId"))
                    .and_then(Value::as_str)
                    .is_some_and(|turn_id| turn_id.starts_with("stub-turn-"))
        }));
    }

    #[tokio::test]
    async fn real_codex_runtime_can_resume_task_with_same_thread_after_session_loss() {
        let _env_lock = codex_stub_env_lock().lock().await;
        let stub_env = TestCodexStubEnvironment::install();
        let workspace_root = unique_temp_path("spotlight-real-codex-task");
        fs::create_dir_all(&workspace_root).unwrap();
        let (app, state) = test_app_with_state(RuntimeMode::RealCodex, workspace_root).await;

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
        let task = snapshot
            .tasks
            .iter()
            .find(|task| task.title.contains("[0.1.2] 接通真实 Codex 长会话运行时"))
            .cloned()
            .expect("expected seeded runtime task");
        let agent = snapshot.agents.first().cloned().expect("expected agent");

        let claim_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tasks/{}/claim/{}", task.id, agent.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(claim_response.status(), StatusCode::OK);

        let start_payload = serde_json::to_vec(&AgentInvocationRequest {
            agent_name_hint: agent.name.clone(),
            prompt: Some("先输出最小实现方案，再保持线程上下文。".into()),
        })
        .unwrap();
        let start_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tasks/{}/start/{}", task.id, agent.id))
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
                    .uri(format!("/api/tasks/{}/pause", task.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pause_response.status(), StatusCode::OK);
        sleep(Duration::from_millis(80)).await;

        state.runtime_sessions.lock().await.remove(&task.id);

        let resume_payload = serde_json::to_vec(&AgentResumeRequest {
            agent_name_hint: agent.name.clone(),
            prompt: "我补充了一段提示词，请在原线程里继续执行。".into(),
        })
        .unwrap();
        let resume_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tasks/{}/resume/{}", task.id, agent.id))
                    .header("content-type", "application/json")
                    .body(Body::from(resume_payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resume_response.status(), StatusCode::OK);
        sleep(Duration::from_millis(80)).await;

        let guard = state.inner.lock().await;
        let updated_task = guard
            .tasks
            .iter()
            .find(|candidate| candidate.id == task.id)
            .expect("expected updated task");
        let runtime = updated_task.runtime.as_ref().expect("expected runtime");
        assert_eq!(updated_task.status, TaskStatus::Running);
        assert!(updated_task
            .activities
            .iter()
            .any(|entry| entry.kind == "task.resumed"));
        assert_eq!(runtime.thread_id.as_deref(), Some("stub-thread-1"));
        assert!(runtime
            .active_turn_id
            .as_deref()
            .is_some_and(|turn_id| turn_id.starts_with("stub-turn-")));
        assert!(runtime
            .log
            .iter()
            .any(|entry| entry.message.contains("原线程里继续执行")));
        drop(guard);

        let requests = stub_env.read_requests();
        assert!(requests.iter().any(|request| {
            request.get("method").and_then(Value::as_str) == Some("thread/resume")
                && request
                    .get("params")
                    .and_then(|params| params.get("threadId"))
                    .and_then(Value::as_str)
                    == Some("stub-thread-1")
        }));
    }

    #[tokio::test]
    async fn real_codex_project_session_can_continue_after_runtime_session_loss() {
        let _env_lock = codex_stub_env_lock().lock().await;
        let stub_env = TestCodexStubEnvironment::install();
        let workspace_root = unique_temp_path("spotlight-real-codex-project-session");
        fs::create_dir_all(&workspace_root).unwrap();
        let (app, state) = test_app_with_state(RuntimeMode::RealCodex, workspace_root).await;

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
        let project = snapshot
            .projects
            .iter()
            .find(|project| project.is_spotlight_self)
            .cloned()
            .expect("expected spotlight project");

        let start_payload = serde_json::json!({
            "title": "Codex 长会话测试",
            "prompt": "先帮我概括这个项目的当前目标。"
        })
        .to_string();
        let start_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/projects/{}/sessions", project.id))
                    .header("content-type", "application/json")
                    .body(Body::from(start_payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::OK);
        let context: ProjectContextSnapshot = read_json(start_response).await;
        let session = context
            .sessions
            .iter()
            .find(|candidate| candidate.title == "Codex 长会话测试")
            .cloned()
            .expect("expected project session");

        {
            let mut guard = state.inner.lock().await;
            let project_session = guard
                .project_sessions
                .iter_mut()
                .find(|candidate| candidate.id == session.id)
                .expect("expected mutable project session");
            project_session.active_turn_id = None;
            project_session.status = "completed".into();
        }
        state.runtime_sessions.lock().await.remove(&session.id);

        let continue_payload = serde_json::json!({
            "prompt": "我补充一点：请沿用原来的上下文继续分析。"
        })
        .to_string();
        let continue_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/project-sessions/{}/turns", session.id))
                    .header("content-type", "application/json")
                    .body(Body::from(continue_payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(continue_response.status(), StatusCode::OK);
        sleep(Duration::from_millis(80)).await;

        let guard = state.inner.lock().await;
        let updated_session = guard
            .project_sessions
            .iter()
            .find(|candidate| candidate.id == session.id)
            .expect("expected updated session");
        assert_eq!(updated_session.thread_id.as_deref(), Some("stub-thread-1"));
        assert!(updated_session
            .active_turn_id
            .as_deref()
            .is_some_and(|turn_id| turn_id.starts_with("stub-turn-")));
        assert_eq!(updated_session.status, "running");
        drop(guard);

        let requests = stub_env.read_requests();
        assert!(requests.iter().any(|request| {
            request.get("method").and_then(Value::as_str) == Some("thread/resume")
                && request
                    .get("params")
                    .and_then(|params| params.get("threadId"))
                    .and_then(Value::as_str)
                    == Some("stub-thread-1")
        }));
    }

    #[tokio::test]
    async fn cancel_task_marks_task_as_canceled_and_blocks_claim_start_resume() {
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
            title: "取消后不应再被执行".into(),
            description: "验证撤销态会阻止后续认领和启动".into(),
            priority: Some(TaskPriority::Medium),
            labels: vec!["cancel".into()],
            requested_agent_id: None,
            approval_required: false,
            acceptance_owner_user_id: None,
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
        let created_task = read_task(create_response).await;

        let cancel_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tasks/{}/cancel", created_task.id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "reason": "该需求改为进入等待队列，当前先不做"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cancel_response.status(), StatusCode::OK);

        let canceled_snapshot = read_snapshot(cancel_response).await;
        let canceled_task = canceled_snapshot
            .tasks
            .iter()
            .find(|task| task.id == created_task.id)
            .unwrap();
        assert_eq!(canceled_task.status, TaskStatus::Canceled);
        assert_eq!(canceled_task.claimed_by, None);
        assert!(canceled_task
            .activities
            .iter()
            .any(|item| item.kind == "task.canceled"));

        let claim_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tasks/{}/claim/{}", created_task.id, agent.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(claim_response.status(), StatusCode::CONFLICT);

        let start_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tasks/{}/start/{}", created_task.id, agent.id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&AgentInvocationRequest {
                            agent_name_hint: agent.name.clone(),
                            prompt: Some("这条已撤销任务不应继续执行".into()),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::CONFLICT);

        let resume_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/tasks/{}/resume/{}",
                        created_task.id, agent.id
                    ))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&AgentResumeRequest {
                            agent_name_hint: agent.name.clone(),
                            prompt: "这条已撤销任务不应恢复".into(),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resume_response.status(), StatusCode::CONFLICT);
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

        let merged_branches = ensure_git_ok(&workspace_root, &["branch", "--merged", "main"]);
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

    #[test]
    fn compose_task_prompt_includes_recent_context_and_scope_signals() {
        let project_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let project = Project {
            id: project_id,
            name: "Spotlight".into(),
            description: "自举项目".into(),
            workspace_roots: vec![WorkspaceRoot {
                id: workspace_id,
                label: "主目录".into(),
                path: "C:/Users/zuoho/code/spotlight".into(),
                writable: true,
            }],
            is_spotlight_self: true,
        };
        let task = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "整理桌面端布局".into(),
            description: "根据最近协作决策收口界面".into(),
            status: TaskStatus::Claimed,
            priority: Some(TaskPriority::High),
            labels: vec!["desktop".into()],
            creator_user_id: None,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![
                TaskActivity {
                    kind: "task.created".into(),
                    message: "创建桌面端布局收口任务".into(),
                    at: "1".into(),
                },
                TaskActivity {
                    kind: "task.updated".into(),
                    message: "左下角聊天框先不做，改为只保留项目聊天室".into(),
                    at: "2".into(),
                },
            ],
            runtime: Some(platform_core::TaskRuntime {
                provider: "codex".into(),
                thread_id: Some("thread-1".into()),
                active_turn_id: None,
                git_auto_merge_enabled: false,
                log: vec![RuntimeLogEntry {
                    kind: "assistant".into(),
                    message: "已确认左侧小聊天框是重复入口，接下来会直接移除。".into(),
                    at: "3".into(),
                }],
                last_error: None,
            }),
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };
        let latest_scan = ProjectScanSummary {
            project_id,
            workspace_id,
            workspace_label: "主目录".into(),
            workspace_path: "C:/Users/zuoho/code/spotlight".into(),
            scanned_at: "4".into(),
            stack_summary: "Rust + Tauri + 原生前端".into(),
            detected_stacks: vec!["Rust".into(), "Tauri".into()],
            top_level_entries: vec!["apps".into(), "crates".into(), "docs".into()],
            key_files: vec!["Cargo.toml".into(), "apps/server/src/main.rs".into()],
            document_files: vec!["docs/system-architecture.md".into()],
            notes: vec!["桌面端和服务端共用一个工作区".into()],
        };
        let chat_messages = vec![ProjectChatMessage {
            id: Uuid::new_v4(),
            project_id,
            user_id: None,
            user_display_name: "架构师".into(),
            content: "左侧的去掉，只保留项目聊天室，这块先别再做重复聊天框。".into(),
            at: "5".into(),
        }];
        let project_constraints = vec![
            "- 保留移动端入口：统一入口不能做成桌面专属。".to_string(),
            "- 共享策略：先做共享观察 + 明确接管，不做多人同时写 live thread。".to_string(),
        ];
        let recent_task_summaries = vec![
            "- [0.1.2] 真实运行接入：已接通长会话，但 watchdog 回收后状态回退问题待修。"
                .to_string(),
            "- [0.1.3] 持久化内核：已补 task-run 持久化设计，待完成跨重启恢复验证。".to_string(),
        ];
        let pending_question_lines = vec![
            "- 是否将恢复失败的任务统一留在 PAUSED，而不是重新回到 OPEN？".to_string(),
            "- 移动端入口保留后，桌面统一入口的主导航如何避免重复？".to_string(),
        ];

        let prompt = super::compose_task_prompt(
            &task,
            &project,
            Some(&latest_scan),
            &project_constraints,
            &recent_task_summaries,
            &pending_question_lines,
            &chat_messages,
            Some("先把客户端这轮体验问题修掉".into()),
        );

        assert!(prompt.contains("最近扫描摘要"));
        assert!(prompt.contains("Rust + Tauri + 原生前端"));
        assert!(prompt.contains("最近任务活动"));
        assert!(prompt.contains("左下角聊天框先不做"));
        assert!(prompt.contains("最近运行输出"));
        assert!(prompt.contains("当前有效项目约束"));
        assert!(prompt.contains("保留移动端入口"));
        assert!(prompt.contains("最近任务摘要"));
        assert!(prompt.contains("watchdog 回收后状态回退问题待修"));
        assert!(prompt.contains("仍待回答的项目问题"));
        assert!(prompt.contains("恢复失败的任务统一留在 PAUSED"));
        assert!(prompt.contains("最近项目聊天室"));
        assert!(prompt.contains("左侧的去掉，只保留项目聊天室"));
        assert!(prompt.contains("任务上下文快照（机器可读）"));
        assert!(prompt.contains("\"recent_task_summaries\""));
        assert!(prompt.contains("\"pending_questions\""));
        assert!(prompt.contains("\"scope_signals\""));
        assert!(prompt.contains("不要继续实现对应子需求"));
        assert!(prompt.contains("用户补充提示词"));
    }

    #[test]
    fn extract_task_completion_report_reads_json_code_block() {
        let log = vec![new_runtime_entry(
            "assistant",
            "任务已完成。\n```json\n{\n  \"result\": \"done\",\n  \"summary\": \"已完成目录扫描\",\n  \"questions\": [\"是否继续补充文档？\"],\n  \"follow_ups\": [\n    {\n      \"kind\": \"doc_update\",\n      \"title\": \"补充目录扫描使用文档\",\n      \"description\": \"补充项目目录接入和扫描说明\",\n      \"priority\": \"P2\",\n      \"can_auto_create_task\": true,\n      \"can_auto_apply\": false\n    }\n  ],\n  \"risks\": [\"文档入口仍需统一\"]\n}\n```",
        )];

        let report =
            super::extract_task_completion_report(&log).expect("expected completion report");
        assert_eq!(report.summary.as_deref(), Some("已完成目录扫描"));
        assert_eq!(report.questions.len(), 1);
        assert_eq!(report.follow_ups.len(), 1);
        assert_eq!(report.risks.len(), 1);
    }

    #[test]
    fn process_task_completion_outputs_creates_tasks_and_pending_questions() {
        let users = default_users();
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("failed to resolve workspace root")
            .to_path_buf();
        let projects = default_projects(&workspace_root);
        let creator_user_id = users.first().map(|user| user.id);
        let source_task = Task {
            id: Uuid::new_v4(),
            project_id: projects.first().unwrap().id,
            title: "实现项目目录接入".into(),
            description: "source".into(),
            status: TaskStatus::Done,
            priority: None,
            labels: Vec::new(),
            creator_user_id,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![TaskActivity {
                kind: "task.done".into(),
                message: "done".into(),
                at: "1".into(),
            }],
            runtime: Some(platform_core::TaskRuntime {
                provider: "codex".into(),
                thread_id: Some("thread".into()),
                active_turn_id: None,
                git_auto_merge_enabled: false,
                log: vec![new_runtime_entry(
                    "assistant",
                    "```json\n{\n  \"result\": \"done\",\n  \"summary\": \"功能完成，但还需要补测试和补一条澄清问题。\",\n  \"questions\": [\n    {\n      \"question\": \"是否要把目录选择器做成原生文件夹选择窗口？\",\n      \"context\": \"当前只支持手工输入绝对路径\"\n    }\n  ],\n  \"follow_ups\": [\n    {\n      \"kind\": \"test_gap\",\n      \"title\": \"补目录接入回归测试\",\n      \"description\": \"覆盖目录不存在和目录切换场景\",\n      \"priority\": \"P1\",\n      \"can_auto_create_task\": true,\n      \"can_auto_apply\": false\n    }\n  ],\n  \"risks\": [\"目录录入仍依赖手输路径\"]\n}\n```",
                )],
                last_error: None,
            }),
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };

        let mut state = BoardState {
            users: users.clone(),
            projects,
            tasks: vec![source_task.clone()],
            agents: default_agents(&users),
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: Vec::new(),
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };

        assert!(super::process_task_completion_outputs(
            &mut state,
            source_task.id
        ));
        assert!(state.tasks.iter().any(|task| {
            task.source_task_id == Some(source_task.id) && task.title == "补目录接入回归测试"
        }));
        assert!(state
            .pending_questions
            .iter()
            .any(|question| question.source_task_id == source_task.id));
        let follow_up_task = state
            .tasks
            .iter()
            .find(|task| task.source_task_id == Some(source_task.id))
            .unwrap();
        assert_eq!(follow_up_task.priority, Some(TaskPriority::High));
        let task_memory_item = state
            .memory_items
            .iter()
            .find(|item| item.scope_kind == "task" && item.scope_id == source_task.id)
            .expect("expected task memory item");
        assert_eq!(task_memory_item.memory_kind, "task_summary");
        let task_memory_tag = state
            .memory_tags
            .iter()
            .find(|tag| tag.memory_item_id == task_memory_item.id)
            .expect("expected task summary tag");
        let task_memory_revision = state
            .memory_revisions
            .iter()
            .find(|revision| revision.id == task_memory_tag.target_revision_id)
            .expect("expected task summary revision");
        assert!(task_memory_revision.content.contains("功能完成"));
        let source = state
            .tasks
            .iter()
            .find(|task| task.id == source_task.id)
            .unwrap();
        assert!(source
            .activities
            .iter()
            .any(|item| item.kind == "task.follow_ups_created"));
        assert!(source
            .activities
            .iter()
            .any(|item| item.kind == "task.questions_captured"));
        assert!(source
            .activities
            .iter()
            .any(|item| item.kind == "task.completion_summary"));
    }

    #[test]
    fn process_project_session_completion_outputs_creates_memory_tasks_and_chat_for_planner() {
        let users = default_users();
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("failed to resolve workspace root")
            .to_path_buf();
        let projects = default_projects(&workspace_root);
        let project_id = projects.first().expect("expected project").id;
        let session_id = Uuid::new_v4();

        let mut state = BoardState {
            users: users.clone(),
            projects,
            tasks: Vec::new(),
            agents: default_agents(&users),
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: vec![ProjectSession {
                id: session_id,
                project_id,
                title: "客户端 Harness 规划".into(),
                mode: "planner".into(),
                status: "completed".into(),
                workspace_path: Some(workspace_root.to_string_lossy().into_owned()),
                thread_id: Some("planner-thread".into()),
                active_turn_id: None,
                messages: Vec::new(),
                log: vec![new_runtime_entry(
                    "assistant",
                    "```json\n{\n  \"result\": \"plan_ready\",\n  \"summary\": \"已整理客户端 Harness 的最小闭环规划。\",\n  \"questions\": [\n    {\n      \"question\": \"是否先只聚焦桌面端客户端可用性？\",\n      \"context\": \"移动端和后台端可以继续排在后面\"\n    }\n  ],\n  \"follow_ups\": [\n    {\n      \"kind\": \"follow_up_task\",\n      \"title\": \"梳理客户端 Harness 最小闭环规格\",\n      \"description\": \"明确 planner / generator / evaluator 在当前版本中的最小落地点、入口和验收标准。\",\n      \"priority\": \"P1\",\n      \"can_auto_create_task\": true,\n      \"can_auto_apply\": false\n    }\n  ],\n  \"risks\": [\"如果没有统一验收标准，后续会继续碎片化推进\"]\n}\n```",
                )],
                last_error: None,
            }],
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };

        assert!(super::process_project_session_completion_outputs(
            &mut state, session_id
        ));
        assert!(state.tasks.iter().any(|task| {
            task.title == "梳理客户端 Harness 最小闭环规格"
                && task.labels.iter().any(|label| label == "harness:planner")
        }));
        assert!(state.project_chat_messages.iter().any(|message| {
            message.user_display_name == "规划器"
                && message.content.contains("待确认问题")
                && message.content.contains("桌面端客户端可用性")
        }));
        let memory_item = state
            .memory_items
            .iter()
            .find(|item| {
                item.scope_kind == "project"
                    && item.scope_id == project_id
                    && item.memory_kind == "project_harness_report"
                    && item.stable_key == "project_harness_report/planner"
            })
            .expect("expected planner memory item");
        let memory_tag = state
            .memory_tags
            .iter()
            .find(|tag| tag.memory_item_id == memory_item.id)
            .expect("expected planner memory tag");
        let memory_revision = state
            .memory_revisions
            .iter()
            .find(|revision| revision.id == memory_tag.target_revision_id)
            .expect("expected planner memory revision");
        assert!(memory_revision.title.contains("规划器报告"));
        assert!(memory_revision.content.contains("最小闭环规划"));
        assert!(state.project_sessions[0]
            .log
            .iter()
            .any(|entry| entry.kind == "project.session.completion_processed"));
    }

    #[test]
    fn process_project_session_completion_outputs_creates_fix_task_for_evaluator() {
        let users = default_users();
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("failed to resolve workspace root")
            .to_path_buf();
        let projects = default_projects(&workspace_root);
        let project_id = projects.first().expect("expected project").id;
        let session_id = Uuid::new_v4();

        let mut state = BoardState {
            users: users.clone(),
            projects,
            tasks: Vec::new(),
            agents: default_agents(&users),
            task_run_history: HashMap::new(),
            pending_questions: Vec::new(),
            project_scans: HashMap::new(),
            project_sessions: vec![ProjectSession {
                id: session_id,
                project_id,
                title: "客户端运行评估".into(),
                mode: "evaluator".into(),
                status: "completed".into(),
                workspace_path: Some(workspace_root.to_string_lossy().into_owned()),
                thread_id: Some("evaluator-thread".into()),
                active_turn_id: None,
                messages: Vec::new(),
                log: vec![new_runtime_entry(
                    "assistant",
                    "```json\n{\n  \"result\": \"needs_fix\",\n  \"summary\": \"发现客户端日志可见性仍不足。\",\n  \"questions\": [],\n  \"follow_ups\": [\n    {\n      \"kind\": \"bug_fix\",\n      \"title\": \"修复任务运行日志可见性\",\n      \"description\": \"确保客户端能看到运行中任务、Agent 输出和 failed to fetch 的真实原因。\",\n      \"priority\": \"P1\",\n      \"can_auto_create_task\": true,\n      \"can_auto_apply\": false\n    }\n  ],\n  \"risks\": [\"如果日志链路不通，客户端会持续表现为像死了一样\"]\n}\n```",
                )],
                last_error: None,
            }],
            project_chat_messages: Vec::new(),
            memory_items: Vec::new(),
            memory_revisions: Vec::new(),
            memory_tags: Vec::new(),
            memory_edges: Vec::new(),
            decisions: Vec::new(),
        };

        assert!(super::process_project_session_completion_outputs(
            &mut state, session_id
        ));
        let fix_task = state
            .tasks
            .iter()
            .find(|task| task.title == "修复任务运行日志可见性")
            .expect("expected evaluator follow-up task");
        assert_eq!(fix_task.priority, Some(TaskPriority::High));
        assert!(fix_task
            .labels
            .iter()
            .any(|label| label == "harness:evaluator"));
        assert!(fix_task.description.contains("客户端运行评估"));
        assert!(state.memory_items.iter().any(|item| {
            item.scope_kind == "project"
                && item.scope_id == project_id
                && item.memory_kind == "project_harness_report"
                && item.stable_key == "project_harness_report/evaluator"
        }));
    }

    #[test]
    fn reassess_gate_marks_done_when_completion_evidence_exists() {
        let task = Task {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            title: "[0.1.2] Agent 调用 MVP".into(),
            description: "test".into(),
            status: TaskStatus::Paused,
            priority: None,
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![
                new_activity(
                    "runtime.turn_completed",
                    "Turn stub-turn 已结束，状态：completed",
                ),
                new_activity("task.watchdog_recovered", "超时回收"),
            ],
            runtime: Some(TaskRuntime {
                provider: "codex".into(),
                thread_id: Some("thread-1".into()),
                active_turn_id: None,
                git_auto_merge_enabled: false,
                log: vec![new_runtime_entry(
                    "assistant",
                    "已完成所有修改\n```json\n{\"result\":\"done\",\"summary\":\"完成\"}\n```",
                )],
                last_error: None,
            }),
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };

        let all_tasks = vec![task.clone()];
        let result = crate::automation::quick_reassess_gate(&task, &all_tasks, task.project_id);
        assert_eq!(result, "DONE");
    }

    #[test]
    fn reassess_gate_detects_cancel_signal() {
        let task = Task {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            title: "[0.1.3] 工作流引擎".into(),
            description: "test".into(),
            status: TaskStatus::Paused,
            priority: None,
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![
                new_activity("task.started", "已开始"),
                new_activity("task.paused", "先不做这个功能，取消"),
            ],
            runtime: Some(TaskRuntime {
                provider: "codex".into(),
                thread_id: Some("thread-1".into()),
                active_turn_id: None,
                git_auto_merge_enabled: false,
                log: Vec::new(),
                last_error: None,
            }),
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };

        let all_tasks = vec![task.clone()];
        let result = crate::automation::quick_reassess_gate(&task, &all_tasks, task.project_id);
        assert_eq!(result, "CANCELED");
    }

    #[test]
    fn reassess_gate_detects_overlapping_done_task() {
        let project_id = Uuid::new_v4();
        let done_task = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "[0.1.2] Agent 调用与运行时接入".into(),
            description: "已完成".into(),
            status: TaskStatus::Done,
            priority: None,
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![new_activity("task.done", "完成")],
            runtime: None,
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };

        let paused_task = Task {
            id: Uuid::new_v4(),
            project_id,
            title: "[0.1.2] Agent 调用与运行时 MVP 接入".into(),
            description: "test".into(),
            status: TaskStatus::Paused,
            priority: None,
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![
                new_activity("runtime.thread_started", "线程启动"),
                new_activity("task.watchdog_recovered", "超时回收"),
            ],
            runtime: Some(TaskRuntime {
                provider: "codex".into(),
                thread_id: Some("thread-2".into()),
                active_turn_id: None,
                git_auto_merge_enabled: false,
                log: Vec::new(),
                last_error: None,
            }),
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };

        let all_tasks = vec![done_task, paused_task.clone()];
        let result = crate::automation::quick_reassess_gate(&paused_task, &all_tasks, project_id);
        assert_eq!(result, "DONE", "应该检测到重叠的已完成任务");
    }

    #[test]
    fn reassess_gate_suggests_restart_when_thread_available() {
        let task = Task {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            title: "[0.1.4] 桌面执行壳".into(),
            description: "test".into(),
            status: TaskStatus::Paused,
            priority: None,
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![new_activity("task.watchdog_recovered", "超时回收")],
            runtime: Some(TaskRuntime {
                provider: "codex".into(),
                thread_id: Some("thread-1".into()),
                active_turn_id: None,
                git_auto_merge_enabled: false,
                log: Vec::new(),
                last_error: None,
            }),
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };

        let all_tasks = vec![task.clone()];
        let result = crate::automation::quick_reassess_gate(&task, &all_tasks, task.project_id);
        assert_eq!(result, "RESTART");
    }

    #[test]
    fn reassess_gate_suggests_reopen_after_recovery_loop_exhausted() {
        let task = Task {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            title: "[0.1.2] 真实 Codex 接入".into(),
            description: "test".into(),
            status: TaskStatus::Paused,
            priority: None,
            labels: Vec::new(),
            creator_user_id: None,
            assignee_user_id: None,
            assignment_mode: TaskAssignmentMode::PublicQueue,
            requested_agent_id: None,
            source_task_id: None,
            claimed_by: None,
            activities: vec![
                new_activity("task.watchdog_recovered", "第一次回收"),
                new_activity("task.auto_retry_queued", "第二次"),
                new_activity("task.runtime_session_lost", "第三次"),
            ],
            runtime: Some(TaskRuntime {
                provider: "codex".into(),
                thread_id: Some("thread-1".into()),
                active_turn_id: None,
                git_auto_merge_enabled: false,
                log: Vec::new(),
                last_error: None,
            }),
            approval: Default::default(),
            acceptance: Default::default(),
            state_snapshot: TaskStateSnapshot::default(),
        };

        let all_tasks = vec![task.clone()];
        let result = crate::automation::quick_reassess_gate(&task, &all_tasks, task.project_id);
        assert_eq!(result, "REOPEN");
    }
}
