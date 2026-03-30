use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use platform_core::{new_activity, Project, Task, TaskPriority, TaskStatus};
use tokio::process::Command;
use uuid::Uuid;

use crate::models::{GitPrepareResult, GitTaskBranchPlan};
use crate::{AppResult, AppState, BoardState};

pub(crate) async fn git_command_output(
    workspace_root: &Path,
    args: &[&str],
) -> Result<std::process::Output, String> {
    Command::new("git")
        .args(args)
        .current_dir(workspace_root)
        .output()
        .await
        .map_err(|error| format!("执行 git {:?} 失败：{error}", args))
}

pub(crate) fn git_stderr_message(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }

    "git 命令失败，但没有返回详细输出".into()
}

pub(crate) async fn git_stdout_trimmed(workspace_root: &Path, args: &[&str]) -> Option<String> {
    let output = git_command_output(workspace_root, args).await.ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!stdout.is_empty()).then_some(stdout)
}

pub(crate) async fn git_ref_exists(workspace_root: &Path, reference: &str) -> bool {
    git_command_output(
        workspace_root,
        &["show-ref", "--verify", "--quiet", reference],
    )
    .await
    .map(|output| output.status.success())
    .unwrap_or(false)
}

pub(crate) async fn is_git_repo(workspace_root: &Path) -> bool {
    git_command_output(workspace_root, &["rev-parse", "--is-inside-work-tree"])
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub(crate) async fn git_worktree_dirty(workspace_root: &Path) -> Result<bool, String> {
    let output = git_command_output(workspace_root, &["status", "--porcelain"]).await?;
    if !output.status.success() {
        return Err(git_stderr_message(&output));
    }

    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

pub(crate) async fn git_tracked_worktree_dirty(workspace_root: &Path) -> Result<bool, String> {
    let output = git_command_output(
        workspace_root,
        &["status", "--porcelain", "--untracked-files=no"],
    )
    .await?;
    if !output.status.success() {
        return Err(git_stderr_message(&output));
    }

    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

fn isolated_task_worktree_path(primary_workspace_root: &Path, task_id: Uuid) -> PathBuf {
    primary_workspace_root
        .join(".spotlight")
        .join("runtime-worktrees")
        .join(task_id.to_string())
}

fn ensure_runtime_worktree_excluded(primary_workspace_root: &Path) -> Result<(), String> {
    let exclude_path = primary_workspace_root
        .join(".git")
        .join("info")
        .join("exclude");
    if let Some(parent) = exclude_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("创建 .git/info 目录失败：{error}"))?;
    }

    let pattern = ".spotlight/runtime-worktrees/";
    let existing = std::fs::read_to_string(&exclude_path).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == pattern) {
        return Ok(());
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(pattern);
    updated.push('\n');
    std::fs::write(&exclude_path, updated)
        .map_err(|error| format!("更新 .git/info/exclude 失败：{error}"))?;
    Ok(())
}

pub(crate) async fn detect_primary_remote(workspace_root: &Path) -> Option<String> {
    if git_command_output(workspace_root, &["remote", "get-url", "origin"])
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
    {
        return Some("origin".into());
    }

    git_stdout_trimmed(workspace_root, &["remote"])
        .await
        .and_then(|stdout| {
            stdout
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(str::to_string)
        })
}

pub(crate) async fn detect_base_branch(workspace_root: &Path, remote_name: Option<&str>) -> String {
    if let Some(remote) = remote_name {
        let remote_head = format!("refs/remotes/{remote}/HEAD");
        if let Some(symbolic_ref) =
            git_stdout_trimmed(workspace_root, &["symbolic-ref", &remote_head]).await
        {
            if let Some((_, branch)) = symbolic_ref.rsplit_once('/') {
                if !branch.trim().is_empty() {
                    return branch.trim().to_string();
                }
            }
        }
    }

    for candidate in ["main", "master"] {
        let local_ref = format!("refs/heads/{candidate}");
        if git_ref_exists(workspace_root, &local_ref).await {
            return candidate.to_string();
        }
    }

    git_stdout_trimmed(workspace_root, &["branch", "--show-current"])
        .await
        .filter(|branch| !branch.trim().is_empty())
        .unwrap_or_else(|| "main".into())
}

pub(crate) async fn detect_git_task_branch_plan(
    workspace_root: &Path,
    task_id: Uuid,
) -> GitTaskBranchPlan {
    let remote_name = detect_primary_remote(workspace_root).await;
    let base_branch = detect_base_branch(workspace_root, remote_name.as_deref()).await;

    GitTaskBranchPlan {
        base_branch,
        task_branch: format!("task/{task_id}"),
        remote_name,
    }
}

pub(crate) async fn prepare_git_task_branch_in_repo(
    workspace_root: &Path,
    task_id: Uuid,
) -> AppResult<GitPrepareResult> {
    let mut activities = Vec::new();
    let execution_workspace_root = isolated_task_worktree_path(workspace_root, task_id);

    if !is_git_repo(workspace_root).await {
        activities.push((
            "git.branch_prepare_skipped".into(),
            "当前工作目录不是 Git 仓库，跳过任务分支预处理。".into(),
        ));
        return Ok(GitPrepareResult {
            workspace_root: workspace_root.to_path_buf(),
            activities,
            auto_merge_enabled: false,
        });
    }

    ensure_runtime_worktree_excluded(workspace_root)
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;

    if git_tracked_worktree_dirty(workspace_root)
        .await
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message.clone()))?
    {
        activities.push((
            "git.primary_workspace_dirty".into(),
            "检测到主工作区存在未提交跟踪改动；隔离 worktree 将基于最近一次已提交基线创建，不会带入这些本地修改。".into(),
        ));
    } else if git_worktree_dirty(workspace_root)
        .await
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message.clone()))?
    {
        activities.push((
            "git.primary_workspace_untracked".into(),
            "检测到主工作区存在未跟踪文件；隔离 worktree 已启用，不再因此跳过任务分支准备。".into(),
        ));
    }

    let plan = detect_git_task_branch_plan(workspace_root, task_id).await;
    activities.push((
        "git.branch_plan".into(),
        format!(
            "任务 Git 计划：主分支={}，任务分支={}，远端={}",
            plan.base_branch,
            plan.task_branch,
            plan.remote_name.as_deref().unwrap_or("无")
        ),
    ));

    if execution_workspace_root.exists() {
        if !is_git_repo(&execution_workspace_root).await {
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "隔离 worktree 路径已存在但不是 Git 工作区：{}",
                    execution_workspace_root.display()
                ),
            ));
        }

        let current_branch =
            git_stdout_trimmed(&execution_workspace_root, &["branch", "--show-current"])
                .await
                .unwrap_or_default();
        if current_branch.trim() != plan.task_branch {
            match git_command_output(&execution_workspace_root, &["checkout", &plan.task_branch])
                .await
            {
                Ok(output) if output.status.success() => {}
                Ok(output) => {
                    return Err((
                        StatusCode::CONFLICT,
                        format!(
                            "复用隔离 worktree 失败，无法切换到任务分支 {}：{}",
                            plan.task_branch,
                            git_stderr_message(&output)
                        ),
                    ));
                }
                Err(error) => return Err((StatusCode::CONFLICT, error)),
            }
        }

        activities.push((
            "git.task_worktree_reused".into(),
            format!(
                "已复用任务隔离 worktree：{}",
                execution_workspace_root.display()
            ),
        ));
        return Ok(GitPrepareResult {
            workspace_root: execution_workspace_root,
            activities,
            auto_merge_enabled: true,
        });
    }

    if let Some(parent) = execution_workspace_root.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("创建隔离 worktree 父目录失败：{error}"),
            )
        })?;
    }

    if let Some(remote) = plan.remote_name.as_deref() {
        let fetch = git_command_output(workspace_root, &["fetch", remote]).await;
        match fetch {
            Ok(output) if output.status.success() => activities.push((
                "git.remote_fetched".into(),
                format!("已获取远端 {remote} 的最新引用。"),
            )),
            Ok(output) => {
                let message = format!("获取远端 {remote} 失败：{}", git_stderr_message(&output));
                activities.push(("git.branch_prepare_failed".into(), message.clone()));
                return Err((StatusCode::BAD_GATEWAY, message));
            }
            Err(error) => {
                activities.push(("git.branch_prepare_failed".into(), error.clone()));
                return Err((StatusCode::BAD_GATEWAY, error));
            }
        }
    }

    let task_branch_ref = format!("refs/heads/{}", plan.task_branch);
    let branch_exists = git_ref_exists(workspace_root, &task_branch_ref).await;
    let start_point = if let Some(remote) = plan.remote_name.as_deref() {
        let remote_branch_ref = format!("refs/remotes/{remote}/{}", plan.base_branch);
        if git_ref_exists(workspace_root, &remote_branch_ref).await {
            format!("{remote}/{}", plan.base_branch)
        } else {
            plan.base_branch.clone()
        }
    } else {
        plan.base_branch.clone()
    };
    let worktree_add_args_owned = if branch_exists {
        vec![
            "worktree".to_string(),
            "add".to_string(),
            execution_workspace_root.display().to_string(),
            plan.task_branch.clone(),
        ]
    } else {
        vec![
            "worktree".to_string(),
            "add".to_string(),
            "-b".to_string(),
            plan.task_branch.clone(),
            execution_workspace_root.display().to_string(),
            start_point.clone(),
        ]
    };
    let worktree_add_args = worktree_add_args_owned
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let checkout_task = git_command_output(workspace_root, &worktree_add_args).await;
    match checkout_task {
        Ok(output) if output.status.success() => activities.push((
            if branch_exists {
                "git.task_worktree_attached".into()
            } else {
                "git.task_worktree_created".into()
            },
            if branch_exists {
                format!(
                    "已为任务分支 {} 挂载隔离 worktree：{}。",
                    plan.task_branch,
                    execution_workspace_root.display()
                )
            } else {
                format!(
                    "已基于 {} 创建任务分支 {}，隔离 worktree={}",
                    start_point,
                    plan.task_branch,
                    execution_workspace_root.display()
                )
            },
        )),
        Ok(output) => {
            let message = format!(
                "创建任务隔离 worktree {} 失败：{}",
                plan.task_branch,
                git_stderr_message(&output)
            );
            activities.push(("git.branch_prepare_failed".into(), message.clone()));
            return Err((StatusCode::CONFLICT, message));
        }
        Err(error) => {
            activities.push(("git.branch_prepare_failed".into(), error.clone()));
            return Err((StatusCode::CONFLICT, error));
        }
    }

    Ok(GitPrepareResult {
        workspace_root: execution_workspace_root,
        activities,
        auto_merge_enabled: true,
    })
}

