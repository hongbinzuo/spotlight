use std::{net::SocketAddr, path::PathBuf};

use axum::{
    routing::{get, post},
    Router,
};

use crate::{handlers, AppState, RuntimeMode};
use crate::{automation::start_background_automation, state::default_state};

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
        .route("/", get(handlers::index))
        .nest("/api", api.clone())
        .nest("/api/v1", api)
        .with_state(state)
}

pub(crate) fn build_api_router() -> Router<AppState> {
    Router::new()
        .route("/me", get(handlers::get_me))
        .route("/auth/login", post(handlers::login))
        .route("/board", get(handlers::get_board))
        .route("/projects", get(handlers::list_projects))
        .route(
            "/projects/{project_id}/tasks",
            get(handlers::list_project_tasks),
        )
        .route("/agents", get(handlers::list_agents))
        .route(
            "/questions/{question_id}/answer",
            post(handlers::answer_pending_question),
        )
        .route(
            "/projects/{project_id}/summary",
            get(handlers::get_project_summary),
        )
        .route(
            "/projects/{project_id}/context",
            get(handlers::get_project_context),
        )
        .route(
            "/projects/{project_id}/memory",
            get(handlers::get_project_memory),
        )
        .route(
            "/projects/{project_id}/chat",
            post(handlers::post_project_chat_message),
        )
        .route(
            "/projects/{project_id}/memory/constraints",
            post(handlers::upsert_project_constraint),
        )
        .route(
            "/projects/{project_id}/workspaces",
            post(handlers::register_project_workspace),
        )
        .route("/projects/{project_id}/scan", post(handlers::scan_project))
        .route(
            "/projects/{project_id}/sessions",
            post(handlers::start_project_session),
        )
        .route(
            "/project-sessions/{session_id}/turns",
            post(handlers::continue_project_session),
        )
        .route("/tasks", post(handlers::create_task))
        .route(
            "/projects/{project_id}/tasks/bootstrap",
            post(handlers::bootstrap_tasks),
        )
        .route(
            "/projects/{project_id}/tasks/seed-docs",
            post(handlers::seed_doc_tasks),
        )
        .route(
            "/projects/{project_id}/tasks/local-build-restart",
            post(handlers::create_local_build_restart_task),
        )
        .route(
            "/projects/{project_id}/tasks/cloud-install-restart",
            post(handlers::create_cloud_install_restart_task),
        )
        .route("/projects/{project_id}/explore", post(handlers::explore_project))
        .route(
            "/agents/{agent_id}/auto-mode/toggle",
            post(handlers::toggle_agent_auto_mode),
        )
        .route("/agents/{agent_id}/pull-next", post(handlers::pull_next_task))
        .route("/tasks/{task_id}/claim/{agent_id}", post(handlers::claim_task))
        .route("/tasks/{task_id}/start/{agent_id}", post(handlers::start_task))
        .route("/tasks/{task_id}/pause", post(handlers::pause_task))
        .route("/tasks/{task_id}/cancel", post(handlers::cancel_task))
        .route("/tasks/{task_id}/resume/{agent_id}", post(handlers::resume_task))
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
