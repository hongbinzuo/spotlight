use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRoot {
    pub id: Uuid,
    pub label: String,
    pub path: String,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub workspace_roots: Vec<WorkspaceRoot>,
    pub is_spotlight_self: bool,
}

impl Project {
    pub fn primary_workspace(&self) -> Option<&WorkspaceRoot> {
        self.workspace_roots.first()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub priority: Option<TaskPriority>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub creator_user_id: Option<Uuid>,
    #[serde(default)]
    pub assignee_user_id: Option<Uuid>,
    #[serde(default)]
    pub assignment_mode: TaskAssignmentMode,
    #[serde(default)]
    pub requested_agent_id: Option<Uuid>,
    #[serde(default)]
    pub source_task_id: Option<Uuid>,
    pub claimed_by: Option<Uuid>,
    pub activities: Vec<TaskActivity>,
    pub runtime: Option<TaskRuntime>,
    #[serde(default)]
    pub approval: TaskApprovalState,
    #[serde(default)]
    pub acceptance: TaskAcceptanceState,
    #[serde(default)]
    pub state_snapshot: TaskStateSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskAssignmentMode {
    PublicQueue,
    AssignedAgent,
}

impl Default for TaskAssignmentMode {
    fn default() -> Self {
        Self::PublicQueue
    }
}

impl TaskAssignmentMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PublicQueue => "public_queue",
            Self::AssignedAgent => "assigned_agent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskApprovalStatus {
    NotRequired,
    Requested,
    Approved,
    Denied,
    Expired,
}

impl Default for TaskApprovalStatus {
    fn default() -> Self {
        Self::NotRequired
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskApprovalState {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub status: TaskApprovalStatus,
    #[serde(default)]
    pub requested_by_user_id: Option<Uuid>,
    #[serde(default)]
    pub requested_at: Option<String>,
    #[serde(default)]
    pub reviewer_user_id: Option<Uuid>,
    #[serde(default)]
    pub reviewed_at: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskAcceptanceStatus {
    NotStarted,
    Pending,
    Accepted,
    Rejected,
}

impl Default for TaskAcceptanceStatus {
    fn default() -> Self {
        Self::NotStarted
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskAcceptanceState {
    #[serde(default)]
    pub owner_user_id: Option<Uuid>,
    #[serde(default)]
    pub status: TaskAcceptanceStatus,
    #[serde(default)]
    pub pending_since: Option<String>,
    #[serde(default)]
    pub reviewed_by_user_id: Option<Uuid>,
    #[serde(default)]
    pub reviewed_at: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskStateSnapshot {
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub last_evaluated_at: Option<String>,
    #[serde(default)]
    pub last_evaluated_by: Option<String>,
    #[serde(default)]
    pub needs_attention: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    Open,
    Claimed,
    ApprovalRequested,
    Approved,
    Running,
    Paused,
    PendingAcceptance,
    Accepted,
    Done,
    Failed,
    ManualReview,
    Canceled,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Claimed => "CLAIMED",
            Self::ApprovalRequested => "APPROVAL_REQUESTED",
            Self::Approved => "APPROVED",
            Self::Running => "RUNNING",
            Self::Paused => "PAUSED",
            Self::PendingAcceptance => "PENDING_ACCEPTANCE",
            Self::Accepted => "ACCEPTED",
            Self::Done => "DONE",
            Self::Failed => "FAILED",
            Self::ManualReview => "MANUAL_REVIEW",
            Self::Canceled => "CANCELED",
        }
    }

    pub fn parse_filter(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_uppercase().as_str() {
            "OPEN" => Some(Self::Open),
            "CLAIMED" => Some(Self::Claimed),
            "APPROVAL_REQUESTED" => Some(Self::ApprovalRequested),
            "APPROVED" => Some(Self::Approved),
            "RUNNING" => Some(Self::Running),
            "PAUSED" => Some(Self::Paused),
            "PENDING_ACCEPTANCE" => Some(Self::PendingAcceptance),
            "ACCEPTED" => Some(Self::Accepted),
            "DONE" => Some(Self::Done),
            "FAILED" => Some(Self::Failed),
            "MANUAL_REVIEW" => Some(Self::ManualReview),
            "CANCELED" => Some(Self::Canceled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskPriority {
    High,
    Medium,
    Low,
}

impl TaskPriority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "HIGH",
            Self::Medium => "MEDIUM",
            Self::Low => "LOW",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskActivity {
    pub kind: String,
    pub message: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRuntime {
    pub provider: String,
    pub thread_id: Option<String>,
    pub active_turn_id: Option<String>,
    #[serde(default)]
    pub git_auto_merge_enabled: bool,
    pub log: Vec<RuntimeLogEntry>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLogEntry {
    pub kind: String,
    pub message: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRunRecord {
    pub id: Uuid,
    pub task_id: Uuid,
    pub run_number: u32,
    pub state: String,
    pub provider: String,
    #[serde(default)]
    pub started_by_agent_id: Option<Uuid>,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub primary_workspace_path: Option<String>,
    #[serde(default)]
    pub session_threads: Vec<String>,
    #[serde(default)]
    pub attempts: Vec<TaskRunAttemptRecord>,
    #[serde(default)]
    pub log: Vec<RuntimeLogEntry>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRunAttemptRecord {
    pub id: Uuid,
    pub attempt_number: u32,
    pub trigger_kind: String,
    pub status: String,
    pub prompt: String,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub error_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: Uuid,
    #[serde(default)]
    pub owner_user_id: Option<Uuid>,
    pub name: String,
    pub provider: String,
    pub status: String,
    pub auto_mode: bool,
    pub current_task_id: Option<Uuid>,
    pub last_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingQuestion {
    pub id: Uuid,
    pub project_id: Uuid,
    pub source_task_id: Uuid,
    pub source_task_title: String,
    pub question: String,
    #[serde(default)]
    pub context: Option<String>,
    pub status: String,
    #[serde(default)]
    pub answer: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub answered_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardSnapshot {
    pub current_user: Option<User>,
    pub users: Vec<User>,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub agents: Vec<Agent>,
    #[serde(default)]
    pub task_run_history: HashMap<Uuid, Vec<TaskRunRecord>>,
    #[serde(default)]
    pub pending_questions: Vec<PendingQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    pub project_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub priority: Option<TaskPriority>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub requested_agent_id: Option<Uuid>,
    #[serde(default)]
    pub approval_required: bool,
    #[serde(default)]
    pub acceptance_owner_user_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInvocationRequest {
    pub agent_name_hint: String,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResumeRequest {
    pub agent_name_hint: String,
    pub prompt: String,
}

pub fn new_activity(kind: impl Into<String>, message: impl Into<String>) -> TaskActivity {
    TaskActivity {
        kind: kind.into(),
        message: message.into(),
        at: now_string(),
    }
}

pub fn new_runtime_entry(kind: impl Into<String>, message: impl Into<String>) -> RuntimeLogEntry {
    RuntimeLogEntry {
        kind: kind.into(),
        message: message.into(),
        at: now_string(),
    }
}

pub fn seed_tasks_from_docs(project_id: Uuid) -> Vec<Task> {
    vec![
        seed_task(
            project_id,
            "[0.1.0] 搭建服务端骨架",
            "建立第一版 Rust 服务端，承载项目列表、任务看板、Agent 面板和统一入口页面。",
        ),
        seed_task(
            project_id,
            "[0.1.1] 完成任务看板 MVP",
            "补齐项目切换、任务详情、活动日志、状态筛选和空项目提示，确保项目目录为空时也能正常使用。",
        ),
        seed_task(
            project_id,
            "[0.1.2] 接通真实 Codex 长会话运行时",
            "接入 codex app-server，支持同一任务内的 thread 启动、暂停、补充提示词和恢复执行。",
        ),
        seed_task(
            project_id,
            "[0.1.3] 抽取 Provider 运行时协议",
            "把当前 Codex 运行时抽成通用 Provider/Runtime 抽象，为后续接入 Claude、Kimi、MiniMax 做准备。",
        ),
        seed_task(
            project_id,
            "[0.1.5] 规划后台与运维控制台切片",
            "拆出项目配置、人员和角色管理、系统监控、风险面板和计费配置等后台功能任务。",
        ),
        seed_task(
            project_id,
            "[0.1.7] 规划 AI 洞察与管理能力",
            "补充日报总结、构建失败解释、验收辅助、工期预测和节省 Token 的智能分析场景。",
        ),
    ]
}

pub fn seed_tasks_from_agents_markdown(content: &str, project_id: Uuid) -> Vec<Task> {
    let mut tasks = Vec::new();
    let mut current_version = String::new();
    let mut current_release_title = String::new();
    let mut in_scope = false;
    let mut scope_index = 0_u32;

    for raw_line in content.lines() {
        let line = raw_line.trim();

        if is_release_heading(line) {
            in_scope = false;
            scope_index = 0;
            let heading = line.trim_start_matches('#').trim();
            let after_version = heading
                .split("版本 ")
                .nth(1)
                .or_else(|| heading.split("Version ").nth(1))
                .unwrap_or_default();

            if let Some((version, title)) = after_version.split_once(" - ") {
                current_version = version.trim().to_string();
                current_release_title = title.trim().to_string();
            }
            continue;
        }

        if matches!(line, "范围：" | "范围:" | "Scope:" | "Scope：") {
            in_scope = true;
            continue;
        }

        if in_scope
            && matches!(
                line,
                "必需测试："
                    | "必需测试:"
                    | "Required tests:"
                    | "退出条件："
                    | "退出条件:"
                    | "Exit criteria:"
            )
        {
            in_scope = false;
            continue;
        }

        if in_scope && line.starts_with("- ") && !current_version.is_empty() {
            let bullet = line.trim_start_matches("- ").trim();
            scope_index += 1;
            let title = format!(
                "[{}.{}] {}",
                current_version,
                scope_index,
                normalize_seed_title(bullet)
            );
            let description = format!(
                "来源于 AGENTS.md 版本计划：{} / {}",
                current_release_title, bullet
            );
            tasks.push(seed_task(project_id, &title, &description));
        }
    }

    tasks
}

pub fn seed_demo_tasks(project_id: Uuid, project_name: &str) -> Vec<Task> {
    vec![
        seed_task(
            project_id,
            "梳理当前迭代目标",
            &format!(
                "为项目“{}”整理下一迭代的范围、里程碑、风险和建议任务列表。",
                project_name
            ),
        ),
        seed_task(
            project_id,
            "准备验收清单",
            &format!(
                "为项目“{}”准备一版可执行的验收清单，覆盖功能、风险点和回归建议。",
                project_name
            ),
        ),
    ]
}

pub fn merge_unique_tasks(existing: &mut Vec<Task>, incoming: Vec<Task>) {
    for task in incoming {
        if existing.iter().any(|candidate| {
            candidate.project_id == task.project_id && candidate.title == task.title
        }) {
            continue;
        }
        existing.push(task);
    }
}

pub fn infer_task_priority(title: &str) -> Option<TaskPriority> {
    let version = extract_task_version(title)?;
    let parts = version
        .split('.')
        .map(|part| part.parse::<u32>().ok())
        .collect::<Option<Vec<_>>>()?;
    if parts.len() < 3 {
        return None;
    }
    let major = parts[0];
    let minor = parts[1];
    let patch = parts[2];

    match (major, minor, patch) {
        (0, 1, 0..=3) => Some(TaskPriority::High),
        (0, 1, 4..=7) => Some(TaskPriority::Medium),
        (0, 1, _) => Some(TaskPriority::Low),
        _ => None,
    }
}

pub fn extract_task_version(title: &str) -> Option<String> {
    let version = title
        .trim()
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .map(|(version, _)| version.trim())?;
    let parts = version.split('.').collect::<Vec<_>>();
    if parts.len() < 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.chars().all(|ch| ch.is_ascii_digit()))
    {
        return None;
    }
    Some(version.to_string())
}

fn seed_task(project_id: Uuid, title: &str, description: &str) -> Task {
    Task {
        id: Uuid::new_v4(),
        project_id,
        title: title.into(),
        description: description.into(),
        status: TaskStatus::Open,
        priority: infer_task_priority(title),
        labels: Vec::new(),
        creator_user_id: None,
        assignee_user_id: None,
        assignment_mode: TaskAssignmentMode::PublicQueue,
        requested_agent_id: None,
        source_task_id: None,
        claimed_by: None,
        activities: vec![new_activity(
            "task.seeded",
            "任务根据 Spotlight 文档规划自动生成",
        )],
        runtime: None,
        approval: TaskApprovalState::default(),
        acceptance: TaskAcceptanceState::default(),
        state_snapshot: TaskStateSnapshot::default(),
    }
}

fn is_release_heading(line: &str) -> bool {
    (line.starts_with("## 4.") || line.starts_with("### 4."))
        && (line.contains("版本 ") || line.contains("Version "))
}

fn normalize_seed_title(input: &str) -> String {
    if input.is_ascii() {
        let mut chars = input.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        }
    } else {
        input.to_string()
    }
}

fn now_string() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    nanos.to_string()
}

// ─── 决策收件箱模型 ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    Approval,
    Acceptance,
    Reassess,
    RiskAck,
    ScopeChange,
    Question,
    Conflict,
    Budget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionUrgency {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Pending,
    Resolved,
    Expired,
    Dismissed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionCard {
    pub id: Uuid,
    pub project_id: Uuid,
    #[serde(default)]
    pub task_id: Option<Uuid>,
    pub kind: DecisionKind,
    pub urgency: DecisionUrgency,
    pub title: String,
    pub context: String,
    pub options: Vec<DecisionOption>,
    #[serde(default)]
    pub recommended: Option<String>,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub timeout_action: Option<String>,
    pub status: DecisionStatus,
    pub created_at: String,
    #[serde(default)]
    pub resolved_at: Option<String>,
    #[serde(default)]
    pub resolved_by: Option<Uuid>,
    #[serde(default)]
    pub chosen_option: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        extract_task_version, infer_task_priority, merge_unique_tasks, seed_demo_tasks,
        seed_tasks_from_agents_markdown, seed_tasks_from_docs, TaskPriority, TaskStatus,
    };
    use std::path::Path;
    use uuid::Uuid;

    #[test]
    fn seeded_tasks_cover_bootstrap_and_runtime_slices() {
        let tasks = seed_tasks_from_docs(Uuid::nil());

        assert!(tasks.len() >= 5);
        assert!(tasks
            .iter()
            .all(|task| matches!(task.status, TaskStatus::Open)));
        assert!(tasks
            .iter()
            .any(|task| task.title.contains("[0.1.2] 接通真实 Codex 长会话运行时")));
        assert_eq!(
            tasks
                .iter()
                .find(|task| task.title.starts_with("[0.1.0]"))
                .and_then(|task| task.priority),
            Some(TaskPriority::High)
        );
        assert_eq!(
            tasks
                .iter()
                .find(|task| task.title.starts_with("[0.1.7]"))
                .and_then(|task| task.priority),
            Some(TaskPriority::Medium)
        );
    }

    #[test]
    fn task_status_string_values_are_stable_for_ui_rendering() {
        assert_eq!(TaskStatus::Open.as_str(), "OPEN");
        assert_eq!(TaskStatus::Claimed.as_str(), "CLAIMED");
        assert_eq!(TaskStatus::ApprovalRequested.as_str(), "APPROVAL_REQUESTED");
        assert_eq!(TaskStatus::Approved.as_str(), "APPROVED");
        assert_eq!(TaskStatus::Running.as_str(), "RUNNING");
        assert_eq!(TaskStatus::Paused.as_str(), "PAUSED");
        assert_eq!(TaskStatus::PendingAcceptance.as_str(), "PENDING_ACCEPTANCE");
        assert_eq!(TaskStatus::Accepted.as_str(), "ACCEPTED");
        assert_eq!(TaskStatus::Done.as_str(), "DONE");
        assert_eq!(TaskStatus::Failed.as_str(), "FAILED");
        assert_eq!(TaskStatus::ManualReview.as_str(), "MANUAL_REVIEW");
        assert_eq!(TaskStatus::Canceled.as_str(), "CANCELED");
    }

    #[test]
    fn task_status_filter_parser_accepts_all_current_statuses() {
        let cases = [
            ("open", TaskStatus::Open),
            ("CLAIMED", TaskStatus::Claimed),
            ("approval_requested", TaskStatus::ApprovalRequested),
            ("approved", TaskStatus::Approved),
            ("running", TaskStatus::Running),
            ("paused", TaskStatus::Paused),
            ("pending_acceptance", TaskStatus::PendingAcceptance),
            ("accepted", TaskStatus::Accepted),
            ("done", TaskStatus::Done),
            ("failed", TaskStatus::Failed),
            ("manual_review", TaskStatus::ManualReview),
            ("canceled", TaskStatus::Canceled),
        ];

        for (raw, expected) in cases {
            assert_eq!(TaskStatus::parse_filter(raw), Some(expected));
        }

        assert_eq!(TaskStatus::parse_filter("not_a_status"), None);
    }

    #[test]
    fn task_priority_string_values_are_stable_for_ui_rendering() {
        assert_eq!(TaskPriority::High.as_str(), "HIGH");
        assert_eq!(TaskPriority::Medium.as_str(), "MEDIUM");
        assert_eq!(TaskPriority::Low.as_str(), "LOW");
    }

    #[test]
    fn infer_task_priority_uses_version_bands_for_bootstrap_plan() {
        assert_eq!(
            infer_task_priority("[0.1.0] 搭建服务端骨架"),
            Some(TaskPriority::High)
        );
        assert_eq!(
            infer_task_priority("[0.1.0.2] 最小服务端"),
            Some(TaskPriority::High)
        );
        assert_eq!(
            infer_task_priority("[0.1.6] Agent 状态视图"),
            Some(TaskPriority::Medium)
        );
        assert_eq!(
            infer_task_priority("[0.1.10] 安全、性能、回归基线统一补齐"),
            Some(TaskPriority::Low)
        );
        assert_eq!(infer_task_priority("探索当前目录并生成建议任务"), None);
    }

    #[test]
    fn agents_markdown_release_plan_can_seed_tasks() {
        let markdown = r#"
## 4.1 版本 0.1.0 - 骨架与自举
范围：
- Rust 工作区
- 最小服务端

必需测试：
"#;

        let tasks = seed_tasks_from_agents_markdown(markdown, Uuid::nil());

        assert_eq!(tasks.len(), 2);
        assert!(tasks[0].title.contains("[0.1.0.1] Rust 工作区"));
        assert!(tasks[1].title.contains("[0.1.0.2] 最小服务端"));
    }

    #[test]
    fn extract_task_version_supports_child_suffixes() {
        assert_eq!(
            extract_task_version("[0.1.0.2] 最小服务端"),
            Some("0.1.0.2".into())
        );
        assert_eq!(extract_task_version("[0.1] 非法版本"), None);
    }

    #[test]
    fn merge_unique_tasks_ignores_duplicate_titles_in_same_project() {
        let mut existing = seed_tasks_from_docs(Uuid::nil());
        let original_len = existing.len();
        let duplicate = existing.clone();

        merge_unique_tasks(&mut existing, duplicate);

        assert_eq!(existing.len(), original_len);
    }

    #[test]
    fn different_projects_can_have_same_seed_title() {
        let mut existing = seed_demo_tasks(Uuid::nil(), "项目甲");
        let other = seed_demo_tasks(Uuid::from_u128(2), "项目乙");

        merge_unique_tasks(&mut existing, other);

        assert_eq!(existing.len(), 4);
    }

    #[test]
    fn spotlight_agents_release_plan_includes_admin_console_slices() {
        let agents_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../AGENTS.md");
        let markdown = std::fs::read_to_string(&agents_path).expect("should read root AGENTS.md");

        let tasks = seed_tasks_from_agents_markdown(&markdown, Uuid::nil());
        let titles: Vec<&str> = tasks.iter().map(|task| task.title.as_str()).collect();

        assert!(titles.contains(&"[0.1.5.1] 后台 Web 壳"));
        assert!(titles.contains(&"[0.1.5.2] 项目配置"));
        assert!(titles.contains(&"[0.1.5.3] 人员与能力管理"));
        assert!(titles.contains(&"[0.1.5.4] Agent 与 Runtime 状态"));
        assert!(titles.contains(&"[0.1.5.5] 系统监控面板"));
        assert!(titles.contains(&"[0.1.5.6] 审计和风险中心第一版"));
        assert!(titles.contains(&"[0.1.5.7] 计费与部署配置视图"));
    }
}