pub(crate) async fn finalize_git_task_branch_in_repo(
    workspace_root: &Path,
    execution_workspace_root: &Path,
    task_id: Uuid,
) -> Vec<(String, String)> {
    let mut activities = Vec::new();

    if !is_git_repo(workspace_root).await || !is_git_repo(execution_workspace_root).await {
        activities.push((
            "git.merge_skipped".into(),
            "主工作区或执行 worktree 不是 Git 仓库，跳过任务完成后的自动合并。".into(),
        ));
        return activities;
    }

    let plan = detect_git_task_branch_plan(workspace_root, task_id).await;
    activities.push((
        "git.merge_plan".into(),
        format!(
            "任务完成后的 Git 合并计划：主分支={}，任务分支={}，远端={}，执行 worktree={}",
            plan.base_branch,
            plan.task_branch,
            plan.remote_name.as_deref().unwrap_or("无"),
            execution_workspace_root.display()
        ),
    ));

    let current_branch =
        git_stdout_trimmed(execution_workspace_root, &["branch", "--show-current"])
            .await
            .unwrap_or_default();
    if current_branch.trim() != plan.task_branch {
        let task_branch_ref = format!("refs/heads/{}", plan.task_branch);
        if !git_ref_exists(execution_workspace_root, &task_branch_ref).await {
            activities.push((
                "git.merge_skipped".into(),
                format!("未找到任务分支 {}，跳过自动合并。", plan.task_branch),
            ));
            return activities;
        }

        match git_command_output(execution_workspace_root, &["checkout", &plan.task_branch]).await {
            Ok(output) if output.status.success() => activities.push((
                "git.task_branch_checked_out".into(),
                format!(
                    "自动合并前已在隔离 worktree 中切换回任务分支 {}。",
                    plan.task_branch
                ),
            )),
            Ok(output) => {
                activities.push((
                    "git.merge_blocked".into(),
                    format!(
                        "自动合并前无法在隔离 worktree 中切换到任务分支 {}：{}",
                        plan.task_branch,
                        git_stderr_message(&output)
                    ),
                ));
                return activities;
            }
            Err(error) => {
                activities.push(("git.merge_blocked".into(), error));
                return activities;
            }
        }
    }

    match git_worktree_dirty(execution_workspace_root).await {
        Ok(true) => {
            match git_command_output(execution_workspace_root, &["add", "-A"]).await {
                Ok(output) if output.status.success() => {}
                Ok(output) => {
                    activities.push((
                        "git.merge_blocked".into(),
                        format!(
                            "自动提交前执行 git add -A 失败：{}",
                            git_stderr_message(&output)
                        ),
                    ));
                    return activities;
                }
                Err(error) => {
                    activities.push(("git.merge_blocked".into(), error));
                    return activities;
                }
            }

            let cached_clean = git_command_output(
                execution_workspace_root,
                &["diff", "--cached", "--quiet", "--exit-code"],
            )
            .await;
            let needs_commit = match cached_clean {
                Ok(output) => !output.status.success(),
                Err(error) => {
                    activities.push(("git.merge_blocked".into(), error));
                    return activities;
                }
            };

            if needs_commit {
                let commit_message = format!("chore(task): 完成任务 {task_id}");
                match git_command_output(
                    execution_workspace_root,
                    &["commit", "-m", commit_message.as_str()],
                )
                .await
                {
                    Ok(output) if output.status.success() => activities.push((
                        "git.task_branch_committed".into(),
                        format!(
                            "已在隔离 worktree 中自动提交任务分支 {} 上的改动。",
                            plan.task_branch
                        ),
                    )),
                    Ok(output) => {
                        activities.push((
                            "git.merge_blocked".into(),
                            format!(
                                "任务分支自动提交失败，已保留分支 {}：{}",
                                plan.task_branch,
                                git_stderr_message(&output)
                            ),
                        ));
                        return activities;
                    }
                    Err(error) => {
                        activities.push(("git.merge_blocked".into(), error));
                        return activities;
                    }
                }
            }
        }
        Ok(false) => activities.push((
            "git.task_branch_clean".into(),
            format!("任务分支 {} 没有额外未提交改动。", plan.task_branch),
        )),
        Err(error) => {
            activities.push(("git.merge_blocked".into(), error));
            return activities;
        }
    }

    let current_primary_branch = git_stdout_trimmed(workspace_root, &["branch", "--show-current"])
        .await
        .unwrap_or_default();
    if current_primary_branch.trim() != plan.base_branch {
        match git_command_output(workspace_root, &["checkout", &plan.base_branch]).await {
            Ok(output) if output.status.success() => activities.push((
                "git.base_checked_out".into(),
                format!("自动合并前已切换主工作区到主分支 {}。", plan.base_branch),
            )),
            Ok(output) => {
                activities.push((
                    "git.merge_blocked".into(),
                    format!(
                        "自动合并前无法切换主工作区到主分支 {}：{}",
                        plan.base_branch,
                        git_stderr_message(&output)
                    ),
                ));
                return activities;
            }
            Err(error) => {
                activities.push(("git.merge_blocked".into(), error));
                return activities;
            }
        }
    }

    if let Some(remote) = plan.remote_name.as_deref() {
        match git_command_output(workspace_root, &["fetch", remote]).await {
            Ok(output) if output.status.success() => activities.push((
                "git.remote_refetched".into(),
                format!("自动合并前已重新获取远端 {remote} 的最新引用。"),
            )),
            Ok(output) => {
                activities.push((
                    "git.merge_blocked".into(),
                    format!(
                        "自动合并前获取远端 {remote} 失败：{}",
                        git_stderr_message(&output)
                    ),
                ));
                return activities;
            }
            Err(error) => {
                activities.push(("git.merge_blocked".into(), error));
                return activities;
            }
        }
    }

    if let Some(remote) = plan.remote_name.as_deref() {
        let remote_branch_ref = format!("refs/remotes/{remote}/{}", plan.base_branch);
        if git_ref_exists(workspace_root, &remote_branch_ref).await {
            let upstream = format!("{remote}/{}", plan.base_branch);
            match git_command_output(workspace_root, &["merge", "--ff-only", &upstream]).await {
                Ok(output) if output.status.success() => activities.push((
                    "git.base_updated".into(),
                    format!("自动合并前已使用 {upstream} 快进更新主分支。"),
                )),
                Ok(output) => {
                    activities.push((
                        "git.merge_blocked".into(),
                        format!(
                            "自动合并前无法先快进主分支到 {upstream}：{}",
                            git_stderr_message(&output)
                        ),
                    ));
                    return activities;
                }
                Err(error) => {
                    activities.push(("git.merge_blocked".into(), error));
                    return activities;
                }
            }
        }
    }

    let merge_message = format!("merge task branch {} for {}", plan.task_branch, task_id);
    match git_command_output(
        workspace_root,
        &[
            "merge",
            "--no-ff",
            "-m",
            merge_message.as_str(),
            &plan.task_branch,
        ],
    )
    .await
    {
        Ok(output) if output.status.success() => activities.push((
            "git.merge_completed".into(),
            format!(
                "已将任务分支 {} 合并回主分支 {}。",
                plan.task_branch, plan.base_branch
            ),
        )),
        Ok(output) => {
            let details = git_stderr_message(&output);
            let _ = git_command_output(workspace_root, &["merge", "--abort"]).await;
            activities.push((
                "git.merge_blocked".into(),
                format!(
                    "任务分支 {} 未能自动合并回主分支 {}，已保留任务分支等待人工处理：{}",
                    plan.task_branch, plan.base_branch, details
                ),
            ));
        }
        Err(error) => {
            let _ = git_command_output(workspace_root, &["merge", "--abort"]).await;
            activities.push((
                "git.merge_blocked".into(),
                format!(
                    "任务分支 {} 自动合并失败，已保留任务分支：{}",
                    plan.task_branch, error
                ),
            ));
        }
    }

    activities
}

