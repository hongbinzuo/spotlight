use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::http::StatusCode;
use provider_runtime::{
    ProviderRegistry, RuntimeError, RuntimeErrorKind, SharedProviderSession, CLAUDE_PROVIDER_ID,
    CODEX_PROVIDER_ID,
};
use tokio::sync::mpsc;

type AppResult<T> = Result<T, (StatusCode, String)>;

pub use provider_runtime::RuntimeEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderLaunchPlan {
    candidate_ids: Vec<&'static str>,
}

/// 默认 Provider 候选链优先使用 Codex，失败后回退到 Claude。
/// 可通过环境变量 `SPOTLIGHT_PROVIDER` 覆盖顺序，支持单值或逗号分隔列表。
fn resolve_provider_launch_plan() -> ProviderLaunchPlan {
    resolve_provider_launch_plan_from_env(std::env::var("SPOTLIGHT_PROVIDER").ok().as_deref())
}

fn resolve_provider_launch_plan_from_env(raw: Option<&str>) -> ProviderLaunchPlan {
    let mut candidate_ids = Vec::new();

    if let Some(raw) = raw {
        for token in raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            push_known_provider_id(&mut candidate_ids, token);
        }
    }

    if candidate_ids.is_empty() {
        candidate_ids.push(CODEX_PROVIDER_ID);
    }

    for fallback_id in [CODEX_PROVIDER_ID, CLAUDE_PROVIDER_ID] {
        if !candidate_ids.contains(&fallback_id) {
            candidate_ids.push(fallback_id);
        }
    }

    ProviderLaunchPlan { candidate_ids }
}

fn push_known_provider_id(candidate_ids: &mut Vec<&'static str>, raw: &str) {
    let provider_id = if raw.eq_ignore_ascii_case(CODEX_PROVIDER_ID) {
        Some(CODEX_PROVIDER_ID)
    } else if raw.eq_ignore_ascii_case(CLAUDE_PROVIDER_ID) {
        Some(CLAUDE_PROVIDER_ID)
    } else {
        None
    };

    if let Some(provider_id) = provider_id {
        if !candidate_ids.contains(&provider_id) {
            candidate_ids.push(provider_id);
        }
    }
}

fn default_registry() -> ProviderRegistry {
    ProviderRegistry::new().with_claude().with_codex()
}

pub struct ProviderRuntimeSession {
    inner: SharedProviderSession,
}

impl ProviderRuntimeSession {
    pub async fn spawn(
        workspace_root: PathBuf,
        event_tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) -> AppResult<Arc<Self>> {
        let registry = default_registry();
        let plan = resolve_provider_launch_plan();
        let candidate_count = plan.candidate_ids.len();
        let mut failures = Vec::new();
        let mut last_kind = RuntimeErrorKind::Unavailable;

        for (index, provider_id) in plan.candidate_ids.into_iter().enumerate() {
            match registry
                .start_session(provider_id, workspace_root.clone(), event_tx.clone())
                .await
            {
                Ok(session) => return Ok(Arc::new(Self { inner: session })),
                Err(error) => {
                    last_kind = error.kind.clone();
                    failures.push(format!("{provider_id}: {}", error.message));
                    if index + 1 < candidate_count {
                        eprintln!(
                            "Provider ({provider_id}) 启动失败：{}；尝试下一个候选 Provider",
                            error.message
                        );
                    }
                }
            }
        }

        Err(runtime_error_to_app(RuntimeError::new(
            last_kind,
            format!("所有候选 Provider 均不可用：{}", failures.join("；")),
        )))
    }

    pub fn provider_id(&self) -> &str {
        self.inner.provider_id()
    }

    pub async fn start_thread(
        &self,
        cwd: &Path,
        developer_instructions: &str,
    ) -> AppResult<String> {
        self.inner
            .start_thread(cwd, developer_instructions)
            .await
            .map_err(runtime_error_to_app)
    }

    pub async fn resume_thread(&self, thread_id: &str) -> AppResult<String> {
        self.inner
            .resume_thread(thread_id)
            .await
            .map_err(runtime_error_to_app)
    }

    pub async fn start_turn(&self, cwd: &Path, thread_id: &str, prompt: &str) -> AppResult<String> {
        self.inner
            .start_turn(cwd, thread_id, prompt)
            .await
            .map_err(runtime_error_to_app)
    }

    pub async fn interrupt_turn(&self, thread_id: &str, turn_id: &str) -> AppResult<()> {
        self.inner
            .interrupt_turn(thread_id, turn_id)
            .await
            .map_err(runtime_error_to_app)
    }

    pub async fn shutdown(&self) {
        let _ = self.inner.shutdown().await;
    }
}

