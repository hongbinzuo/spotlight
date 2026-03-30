use std::collections::HashSet;

use axum::http::StatusCode;
use platform_core::{new_activity, new_runtime_entry, Task, TaskRuntime, TaskStatus};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

use crate::git_ops::task_priority_order;
use crate::models::RuntimeMode;
use crate::runtime::{
    spawn_runtime_session_with_timeout, start_runtime_thread_with_timeout,
    start_runtime_turn_with_timeout,
};
use crate::state::{
    current_time_nanos, persist_state, record_task_run_start, record_task_run_transition,
    stale_timeout_nanos, task_has_completion_evidence, task_has_progress_evidence,
    task_last_touch_nanos,
};
use crate::task_ops::*;
use crate::{
    AppResult, AppState, BoardState, AUTO_MAINTENANCE_INTERVAL_SECS, TASK_STALE_TIMEOUT_SECS,
};

pub(crate) fn start_background_automation(state: AppState) {
    if !matches!(state.runtime_mode, RuntimeMode::RealCodex) {
        return;
    }

    tokio::spawn(async move {
        loop {
            if let Err((_, message)) = run_automation_cycle_once(&state).await {
                eprintln!("background automation cycle failed: {message}");
            }
            sleep(Duration::from_secs(AUTO_MAINTENANCE_INTERVAL_SECS)).await;
        }
    });
}

pub(crate) async fn run_automation_cycle_once(state: &AppState) -> AppResult<()> {
    let mut sessions_to_stop = recover_stale_runtime_tasks(state).await?;
    let parallel_sessions_to_stop = recover_parallel_active_tasks(state).await?;
    sessions_to_stop.extend(parallel_sessions_to_stop);
    stop_runtime_sessions(state, &sessions_to_stop).await;
    drive_auto_mode_agents(state).await;
    Ok(())
}

async fn recover_stale_runtime_tasks(state: &AppState) -> AppResult<Vec<Uuid>> {
    let active_runtime_task_ids = {
        let sessions = state.runtime_sessions.lock().await;
        sessions.keys().copied().collect::<HashSet<_>>()
    };
    let now_nanos = current_time_nanos();

    let sessions_to_stop = {
        let mut guard = state.inner.lock().await;
        reconcile_watchdog_state(&mut guard, &active_runtime_task_ids, now_nanos)
    };

    if !sessions_to_stop.is_empty() {
        persist_state(state).await?;
    }

    Ok(sessions_to_stop)
}

async fn recover_parallel_active_tasks(state: &AppState) -> AppResult<Vec<Uuid>> {
    let sessions_to_stop = {
        let mut guard = state.inner.lock().await;
        reconcile_parallel_active_tasks(&mut guard)
    };

    if !sessions_to_stop.is_empty() {
        persist_state(state).await?;
    }

    Ok(sessions_to_stop)
}

async fn stop_runtime_sessions(state: &AppState, task_ids: &[Uuid]) {
    for task_id in task_ids {
        let session = {
            let mut sessions = state.runtime_sessions.lock().await;
            sessions.remove(task_id)
        };
        if let Some(session) = session {
            session.shutdown().await;
        }
    }
}

async fn drive_auto_mode_agents(state: &AppState) {
    let auto_agents = {
        let guard = state.inner.lock().await;
        guard
            .agents
            .iter()
            .filter(|agent| agent.auto_mode && agent.current_task_id.is_none())
            .map(|agent| (agent.id, agent.name.clone(), agent.owner_user_id))
            .collect::<Vec<_>>()
    };

    for (agent_id, agent_name, owner_user_id) in auto_agents {
        if let Some(task_id) = {
            let guard = state.inner.lock().await;
            select_next_auto_resume_task_id(&guard.tasks, owner_user_id)
        } {
            if let Err((_, message)) = auto_resume_task(state, task_id, agent_id, &agent_name).await
            {
                let _ = reopen_task_for_auto_retry(state, task_id, agent_id, &message).await;
            }
            return;
        }

        // 第二步：尝试认领新任务（对有历史痕迹的任务过门禁）
        let claimed_task = {
            let mut guard = state.inner.lock().await;
            let claimed = auto_claim_next_task(&mut guard, agent_id).ok().flatten();
            claimed
        };
        if claimed_task.is_some() {
            let _ = persist_state(state).await;
        }

        if let Some(task) = claimed_task {
            if let Err((_, message)) = auto_start_task(state, task.id, agent_id, &agent_name).await
            {
                let _ = reopen_task_for_auto_retry(state, task.id, agent_id, &message).await;
            }
            return;
        } else {
            let _ = owner_user_id;
        }
    }
}