pub(crate) async fn apply_git_snapshot(workspace_root: &Path, task_id: Uuid, state: &AppState) {
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

    let dirty = git_worktree_dirty(workspace_root).await.unwrap_or(false);
    let branch = git_stdout_trimmed(workspace_root, &["branch", "--show-current"])
        .await
        .unwrap_or_else(|| "unknown".into());
    let head = git_stdout_trimmed(workspace_root, &["rev-parse", "HEAD"])
        .await
        .unwrap_or_else(|| "unknown".into());

    let tag = format!("task/{task_id}/pre-run/{}", timestamp_compact());
    let result = Command::new("git")
        .args(["tag", &tag])
        .current_dir(workspace_root)
        .output()
        .await;

    let message = match result {
        Ok(output) if output.status.success() => {
            format!("已创建预执行 tag：{tag}，branch={branch}，HEAD={head}，dirty={dirty}")
        }
        Ok(output) => format!(
            "创建预执行 tag 失败：branch={branch}，HEAD={head}，dirty={dirty}，{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(error) => {
            format!("创建预执行 tag 失败：branch={branch}，HEAD={head}，dirty={dirty}，{error}")
        }
    };

    let mut guard = state.inner.lock().await;
    if let Ok(task) = crate::find_task_mut(&mut guard, task_id) {
        task.activities
            .push(new_activity("git.pre_run_snapshot", message));
    }
}

