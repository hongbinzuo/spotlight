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
    select_next_auto_resume_task_id,
};
use completion::*;
use git_ops::*;
use handlers::{runtime_event_loop, timestamp_string, write_memory_revision};
use models::*;
#[cfg(test)]
use server::{build_api_router, parse_server_port};
use server::{build_app, server_listen_addr};
use state::{
    default_agents, default_projects, default_state, default_users, load_or_initialize_state,
    normalize_persisted_state,
};
use task_ops::{
    active_task_conflict, assign_agent_claimed, auto_claim_next_task, find_task_mut,
};

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
};

use axum::{
    http::StatusCode,
};
use platform_core::{
    Agent, CoordinationWriteIntent, DecisionCard, ExecutionSlotRecord, PendingQuestion, Project,
    Task, TaskRunRecord, User, WorkspaceLeaseRecord,
};
use runtime::{ProviderRuntimeSession, RuntimeEvent};
use tokio::{
    sync::Mutex,
};
use uuid::Uuid;

type AppResult<T> = Result<T, (StatusCode, String)>;
const AUTO_MAINTENANCE_INTERVAL_SECS: u64 = 5;
const TASK_STALE_TIMEOUT_SECS: u64 = 300;
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
    execution_slots: Vec<ExecutionSlotRecord>,
    workspace_leases: Vec<WorkspaceLeaseRecord>,
    coordination_write_intents: Vec<CoordinationWriteIntent>,
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

#[cfg(test)]
mod tests {
    use super::{
        active_task_conflict, auto_claim_next_task, build_api_router, build_app, default_agents,
        default_projects, default_state, default_users,
        finalize_git_task_branch_in_repo, prepare_git_task_branch_in_repo,
        reconcile_parallel_active_tasks, reconcile_watchdog_state, run_automation_cycle_once,
        runtime_event_loop, select_next_auto_resume_task_id,
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
    use crate::task_ops::{
        detect_project_stack, mark_task_running_with_provider, sanitize_credential_hint,
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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
    fn auto_claim_next_task_prefers_owner_queue_before_shared_priority() {
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
        let owner_medium_id = owner_medium.id;
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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
            .expect("should auto claim the highest-priority eligible task within the best queue");

        assert_eq!(claimed.id, owner_medium_id);
        assert_eq!(claimed.status, TaskStatus::Claimed);
        assert!(
            claimed
                .activities
                .iter()
                .any(|item| item.kind == "task.auto_claim_reason"
                    && item.message.contains("中优先级"))
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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
            mark_task_running_with_provider(
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
            mark_task_running_with_provider(
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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

        let prompt = crate::prompt::compose_task_prompt_with_snapshot(
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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
            execution_slots: Vec::new(),
            workspace_leases: Vec::new(),
            coordination_write_intents: Vec::new(),
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