/// 处理超时的决策卡片：按 timeout_action 自动执行
#[allow(dead_code)]
async fn expire_timed_out_decisions(state: &AppState) {
    let now_secs: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let expired: Vec<(Uuid, String)> = {
        let guard = state.inner.lock().await;
        guard
            .decisions
            .iter()
            .filter(|d| matches!(d.status, platform_core::DecisionStatus::Pending))
            .filter(|d| {
                if let (Some(timeout_secs), Ok(created_secs)) =
                    (d.timeout_secs, d.created_at.parse::<u64>())
                {
                    now_secs.saturating_sub(created_secs) > timeout_secs
                } else {
                    false
                }
            })
            .filter_map(|d| {
                d.timeout_action
                    .as_ref()
                    .map(|action| (d.id, action.clone()))
            })
            .collect()
    };

    if expired.is_empty() {
        return;
    }

    for (decision_id, action) in &expired {
        let mut guard = state.inner.lock().await;
        if let Some(decision) = guard.decisions.iter_mut().find(|d| d.id == *decision_id) {
            decision.status = platform_core::DecisionStatus::Expired;
            decision.chosen_option = Some(action.clone());
            decision.resolved_at = Some(now_secs.to_string());
            let decision_clone = decision.clone();
            drop(guard);
            crate::handlers::apply_decision_effect(state, &decision_clone, action).await;
        }
    }

    let _ = crate::state::persist_state(state).await;
}

/// 策略扫描：定期对所有滞留任务做快速重评估
/// 高置信度结果直接执行，中低置信度投递到决策收件箱等人确认
#[allow(dead_code)]
async fn reassess_stale_tasks(state: &AppState) {
    let candidates: Vec<(Uuid, Uuid, String, String, f32)> = {
        let guard = state.inner.lock().await;
        guard
            .tasks
            .iter()
            .filter(|task| {
                matches!(task.status, TaskStatus::Paused)
                    && task.claimed_by.is_none()
                    && task_has_progress_evidence(task)
            })
            .map(|task| {
                let decision = quick_reassess_gate(task, &guard.tasks, task.project_id);
                let confidence = match decision.as_str() {
                    "DONE" => 0.9_f32,
                    "CANCELED" => 0.8,
                    "REOPEN" => 0.85,
                    "RESTART" => 0.8,
                    _ => 0.5,
                };
                (
                    task.id,
                    task.project_id,
                    task.title.clone(),
                    decision,
                    confidence,
                )
            })
            .filter(|(_, _, _, decision, _)| decision != "RESTART") // RESTART 让 auto-mode 处理
            .collect()
    };

    if candidates.is_empty() {
        return;
    }

    let mut guard = state.inner.lock().await;
    for (task_id, project_id, task_title, decision, confidence) in &candidates {
        // 高置信度 (>= 0.9) + 明确结论 → 直接执行，不打扰人
        if *confidence >= 0.9 && matches!(decision.as_str(), "DONE" | "CANCELED") {
            if let Ok(task) = find_task_mut(&mut guard, *task_id) {
                let new_status = if decision == "DONE" {
                    TaskStatus::Done
                } else {
                    TaskStatus::Canceled
                };
                task.status = new_status;
                task.claimed_by = None;
                task.activities.push(new_activity(
                    "task.reassess_auto_resolved",
                    format!("策略扫描自动处理（置信度 {confidence}）：{decision}"),
                ));
            }
            continue;
        }

        // 避免重复投递：检查是否已有相同任务的 pending 决策
        let already_posted = guard.decisions.iter().any(|d| {
            d.task_id == Some(*task_id)
                && d.kind == platform_core::DecisionKind::Reassess
                && matches!(d.status, platform_core::DecisionStatus::Pending)
        });
        if already_posted {
            continue;
        }

        // 中低置信度 → 投递到收件箱
        let _legacy_options = match decision.as_str() {
            "DONE" => vec![
                decision_option("done", "标记为已完成", "success"),
                decision_option("restart", "继续执行", "secondary"),
                decision_option("dismiss", "暂不处理", "secondary"),
            ],
            "CANCELED" => vec![
                decision_option("cancel", "确认撤销", "warn"),
                decision_option("reopen", "重新放回队列", "secondary"),
                decision_option("dismiss", "暂不处理", "secondary"),
            ],
            "REOPEN" => vec![
                decision_option("reopen", "清理后重新排队", "primary"),
                decision_option("cancel", "直接撤销", "warn"),
                decision_option("dismiss", "暂不处理", "secondary"),
            ],
            _ => vec![
                decision_option("restart", "恢复执行", "primary"),
                decision_option("reopen", "从头执行", "secondary"),
                decision_option("cancel", "撤销", "warn"),
            ],
        };

        let options = match decision.as_str() {
            "DONE" => vec![
                decision_option("done", "标记为已完成", "success"),
                decision_option("restart", "继续执行", "secondary"),
                decision_option("dismiss", "暂不处理", "secondary"),
            ],
            "CANCELED" => vec![
                decision_option("cancel", "确认撤销", "warn"),
                decision_option("reopen", "重新放回队列", "secondary"),
                decision_option("dismiss", "暂不处理", "secondary"),
            ],
            "REOPEN" => vec![
                decision_option("reopen", "清理后重新排队", "primary"),
                decision_option("cancel", "直接撤销", "warn"),
                decision_option("dismiss", "暂不处理", "secondary"),
            ],
            _ => vec![
                decision_option("restart", "恢复执行", "primary"),
                decision_option("reopen", "从头执行", "secondary"),
                decision_option("cancel", "撤销", "warn"),
            ],
        };

        let recommended = match decision.as_str() {
            "DONE" => Some("done".into()),
            "CANCELED" => Some("cancel".into()),
            "REOPEN" => Some("reopen".into()),
            "RESTART" => Some("restart".into()),
            _ => None,
        };

        crate::handlers::post_decision(
            &mut guard,
            *project_id,
            Some(*task_id),
            platform_core::DecisionKind::Reassess,
            platform_core::DecisionUrgency::Low,
            format!("任务重评估：{task_title}"),
            format!(
                "规则引擎判断该任务应 {decision}（置信度 {confidence}）。\n\
任务标题：{task_title}\n\
当前状态：已暂停",
            ),
            options,
            recommended,
            Some(*confidence),
        );
    }

    drop(guard);
    let _ = crate::state::persist_state(state).await;
}

