use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use platform_core::{
    new_activity, new_runtime_entry, PendingQuestion, Project, RuntimeLogEntry, Task,
    TaskAssignmentMode, TaskPriority, TaskRuntime, TaskStateSnapshot, TaskStatus,
};
use uuid::Uuid;

use crate::git_ops::task_priority_order;
use crate::models::*;
use crate::{AppResult, AppState, BoardState};

fn normalize_serialization_workspace_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

pub(crate) fn task_serialization_lane_key(projects: &[Project], task: &Task) -> String {
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

pub(crate) fn task_is_serialized_active(task: &Task) -> bool {
    matches!(task.status, TaskStatus::Claimed | TaskStatus::Running)
}

pub(crate) fn active_task_conflict<'a>(
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

pub(crate) fn active_task_conflict_message(task: &Task) -> String {
    format!(
        "任务《{}》当前已在同一工作区处于{}，同一工作区一次只允许一个活跃任务",
        task.title,
        match task.status {
            TaskStatus::Open => "待处理",
            TaskStatus::Claimed => "已认领",
            TaskStatus::ApprovalRequested => "待审批",
            TaskStatus::Approved => "已审批",
            TaskStatus::Running => "执行中",
            TaskStatus::Paused => "已暂停",
            TaskStatus::PendingAcceptance => "待验收",
            TaskStatus::Accepted => "已验收",
            TaskStatus::Done => "已完成",
            TaskStatus::Failed => "已失败",
            TaskStatus::ManualReview => "人工复核",
            TaskStatus::Canceled => "已撤销",
        }
    )
}

pub(crate) fn mark_task_running(
    state: &mut BoardState,
    task_id: Uuid,
    agent_id: Uuid,
    agent_name: &str,
    prompt: &str,
    thread_id: Option<String>,
    turn_id: Option<String>,
    git_auto_merge_enabled: bool,
) -> AppResult<()> {
    mark_task_running_with_provider(
        state,
        task_id,
        agent_id,
        agent_name,
        "codex",
        prompt,
        thread_id,
        turn_id,
        git_auto_merge_enabled,
    )
}

pub(crate) fn mark_task_running_with_provider(
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
            provider: provider_id.into(),
            thread_id: None,
            active_turn_id: None,
            git_auto_merge_enabled: false,
            log: Vec::new(),
            last_error: None,
        });
        runtime.provider = provider_id.into();
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

