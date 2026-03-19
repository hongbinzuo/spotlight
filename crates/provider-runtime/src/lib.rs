use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub streaming_text: bool,
    pub tool_calls: bool,
    pub session_resume: bool,
}

pub fn codex_capabilities() -> ProviderCapabilities {
    ProviderCapabilities {
        streaming_text: true,
        tool_calls: true,
        session_resume: true,
    }
}