#[allow(dead_code)]
fn decision_option(id: &str, label: &str, style: &str) -> platform_core::DecisionOption {
    platform_core::DecisionOption {
        id: id.into(),
        label: label.into(),
        style: style.into(),
        detail: None,
    }
}

/// 规则引擎快速重评估（不依赖 Agent，在自动循环中使用）
///
/// 检查三个维度：
/// 1. 任务自身是否有完成证据
/// 2. 同项目是否有重叠的已完成任务
/// 3. 是否有取消信号
pub(crate) fn quick_reassess_gate(task: &Task, all_tasks: &[Task], project_id: Uuid) -> String {
    // 维度 1：自身完成证据
    if task_has_completion_evidence(task) {
        return "DONE".into();
    }

    // 维度 2：取消信号
    let has_cancel_signal = task.activities.iter().any(|a| {
        a.kind == "task.canceled" || crate::prompt::contains_scope_change_signal(&a.message)
    });
    if has_cancel_signal {
        return "CANCELED".into();
    }

    // 维度 3：同项目已完成任务的工作重叠
    let sibling_done_titles: Vec<&str> = all_tasks
        .iter()
        .filter(|t| {
            t.project_id == project_id
                && t.id != task.id
                && matches!(t.status, TaskStatus::Done | TaskStatus::Accepted)
        })
        .map(|t| t.title.as_str())
        .collect();

    if !sibling_done_titles.is_empty() {
        let task_keywords = significant_task_keywords(&task.title);
        for done_title in &sibling_done_titles {
            let done_keywords = significant_task_keywords(done_title);
            let overlap = task_keywords
                .iter()
                .filter(|kw| done_keywords.contains(*kw))
                .count();
            // 2 个以上关键词重叠，且对方已完成，很可能工作已被覆盖
            if overlap >= 2 {
                return "DONE".into();
            }
        }
    }

    // 维度 4：恢复能力
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

    if recovery_loop_count >= 3 || thread_not_found {
        return "REOPEN".into();
    }
    if has_thread {
        return "RESTART".into();
    }

    if matches!(task.status, TaskStatus::Paused) {
        return "REOPEN".into();
    }

    "MANUAL_REVIEW".into()
}

/// 提取任务标题中的有效关键词（用于重叠检测）
fn significant_task_keywords(title: &str) -> Vec<String> {
    title
        .split(|c: char| {
            c.is_whitespace()
                || c == '/'
                || c == '['
                || c == ']'
                || c == '('
                || c == ')'
                || c == ','
                || c == '、'
                || c == '，'
        })
        .map(|w| w.trim().to_lowercase())
        .filter(|w| w.chars().count() >= 2)
        .collect()
}

/// 应用重评估结果：改变任务状态
#[allow(dead_code)]
async fn apply_reassess_decision(
    state: &AppState,
    task_id: Uuid,
    new_status: TaskStatus,
    message: &str,
) {
    let mut guard = state.inner.lock().await;
    if let Ok(task) = find_task_mut(&mut guard, task_id) {
        task.status = new_status;
        task.claimed_by = None;
        task.activities
            .push(new_activity("task.reassess_gate", message));
        if let Some(runtime) = task.runtime.as_mut() {
            runtime.active_turn_id = None;
        }
    }
    drop(guard);
    let _ = persist_state(state).await;
}

