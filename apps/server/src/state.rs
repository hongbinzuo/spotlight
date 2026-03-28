use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use platform_core::{
    infer_task_priority, merge_unique_tasks, new_activity, seed_tasks_from_agents_markdown,
    seed_tasks_from_docs, Agent, Project, Task, TaskStatus, User, WorkspaceRoot,
};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::models::*;
use crate::{
    priority_label, refresh_task_state_snapshot, task_has_completion_evidence,
    task_has_progress_evidence, task_state_snapshot_needs_refresh, AppResult, AppState, BoardState,
};

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

    persist_state_to_path(&store_path, &persisted)
        .map_err(|message| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, message))
}
