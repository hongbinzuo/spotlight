use platform_core::{Agent, PendingQuestion, Project, RuntimeLogEntry, Task, User, WorkspaceRoot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct TaskExecutionContext {
    pub workspace_root: std::path::PathBuf,
    pub prompt: String,
}

pub(crate) struct ResolvedProviderRuntimeSession {
    pub session: std::sync::Arc<crate::runtime::ProviderRuntimeSession>,
    pub thread_id: String,
    pub event_rx: Option<tokio::sync::mpsc::UnboundedReceiver<crate::runtime::RuntimeEvent>>,
}

#[allow(dead_code)]
pub(crate) type ResolvedRuntimeSession = ResolvedProviderRuntimeSession;

#[derive(Debug, Deserialize)]
pub(crate) struct CloudInstallRestartTaskRequest {
    pub host: String,
    pub port: Option<u16>,
    pub username: String,
    pub auth_method: Option<String>,
    pub credential_hint: Option<String>,
    pub deploy_path: Option<String>,
    pub service_hint: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct StackDetection {
    pub stacks: Vec<&'static str>,
    pub evidence: Vec<String>,
}

impl StackDetection {
    pub fn summary(&self) -> String {
        if self.stacks.is_empty() {
            "未识别到常见构建清单；当前目录可能是文档目录、交付目录，或需要在更深层继续探索。"
                .into()
        } else if self.evidence.is_empty() {
            format!("{}", self.stacks.join("、"))
        } else {
            format!(
                "{}（线索：{}）",
                self.stacks.join("、"),
                self.evidence.join("，")
            )
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectScanSummary {
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub workspace_label: String,
    pub workspace_path: String,
    pub scanned_at: String,
    pub stack_summary: String,
    pub detected_stacks: Vec<String>,
    pub top_level_entries: Vec<String>,
    pub key_files: Vec<String>,
    pub document_files: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectSessionMessage {
    pub role: String,
    pub content: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectChatMessage {
    pub id: Uuid,
    pub project_id: Uuid,
    pub user_id: Option<Uuid>,
    pub user_display_name: String,
    pub content: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectSession {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    #[serde(default = "default_project_session_mode")]
    pub mode: String,
    pub status: String,
    pub workspace_path: Option<String>,
    pub thread_id: Option<String>,
    pub active_turn_id: Option<String>,
    pub messages: Vec<ProjectSessionMessage>,
    pub log: Vec<RuntimeLogEntry>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MemoryItem {
    pub id: Uuid,
    pub scope_kind: String,
    pub scope_id: Uuid,
    pub memory_kind: String,
    pub stable_key: String,
    pub created_at: String,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MemoryRevision {
    pub id: Uuid,
    pub memory_item_id: Uuid,
    pub revision_no: u32,
    pub status: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub structured_payload: Option<Value>,
    pub source_kind: String,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub supersedes_revision_id: Option<Uuid>,
    pub created_at: String,
    #[serde(default)]
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MemoryTag {
    pub id: Uuid,
    pub memory_item_id: Uuid,
    pub tag: String,
    pub target_revision_id: Uuid,
    pub updated_at: String,
    #[serde(default)]
    pub updated_by: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MemoryEdge {
    pub id: Uuid,
    pub from_revision_id: Uuid,
    pub to_revision_id: Uuid,
    pub edge_kind: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GitTaskBranchPlan {
    pub base_branch: String,
    pub task_branch: String,
    pub remote_name: Option<String>,
}

pub(crate) struct GitPrepareResult {
    pub activities: Vec<(String, String)>,
    pub auto_merge_enabled: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoginRequest {
    pub username: String,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ListProjectTasksQuery {
    pub status: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AuthSnapshot {
    pub current_user: Option<platform_core::User>,
    pub users: Vec<platform_core::User>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PullNextResponse {
    pub task: Option<platform_core::Task>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ProjectContextSnapshot {
    pub project_id: Uuid,
    pub primary_workspace: Option<WorkspaceRoot>,
    pub latest_scan: Option<ProjectScanSummary>,
    pub sessions: Vec<ProjectSession>,
    #[serde(default)]
    pub chat_messages: Vec<ProjectChatMessage>,
    pub memory: ProjectMemorySnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectSummarySnapshot {
    pub project_id: Uuid,
    pub project_name: String,
    pub generated_at: String,
    pub primary_workspace: Option<WorkspaceRoot>,
    pub latest_scan: Option<ProjectScanSummary>,
    pub task_counts: ProjectTaskStatusCounts,
    pub agent_summary: ProjectAgentSummary,
    pub session_summary: ProjectSessionSummary,
    pub open_pending_question_count: usize,
    pub pending_questions: Vec<ProjectPendingQuestionDigest>,
    pub active_constraints: Vec<ProjectConstraintDigest>,
    pub recent_task_summaries: Vec<ProjectTaskSummaryDigest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ProjectTaskStatusCounts {
    pub open: usize,
    pub claimed: usize,
    pub running: usize,
    pub paused: usize,
    pub done: usize,
    pub failed: usize,
    pub canceled: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ProjectAgentSummary {
    pub total: usize,
    pub auto_mode_enabled: usize,
    pub busy: usize,
    pub idle: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ProjectSessionSummary {
    pub total: usize,
    pub running: usize,
    pub paused: usize,
    pub completed: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectPendingQuestionDigest {
    pub id: Uuid,
    pub source_task_id: Uuid,
    pub source_task_title: String,
    pub question: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectConstraintDigest {
    pub stable_key: String,
    pub title: String,
    pub content: String,
    pub revision_no: u32,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectTaskSummaryDigest {
    pub task_id: Uuid,
    pub task_title: String,
    pub summary: String,
    pub created_at: String,
    pub source_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ProjectMemorySnapshot {
    #[serde(default)]
    pub items: Vec<MemoryItem>,
    #[serde(default)]
    pub revisions: Vec<MemoryRevision>,
    #[serde(default)]
    pub tags: Vec<MemoryTag>,
    #[serde(default)]
    pub edges: Vec<MemoryEdge>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegisterWorkspaceRequest {
    pub label: String,
    pub path: String,
    pub is_primary_default: Option<bool>,
    pub is_writable: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StartProjectSessionRequest {
    pub title: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    pub prompt: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ContinueProjectSessionRequest {
    pub prompt: String,
}

pub(crate) fn default_project_session_mode() -> String {
    "general".into()
}

pub(crate) fn normalize_project_session_mode(mode: Option<&str>) -> &'static str {
    match mode.map(str::trim).filter(|value| !value.is_empty()) {
        Some(raw) if raw.eq_ignore_ascii_case("planner") => "planner",
        Some(raw) if raw.eq_ignore_ascii_case("evaluator") => "evaluator",
        _ => "general",
    }
}

pub(crate) fn project_session_mode_label(mode: &str) -> &'static str {
    match normalize_project_session_mode(Some(mode)) {
        "planner" => "规划器",
        "evaluator" => "评估器",
        _ => "普通会话",
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnswerPendingQuestionRequest {
    pub answer: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CancelTaskRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProjectChatRequest {
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpsertProjectConstraintRequest {
    pub stable_key: Option<String>,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TaskCompletionReport {
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub questions: Vec<TaskCompletionQuestion>,
    #[serde(default)]
    pub follow_ups: Vec<TaskCompletionFollowUp>,
    #[serde(default)]
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum TaskCompletionQuestion {
    Text(String),
    Detailed {
        question: String,
        #[serde(default)]
        context: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TaskCompletionFollowUp {
    #[serde(default)]
    pub kind: Option<String>,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub can_auto_create_task: Option<bool>,
    #[serde(default)]
    pub can_auto_apply: Option<bool>,
}

#[derive(Clone, Copy)]
pub(crate) enum RuntimeMode {
    RealCodex,
    Stub,
}

pub(crate) struct MemoryWriteSpec {
    pub scope_kind: &'static str,
    pub scope_id: Uuid,
    pub memory_kind: &'static str,
    pub stable_key: String,
    pub tag: String,
    pub title: String,
    pub content: String,
    pub structured_payload: Option<Value>,
    pub source_kind: &'static str,
    pub source_id: Option<String>,
    pub confidence: Option<f32>,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PersistedState {
    pub users: Vec<User>,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub agents: Vec<Agent>,
    #[serde(default)]
    pub task_run_history: std::collections::HashMap<Uuid, Vec<platform_core::TaskRunRecord>>,
    #[serde(default)]
    pub execution_slots: Vec<platform_core::ExecutionSlotRecord>,
    #[serde(default)]
    pub workspace_leases: Vec<platform_core::WorkspaceLeaseRecord>,
    #[serde(default)]
    pub coordination_write_intents: Vec<platform_core::CoordinationWriteIntent>,
    #[serde(default)]
    pub pending_questions: Vec<PendingQuestion>,
    #[serde(default)]
    pub project_scans: std::collections::HashMap<Uuid, ProjectScanSummary>,
    #[serde(default)]
    pub project_sessions: Vec<ProjectSession>,
    #[serde(default)]
    pub project_chat_messages: Vec<ProjectChatMessage>,
    #[serde(default)]
    pub memory_items: Vec<MemoryItem>,
    #[serde(default)]
    pub memory_revisions: Vec<MemoryRevision>,
    #[serde(default)]
    pub memory_tags: Vec<MemoryTag>,
    #[serde(default)]
    pub memory_edges: Vec<MemoryEdge>,
    #[serde(default)]
    pub decisions: Vec<platform_core::DecisionCard>,
}