/// 应用重评估结果：重新开放（清理运行上下文）
#[allow(dead_code)]
async fn apply_reassess_reopen(state: &AppState, task_id: Uuid, message: &str) {
    let mut guard = state.inner.lock().await;
    if let Ok(task) = find_task_mut(&mut guard, task_id) {
        task.status = TaskStatus::Open;
        task.claimed_by = None;
        task.runtime = None;
        task.activities
            .push(new_activity("task.reassess_reopened", message));
    }
    drop(guard);
    let _ = persist_state(state).await;
}

pub(crate) fn reconcile_watchdog_state(
    state: &mut BoardState,
    active_runtime_task_ids: &HashSet<Uuid>,
    now_nanos: u128,
) -> Vec<Uuid> {
    let mut sessions_to_stop = Vec::new();

    for task in &mut state.tasks {
        let has_session = active_runtime_task_ids.contains(&task.id);
        let Some(reason) = stale_task_recovery_reason(task, has_session, now_nanos) else {
            continue;
        };

        if let Some(runtime) = task.runtime.as_mut() {
            runtime.active_turn_id = None;
            runtime.last_error = Some(reason.clone());
            runtime.log.push(new_runtime_entry(
                "watchdog",
                format!("检测到运行卡住，已自动回收：{reason}"),
            ));
        }

        task.status = TaskStatus::Paused;
        task.claimed_by = None;

        task.activities.push(new_activity(
            "task.watchdog_recovered",
            format!("系统检测到任务长时间无进展，已自动回收：{reason}"),
        ));

        if has_session {
            sessions_to_stop.push(task.id);
        }
    }

    for agent in &mut state.agents {
        let should_release = agent.current_task_id.is_some_and(|task_id| {
            state
                .tasks
                .iter()
                .find(|task| task.id == task_id)
                .map(|task| {
                    !matches!(task.status, TaskStatus::Running | TaskStatus::Claimed)
                        || task.claimed_by != Some(agent.id)
                })
                .unwrap_or(true)
        });

        if should_release {
            agent.current_task_id = None;
            agent.status = "空闲".into();
            agent.last_action = "已自动释放失效占用，等待继续执行".into();
        }
    }

    sessions_to_stop.sort_unstable();
    sessions_to_stop.dedup();
    sessions_to_stop
}

pub(crate) fn reconcile_parallel_active_tasks(state: &mut BoardState) -> Vec<Uuid> {
    let mut sessions_to_stop = Vec::new();
    let mut active_task_groups = std::collections::HashMap::<String, Vec<Uuid>>::new();

    for task in state
        .tasks
        .iter()
        .filter(|task| task_is_serialized_active(task))
    {
            let lane_key = task_serialization_lane_key(&state.projects, task);
        active_task_groups
            .entry(lane_key)
            .or_default()
            .push(task.id);
    }

    for active_task_ids in active_task_groups.into_values() {
        if active_task_ids.len() <= 1 {
            continue;
        }

        let Some(keep_task_id) = state
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, task)| active_task_ids.iter().any(|task_id| *task_id == task.id))
            .min_by_key(|(index, task)| active_task_keep_order(task, *index))
            .map(|(_, task)| task.id)
        else {
            continue;
        };

        let keep_task_title = state
            .tasks
            .iter()
            .find(|task| task.id == keep_task_id)
            .map(|task| task.title.clone())
            .unwrap_or_else(|| "当前保留任务".into());

        let mut requeued_titles = Vec::new();
        for task_id in active_task_ids {
            if task_id == keep_task_id {
                continue;
            }

            let task_title = {
                let task =
                    find_task_mut(state, task_id).expect("parallel active task should exist");
                let task_title = task.title.clone();
                task.status = TaskStatus::Open;
                task.claimed_by = None;
                if let Some(runtime) = task.runtime.as_mut() {
                    runtime.active_turn_id = None;
                    runtime.log.push(new_runtime_entry(
                        "system",
                        format!(
                            "系统检测到同一工作区存在多个并行执行任务，已将当前任务回收到等待队列；保留继续执行的任务是《{}》。",
                            keep_task_title
                        ),
                    ));
                }
                task.activities.push(new_activity(
                    "task.parallel_requeued",
                    format!(
                        "系统检测到同一工作区存在多个并行执行任务，为保证同一工作区串行执行，已将该任务回收到等待队列；保留继续执行的任务是《{}》。",
                        keep_task_title
                    ),
                ));
                task_title
            };

            requeued_titles.push(task_title);
            reset_agent_if_needed(state, task_id, "检测到同一工作区并行执行，已回收到等待队列");
            sessions_to_stop.push(task_id);
        }

        if !requeued_titles.is_empty() {
            let keep_task = find_task_mut(state, keep_task_id).expect("keep task should exist");
            keep_task.activities.push(new_activity(
                "task.parallel_serialized",
                format!(
                    "系统检测到同一工作区此前存在并行执行，已保留当前任务继续推进；回收到等待队列的任务：{}",
                    requeued_titles.join("、")
                ),
            ));
        }
    }

    sessions_to_stop.sort_unstable();
    sessions_to_stop.dedup();
    sessions_to_stop
}

