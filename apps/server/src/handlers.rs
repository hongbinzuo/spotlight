use std::{
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
    merge_unique_tasks, new_activity, new_runtime_entry, seed_tasks_from_docs,
    AgentInvocationRequest, AgentResumeRequest, BoardSnapshot, CreateTaskRequest, Project, Task,
    TaskAcceptanceState, TaskApprovalState, TaskAssignmentMode, TaskRuntime, TaskStateSnapshot,
    TaskStatus, WorkspaceRoot,
};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::completion::{
    process_project_session_completion_outputs, process_task_completion_outputs,
};
use crate::git_ops;
use crate::models::*;
use crate::prompt;
use crate::runtime::{CodexRuntimeSession, RuntimeEvent};
use crate::snapshot::*;
use crate::state::{
    persist_state, seed_tasks_from_agents_file, task_has_completion_evidence,
    task_has_progress_evidence,
};
use crate::task_ops::*;
use crate::{AppResult, AppState, BoardState};

pub(crate) async fn index() -> Html<&'static str> {
    Html(crate::ui::INDEX_HTML)
}

pub(crate) async fn get_me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<AuthSnapshot> {
    let guard = state.inner.lock().await;
    let current_user = resolve_current_user(&guard, &headers);
    Json(AuthSnapshot {
        current_user,
        users: guard.users.clone(),
    })
}

fn refresh_pending_reassess_decision(
    decision: &mut platform_core::DecisionCard,
    guard: &BoardState,
) {
    if !matches!(decision.kind, platform_core::DecisionKind::Reassess) {
        return;
    }

    let Some(task_id) = decision.task_id else {
        return;
    };
    let Some(task) = guard.tasks.iter().find(|task| task.id == task_id) else {
        return;
    };

    let rule_decision = evaluate_reassess_by_rules(task);
    let decision_name = rule_decision
        .get("decision")
        .and_then(|value| value.as_str())
        .unwrap_or("MANUAL_REVIEW");
    let confidence = rule_decision
        .get("confidence")
        .and_then(|value| value.as_f64())
        .unwrap_or(0.5);
    let reason = rule_decision
        .get("reason")
        .and_then(|value| value.as_str())
        .unwrap_or("证据不足，建议人工复核");

    let action_hint = match decision_name {
        "DONE" => "建议直接标记完成。",
        "CANCELED" => "建议撤销该任务，避免继续投入。",
        "REOPEN" => "建议清理失效上下文后重新排队，并补充更明确的任务说明。",
        "RESTART" => "建议沿用现有 thread 恢复，而不是从头新开。",
        _ => "建议先检查任务描述、最近活动和运行日志，再决定是否重开或撤销。",
    };

    decision.title = format!("任务重评估：{}", task.title);
    decision.context = format!(
        "规则引擎当前建议：{decision_name}（置信度 {:.0}%）。\n任务标题：{}\n任务描述：{}\n当前状态：{}\n原因：{}\n{}",
        confidence * 100.0,
        task.title,
        task.description,
        task.status.as_str(),
        reason,
        action_hint
    );
    let display_reason = if reason.contains('�') || reason.contains("璇") {
        "证据不足，建议人工复核。"
    } else {
        reason
    };
    let action_hint = match decision_name {
        "DONE" => "建议直接标记为完成。",
        "CANCELED" => "建议撤销该任务，避免继续投入。",
        "REOPEN" => "建议清理失效上下文后重新排队，并补充更明确的任务说明。",
        "RESTART" => "建议沿用现有 thread 恢复，而不是从头新开。",
        _ => "建议先检查任务描述、最近活动和运行日志，再决定是重开、恢复还是撤销。",
    };
    let status_label = match task.status {
        TaskStatus::Open => "待处理",
        TaskStatus::Claimed => "已认领",
        TaskStatus::Running => "运行中",
        TaskStatus::Paused => "已暂停",
        TaskStatus::Done => "已完成",
        TaskStatus::Failed => "失败",
        TaskStatus::Canceled => "已撤销",
        TaskStatus::ApprovalRequested => "待审批",
        TaskStatus::Approved => "已审批",
        TaskStatus::PendingAcceptance => "待验收",
        TaskStatus::Accepted => "已验收",
        TaskStatus::ManualReview => "待复核",
    };
    decision.title = format!("任务重评估：{}", task.title);
    decision.context = format!(
        "规则引擎当前建议：{decision_name}（置信度 {:.0}%）。\n任务标题：{}\n任务描述：{}\n当前状态：{}\n原因：{}\n{}",
        confidence * 100.0,
        task.title,
        task.description,
        status_label,
        display_reason,
        action_hint
    );
    decision.recommended = match decision_name {
        "DONE" => Some("done".into()),
        "CANCELED" => Some("cancel".into()),
        "REOPEN" => Some("reopen".into()),
        "RESTART" => Some("restart".into()),
        _ => None,
    };
    decision.confidence = Some(confidence as f32);
}