pub(crate) fn task_pause_runtime_ids(task: &Task) -> AppResult<(String, String)> {
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

pub(crate) fn task_resume_thread_id(task: &Task) -> AppResult<String> {
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

pub(crate) fn assign_agent_running(
    state: &mut BoardState,
    agent_id: Uuid,
    task_id: Uuid,
    action: String,
) {
    assign_agent_task(state, agent_id, task_id, "RUNNING", action);
}

pub(crate) fn assign_agent_claimed(
    state: &mut BoardState,
    agent_id: Uuid,
    task_id: Uuid,
    action: String,
) {
    assign_agent_task(state, agent_id, task_id, "CLAIMED", action);
}

#[allow(dead_code)]
pub(crate) fn claim_task_for_agent(
    state: &mut BoardState,
    task_id: Uuid,
    agent_id: Uuid,
) -> AppResult<()> {
    if let Some(conflict) = active_task_conflict(&state.projects, &state.tasks, task_id, Some(task_id)) {
        return Err((
            StatusCode::CONFLICT,
            active_task_conflict_message(conflict),
        ));
    }

    let (agent_name, owner_user_id) = state
        .agents
        .iter()
        .find(|agent| agent.id == agent_id)
        .map(|agent| (agent.name.clone(), agent.owner_user_id))
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到 Agent".into()))?;

    let (task_title, already_claimed) = {
        let task = find_task_mut(state, task_id)?;
        if matches!(task.status, TaskStatus::Claimed) && task.claimed_by == Some(agent_id) {
            task.assignee_user_id = task.assignee_user_id.or(owner_user_id);
            (task.title.clone(), true)
        } else {
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
            if !matches!(task.status, TaskStatus::Open | TaskStatus::Approved) {
                return Err((StatusCode::CONFLICT, "当前任务状态不允许认领".into()));
            }

            task.claimed_by = Some(agent_id);
            task.assignee_user_id = owner_user_id;
            task.status = TaskStatus::Claimed;
            task.activities.push(new_activity(
                "task.claimed",
                format!("任务已由 {} 认领", agent_name),
            ));
            (task.title.clone(), false)
        }
    };

    let action = if already_claimed {
        format!("claim retained: {task_title}")
    } else {
        format!("claim acquired: {task_title}")
    };
    assign_agent_claimed(state, agent_id, task_id, action);
    Ok(())
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

pub(crate) fn reset_agent_if_needed(state: &mut BoardState, task_id: Uuid, action: &str) {
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

pub(crate) fn push_runtime_delta(log: &mut Vec<RuntimeLogEntry>, kind: &str, delta: &str) {
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

pub(crate) fn find_project(state: &BoardState, project_id: Uuid) -> AppResult<&Project> {
    state
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到项目".into()))
}

pub(crate) fn find_project_mut(
    state: &mut BoardState,
    project_id: Uuid,
) -> AppResult<&mut Project> {
    state
        .projects
        .iter_mut()
        .find(|project| project.id == project_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到项目".into()))
}

pub(crate) fn ensure_project_exists(state: &BoardState, project_id: Uuid) -> AppResult<()> {
    find_project(state, project_id).map(|_| ())
}

pub(crate) fn find_task_mut(state: &mut BoardState, task_id: Uuid) -> AppResult<&mut Task> {
    state
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到任务".into()))
}

pub(crate) fn find_task(state: &BoardState, task_id: Uuid) -> AppResult<&Task> {
    state
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到任务".into()))
}

pub(crate) fn find_project_session_mut(
    state: &mut BoardState,
    session_id: Uuid,
) -> Option<&mut ProjectSession> {
    state
        .project_sessions
        .iter_mut()
        .find(|session| session.id == session_id)
}

pub(crate) fn find_pending_question_mut(
    state: &mut BoardState,
    question_id: Uuid,
) -> AppResult<&mut PendingQuestion> {
    state
        .pending_questions
        .iter_mut()
        .find(|question| question.id == question_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到待回答问题".into()))
}

pub(crate) fn resolve_project_for_new_task(
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

pub(crate) fn primary_workspace_path(project: &Project) -> AppResult<PathBuf> {
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

pub(crate) async fn resolve_workspace_for_task(
    state: &AppState,
    task_id: Uuid,
) -> AppResult<PathBuf> {
    let guard = state.inner.lock().await;
    let task = guard
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "未找到任务".into()))?;
    let project = find_project(&guard, task.project_id)?;
    primary_workspace_path(project)
}

pub(crate) fn build_local_build_restart_task(project: &Project) -> Task {
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

pub(crate) fn build_cloud_install_restart_task(
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

pub(crate) fn sanitize_credential_hint(auth_method: &str, credential_hint: Option<&str>) -> String {
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

pub(crate) fn build_exploration_task(project: &Project) -> Task {
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

pub(crate) fn detect_project_stack(workspace_root: &Path) -> StackDetection {
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

pub(crate) fn collect_workspace_files(base: &Path, max_depth: usize) -> Vec<PathBuf> {
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

pub(crate) fn should_skip_workspace_dir(base: &Path, path: &Path, name: &str) -> bool {
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

pub(crate) fn display_relative_path(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

pub(crate) fn auto_claim_next_task(
    state: &mut BoardState,
    agent_id: Uuid,
) -> AppResult<Option<Task>> {
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
        select_next_auto_claim_task_index(&state.projects, &state.tasks, agent_id, owner_user_id)
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
            auto_claim_selection_reason(task, agent_id, owner_user_id),
        ));

        task.clone()
    };
    let task_title = claimed_task.title.clone();
    assign_agent_claimed(
        state,
        agent_id,
        claimed_task.id,
        format!("auto claim acquired: {task_title}"),
    );

    Ok(Some(claimed_task))
}

pub(crate) fn select_next_auto_claim_task_index(
    projects: &[Project],
    tasks: &[Task],
    agent_id: Uuid,
    owner_user_id: Option<Uuid>,
) -> Option<usize> {
    tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| matches!(task.status, TaskStatus::Open) && task.claimed_by.is_none())
        .filter(|(_, task)| task_is_claimable_by_agent(task, agent_id, owner_user_id))
        .filter(|(_, task)| {
            active_task_conflict(projects, tasks, task.id, Some(task.id)).is_none()
        })
        .min_by_key(|(index, task)| {
            (
                task_assignment_order(task, owner_user_id),
                task_priority_order(task.priority),
                task_created_order(task),
                *index,
            )
        })
        .map(|(index, _)| index)
}

fn task_assignment_order(task: &Task, owner_user_id: Option<Uuid>) -> u8 {
    match task.assignment_mode {
        TaskAssignmentMode::AssignedAgent => 0,
        TaskAssignmentMode::PublicQueue => match owner_user_id {
            Some(owner_user_id) if task.assignee_user_id == Some(owner_user_id) => 1,
            _ => 2,
        },
    }
}

fn task_queue_scope_for_auto_claim(
    task: &Task,
    agent_id: Uuid,
    owner_user_id: Option<Uuid>,
) -> &'static str {
    if matches!(task.assignment_mode, TaskAssignmentMode::AssignedAgent)
        && task.requested_agent_id == Some(agent_id)
    {
        "定向 Agent 队列"
    } else if owner_user_id
        .is_some_and(|owner_user_id| task.assignee_user_id == Some(owner_user_id))
    {
        "鏈汉寰呭姙闃熷垪"
    } else {
        "鍏变韩寰呭姙闃熷垪"
    }
}

pub(crate) fn auto_claim_selection_reason(
    task: &Task,
    agent_id: Uuid,
    owner_user_id: Option<Uuid>,
) -> String {
    let queue_scope = task_queue_scope_for_auto_claim(task, agent_id, owner_user_id);
    /*
        Some(owner_user_id) if task.assignee_user_id == Some(owner_user_id) => "本人待办队列",
        _ => "共享待办队列",
    */
    let priority_basis = match task.priority {
        Some(TaskPriority::High) => "高优先级",
        Some(TaskPriority::Medium) => "中优先级",
        Some(TaskPriority::Low) => "低优先级",
        None => "时间顺序",
    };

    format!("选择依据：{} / {}", queue_scope, priority_basis)
}

pub(crate) fn task_is_claimable_by_agent(
    task: &Task,
    agent_id: Uuid,
    owner_user_id: Option<Uuid>,
) -> bool {
    match task.assignment_mode {
        TaskAssignmentMode::AssignedAgent => task.requested_agent_id == Some(agent_id),
        TaskAssignmentMode::PublicQueue => {
            task.requested_agent_id.is_none()
                && (task.assignee_user_id.is_none() || task.assignee_user_id == owner_user_id)
        }
    }
}

pub(crate) fn task_created_order(task: &Task) -> u128 {
    task.activities
        .first()
        .and_then(|activity| activity.at.parse::<u128>().ok())
        .unwrap_or(u128::MAX)
}

pub(crate) async fn record_task_activity(
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