fn active_task_keep_order(task: &Task, index: usize) -> (u8, u8, u128, u128, usize) {
    let status_order = match task.status {
        TaskStatus::Running => 0,
        TaskStatus::Claimed => 1,
        _ => 2,
    };
    let last_touch_order = u128::MAX - task_last_touch_nanos(task).unwrap_or_default();
    (
        status_order,
        task_priority_order(task.priority),
        last_touch_order,
        task_created_order(task),
        index,
    )
}

fn stale_task_recovery_reason(task: &Task, has_session: bool, now_nanos: u128) -> Option<String> {
    match task.status {
        TaskStatus::Running => {
            let runtime = task.runtime.as_ref()?;
            if runtime.active_turn_id.is_none() {
                return Some("运行中任务缺少活动 turn".into());
            }
            if !has_session {
                return Some("本地运行会话已丢失，无法继续流式执行".into());
            }
            let last_touch = task_last_touch_nanos(task)?;
            if now_nanos.saturating_sub(last_touch) > stale_timeout_nanos() {
                return Some(format!(
                    "超过 {} 秒没有新的日志或事件输出",
                    TASK_STALE_TIMEOUT_SECS
                ));
            }
            None
        }
        TaskStatus::Claimed => {
            let last_touch = task_last_touch_nanos(task)?;
            (now_nanos.saturating_sub(last_touch) > stale_timeout_nanos()).then_some(format!(
                "任务被认领后超过 {} 秒仍未启动",
                TASK_STALE_TIMEOUT_SECS
            ))
        }
        _ => None,
    }
}

pub(crate) fn select_next_auto_resume_task_id(
    tasks: &[Task],
    owner_user_id: Option<Uuid>,
) -> Option<Uuid> {
    tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| is_auto_resumable_task(task, owner_user_id))
        .min_by_key(|(index, task)| {
            (
                task_priority_order(task.priority),
                task_created_order(task),
                *index,
            )
        })
        .map(|(_, task)| task.id)
}

fn is_auto_resumable_task(task: &Task, owner_user_id: Option<Uuid>) -> bool {
    if !matches!(task.status, TaskStatus::Paused) {
        return false;
    }

    let Some(runtime) = task.runtime.as_ref() else {
        return false;
    };
    if runtime.thread_id.is_none() {
        return false;
    }
    if runtime
        .last_error
        .as_deref()
        .is_some_and(is_non_resumable_thread_error)
    {
        return false;
    }

    let Some(last_activity) = task.activities.last() else {
        return false;
    };
    if !matches!(
        last_activity.kind.as_str(),
        "task.watchdog_recovered" | "task.runtime_session_lost"
    ) {
        return false;
    }

    match owner_user_id {
        Some(owner_user_id) => task.assignee_user_id == Some(owner_user_id),
        None => task.assignee_user_id.is_none(),
    }
}

fn is_non_resumable_thread_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("thread not found")
        || normalized.contains("no rollout found")
        || normalized.contains("rollout not found")
}

