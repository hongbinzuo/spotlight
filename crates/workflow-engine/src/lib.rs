use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTemplate {
    pub key: String,
    pub version: u32,
    pub states: Vec<String>,
}

impl WorkflowTemplate {
    pub fn spotlight_mvp() -> Self {
        Self {
            key: "spotlight_mvp_task_flow".into(),
            version: 1,
            states: vec![
                "open".into(),
                "claimed".into(),
                "running".into(),
                "agent_done".into(),
                "pending_acceptance".into(),
            ],
        }
    }
}