pub(crate) fn timestamp_compact() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
        .to_string()
}

// Legacy duplicate kept only as a temporary reference while this file still has
// encoding pollution. Production auto-claim behavior is owned by task_ops.rs.
// Do not reconnect new selection rules to this block.
#[allow(dead_code)]
fn legacy_auto_claim_next_task(state: &mut BoardState, agent_id: Uuid) -> AppResult<Option<Task>> {
    let (agent_name, owner_user_id, auto_mode_enabled, agent_busy) = state
        .agents
        .iter()
        .find(|agent| agent.id == agent_id)
        .map(|agent| {
            (
                agent.name.clone(),
                agent.owner_user_id,
                agent.auto_mode,
                agent.current_task_id.is_some(),
            )
        })
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到 Agent".into()))?;

    if !auto_mode_enabled || agent_busy {
        return Ok(None);
    }

    let Some(task_index) =
        legacy_select_next_auto_claim_task_index(&state.projects, &state.tasks, owner_user_id)
    else {
        return Ok(None);
    };

    let claimed_task = {
        let task = &mut state.tasks[task_index];
        task.claimed_by = Some(agent_id);
        task.assignee_user_id = task.assignee_user_id.or(owner_user_id);
        task.status = TaskStatus::Claimed;
        task.activities.push(new_activity(
            "task.auto_claimed",
            format!("娴犺濮熷鑼暠 {} 閼奉亜濮╃拋銈夘暙", agent_name),
        ));

        task.activities.push(new_activity(
            "task.auto_claim_reason",
            legacy_auto_claim_selection_reason(task, owner_user_id),
        ));

        task.clone()
    };
    let task_title = claimed_task.title.clone();
    crate::assign_agent_claimed(
        state,
        agent_id,
        claimed_task.id,
        format!("auto claim acquired: {task_title}"),
    );

    Ok(Some(claimed_task))
}