pub(crate) async fn auto_start_task(
    state: &AppState,
    task_id: Uuid,
    agent_id: Uuid,
    agent_name: &str,
) -> AppResult<()> {
    {
        let mut guard = state.inner.lock().await;
        if let Some(conflict) =
            active_task_conflict(&guard.projects, &guard.tasks, task_id, Some(task_id))
        {
            return Err((StatusCode::CONFLICT, active_task_conflict_message(conflict)));
        }
        let task = find_task_mut(&mut guard, task_id)?;
        if matches!(task.status, TaskStatus::Canceled) {
            return Err((StatusCode::CONFLICT, "已撤销任务不能自动启动".into()));
        }
    }

    let context = crate::prompt::resolve_task_execution_context(state, task_id, None).await?;
    let _ = std::fs::create_dir_all(&context.workspace_root);
    let mut git_auto_merge_enabled = false;
    if matches!(state.runtime_mode, RuntimeMode::RealCodex) {
        let git_prepare =
            crate::git_ops::prepare_git_task_branch_in_repo(&context.workspace_root, task_id)
                .await?;
        git_auto_merge_enabled = git_prepare.auto_merge_enabled;
        for (kind, message) in git_prepare.activities {
            record_task_activity(state, task_id, kind, message).await;
        }
        crate::git_ops::apply_git_snapshot(&context.workspace_root, task_id, state).await;
    }

    match state.runtime_mode {
        RuntimeMode::Stub => {
            let mut guard = state.inner.lock().await;
            crate::task_ops::mark_task_running_with_provider(
                &mut guard,
                task_id,
                agent_id,
                agent_name,
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
                "auto_start",
            );
            if let Ok(task) = find_task_mut(&mut guard, task_id) {
                task.activities.push(new_activity(
                    "task.auto_started",
                    "系统检测到空闲 Agent，已自动开始执行该任务",
                ));
            }
            drop(guard);
            persist_state(state).await?;
            Ok(())
        }
        RuntimeMode::RealCodex => {
            let (event_tx, event_rx) = mpsc::unbounded_channel();
            let session = spawn_runtime_session_with_timeout(
                context.workspace_root.clone(),
                event_tx,
                "鑷姩鍚姩浠诲姟鏃跺垱寤鸿繍琛屾椂浼氳瘽",
            )
            .await?;
            let provider_id = session.provider_id().to_string();
            let thread_id = start_runtime_thread_with_timeout(
                &session,
                &context.workspace_root,
                &crate::prompt::task_developer_instructions(),
                "鑷姩鍚姩浠诲姟鏃跺垱寤虹嚎绋?",
            )
            .await?;
            let turn_id = start_runtime_turn_with_timeout(
                &session,
                &context.workspace_root,
                &thread_id,
                &context.prompt,
                "鑷姩鍚姩浠诲姟鏃跺惎鍔ㄤ細璇濊疆娆?",
            )
            .await?;
            let run_thread_id = thread_id.clone();
            let run_turn_id = turn_id.clone();
            {
                let mut guard = state.inner.lock().await;
                crate::task_ops::mark_task_running_with_provider(
                    &mut guard,
                    task_id,
                    agent_id,
                    agent_name,
                    &provider_id,
                    &context.prompt,
                    Some(thread_id),
                    Some(turn_id),
                    git_auto_merge_enabled,
                )?;
                if let Ok(task) = find_task_mut(&mut guard, task_id) {
                    task.activities.push(new_activity(
                        "task.auto_started",
                        "系统检测到空闲 Agent，已自动开始执行该任务",
                    ));
                }
                record_task_run_start(
                    &mut guard,
                    task_id,
                    agent_id,
                    &provider_id,
                    &context.prompt,
                    Some(context.workspace_root.display().to_string()),
                    Some(run_thread_id),
                    Some(run_turn_id),
                    "auto_start",
                );
            }
            crate::handlers::register_task_runtime_session(
                state,
                task_id,
                agent_id,
                session,
                Some(event_rx),
            )
            .await;
            persist_state(state).await?;
            Ok(())
        }
    }
}

