use std::{net::SocketAddr, path::PathBuf};

use axum::{
    extract::{Path as AxumPath, State},
    routing::{get, post},
    Json, Router,
};
use platform_core::BoardSnapshot;
use uuid::Uuid;

use crate::{
    answer_pending_question, bootstrap_tasks, cancel_task, create_cloud_install_restart_task,
    create_local_build_restart_task, create_task, default_state, explore_project, get_board,
    get_me, get_project_context, get_project_memory, get_project_summary, index, list_agents,
    list_project_tasks, list_projects, login, pause_task, post_project_chat_message,
    pull_next_task, register_project_workspace, resume_task, scan_project, seed_doc_tasks,
    start_background_automation, start_project_session, start_task, toggle_agent_auto_mode,
    upsert_project_constraint,
};
use crate::{continue_project_session, RuntimeMode};
use crate::{handlers, AppState};
use crate::{
    snapshot::snapshot_from_state, state::persist_state, task_ops::claim_task_for_agent, AppResult,
};

pub(crate) fn server_listen_addr() -> SocketAddr {
    let port = parse_server_port(std::env::var("SPOTLIGHT_SERVER_PORT").ok().as_deref());
    SocketAddr::from(([127, 0, 0, 1], port))
}

pub(crate) fn parse_server_port(raw: Option<&str>) -> u16 {
    raw.and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3000)
}

pub(crate) fn build_app(runtime_mode: RuntimeMode, workspace_root: PathBuf) -> Router {
    let state = default_state(runtime_mode, workspace_root);
    start_background_automation(state.clone());

    let api = build_api_router();

    Router::new()
        .route("/", get(index))
        .nest("/api", api.clone())
        .nest("/api/v1", api)
        .with_state(state)
}

pub(crate) fn build_api_router() -> Router<AppState> {
    Router::new()
        .route("/me", get(get_me))
        .route("/auth/login", post(login))
        .route("/board", get(get_board))
        .route("/projects", get(list_projects))
        .route("/projects/{project_id}/tasks", get(list_project_tasks))
        .route("/agents", get(list_agents))
        .route(
            "/questions/{question_id}/answer",
            post(answer_pending_question),
        )
        .route("/projects/{project_id}/summary", get(get_project_summary))
        .route("/projects/{project_id}/context", get(get_project_context))
        .route("/projects/{project_id}/memory", get(get_project_memory))
        .route(
            "/projects/{project_id}/chat",
            post(post_project_chat_message),
        )
        .route(
            "/projects/{project_id}/memory/constraints",
            post(upsert_project_constraint),
        )
        .route(
            "/projects/{project_id}/workspaces",
            post(register_project_workspace),
        )
        .route("/projects/{project_id}/scan", post(scan_project))
        .route(
            "/projects/{project_id}/sessions",
            post(start_project_session),
        )
        .route(
            "/project-sessions/{session_id}/turns",
            post(continue_project_session),
        )
        .route("/tasks", post(create_task))
        .route(
            "/projects/{project_id}/tasks/bootstrap",
            post(bootstrap_tasks),
        )
        .route(
            "/projects/{project_id}/tasks/seed-docs",
            post(seed_doc_tasks),
        )
        .route(
            "/projects/{project_id}/tasks/local-build-restart",
            post(create_local_build_restart_task),
        )
        .route(
            "/projects/{project_id}/tasks/cloud-install-restart",
            post(create_cloud_install_restart_task),
        )
        .route("/projects/{project_id}/explore", post(explore_project))
        .route(
            "/agents/{agent_id}/auto-mode/toggle",
            post(toggle_agent_auto_mode),
        )
        .route("/agents/{agent_id}/pull-next", post(pull_next_task))
        .route("/tasks/{task_id}/claim/{agent_id}", post(claim_task_route))
        .route("/tasks/{task_id}/start/{agent_id}", post(start_task))
        .route("/tasks/{task_id}/pause", post(pause_task))
        .route("/tasks/{task_id}/cancel", post(cancel_task))
        .route("/tasks/{task_id}/resume/{agent_id}", post(resume_task))
        .route("/tasks/{task_id}/reassess", post(handlers::reassess_task))
        .route(
            "/projects/{project_id}/reassess",
            post(handlers::reassess_project_tasks),
        )
        .route("/decisions", get(handlers::list_decisions))
        .route(
            "/decisions/{decision_id}/resolve",
            post(handlers::resolve_decision),
        )
        .route(
            "/decisions/batch-resolve",
            post(handlers::batch_resolve_decisions),
        )
}

async fn claim_task_route(
    AxumPath((task_id, agent_id)): AxumPath<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> AppResult<Json<BoardSnapshot>> {
    let mut guard = state.inner.lock().await;
    claim_task_for_agent(&mut guard, task_id, agent_id)?;
    let snapshot = snapshot_from_state(&guard);
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(snapshot))
}
