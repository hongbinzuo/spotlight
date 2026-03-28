use std::collections::{HashMap, HashSet};

use axum::http::{header, HeaderMap};
use platform_core::{Agent, BoardSnapshot, PendingQuestion, Task, TaskStatus, User};
use uuid::Uuid;

use crate::models::*;
use crate::{
    refresh_task_state_snapshot, AppResult, BoardState, BOARD_MESSAGE_CHAR_LIMIT,
    BOARD_TASK_ACTIVITY_LIMIT, BOARD_TASK_RUNTIME_LOG_LIMIT,
};

pub(crate) fn snapshot_from_state(state: &BoardState) -> BoardSnapshot {
    snapshot_from_state_with_user(state, state.users.first().cloned())
}

pub(crate) fn snapshot_from_state_with_user(
    state: &BoardState,
    current_user: Option<User>,
) -> BoardSnapshot {
    BoardSnapshot {
        current_user,
        users: state.users.clone(),
        projects: state.projects.clone(),
        tasks: state.tasks.iter().map(board_snapshot_task).collect(),
        agents: state.agents.clone(),
        task_run_history: state.task_run_history.clone(),
        pending_questions: state.pending_questions.clone(),
    }
}

fn board_snapshot_task(task: &Task) -> Task {
    let mut snapshot = task_with_state_snapshot(task, "server.board_snapshot");
    snapshot.activities = trim_task_activities(&task.activities, BOARD_TASK_ACTIVITY_LIMIT);
    snapshot.runtime = task.runtime.as_ref().map(board_snapshot_runtime);
    snapshot
}

pub(crate) fn task_with_state_snapshot(task: &Task, evaluator: &str) -> Task {
    let mut snapshot = task.clone();
    refresh_task_state_snapshot(&mut snapshot, evaluator);
    snapshot
}

fn board_snapshot_runtime(runtime: &platform_core::TaskRuntime) -> platform_core::TaskRuntime {
    let mut snapshot = runtime.clone();
    snapshot.log = trim_runtime_entries(&runtime.log, BOARD_TASK_RUNTIME_LOG_LIMIT);
    snapshot.last_error = runtime
        .last_error
        .as_deref()
        .map(|message| truncate_message(message, BOARD_MESSAGE_CHAR_LIMIT));
    snapshot
}

pub(crate) fn trim_task_activities(
    items: &[platform_core::TaskActivity],
    limit: usize,
) -> Vec<platform_core::TaskActivity> {
    trim_tail(items, limit)
        .into_iter()
        .map(|item| platform_core::TaskActivity {
            message: truncate_message(&item.message, BOARD_MESSAGE_CHAR_LIMIT),
            ..item
        })
        .collect()
}

pub(crate) fn trim_runtime_entries(
    items: &[platform_core::RuntimeLogEntry],
    limit: usize,
) -> Vec<platform_core::RuntimeLogEntry> {
    trim_tail(items, limit)
        .into_iter()
        .map(|item| platform_core::RuntimeLogEntry {
            message: truncate_message(&item.message, BOARD_MESSAGE_CHAR_LIMIT),
            ..item
        })
        .collect()
}

pub(crate) fn trim_tail<T: Clone>(items: &[T], limit: usize) -> Vec<T> {
    if items.len() <= limit {
        return items.to_vec();
    }
    items[items.len() - limit..].to_vec()
}

pub(crate) fn truncate_message(message: &str, limit: usize) -> String {
    let total_chars = message.chars().count();
    if total_chars <= limit {
        return message.to_string();
    }
    let truncated = message.chars().take(limit).collect::<String>();
    format!("{truncated}...(已截断，共 {total_chars} 字符)")
}

pub(crate) fn build_auth_cookie(user_id: &Uuid) -> String {
    format!("spotlight_user_id={user_id}; Path=/; HttpOnly; SameSite=Lax")
}

pub(crate) fn cookie_value<'a>(headers: &'a HeaderMap, key: &str) -> Option<&'a str> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie_header.split(';').find_map(|part| {
        let (cookie_key, cookie_value) = part.trim().split_once('=')?;
        (cookie_key == key).then_some(cookie_value.trim())
    })
}

pub(crate) fn resolve_current_user(state: &BoardState, headers: &HeaderMap) -> Option<User> {
    if let Some(raw_user_id) = cookie_value(headers, "spotlight_user_id") {
        if let Ok(user_id) = Uuid::parse_str(raw_user_id) {
            if let Some(user) = state.users.iter().find(|user| user.id == user_id) {
                return Some(user.clone());
            }
        }
    }

    state.users.first().cloned()
}

pub(crate) fn project_context_snapshot(
    state: &BoardState,
    project_id: Uuid,
) -> AppResult<ProjectContextSnapshot> {
    let project = crate::task_ops::find_project(state, project_id)?;
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

pub(crate) fn project_summary_snapshot(
    state: &BoardState,
    project_id: Uuid,
) -> AppResult<ProjectSummarySnapshot> {
    let project = crate::task_ops::find_project(state, project_id)?;
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
        generated_at: crate::handlers::timestamp_string(),
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

pub(crate) fn project_task_status_counts(tasks: &[&Task]) -> ProjectTaskStatusCounts {
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

pub(crate) fn project_agent_summary(
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

pub(crate) fn project_session_summary(sessions: &[&ProjectSession]) -> ProjectSessionSummary {
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

pub(crate) fn active_project_constraint_digests(
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

pub(crate) fn recent_task_summary_digests(
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

pub(crate) fn project_memory_snapshot(
    state: &BoardState,
    project_id: Uuid,
) -> ProjectMemorySnapshot {
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