async fn auto_resume_task(
    state: &AppState,
    task_id: Uuid,
    agent_id: Uuid,
    agent_name: &str,
) -> AppResult<()> {
    let workspace_root = resolve_workspace_for_task(state, task_id).await?;
    let prompt = auto_resume_prompt(state, task_id).await?;
    let (thread_id, git_auto_merge_enabled) = {
        let mut guard = state.inner.lock().await;
        let task = find_task_mut(&mut guard, task_id)?;
        if matches!(task.status, TaskStatus::Canceled) {
            return Err((StatusCode::CONFLICT, "已撤销任务不能自动恢复".into()));
        }
        let runtime = task
            .runtime
            .as_ref()
            .ok_or_else(|| (StatusCode::CONFLICT, "缺少可恢复的运行时上下文".into()))?;
        (
            runtime
                .thread_id
                .clone()
                .ok_or_else(|| (StatusCode::CONFLICT, "缺少 thread_id，无法自动恢复".into()))?,
            runtime.git_auto_merge_enabled,
        )
    };

    match state.runtime_mode {
        RuntimeMode::Stub => {
            let mut guard = state.inner.lock().await;
            let prompt_for_history = prompt.clone();
            let task = find_task_mut(&mut guard, task_id)?;
            task.status = TaskStatus::Done;
            task.claimed_by = Some(agent_id);
            task.activities.push(new_activity(
                "task.auto_resumed",
                "系统检测到上次运行中断，已自动恢复并完成该任务",
            ));
            let runtime = task.runtime.get_or_insert_with(|| TaskRuntime {
                provider: "stub-codex".into(),
                thread_id: Some("stub-thread".into()),
                active_turn_id: Some("stub-turn-auto-resume".into()),
                git_auto_merge_enabled: false,
                log: Vec::new(),
                last_error: None,
            });
            runtime.last_error = None;
            runtime.log.push(new_runtime_entry("user", prompt));
            runtime
                .log
                .push(new_runtime_entry("assistant", "Stub 自动恢复后已完成任务"));
            runtime.active_turn_id = None;
            record_task_run_start(
                &mut guard,
                task_id,
                agent_id,
                "stub-codex",
                &prompt_for_history,
                Some(workspace_root.display().to_string()),
                Some("stub-thread".into()),
                Some("stub-turn-auto-resume".into()),
                "auto_resume",
            );
            record_task_run_transition(
                &mut guard,
                task_id,
                "completed",
                Some("stub auto resume completed"),
                Some("stub-thread"),
                Some("stub-turn-auto-resume"),
            );
            crate::completion::process_task_completion_outputs(&mut guard, task_id);
            reset_agent_if_needed(&mut guard, task_id, "最近一次任务已自动恢复完成");
            drop(guard);
            persist_state(state).await?;
            Ok(())
        }
        RuntimeMode::RealCodex => {
            let resolved_session = crate::handlers::resolve_task_runtime_session(
                state,
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
                &prompt,
                "鑷姩鎭㈠浠诲姟鏃跺惎鍔ㄤ細璇濊疆娆?",
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

            crate::handlers::register_task_runtime_session(
                state,
                task_id,
                agent_id,
                resolved_session.session.clone(),
                resolved_session.event_rx,
            )
            .await;

            {
                let mut guard = state.inner.lock().await;
                let run_provider_id = resolved_session.session.provider_id().to_string();
                let run_thread_id = resolved_session.thread_id.clone();
                let run_turn_id = turn_id.clone();
                let task = find_task_mut(&mut guard, task_id)?;
                task.status = TaskStatus::Running;
                task.claimed_by = Some(agent_id);
                task.activities.push(new_activity(
                    "task.auto_resumed",
                    "系统检测到上次运行中断，已自动恢复该任务",
                ));
                let runtime = task
                    .runtime
                    .as_mut()
                    .ok_or_else(|| (StatusCode::CONFLICT, "缺少可恢复的运行时上下文".into()))?;
                runtime.provider = run_provider_id.clone();
                runtime.thread_id = Some(run_thread_id.clone());
                runtime.active_turn_id = Some(run_turn_id.clone());
                runtime.git_auto_merge_enabled = git_auto_merge_enabled;
                runtime.last_error = None;
                runtime.log.push(new_runtime_entry("user", prompt.clone()));
                let task_title = task.title.clone();
            record_task_run_start(
                &mut guard,
                task_id,
                agent_id,
                    &run_provider_id,
                    &prompt,
                    Some(workspace_root.display().to_string()),
                    Some(run_thread_id),
                    Some(run_turn_id),
                    "auto_resume",
                );
                assign_agent_running(
                    &mut guard,
                    agent_id,
                    task_id,
                    format!("正在自动恢复任务：{task_title}"),
                );
            }

            persist_state(state).await?;
            let _ = agent_name;
            Ok(())
        }
    }
}

async fn reopen_task_for_auto_retry(
    state: &AppState,
    task_id: Uuid,
    agent_id: Uuid,
    reason: &str,
) -> AppResult<()> {
    let mut guard = state.inner.lock().await;
    let task = find_task_mut(&mut guard, task_id)?;
    task.status = TaskStatus::Paused;
    task.claimed_by = None;
    task.activities.push(new_activity(
        "task.auto_retry_queued",
        format!("自动恢复失败，已重新放回等待队列：{reason}"),
    ));
    if let Some(runtime) = task.runtime.as_mut() {
        runtime.active_turn_id = None;
        runtime.last_error = Some(reason.to_string());
        runtime.log.push(new_runtime_entry(
            "watchdog",
            format!("自动恢复失败，已重新排队等待继续执行：{reason}"),
        ));
    }
    reset_agent_if_needed(&mut guard, task_id, "自动恢复失败，已重新排队");
    drop(guard);
    let _ = agent_id;
    persist_state(state).await
}

async fn auto_resume_prompt(state: &AppState, task_id: Uuid) -> AppResult<String> {
    let guard = state.inner.lock().await;
    let task = guard
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到任务".into()))?;
    Ok(format!(
        "系统自动恢复：任务《{}》在本地运行中断或长时间无输出后被自动接管。请先快速回顾当前进展和工作区，再继续推进；如果现场已经变化，请直接基于现状收敛并继续完成。",
        task.title
    ))
}

/// 策略 Agent 全局扫描
///
/// 职责（参考 ComposioHQ/agent-orchestrator 的 notification-driven 模式）：
/// 1. 评估所有项目中滞留的任务
/// 2. 检测跨项目的资源冲突
/// 3. 标记需要人工注意的异常
/// 4. 动态调整优先级（基于项目进度）
#[allow(dead_code)]
async fn run_strategy_sweep(state: &AppState) {
    // 阶段 0：处理超时的决策卡片
    expire_timed_out_decisions(state).await;

    // 阶段 1：清理明确可以结束的滞留任务
    reassess_stale_tasks(state).await;

    // 阶段 2：检测跨项目的异常模式
    detect_system_anomalies(state).await;

    // 阶段 3：动态优先级调整
    adjust_task_priorities(state).await;
}

/// 检测系统级异常并记录到活动日志
#[allow(dead_code)]
async fn detect_system_anomalies(state: &AppState) {
    let anomalies: Vec<String> = {
        let guard = state.inner.lock().await;

        let mut issues = Vec::new();

        // 检测：所有 Agent 都空闲但有待处理任务
        let all_agents_idle = guard.agents.iter().all(|a| a.current_task_id.is_none());
        let has_open_tasks = guard
            .tasks
            .iter()
            .any(|t| matches!(t.status, TaskStatus::Open));
        let has_auto_agent = guard.agents.iter().any(|a| a.auto_mode);
        if all_agents_idle && has_open_tasks && has_auto_agent {
            issues.push("所有 auto-mode Agent 空闲但存在待处理任务，可能存在认领阻塞".into());
        }

        // 检测：大量任务堆积在 Paused
        let paused_count = guard
            .tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Paused))
            .count();
        if paused_count >= 5 {
            issues.push(format!(
                "有 {paused_count} 个任务堆积在暂停状态，建议批量重评估"
            ));
        }

        // 检测：ManualReview 堆积
        let review_count = guard
            .tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::ManualReview))
            .count();
        if review_count >= 3 {
            issues.push(format!(
                "有 {review_count} 个任务等待人工复核，注意及时处理"
            ));
        }

        issues
    };

    if !anomalies.is_empty() {
        let mut guard = state.inner.lock().await;
        // 记录到 Spotlight 自身项目的第一个任务（如果有）
        if let Some(task) = guard
            .tasks
            .iter_mut()
            .find(|t| matches!(t.status, TaskStatus::Open | TaskStatus::Paused))
        {
            for anomaly in &anomalies {
                // 避免重复：如果最近 5 条活动已经有相同内容就跳过
                let already_logged = task
                    .activities
                    .iter()
                    .rev()
                    .take(5)
                    .any(|a| a.kind == "strategy.anomaly" && a.message == *anomaly);
                if !already_logged {
                    task.activities
                        .push(new_activity("strategy.anomaly", anomaly.clone()));
                }
            }
        }
    }
}