// 兼容仍在拆分中的旧模块命名，避免一次性重写整个服务端。
#[allow(dead_code)]
pub type CodexRuntimeSession = ProviderRuntimeSession;

fn runtime_error_to_app(error: RuntimeError) -> (StatusCode, String) {
    let status = match error.kind {
        RuntimeErrorKind::InvalidRequest => StatusCode::BAD_REQUEST,
        RuntimeErrorKind::NotFound => StatusCode::NOT_FOUND,
        RuntimeErrorKind::Unsupported => StatusCode::NOT_IMPLEMENTED,
        RuntimeErrorKind::Timeout => StatusCode::GATEWAY_TIMEOUT,
        RuntimeErrorKind::Unavailable | RuntimeErrorKind::Protocol => StatusCode::BAD_GATEWAY,
        RuntimeErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.message)
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_provider_launch_plan_from_env, runtime_error_to_app, ProviderRuntimeSession,
        RuntimeEvent,
    };
    use axum::http::StatusCode;
    use provider_runtime::{RuntimeError, RuntimeErrorKind, CLAUDE_PROVIDER_ID, CODEX_PROVIDER_ID};
    use std::path::PathBuf;
    use tokio::{
        sync::mpsc,
        time::{timeout, Duration},
    };

    #[test]
    fn runtime_error_kind_maps_to_expected_status_code() {
        let cases = [
            (RuntimeErrorKind::InvalidRequest, StatusCode::BAD_REQUEST),
            (RuntimeErrorKind::NotFound, StatusCode::NOT_FOUND),
            (RuntimeErrorKind::Unsupported, StatusCode::NOT_IMPLEMENTED),
            (RuntimeErrorKind::Timeout, StatusCode::GATEWAY_TIMEOUT),
            (RuntimeErrorKind::Unavailable, StatusCode::BAD_GATEWAY),
            (RuntimeErrorKind::Protocol, StatusCode::BAD_GATEWAY),
            (
                RuntimeErrorKind::Internal,
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];

        for (kind, expected_status) in cases {
            let (actual_status, message) = runtime_error_to_app(RuntimeError::new(kind, "test"));
            assert_eq!(actual_status, expected_status);
            assert_eq!(message, "test");
        }
    }

    #[test]
    fn defaults_to_codex_then_claude() {
        let plan = resolve_provider_launch_plan_from_env(None);
        assert_eq!(
            plan.candidate_ids,
            vec![CODEX_PROVIDER_ID, CLAUDE_PROVIDER_ID]
        );
    }

    #[test]
    fn honors_explicit_provider_override() {
        let plan = resolve_provider_launch_plan_from_env(Some("claude"));
        assert_eq!(
            plan.candidate_ids,
            vec![CLAUDE_PROVIDER_ID, CODEX_PROVIDER_ID]
        );
    }

    #[test]
    fn supports_multiple_provider_candidates_and_ignores_unknown_values() {
        let plan = resolve_provider_launch_plan_from_env(Some("unknown, claude , codex, claude"));
        assert_eq!(
            plan.candidate_ids,
            vec![CLAUDE_PROVIDER_ID, CODEX_PROVIDER_ID]
        );
    }

    #[tokio::test]
    #[ignore = "requires local Claude or Codex CLI install"]
    async fn real_session_smoke_test() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("failed to resolve workspace root")
            .to_path_buf();

        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let session = ProviderRuntimeSession::spawn(workspace_root.clone(), event_tx)
            .await
            .expect("failed to spawn runtime session");

        let thread_id = session
            .start_thread(&workspace_root, "You are a concise smoke-test agent.")
            .await
            .expect("failed to start thread");
        assert!(!thread_id.is_empty());

        let turn_id = session
            .start_turn(
                &workspace_root,
                &thread_id,
                "Please reply with one short sentence confirming the session is active.",
            )
            .await
            .expect("failed to start turn");
        assert!(!turn_id.is_empty());

        let mut saw_completion = false;
        let mut saw_error = None;

        while let Ok(Some(event)) = timeout(Duration::from_secs(30), event_rx.recv()).await {
            match event {
                RuntimeEvent::TurnCompleted { status, .. } => {
                    saw_completion = status == "completed" || status == "interrupted";
                    break;
                }
                RuntimeEvent::Error { message } => {
                    saw_error = Some(message);
                    break;
                }
                _ => {}
            }
        }

        if let Some(message) = saw_error {
            session.shutdown().await;
            panic!("smoke test received runtime error: {message}");
        }
        session.shutdown().await;
        assert!(saw_completion, "did not observe a turn completion event");
    }
}