#[allow(dead_code)]
fn legacy_select_next_auto_claim_task_index(
    projects: &[Project],
    tasks: &[Task],
    owner_user_id: Option<Uuid>,
) -> Option<usize> {
    tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| matches!(task.status, TaskStatus::Open) && task.claimed_by.is_none())
        .filter(|(_, task)| {
            task.assignee_user_id.is_none() || task.assignee_user_id == owner_user_id
        })
        .filter(|(_, task)| {
            crate::task_ops::active_task_conflict(projects, tasks, task.id, Some(task.id)).is_none()
        })
        .min_by_key(|(index, task)| {
            (
                task_priority_order(task.priority),
                legacy_task_assignment_order(task, owner_user_id),
                legacy_task_created_order(task),
                *index,
            )
        })
        .map(|(index, _)| index)
}

pub(crate) fn task_priority_order(priority: Option<TaskPriority>) -> u8 {
    match priority {
        Some(TaskPriority::High) => 0,
        Some(TaskPriority::Medium) => 1,
        Some(TaskPriority::Low) => 2,
        None => 3,
    }
}

#[allow(dead_code)]
fn legacy_task_assignment_order(task: &Task, owner_user_id: Option<Uuid>) -> u8 {
    match owner_user_id {
        Some(owner_user_id) if task.assignee_user_id == Some(owner_user_id) => 0,
        _ => 1,
    }
}

#[allow(dead_code)]
fn legacy_auto_claim_selection_reason(task: &Task, owner_user_id: Option<Uuid>) -> String {
    let queue_scope = match owner_user_id {
        Some(owner_user_id) if task.assignee_user_id == Some(owner_user_id) => "本人待办队列",
        _ => "共享待办队列",
    };
    let priority_basis = match task.priority {
        Some(TaskPriority::High) => "高优先级",
        Some(TaskPriority::Medium) => "中优先级",
        Some(TaskPriority::Low) => "低优先级",
        None => "时间顺序",
    };

    format!("选择依据：{} / {}", queue_scope, priority_basis)
}

#[allow(dead_code)]
fn legacy_task_created_order(task: &Task) -> u128 {
    task.activities
        .first()
        .and_then(|activity| activity.at.parse::<u128>().ok())
        .unwrap_or(u128::MAX)
}
