use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use platform_core::{
    infer_task_priority, merge_unique_tasks, new_activity, new_runtime_entry,
    seed_tasks_from_agents_markdown, seed_tasks_from_docs, Agent, CoordinationConflictPolicy,
    CoordinationIntentStatus, CoordinationWriteIntent, ExecutionSlotRecord, ExecutionSlotState,
    Project, Task, TaskPriority, TaskRunAttemptRecord, TaskRunRecord, TaskStateSnapshot,
    TaskStatus, User, WorkspaceLeaseRecord, WorkspaceLeaseState, WorkspaceRoot,
};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::completion::extract_task_completion_report;
use crate::models::*;
use crate::{AppResult, AppState, BoardState};

pub(crate) fn current_time_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

pub(crate) fn stale_timeout_nanos() -> u128 {
    current_time_nanos() + 300_000_000_000 - current_time_nanos()
}

fn latest_timestamp_string<I>(candidates: I) -> Option<String>
where
    I: IntoIterator<Item = Option<String>>,
{
    candidates
        .into_iter()
        .flatten()
        .max_by_key(|value| value.parse::<u128>().unwrap_or_default())
}

pub(crate) fn task_last_touch_nanos(task: &Task) -> Option<u128> {
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

pub(crate) fn task_status_label(status: TaskStatus) -> &'static str {
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

pub(crate) fn task_state_snapshot_needs_refresh(task: &Task) -> bool {
    task.state_snapshot.reason.is_none()
        || task.state_snapshot.evidence.is_empty()
        || task.state_snapshot.last_evaluated_by.is_none()
}

pub(crate) fn refresh_task_state_snapshot(task: &mut Task, evaluator: &str) {
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

pub(crate) fn priority_label(priority: TaskPriority) -> &'static str {
    match priority {
        TaskPriority::High => "高优先级",
        TaskPriority::Medium => "中优先级",
        TaskPriority::Low => "低优先级",
    }
}

pub(crate) fn default_state(runtime_mode: RuntimeMode, workspace_root: PathBuf) -> AppState {
    let store_path = state_store_path(&workspace_root);
    let persisted = load_or_initialize_state(&workspace_root, &store_path);

    AppState {
        inner: Arc::new(Mutex::new(BoardState {
            users: persisted.users,
            projects: persisted.projects,
            tasks: persisted.tasks,
            agents: persisted.agents,
            task_run_history: persisted.task_run_history,
            execution_slots: persisted.execution_slots,
            workspace_leases: persisted.workspace_leases,
            coordination_write_intents: persisted.coordination_write_intents,
            pending_questions: persisted.pending_questions,
            project_scans: persisted.project_scans,
            project_sessions: persisted.project_sessions,
            project_chat_messages: persisted.project_chat_messages,
            memory_items: persisted.memory_items,
            memory_revisions: persisted.memory_revisions,
            memory_tags: persisted.memory_tags,
            memory_edges: persisted.memory_edges,
            decisions: persisted.decisions,
        })),
        runtime_mode,
        runtime_sessions: Arc::new(Mutex::new(HashMap::new())),
        store_path,
    }
}

pub(crate) fn default_projects(workspace_root: &Path) -> Vec<Project> {
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

pub(crate) fn default_users() -> Vec<User> {
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

pub(crate) fn default_agents(users: &[User]) -> Vec<Agent> {
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

pub(crate) fn state_store_path(workspace_root: &Path) -> PathBuf {
    #[cfg(test)]
    {
        let _ = workspace_root;
        return std::env::temp_dir()
            .join(format!("spotlight-server-state-{}.json", Uuid::new_v4()));
    }

    #[cfg(not(test))]
    workspace_root.join(".spotlight").join("server-state.json")
}

pub(crate) fn load_or_initialize_state(workspace_root: &Path, store_path: &Path) -> PersistedState {
    if let Ok(content) = std::fs::read_to_string(store_path) {
        if let Ok(mut state) = serde_json::from_str::<PersistedState>(&content) {
            if normalize_persisted_state(&mut state) {
                let _ = persist_state_to_path(store_path, &state);
            }
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
    let mut state = state;
    let _ = normalize_persisted_state(&mut state);
    let _ = persist_state_to_path(store_path, &state);
    state
}

pub(crate) fn normalize_persisted_state(state: &mut PersistedState) -> bool {
    let mut changed = false;

    for task in &mut state.tasks {
        if task.priority.is_none() {
            if let Some(priority) = infer_task_priority(&task.title) {
                task.priority = Some(priority);
                task.activities.push(new_activity(
                    "task.priority_inferred",
                    format!("系统按版本阶段补齐默认优先级：{}", priority_label(priority)),
                ));
                changed = true;
            }
        }

        if should_recover_active_task_as_paused(task) {
            task.status = TaskStatus::Paused;
            task.claimed_by = None;
            if let Some(runtime) = task.runtime.as_mut() {
                runtime.active_turn_id = None;
                runtime.last_error.get_or_insert_with(|| {
                    "服务端启动时发现任务仍被标记为执行中，但本地运行会话不存在，已转为可恢复状态。"
                        .into()
                });
            }
            task.activities.push(new_activity(
                "task.state_normalized",
                "服务端启动时发现任务仍被标记为执行中，但本地运行会话不存在，已自动归一化为 PAUSED，等待恢复或人工处理。",
            ));
            changed = true;
        }

        if should_recover_task_as_done(task) {
            task.status = TaskStatus::Done;
            task.claimed_by = None;
            if let Some(runtime) = task.runtime.as_mut() {
                runtime.active_turn_id = None;
                if runtime
                    .last_error
                    .as_deref()
                    .is_some_and(|message| message.contains("thread not found"))
                {
                    runtime.last_error = None;
                }
            }
            task.activities.push(new_activity(
                "task.state_normalized",
                "服务端启动时检测到明确完成证据，但任务状态未落到 DONE，已自动归一化为 DONE。",
            ));
            changed = true;
        }

        if should_clear_inactive_turn(task) {
            if let Some(runtime) = task.runtime.as_mut() {
                runtime.active_turn_id = None;
            }
            task.activities.push(new_activity(
                "task.state_normalized",
                "服务端启动时检测到任务已不在 RUNNING，但仍残留 active_turn_id，已自动清理。",
            ));
            changed = true;
        }

        if should_recover_open_task_as_paused(task) {
            task.status = TaskStatus::Paused;
            task.claimed_by = None;
            if let Some(runtime) = task.runtime.as_mut() {
                runtime.last_error.get_or_insert_with(|| {
                    "服务端启动时检测到任务已有运行痕迹却回退为 OPEN，已自动归一化为 PAUSED。"
                        .into()
                });
            }
            task.activities.push(new_activity(
                "task.state_normalized",
                "服务端启动时检测到任务已有运行痕迹却回退为 OPEN，已自动归一化为 PAUSED，等待恢复或人工处理。",
            ));
            changed = true;
        }

        if task_state_snapshot_needs_refresh(task) {
            refresh_task_state_snapshot(task, "server.load.normalize");
            changed = true;
        }
    }

    let active_task_ids = state
        .tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Claimed | TaskStatus::Running))
        .map(|task| task.id)
        .collect::<HashSet<_>>();

    for agent in &mut state.agents {
        if let Some(task_id) = agent.current_task_id {
            if !active_task_ids.contains(&task_id) {
                agent.current_task_id = None;
                agent.status = "空闲".into();
                agent.last_action = "服务启动时释放了失效的任务占用".into();
                changed = true;
            }
        }
    }

    if restore_primary_auto_agent_if_needed(state) {
        changed = true;
    }

    if normalize_execution_coordination_state(state) {
        changed = true;
    }

    changed
}

fn restore_primary_auto_agent_if_needed(state: &mut PersistedState) -> bool {
    if state.agents.iter().any(|agent| agent.auto_mode) {
        return false;
    }

    if state
        .tasks
        .iter()
        .any(|task| matches!(task.status, TaskStatus::Claimed | TaskStatus::Running))
    {
        return false;
    }

    if !state
        .tasks
        .iter()
        .any(|task| matches!(task.status, TaskStatus::Open | TaskStatus::Paused))
    {
        return false;
    }

    let agent_index = state
        .agents
        .iter()
        .position(|agent| agent.provider == "codex")
        .or_else(|| (!state.agents.is_empty()).then_some(0));
    let Some(agent_index) = agent_index else {
        return false;
    };
    let agent = &mut state.agents[agent_index];

    agent.auto_mode = true;
    agent.status = "空闲".into();
    agent.last_action = "服务启动时检测到无人自动执行，已恢复主 Agent 的自动模式".into();
    true
}

fn should_recover_active_task_as_paused(task: &Task) -> bool {
    matches!(task.status, TaskStatus::Running | TaskStatus::Claimed)
        && task.claimed_by.is_some()
        && task_has_progress_evidence(task)
}

fn should_recover_task_as_done(task: &Task) -> bool {
    !matches!(task.status, TaskStatus::Done | TaskStatus::Canceled)
        && task_has_completion_evidence(task)
}

fn should_clear_inactive_turn(task: &Task) -> bool {
    !matches!(task.status, TaskStatus::Running)
        && task
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.active_turn_id.as_deref())
            .is_some()
}

fn should_recover_open_task_as_paused(task: &Task) -> bool {
    if !matches!(task.status, TaskStatus::Open) || task.claimed_by.is_some() {
        return false;
    }

    if task
        .activities
        .iter()
        .any(|activity| activity.kind == "task.reassessed_reopened")
    {
        return false;
    }

    task_has_progress_evidence(task)
}

fn normalize_execution_coordination_state(state: &mut PersistedState) -> bool {
    let mut changed = false;
    let task_ids = state
        .tasks
        .iter()
        .map(|task| task.id)
        .collect::<HashSet<_>>();

    let slot_count_before = state.execution_slots.len();
    state
        .execution_slots
        .retain(|slot| task_ids.contains(&slot.task_id));
    if state.execution_slots.len() != slot_count_before {
        changed = true;
    }

    let slot_ids = state
        .execution_slots
        .iter()
        .map(|slot| slot.id)
        .collect::<HashSet<_>>();
    let lease_count_before = state.workspace_leases.len();
    state
        .workspace_leases
        .retain(|lease| slot_ids.contains(&lease.slot_id));
    if state.workspace_leases.len() != lease_count_before {
        changed = true;
    }

    let tasks = state.tasks.clone();
    let projects = state.projects.clone();

    for task in tasks {
        let run_context = {
            let runs = state.task_run_history.entry(task.id).or_default();
            if runs.is_empty() && task_has_progress_evidence(&task) {
                runs.push(build_legacy_task_run_record_for_state(&task, 1));
                changed = true;
            }

            if let Some(run) = runs.last_mut() {
                if normalize_task_run_entry_for_task(&task, run) {
                    changed = true;
                }

                Some((
                    run.primary_workspace_path.clone(),
                    run.id,
                    run.started_at.clone(),
                    run.ended_at.clone(),
                    run.started_by_agent_id.or(task.claimed_by),
                    run.last_error.clone(),
                    run.execution_slot_id,
                ))
            } else {
                None
            }
        };

        let Some((
            preferred_workspace_path,
            run_id,
            run_started_at,
            run_ended_at,
            run_started_by_agent_id,
            run_last_error,
            run_execution_slot_id,
        )) = run_context
        else {
            continue;
        };
        let desired_slot_state = execution_slot_state_for_task_status(task.status);
        let task_last_touch_at = task_last_touch_nanos(&task).map(|value| value.to_string());
        let slot_last_error = task
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.last_error.clone())
            .or(run_last_error.clone());
        let lane_key =
            coordination_lane_key_for_task(&projects, &task, preferred_workspace_path.as_deref());
        let workspace_path = coordination_workspace_path_for_task(
            &projects,
            &task,
            preferred_workspace_path.as_deref(),
        );
        let workspace_root_id = workspace_path
            .as_deref()
            .and_then(|path| coordination_workspace_root_id_for_task(&projects, &task, path));

        let slot_index = run_execution_slot_id
            .and_then(|slot_id| {
                state
                    .execution_slots
                    .iter()
                    .position(|slot| slot.id == slot_id && slot.task_id == task.id)
            })
            .or_else(|| {
                state
                    .execution_slots
                    .iter()
                    .rposition(|slot| slot.task_id == task.id && slot.task_run_id == Some(run_id))
            })
            .or_else(|| {
                state
                    .execution_slots
                    .iter()
                    .rposition(|slot| slot.task_id == task.id)
            });

        let (slot_id, slot_index) = if let Some(slot_index) = slot_index {
            (state.execution_slots[slot_index].id, slot_index)
        } else {
            let slot_id = Uuid::new_v4();
            state.execution_slots.push(ExecutionSlotRecord {
                id: slot_id,
                project_id: task.project_id,
                task_id: task.id,
                task_run_id: Some(run_id),
                assigned_agent_id: run_started_by_agent_id,
                workspace_lease_id: None,
                lane_key: Some(lane_key.clone()),
                state: desired_slot_state,
                opened_at: run_started_at.clone(),
                updated_at: latest_timestamp_string([
                    task_last_touch_at.clone(),
                    run_ended_at.clone(),
                    Some(run_started_at.clone()),
                ])
                .unwrap_or_else(|| run_started_at.clone()),
                last_heartbeat_at: (!matches!(
                    desired_slot_state,
                    ExecutionSlotState::Released | ExecutionSlotState::Failed
                ))
                .then(|| {
                    latest_timestamp_string([
                        task_last_touch_at.clone(),
                        run_ended_at.clone(),
                        Some(run_started_at.clone()),
                    ])
                    .unwrap_or_else(|| run_started_at.clone())
                }),
                released_at: matches!(
                    desired_slot_state,
                    ExecutionSlotState::Released | ExecutionSlotState::Failed
                )
                .then(|| {
                    latest_timestamp_string([
                        task_last_touch_at.clone(),
                        run_ended_at.clone(),
                        Some(run_started_at.clone()),
                    ])
                    .unwrap_or_else(|| run_started_at.clone())
                }),
                last_error: match desired_slot_state {
                    ExecutionSlotState::Released => None,
                    _ => slot_last_error.clone(),
                },
            });
            append_coordination_backfill_intent(
                state,
                "execution_slot",
                slot_id,
                "slot.backfill",
                Some(slot_id),
                Some("服务端升级时为历史 task run 回填 execution slot".into()),
                Some("已补建 execution slot".into()),
            );
            changed = true;
            (slot_id, state.execution_slots.len() - 1)
        };

        {
            let slot = &mut state.execution_slots[slot_index];
            if slot.project_id != task.project_id {
                slot.project_id = task.project_id;
                changed = true;
            }
            if slot.task_id != task.id {
                slot.task_id = task.id;
                changed = true;
            }
            if slot.task_run_id != Some(run_id) {
                slot.task_run_id = Some(run_id);
                changed = true;
            }
            if slot.assigned_agent_id != run_started_by_agent_id {
                slot.assigned_agent_id = run_started_by_agent_id;
                changed = true;
            }
            if slot.lane_key.as_deref() != Some(lane_key.as_str()) {
                slot.lane_key = Some(lane_key.clone());
                changed = true;
            }
            if slot.state != desired_slot_state {
                slot.state = desired_slot_state;
                changed = true;
            }
            if slot.opened_at.is_empty() {
                slot.opened_at = run_started_at.clone();
                changed = true;
            }
            let updated_at = latest_timestamp_string([
                task_last_touch_at.clone(),
                run_ended_at.clone(),
                Some(run_started_at.clone()),
            ])
            .unwrap_or_else(|| run_started_at.clone());
            if slot.updated_at != updated_at {
                slot.updated_at = updated_at.clone();
                changed = true;
            }
            let desired_heartbeat = (!matches!(
                desired_slot_state,
                ExecutionSlotState::Released | ExecutionSlotState::Failed
            ))
            .then(|| {
                latest_timestamp_string([task_last_touch_at.clone(), Some(updated_at.clone())])
                    .unwrap_or_else(|| updated_at.clone())
            });
            if slot.last_heartbeat_at != desired_heartbeat {
                slot.last_heartbeat_at = desired_heartbeat;
                changed = true;
            }
            let desired_released_at = matches!(
                desired_slot_state,
                ExecutionSlotState::Released | ExecutionSlotState::Failed
            )
            .then(|| {
                latest_timestamp_string([
                    task_last_touch_at.clone(),
                    run_ended_at.clone(),
                    Some(updated_at.clone()),
                ])
                .unwrap_or(updated_at.clone())
            });
            if slot.released_at != desired_released_at {
                slot.released_at = desired_released_at;
                changed = true;
            }
            let desired_last_error = match desired_slot_state {
                ExecutionSlotState::Released => None,
                _ => slot_last_error.clone(),
            };
            if slot.last_error != desired_last_error {
                slot.last_error = desired_last_error;
                changed = true;
            }
        }

        {
            let runs = state.task_run_history.entry(task.id).or_default();
            if let Some(run) = runs.last_mut() {
                if run.execution_slot_id != Some(slot_id) {
                    run.execution_slot_id = Some(slot_id);
                    changed = true;
                }
            }
        }

        let Some(workspace_path) = workspace_path else {
            continue;
        };

        let existing_lease_id = state.execution_slots[slot_index].workspace_lease_id;
        let lease_index = existing_lease_id
            .and_then(|lease_id| {
                state
                    .workspace_leases
                    .iter()
                    .position(|lease| lease.id == lease_id)
            })
            .or_else(|| {
                state
                    .workspace_leases
                    .iter()
                    .rposition(|lease| lease.slot_id == slot_id)
            });

        let desired_lease_state = match desired_slot_state {
            ExecutionSlotState::Released | ExecutionSlotState::Failed => {
                WorkspaceLeaseState::Released
            }
            _ => WorkspaceLeaseState::Active,
        };

        let lease_id = if let Some(lease_index) = lease_index {
            let lease_id = state.workspace_leases[lease_index].id;
            let lease = &mut state.workspace_leases[lease_index];
            if lease.project_id != task.project_id {
                lease.project_id = task.project_id;
                changed = true;
            }
            if lease.slot_id != slot_id {
                lease.slot_id = slot_id;
                changed = true;
            }
            if lease.workspace_root_id != workspace_root_id {
                lease.workspace_root_id = workspace_root_id;
                changed = true;
            }
            if lease.workspace_path != workspace_path {
                lease.workspace_path = workspace_path.clone();
                changed = true;
            }
            if lease.lane_key != lane_key {
                lease.lane_key = lane_key.clone();
                changed = true;
            }
            if lease.state != desired_lease_state {
                lease.state = desired_lease_state;
                changed = true;
            }
            if lease.acquired_at.is_empty() {
                lease.acquired_at = run_started_at.clone();
                changed = true;
            }
            let desired_released_at = matches!(desired_lease_state, WorkspaceLeaseState::Released)
                .then(|| {
                    run_ended_at
                        .clone()
                        .unwrap_or_else(|| run_started_at.clone())
                });
            if lease.released_at != desired_released_at {
                lease.released_at = desired_released_at;
                changed = true;
            }
            let desired_release_reason = match desired_lease_state {
                WorkspaceLeaseState::Released => slot_last_error
                    .clone()
                    .or_else(|| Some("task run 已进入终态".into())),
                _ => None,
            };
            if lease.release_reason != desired_release_reason {
                lease.release_reason = desired_release_reason;
                changed = true;
            }
            lease_id
        } else {
            let lease_id = Uuid::new_v4();
            state.workspace_leases.push(WorkspaceLeaseRecord {
                id: lease_id,
                project_id: task.project_id,
                slot_id,
                workspace_root_id,
                workspace_path: workspace_path.clone(),
                lane_key: lane_key.clone(),
                state: desired_lease_state,
                acquired_at: run_started_at.clone(),
                released_at: matches!(desired_lease_state, WorkspaceLeaseState::Released).then(
                    || {
                        run_ended_at
                            .clone()
                            .unwrap_or_else(|| run_started_at.clone())
                    },
                ),
                release_reason: match desired_lease_state {
                    WorkspaceLeaseState::Released => slot_last_error
                        .clone()
                        .or_else(|| Some("task run 已进入终态".into())),
                    _ => None,
                },
            });
            append_coordination_backfill_intent(
                state,
                "workspace_lease",
                lease_id,
                "lease.backfill",
                Some(slot_id),
                Some("服务端升级时为历史 execution slot 回填 workspace lease".into()),
                Some("已补建 workspace lease".into()),
            );
            changed = true;
            lease_id
        };

        if state.execution_slots[slot_index].workspace_lease_id != Some(lease_id) {
            state.execution_slots[slot_index].workspace_lease_id = Some(lease_id);
            changed = true;
        }
    }

    let valid_slot_ids = state
        .execution_slots
        .iter()
        .map(|slot| slot.id)
        .collect::<HashSet<_>>();
    for slot in &mut state.execution_slots {
        if slot.workspace_lease_id.is_some_and(|lease_id| {
            !state
                .workspace_leases
                .iter()
                .any(|lease| lease.id == lease_id)
        }) {
            slot.workspace_lease_id = None;
            changed = true;
        }
    }

    let lease_count_before = state.workspace_leases.len();
    state
        .workspace_leases
        .retain(|lease| valid_slot_ids.contains(&lease.slot_id));
    if state.workspace_leases.len() != lease_count_before {
        changed = true;
    }

    changed
}

fn normalize_task_run_entry_for_task(task: &Task, run: &mut TaskRunRecord) -> bool {
    let mut changed = false;
    let expected_state = task_run_state_for_task_status(task.status);

    if run.state != expected_state {
        run.state = expected_state.into();
        changed = true;
    }

    if let Some(thread_id) = task
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.thread_id.as_deref())
    {
        if !run
            .session_threads
            .iter()
            .any(|existing| existing == thread_id)
        {
            run.session_threads.push(thread_id.to_string());
            changed = true;
        }
    }

    if task_run_is_terminal(expected_state) && run.ended_at.is_none() {
        run.ended_at = task
            .activities
            .last()
            .map(|activity| activity.at.clone())
            .or_else(|| Some(timestamp_string_for_state()));
        changed = true;
    }

    if run.attempts.is_empty() {
        run.attempts.push(TaskRunAttemptRecord {
            id: Uuid::new_v4(),
            attempt_number: 1,
            trigger_kind: "legacy_backfill".into(),
            status: expected_state.into(),
            prompt: task.description.clone(),
            started_at: run.started_at.clone(),
            ended_at: run.ended_at.clone(),
            thread_id: task
                .runtime
                .as_ref()
                .and_then(|runtime| runtime.thread_id.clone()),
            turn_id: task
                .runtime
                .as_ref()
                .and_then(|runtime| runtime.active_turn_id.clone()),
            error_summary: task
                .runtime
                .as_ref()
                .and_then(|runtime| runtime.last_error.clone()),
        });
        changed = true;
    } else if let Some(attempt) = run.attempts.last_mut() {
        if attempt.status != expected_state {
            attempt.status = expected_state.into();
            changed = true;
        }
        if let Some(runtime) = task.runtime.as_ref() {
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
            attempt.ended_at = run
                .ended_at
                .clone()
                .or_else(|| Some(timestamp_string_for_state()));
            changed = true;
        }
    }

    changed
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

fn build_legacy_task_run_record_for_state(task: &Task, run_number: u32) -> TaskRunRecord {
    let started_at = task
        .activities
        .first()
        .map(|activity| activity.at.clone())
        .unwrap_or_else(timestamp_string_for_state);
    let ended_at = (!matches!(task.status, TaskStatus::Running | TaskStatus::Claimed)).then(|| {
        task.activities
            .last()
            .map(|activity| activity.at.clone())
            .unwrap_or_else(timestamp_string_for_state)
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
        execution_slot_id: None,
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
                .or_else(|| Some(timestamp_string_for_state())),
            thread_id,
            turn_id,
            error_summary: last_error.clone(),
        }],
        log: runtime.map(|item| item.log.clone()).unwrap_or_default(),
        last_error,
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

pub(crate) fn record_task_run_start(
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
    let now = timestamp_string_for_state();

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
        execution_slot_id: None,
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

pub(crate) fn record_task_run_transition(
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

    let now = timestamp_string_for_state();
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

pub(crate) fn normalize_task_run_history(state: &mut PersistedState) -> bool {
    let mut changed = false;
    let tasks = state.tasks.clone();

    for task in tasks {
        let runs = state.task_run_history.entry(task.id).or_default();
        if runs.is_empty() && task_has_progress_evidence(&task) {
            runs.push(build_legacy_task_run_record_for_state(&task, 1));
            changed = true;
        }

        let Some(run) = runs.last_mut() else {
            continue;
        };

        if normalize_task_run_entry_for_task(&task, run) {
            changed = true;
        }
    }

    changed
}

fn normalize_coordination_workspace_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

fn coordination_workspace_path_for_task(
    projects: &[Project],
    task: &Task,
    preferred_workspace_path: Option<&str>,
) -> Option<String> {
    preferred_workspace_path.map(str::to_string).or_else(|| {
        projects
            .iter()
            .find(|project| project.id == task.project_id)
            .and_then(Project::primary_workspace)
            .map(|workspace| workspace.path.clone())
    })
}

fn coordination_workspace_root_id_for_task(
    projects: &[Project],
    task: &Task,
    workspace_path: &str,
) -> Option<Uuid> {
    let normalized = normalize_coordination_workspace_path(workspace_path);
    projects
        .iter()
        .find(|project| project.id == task.project_id)
        .and_then(|project| {
            project.workspace_roots.iter().find(|workspace| {
                normalize_coordination_workspace_path(&workspace.path) == normalized
            })
        })
        .map(|workspace| workspace.id)
}

fn coordination_lane_key_for_task(
    projects: &[Project],
    task: &Task,
    preferred_workspace_path: Option<&str>,
) -> String {
    coordination_workspace_path_for_task(projects, task, preferred_workspace_path)
        .map(|path| format!("workspace:{}", normalize_coordination_workspace_path(&path)))
        .unwrap_or_else(|| format!("project:{}", task.project_id))
}

fn execution_slot_state_for_task_status(status: TaskStatus) -> ExecutionSlotState {
    match status {
        TaskStatus::Running | TaskStatus::Claimed => ExecutionSlotState::Running,
        TaskStatus::Paused => ExecutionSlotState::Paused,
        TaskStatus::Failed => ExecutionSlotState::Failed,
        TaskStatus::Done | TaskStatus::Accepted | TaskStatus::Canceled => {
            ExecutionSlotState::Released
        }
        _ => ExecutionSlotState::Pending,
    }
}

fn coordination_intent_resource_key(resource_kind: &str, resource_id: Uuid) -> String {
    format!("{resource_kind}:{resource_id}")
}

fn append_coordination_backfill_intent(
    state: &mut PersistedState,
    resource_kind: &str,
    resource_id: Uuid,
    action_kind: &str,
    proposed_by_slot_id: Option<Uuid>,
    justification: Option<String>,
    resolution_note: Option<String>,
) {
    state
        .coordination_write_intents
        .push(CoordinationWriteIntent {
            id: Uuid::new_v4(),
            resource_kind: resource_kind.into(),
            resource_key: coordination_intent_resource_key(resource_kind, resource_id),
            action_kind: action_kind.into(),
            conflict_policy: CoordinationConflictPolicy::FirstCommitWins,
            status: CoordinationIntentStatus::Committed,
            proposed_by_agent_id: None,
            proposed_by_slot_id,
            justification,
            proposed_at: timestamp_string_for_state(),
            resolved_at: Some(timestamp_string_for_state()),
            resolution_note,
        });
}

fn timestamp_string_for_state() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

pub(crate) fn seed_tasks_from_agents_file(project: &Project) -> Vec<Task> {
    let Some(path) = crate::task_ops::primary_workspace_path(project).ok() else {
        return Vec::new();
    };
    std::fs::read_to_string(path.join("AGENTS.md"))
        .map(|content| seed_tasks_from_agents_markdown(&content, project.id))
        .unwrap_or_default()
}

pub(crate) fn persist_state_to_path(
    store_path: &Path,
    state: &PersistedState,
) -> Result<(), String> {
    if let Some(parent) = store_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建状态目录失败：{error}"))?;
    }
    let content = serde_json::to_string_pretty(state)
        .map_err(|error| format!("序列化服务端状态失败：{error}"))?;
    std::fs::write(store_path, content).map_err(|error| format!("写入状态文件失败：{error}"))
}

pub(crate) async fn persist_state(state: &AppState) -> AppResult<()> {
    let (mut persisted, store_path) = {
        let guard = state.inner.lock().await;
        (
            PersistedState {
                users: guard.users.clone(),
                projects: guard.projects.clone(),
                tasks: guard.tasks.clone(),
                agents: guard.agents.clone(),
                task_run_history: guard.task_run_history.clone(),
                execution_slots: guard.execution_slots.clone(),
                workspace_leases: guard.workspace_leases.clone(),
                coordination_write_intents: guard.coordination_write_intents.clone(),
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
        .map_err(|message| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, message))
}