pub(crate) async fn login(
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

pub(crate) async fn get_board(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<BoardSnapshot> {
    let guard = state.inner.lock().await;
    Json(snapshot_from_state_with_user(
        &guard,
        resolve_current_user(&guard, &headers),
    ))
}

pub(crate) async fn list_projects(State(state): State<AppState>) -> Json<Vec<Project>> {
    let guard = state.inner.lock().await;
    Json(guard.projects.clone())
}

pub(crate) async fn list_project_tasks(
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

pub(crate) async fn list_agents(State(state): State<AppState>) -> Json<Vec<platform_core::Agent>> {
    let guard = state.inner.lock().await;
    Json(guard.agents.clone())
}

pub(crate) fn parse_task_status(raw: &str) -> AppResult<TaskStatus> {
    TaskStatus::parse_filter(raw).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("不支持的任务状态筛选：{raw}"),
        )
    })
}

pub(crate) async fn answer_pending_question(
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

pub(crate) async fn get_project_context(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<ProjectContextSnapshot>> {
    let guard = state.inner.lock().await;
    let snapshot = project_context_snapshot(&guard, project_id)?;
    Ok(Json(snapshot))
}

pub(crate) async fn get_project_summary(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<ProjectSummarySnapshot>> {
    let guard = state.inner.lock().await;
    let snapshot = project_summary_snapshot(&guard, project_id)?;
    Ok(Json(snapshot))
}

pub(crate) async fn get_project_memory(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<ProjectMemorySnapshot>> {
    let guard = state.inner.lock().await;
    ensure_project_exists(&guard, project_id)?;
    Ok(Json(project_memory_snapshot(&guard, project_id)))
}

pub(crate) async fn upsert_project_constraint(
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

pub(crate) async fn post_project_chat_message(
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

pub(crate) async fn register_project_workspace(
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

pub(crate) async fn scan_project(
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

pub(crate) async fn start_project_session(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<StartProjectSessionRequest>,
) -> AppResult<Json<ProjectContextSnapshot>> {
    let user_prompt = request.prompt.trim();
    if user_prompt.is_empty() {
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
            let preview = truncate_title(user_prompt);
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
        content: user_prompt.to_string(),
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
                    user_prompt.to_string(),
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
                user_prompt,
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
        user_prompt,
        &mode,
    );
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
        .start_thread(
            &workspace_root,
            &prompt::project_session_developer_instructions_for_mode(&mode),
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

pub(crate) async fn continue_project_session(
    AxumPath(session_id): AxumPath<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<ContinueProjectSessionRequest>,
) -> AppResult<Json<ProjectContextSnapshot>> {
    let user_prompt = request.prompt.trim();
    if user_prompt.is_empty() {
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
                content: user_prompt.to_string(),
                at: timestamp_string(),
            });
            project_session.log.push(new_runtime_entry(
                "project.session.user",
                user_prompt.to_string(),
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
                user_prompt,
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
        user_prompt,
        &mode,
    );

    let resolved_session =
        resolve_project_runtime_session(&state, session_id, workspace_root.clone(), &thread_id)
            .await?;

    let turn_id = match resolved_session
        .session
        .start_turn(
            &workspace_root,
            &resolved_session.thread_id,
            &composed_prompt,
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

pub(crate) async fn create_task(
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
    let (assignment_mode, requested_agent_id, assignee_user_id, assignment_activity) =
        if let Some(requested_agent_id) = request.requested_agent_id {
            let agent = guard
                .agents
                .iter()
                .find(|agent| agent.id == requested_agent_id)
                .cloned()
                .ok_or_else(|| (StatusCode::NOT_FOUND, "链壘鍒拌姹傜殑 Agent".into()))?;
            (
                TaskAssignmentMode::AssignedAgent,
                Some(agent.id),
                agent.owner_user_id,
                Some(new_activity(
                    "task.assigned",
                    format!("任务创建时已绑定到 Agent {}", agent.name),
                )),
            )
        } else {
            (TaskAssignmentMode::PublicQueue, None, None, None)
        };
    let mut activities = vec![new_activity(
        "task.created",
        format!(
            "浠诲姟鐢辩晫闈㈠垱寤猴紝骞跺綊灞炲埌椤圭洰\u{201c}{}\u{201d}",
            project.name
        ),
    )];
    if let Some(activity) = assignment_activity {
        activities.push(activity);
    }
    let task = Task {
        id: Uuid::new_v4(),
        project_id: project.id,
        title: title.to_string(),
        description: description.to_string(),
        status: TaskStatus::Open,
        priority: request.priority,
        labels: request.labels,
        creator_user_id: current_user.as_ref().map(|user| user.id),
        assignee_user_id,
        assignment_mode,
        requested_agent_id,
        source_task_id: None,
        claimed_by: None,
        activities, /*
                        "task.created",
                        format!(
                            "任务由界面创建，并归属到项目\u{201c}{}\u{201d}",
                            project.name
                        ),
                    */
        runtime: None,
        approval: TaskApprovalState {
            required: request.approval_required,
            ..TaskApprovalState::default()
        },
        acceptance: TaskAcceptanceState {
            owner_user_id: request.acceptance_owner_user_id,
            ..TaskAcceptanceState::default()
        },
        state_snapshot: TaskStateSnapshot::default(),
    };
    guard.tasks.insert(0, task.clone());
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(task))
}

pub(crate) async fn seed_doc_tasks(
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

pub(crate) async fn create_local_build_restart_task(
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

pub(crate) async fn create_cloud_install_restart_task(
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

pub(crate) async fn bootstrap_tasks(
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

pub(crate) async fn explore_project(
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

pub(crate) async fn toggle_agent_auto_mode(
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

pub(crate) async fn pull_next_task(
    AxumPath(agent_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<PullNextResponse>> {
    let mut guard = state.inner.lock().await;
    let task = auto_claim_next_task(&mut guard, agent_id)?;
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(PullNextResponse { task }))
}

pub(crate) async fn claim_task(
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
    if task
        .requested_agent_id
        .is_some_and(|requested_agent_id| requested_agent_id != agent_id)
    {
        return Err((
            StatusCode::CONFLICT,
            "当前任务已绑定到其他 Agent，请先重新分配".into(),
        ));
    }
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

pub(crate) async fn start_task(
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

    let context =
        prompt::resolve_task_execution_context(&state, task_id, request.prompt.clone()).await?;
    let _ = std::fs::create_dir_all(&context.workspace_root);
    let mut git_auto_merge_enabled = false;
    if matches!(state.runtime_mode, RuntimeMode::RealCodex) {
        let git_prepare =
            git_ops::prepare_git_task_branch_in_repo(&context.workspace_root, task_id).await?;
        git_auto_merge_enabled = git_prepare.auto_merge_enabled;
        for (kind, message) in git_prepare.activities {
            record_task_activity(&state, task_id, kind, message).await;
        }
        git_ops::apply_git_snapshot(&context.workspace_root, task_id, &state).await;
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
                .start_thread(
                    &context.workspace_root,
                    &prompt::task_developer_instructions(),
                )
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
                "codex".into(),
            ));
            let guard = state.inner.lock().await;
            let snapshot = snapshot_from_state(&guard);
            drop(guard);
            persist_state(&state).await?;
            Ok(Json(snapshot))
        }
    }
}

pub(crate) async fn pause_task(
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
        reset_agent_if_needed(&mut guard, task_id, "任务已暂停，等待恢复");
        snapshot_from_state(&guard)
    };
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

pub(crate) async fn cancel_task(
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
        reset_agent_if_needed(&mut guard, task_id, "最近一次任务已撤销");
    }

    let guard = state.inner.lock().await;
    let snapshot = snapshot_from_state_with_user(&guard, resolve_current_user(&guard, &headers));
    drop(guard);
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

pub(crate) async fn resume_task(
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

    let turn_id = match resolved_session
        .session
        .start_turn(
            &workspace_root,
            &resolved_session.thread_id,
            request.prompt.trim(),
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
                runtime.thread_id = Some(resolved_session.thread_id);
                runtime.active_turn_id = Some(turn_id);
            }
        }
        assign_agent_running(&mut guard, agent_id, task_id, "继续执行当前任务".into());
        snapshot_from_state(&guard)
    };
    persist_state(&state).await?;
    Ok(Json(snapshot))
}

pub(crate) async fn resolve_task_runtime_session(
    state: &AppState,
    task_id: Uuid,
    agent_id: Uuid,
    workspace_root: PathBuf,
    thread_id: &str,
) -> AppResult<ResolvedRuntimeSession> {
    let existing_session = {
        let sessions = state.runtime_sessions.lock().await;
        sessions.get(&task_id).cloned()
    };

    if let Some(session) = existing_session {
        return Ok(ResolvedRuntimeSession {
            session,
            thread_id: thread_id.to_string(),
            event_rx: None,
        });
    }

    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let session = CodexRuntimeSession::spawn(workspace_root, event_tx).await?;
    let resumed_thread_id = match session.resume_thread(thread_id).await {
        Ok(thread_id) => thread_id,
        Err(error) => {
            session.shutdown().await;
            return Err(error);
        }
    };

    let _ = agent_id;
    Ok(ResolvedRuntimeSession {
        session,
        thread_id: resumed_thread_id,
        event_rx: Some(event_rx),
    })
}

pub(crate) async fn register_task_runtime_session(
    state: &AppState,
    task_id: Uuid,
    agent_id: Uuid,
    session: Arc<CodexRuntimeSession>,
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

pub(crate) async fn resolve_project_runtime_session(
    state: &AppState,
    session_id: Uuid,
    workspace_root: PathBuf,
    thread_id: &str,
) -> AppResult<ResolvedRuntimeSession> {
    let existing_session = {
        let sessions = state.runtime_sessions.lock().await;
        sessions.get(&session_id).cloned()
    };

    if let Some(session) = existing_session {
        return Ok(ResolvedRuntimeSession {
            session,
            thread_id: thread_id.to_string(),
            event_rx: None,
        });
    }

    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let session = CodexRuntimeSession::spawn(workspace_root, event_tx).await?;
    let resumed_thread_id = match session.resume_thread(thread_id).await {
        Ok(thread_id) => thread_id,
        Err(error) => {
            session.shutdown().await;
            return Err(error);
        }
    };

    Ok(ResolvedRuntimeSession {
        session,
        thread_id: resumed_thread_id,
        event_rx: Some(event_rx),
    })
}

async fn register_project_runtime_session(
    state: &AppState,
    session_id: Uuid,
    session: Arc<CodexRuntimeSession>,
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

pub(crate) async fn runtime_event_loop(
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
            let merge_activities =
                git_ops::finalize_git_task_branch_in_repo(&workspace_root, task_id).await;
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

pub(crate) async fn reconcile_task_runtime_session_lost(state: &AppState, task_id: Uuid) {
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

pub(crate) async fn project_session_event_loop(
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

pub(crate) async fn mark_project_session_failed(state: &AppState, session_id: Uuid, message: &str) {
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

fn build_stub_project_session_reply(
    project: &Project,
    latest_scan: Option<&ProjectScanSummary>,
    mode: &str,
    user_prompt: &str,
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
            project.name, scan_line, user_prompt
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
            project.name, scan_line, user_prompt
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
            project.name, scan_line, user_prompt
        ),
    }
}

async fn complete_stub_project_session(
    state: &AppState,
    session_id: Uuid,
    project: &Project,
    latest_scan: Option<&ProjectScanSummary>,
    mode: &str,
    user_prompt: &str,
) -> AppResult<()> {
    let reply = build_stub_project_session_reply(project, latest_scan, mode, user_prompt);
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

pub(crate) fn build_project_scan_summary(
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

pub(crate) fn collect_top_level_entries(base: &Path, limit: usize) -> Vec<String> {
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

pub(crate) fn is_key_project_file(path: &Path) -> bool {
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

pub(crate) fn is_document_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("md" | "txt" | "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx")
    )
}

pub(crate) fn truncate_title(input: &str) -> String {
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

pub(crate) fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

pub(crate) fn normalized_constraint_stable_key(raw: Option<&str>, title: &str) -> String {
    if let Some(key) = raw.map(str::trim).filter(|value| !value.is_empty()) {
        return format!("project_constraint/{key}");
    }

    let normalized = title
        .chars()
        .map(|ch| if ch.is_whitespace() { '-' } else { ch })
        .collect::<String>();
    format!("project_constraint/{normalized}")
}

pub(crate) fn write_memory_revision(state: &mut BoardState, spec: MemoryWriteSpec) -> Uuid {
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

/// POST /tasks/{task_id}/reassess
/// 对单个任务做状态重评估。
/// 终结态（Done/Canceled/Accepted/Failed）直接返回 skip。
/// 被其他 Agent 持有的（Claimed/Running 且 claimed_by 非空）也直接 skip。
pub(crate) async fn reassess_task(
    AxumPath(task_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<serde_json::Value>> {
    let guard = state.inner.lock().await;
    let task = guard
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到任务".into()))?;

    // 终结态直接跳过
    if matches!(
        task.status,
        TaskStatus::Done | TaskStatus::Canceled | TaskStatus::Accepted | TaskStatus::Failed
    ) {
        return Ok(Json(serde_json::json!({
            "task_id": task_id,
            "task_title": task.title,
            "current_status": task.status.as_str(),
            "action": "skip",
            "skip_reason": "终结态任务，无需评估",
        })));
    }

    // 被其他 Agent 持有中（Claimed/Running 且有 claimed_by）直接跳过
    if matches!(task.status, TaskStatus::Claimed | TaskStatus::Running) && task.claimed_by.is_some()
    {
        let holder = task
            .claimed_by
            .and_then(|agent_id| {
                guard
                    .agents
                    .iter()
                    .find(|a| a.id == agent_id)
                    .map(|a| a.name.clone())
            })
            .unwrap_or_else(|| "未知 Agent".into());
        return Ok(Json(serde_json::json!({
            "task_id": task_id,
            "task_title": task.title,
            "current_status": task.status.as_str(),
            "action": "skip",
            "skip_reason": format!("任务正由 {holder} 持有中，无需评估"),
        })));
    }

    // 需要评估的状态：Paused、Open（有进度痕迹）、ManualReview 等
    let project = find_project(&guard, task.project_id)?.clone();
    let sibling_tasks = guard
        .tasks
        .iter()
        .filter(|t| t.project_id == task.project_id)
        .cloned()
        .collect::<Vec<_>>();
    let sibling_refs = sibling_tasks.iter().collect::<Vec<_>>();
    let latest_scan = guard.project_scans.get(&task.project_id).cloned();
    let project_constraints =
        crate::prompt::active_project_constraint_lines(&guard, task.project_id);
    let recent_chat = crate::prompt::recent_project_chat_messages(
        &guard.project_chat_messages,
        task.project_id,
        8,
    );
    drop(guard);

    let reassess_prompt = prompt::compose_task_reassess_prompt(
        &task,
        &project,
        &sibling_refs,
        latest_scan.as_ref(),
        &project_constraints,
        &recent_chat,
    );

    let rule_decision = evaluate_reassess_by_rules(&task);

    Ok(Json(serde_json::json!({
        "task_id": task_id,
        "task_title": task.title,
        "current_status": task.status.as_str(),
        "action": "reassess",
        "reassess_prompt": reassess_prompt,
        "rule_decision": rule_decision,
    })))
}

/// POST /projects/{project_id}/reassess
/// 批量评估一个项目下所有需要决策的任务。
/// 自动跳过终结态和被持有的任务，只返回需要评估的。
pub(crate) async fn reassess_project_tasks(
    AxumPath(project_id): AxumPath<Uuid>,
    State(state): State<AppState>,
) -> AppResult<Json<serde_json::Value>> {
    let guard = state.inner.lock().await;
    find_project(&guard, project_id)?;

    let mut needs_reassess = Vec::new();
    let mut skipped = Vec::new();

    let sibling_tasks = guard
        .tasks
        .iter()
        .filter(|t| t.project_id == project_id)
        .collect::<Vec<_>>();
    let latest_scan = guard.project_scans.get(&project_id).cloned();
    let project_constraints = crate::prompt::active_project_constraint_lines(&guard, project_id);
    let recent_chat =
        crate::prompt::recent_project_chat_messages(&guard.project_chat_messages, project_id, 8);

    for task in guard.tasks.iter().filter(|t| t.project_id == project_id) {
        // 终结态
        if matches!(
            task.status,
            TaskStatus::Done | TaskStatus::Canceled | TaskStatus::Accepted | TaskStatus::Failed
        ) {
            skipped.push(serde_json::json!({
                "task_id": task.id,
                "title": task.title,
                "status": task.status.as_str(),
                "reason": "终结态",
            }));
            continue;
        }

        // 被其他 Agent 持有
        if matches!(task.status, TaskStatus::Claimed | TaskStatus::Running)
            && task.claimed_by.is_some()
        {
            skipped.push(serde_json::json!({
                "task_id": task.id,
                "title": task.title,
                "status": task.status.as_str(),
                "reason": "被 Agent 持有中",
            }));
            continue;
        }

        // Open 但没有任何进度痕迹的——纯新任务，不需要评估
        if matches!(task.status, TaskStatus::Open) && !task_has_progress_evidence(task) {
            skipped.push(serde_json::json!({
                "task_id": task.id,
                "title": task.title,
                "status": task.status.as_str(),
                "reason": "新任务，无进度痕迹",
            }));
            continue;
        }

        // 剩下的需要评估
        let project = find_project(&guard, project_id)?.clone();
        let rule_decision = evaluate_reassess_by_rules(task);
        needs_reassess.push(serde_json::json!({
            "task_id": task.id,
            "title": task.title,
            "status": task.status.as_str(),
            "rule_decision": rule_decision,
            "reassess_prompt": prompt::compose_task_reassess_prompt(
                task,
                &project,
                &sibling_tasks,
                latest_scan.as_ref(),
                &project_constraints,
                &recent_chat,
            ),
        }));
    }

    Ok(Json(serde_json::json!({
        "project_id": project_id,
        "needs_reassess": needs_reassess,
        "skipped": skipped,
        "summary": {
            "total_needs_reassess": needs_reassess.len(),
            "total_skipped": skipped.len(),
        },
    })))
}

/// 基于规则引擎给出重评估建议（不依赖 Agent，作为快速参考）
fn evaluate_reassess_by_rules(task: &Task) -> serde_json::Value {
    let has_completion_evidence = task_has_completion_evidence(task);
    let has_thread = task
        .runtime
        .as_ref()
        .and_then(|rt| rt.thread_id.as_deref())
        .is_some();
    let thread_not_found = task
        .runtime
        .as_ref()
        .and_then(|rt| rt.last_error.as_deref())
        .is_some_and(|e| e.to_ascii_lowercase().contains("thread not found"));
    let recovery_loop_count = task
        .activities
        .iter()
        .filter(|a| {
            matches!(
                a.kind.as_str(),
                "task.watchdog_recovered" | "task.auto_retry_queued" | "task.runtime_session_lost"
            )
        })
        .count();
    let has_cancel_signal = task.activities.iter().any(|a| {
        a.kind == "task.canceled" || crate::prompt::contains_scope_change_signal(&a.message)
    });

    let (decision, confidence, reason) = if has_completion_evidence {
        ("DONE", 0.9, "运行输出中存在明确的完成证据")
    } else if has_cancel_signal {
        ("CANCELED", 0.8, "最近活动中存在取消/不做信号")
    } else if recovery_loop_count >= 3 || thread_not_found {
        (
            "REOPEN",
            0.85,
            "恢复循环已耗尽或 thread 不可用，需清理后重新执行",
        )
    } else if has_thread && matches!(task.status, TaskStatus::Paused) {
        (
            "RESTART",
            0.8,
            "任务有可用 thread 且目标仍然有效，可以恢复继续",
        )
    } else if matches!(task.status, TaskStatus::Paused) {
        (
            "REOPEN",
            0.7,
            "任务暂停但缺少可恢复的 thread，建议重新放回队列",
        )
    } else {
        ("MANUAL_REVIEW", 0.5, "证据不足，建议人工复核")
    };

    serde_json::json!({
        "decision": decision,
        "confidence": confidence,
        "reason": reason,
        "resume_hint": if decision == "RESTART" {
            Some(format!(
                "系统重新评估后判定任务可以继续推进。请先回顾当前进展和工作区状态，再继续完成。",
            ))
        } else {
            None::<String>
        },
    })
}

// ─── 决策收件箱 API ─────────────────────────────────────────────────────────

/// GET /decisions — 获取所有待处理决策
pub(crate) async fn list_decisions(State(state): State<AppState>) -> Json<serde_json::Value> {
    let guard = state.inner.lock().await;
    let pending: Vec<_> = guard
        .decisions
        .iter()
        .filter(|d| matches!(d.status, platform_core::DecisionStatus::Pending))
        .cloned()
        .map(|mut decision| {
            refresh_pending_reassess_decision(&mut decision, &guard);
            decision
        })
        .collect();
    let resolved_count = guard
        .decisions
        .iter()
        .filter(|d| !matches!(d.status, platform_core::DecisionStatus::Pending))
        .count();
    Json(serde_json::json!({
        "pending": pending,
        "pending_count": pending.len(),
        "resolved_count": resolved_count,
    }))
}

/// POST /decisions/{decision_id}/resolve — 处理一个决策
pub(crate) async fn resolve_decision(
    AxumPath(decision_id): AxumPath<Uuid>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<serde_json::Value>,
) -> AppResult<Json<serde_json::Value>> {
    let chosen = request
        .get("chosen_option")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "必须指定 chosen_option".into()))?
        .to_string();

    let mut guard = state.inner.lock().await;
    let current_user = resolve_current_user(&guard, &headers);
    let decision = guard
        .decisions
        .iter_mut()
        .find(|d| d.id == decision_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到决策".into()))?;

    if !matches!(decision.status, platform_core::DecisionStatus::Pending) {
        return Err((StatusCode::CONFLICT, "该决策已处理".into()));
    }

    decision.status = platform_core::DecisionStatus::Resolved;
    decision.chosen_option = Some(chosen.clone());
    decision.resolved_at = Some(timestamp_string());
    decision.resolved_by = current_user.as_ref().map(|u| u.id);

    let decision_clone = decision.clone();
    drop(guard);

    // 根据决策类型和选择执行对应动作
    apply_decision_effect(&state, &decision_clone, &chosen).await;
    crate::state::persist_state(&state).await?;

    Ok(Json(serde_json::json!({
        "decision_id": decision_id,
        "chosen_option": chosen,
        "status": "resolved",
    })))
}

/// POST /decisions/batch-resolve — 批量处理决策
pub(crate) async fn batch_resolve_decisions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<serde_json::Value>,
) -> AppResult<Json<serde_json::Value>> {
    let mode = request
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("recommended");

    let min_confidence = request
        .get("min_confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.8) as f32;

    let mut resolved = Vec::new();
    let mut skipped = Vec::new();

    let decisions_to_process: Vec<_> = {
        let guard = state.inner.lock().await;
        let current_user = resolve_current_user(&guard, &headers);
        guard
            .decisions
            .iter()
            .filter(|d| matches!(d.status, platform_core::DecisionStatus::Pending))
            .filter(|d| {
                mode == "all"
                    || (mode == "recommended"
                        && d.recommended.is_some()
                        && d.confidence.unwrap_or(0.0) >= min_confidence)
            })
            .map(|d| {
                (
                    d.id,
                    d.recommended.clone(),
                    d.title.clone(),
                    current_user.as_ref().map(|u| u.id),
                )
            })
            .collect()
    };

    for (id, recommended, title, user_id) in decisions_to_process {
        let Some(chosen) = recommended else {
            skipped.push(serde_json::json!({"id": id, "title": title, "reason": "无推荐选项"}));
            continue;
        };

        let mut guard = state.inner.lock().await;
        if let Some(decision) = guard.decisions.iter_mut().find(|d| d.id == id) {
            decision.status = platform_core::DecisionStatus::Resolved;
            decision.chosen_option = Some(chosen.clone());
            decision.resolved_at = Some(timestamp_string());
            decision.resolved_by = user_id;
            let decision_clone = decision.clone();
            drop(guard);
            apply_decision_effect(&state, &decision_clone, &chosen).await;
            resolved.push(serde_json::json!({"id": id, "title": title, "chosen": chosen}));
        }
    }

    crate::state::persist_state(&state).await?;

    Ok(Json(serde_json::json!({
        "resolved": resolved,
        "skipped": skipped,
        "resolved_count": resolved.len(),
        "skipped_count": skipped.len(),
    })))
}

/// 根据决策的选择执行实际效果
pub(crate) async fn apply_decision_effect(
    state: &AppState,
    decision: &platform_core::DecisionCard,
    chosen: &str,
) {
    let Some(task_id) = decision.task_id else {
        return;
    };

    match (decision.kind, chosen) {
        (platform_core::DecisionKind::Approval, "approve") => {
            let mut guard = state.inner.lock().await;
            if let Ok(task) = find_task_mut(&mut guard, task_id) {
                if matches!(task.status, TaskStatus::ApprovalRequested) {
                    task.status = TaskStatus::Approved;
                    task.activities.push(platform_core::new_activity(
                        "task.approved",
                        "已通过决策收件箱批准",
                    ));
                }
            }
        }
        (platform_core::DecisionKind::Approval, "reject") => {
            let mut guard = state.inner.lock().await;
            if let Ok(task) = find_task_mut(&mut guard, task_id) {
                task.status = TaskStatus::Open;
                task.activities.push(platform_core::new_activity(
                    "task.approval_denied",
                    "审批被拒绝，任务回到待处理队列",
                ));
            }
        }
        (platform_core::DecisionKind::Acceptance, "accept") => {
            let mut guard = state.inner.lock().await;
            if let Ok(task) = find_task_mut(&mut guard, task_id) {
                if matches!(task.status, TaskStatus::PendingAcceptance) {
                    task.status = TaskStatus::Accepted;
                    task.activities.push(platform_core::new_activity(
                        "task.accepted",
                        "已通过决策收件箱验收",
                    ));
                }
            }
        }
        (platform_core::DecisionKind::Acceptance, "reject") => {
            let mut guard = state.inner.lock().await;
            if let Ok(task) = find_task_mut(&mut guard, task_id) {
                task.status = TaskStatus::Open;
                task.activities.push(platform_core::new_activity(
                    "task.acceptance_rejected",
                    "验收被拒绝，任务回到待处理队列",
                ));
            }
        }
        (platform_core::DecisionKind::Reassess, option) => {
            let new_status = match option {
                "done" => Some(TaskStatus::Done),
                "cancel" => Some(TaskStatus::Canceled),
                "restart" | "reopen" => Some(TaskStatus::Open),
                _ => None,
            };
            if let Some(status) = new_status {
                let mut guard = state.inner.lock().await;
                if let Ok(task) = find_task_mut(&mut guard, task_id) {
                    task.status = status;
                    task.claimed_by = None;
                    if option == "reopen" {
                        task.runtime = None;
                    }
                    task.activities.push(platform_core::new_activity(
                        "task.decision_applied",
                        format!("决策收件箱处理：{option}"),
                    ));
                }
            }
        }
        _ => {}
    }
}

/// 创建一个决策卡片并投递到收件箱
#[allow(dead_code)]
pub(crate) fn post_decision(
    state: &mut crate::BoardState,
    project_id: Uuid,
    task_id: Option<Uuid>,
    kind: platform_core::DecisionKind,
    urgency: platform_core::DecisionUrgency,
    title: impl Into<String>,
    context: impl Into<String>,
    options: Vec<platform_core::DecisionOption>,
    recommended: Option<String>,
    confidence: Option<f32>,
) -> Uuid {
    let timeout_secs = match urgency {
        platform_core::DecisionUrgency::High => None,
        platform_core::DecisionUrgency::Medium => Some(7200),
        platform_core::DecisionUrgency::Low => Some(1800),
    };
    let timeout_action = recommended.clone();

    let id = Uuid::new_v4();
    state.decisions.push(platform_core::DecisionCard {
        id,
        project_id,
        task_id,
        kind,
        urgency,
        title: title.into(),
        context: context.into(),
        options,
        recommended,
        confidence,
        timeout_secs,
        timeout_action,
        status: platform_core::DecisionStatus::Pending,
        created_at: timestamp_string(),
        resolved_at: None,
        resolved_by: None,
        chosen_option: None,
    });
    id
}
