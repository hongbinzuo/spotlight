use std::time::{SystemTime, UNIX_EPOCH};

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
    pub creator_user_id: Option<Uuid>,
    #[serde(default)]
    pub assignee_user_id: Option<Uuid>,
    #[serde(default)]
    pub source_task_id: Option<Uuid>,
    pub claimed_by: Option<Uuid>,
    pub activities: Vec<TaskActivity>,
    pub runtime: Option<TaskRuntime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    Open,
    Claimed,
    Running,
    Paused,
    Done,
    Failed,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Claimed => "CLAIMED",
            Self::Running => "RUNNING",
            Self::Paused => "PAUSED",
            Self::Done => "DONE",
            Self::Failed => "FAILED",
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
    pub pending_questions: Vec<PendingQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    pub project_id: Option<Uuid>,
    pub title: String,
    pub description: String,
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

    for raw_line in content.lines() {
        let line = raw_line.trim();

        if is_release_heading(line) {
            in_scope = false;
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
            let title = format!("[{}] {}", current_version, normalize_seed_title(bullet));
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

fn seed_task(project_id: Uuid, title: &str, description: &str) -> Task {
    Task {
        id: Uuid::new_v4(),
        project_id,
        title: title.into(),
        description: description.into(),
        status: TaskStatus::Open,
        creator_user_id: None,
        assignee_user_id: None,
        source_task_id: None,
        claimed_by: None,
        activities: vec![new_activity(
            "task.seeded",
            "任务根据 Spotlight 文档规划自动生成",
        )],
        runtime: None,
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

#[cfg(test)]
mod tests {
    use super::{
        merge_unique_tasks, seed_demo_tasks, seed_tasks_from_agents_markdown, seed_tasks_from_docs,
        TaskStatus,
    };
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
    }

    #[test]
    fn task_status_string_values_are_stable_for_ui_rendering() {
        assert_eq!(TaskStatus::Open.as_str(), "OPEN");
        assert_eq!(TaskStatus::Claimed.as_str(), "CLAIMED");
        assert_eq!(TaskStatus::Running.as_str(), "RUNNING");
        assert_eq!(TaskStatus::Paused.as_str(), "PAUSED");
        assert_eq!(TaskStatus::Done.as_str(), "DONE");
        assert_eq!(TaskStatus::Failed.as_str(), "FAILED");
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
        assert!(tasks[0].title.contains("[0.1.0] Rust 工作区"));
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
}