/// 根据项目进度动态调整未启动任务的优先级
#[allow(dead_code)]
async fn adjust_task_priorities(state: &AppState) {
    let adjustments: Vec<(Uuid, String)> = {
        let guard = state.inner.lock().await;

        let mut changes = Vec::new();

        for project in &guard.projects {
            let project_tasks: Vec<&Task> = guard
                .tasks
                .iter()
                .filter(|t| t.project_id == project.id)
                .collect();

            let done_count = project_tasks
                .iter()
                .filter(|t| matches!(t.status, TaskStatus::Done | TaskStatus::Accepted))
                .count();
            let total = project_tasks.len();

            // 如果项目完成率 > 80%，给剩余 Open 任务提升优先级（收尾冲刺）
            if total > 3 && done_count * 100 / total > 80 {
                for task in project_tasks
                    .iter()
                    .filter(|t| matches!(t.status, TaskStatus::Open) && t.priority.is_none())
                {
                    changes.push((
                        task.id,
                        format!(
                            "项目已完成 {}%，剩余任务自动提升为中优先级（收尾冲刺）",
                            done_count * 100 / total
                        ),
                    ));
                }
            }
        }

        changes
    };

    if adjustments.is_empty() {
        return;
    }

    let mut guard = state.inner.lock().await;
    for (task_id, reason) in &adjustments {
        if let Ok(task) = find_task_mut(&mut guard, *task_id) {
            if task.priority.is_none() {
                task.priority = Some(platform_core::TaskPriority::Medium);
                task.activities
                    .push(new_activity("strategy.priority_adjusted", reason.clone()));
            }
        }
    }
    drop(guard);
    let _ = persist_state(state).await;
}
