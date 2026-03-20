use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{mpsc, oneshot, Mutex},
    time::{timeout, Duration},
};

pub const CODEX_PROVIDER_ID: &str = "codex";

pub type RuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = RuntimeResult<T>> + Send + 'a>>;
pub type RuntimeResult<T> = Result<T, RuntimeError>;
pub type SharedProviderSession = Arc<dyn ProviderSession>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeErrorKind {
    InvalidRequest,
    NotFound,
    Unsupported,
    Timeout,
    Unavailable,
    Protocol,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub message: String,
}

impl RuntimeError {
    pub fn new(kind: RuntimeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderMode {
    NativeAcp,
    Adapted,
    TextOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub streaming_text: bool,
    pub tool_calls: bool,
    pub session_resume: bool,
    pub command_visibility: bool,
    pub custom_system_prompt: bool,
    pub working_directory_control: bool,
    pub interruption: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderMetadata {
    pub id: String,
    pub display_name: String,
    pub mode: ProviderMode,
    pub capabilities: ProviderCapabilities,
}

#[derive(Debug)]
pub enum RuntimeEvent {
    ThreadStarted { thread_id: String },
    TurnStarted { turn_id: String },
    AgentDelta { delta: String },
    CommandDelta { delta: String },
    PlanDelta { delta: String },
    TurnCompleted { turn_id: String, status: String },
    Error { message: String },
    Stderr { message: String },
    Exited { message: String },
}

pub trait ProviderSession: Send + Sync {
    fn provider_id(&self) -> &str;

    fn start_thread<'a>(
        &'a self,
        cwd: &'a Path,
        developer_instructions: &'a str,
    ) -> RuntimeFuture<'a, String>;

    fn resume_thread<'a>(&'a self, thread_id: &'a str) -> RuntimeFuture<'a, String>;

    fn start_turn<'a>(
        &'a self,
        cwd: &'a Path,
        thread_id: &'a str,
        prompt: &'a str,
    ) -> RuntimeFuture<'a, String>;

    fn interrupt_turn<'a>(&'a self, thread_id: &'a str, turn_id: &'a str) -> RuntimeFuture<'a, ()>;
}

pub trait ProviderAdapter: Send + Sync {
    fn metadata(&self) -> ProviderMetadata;

    fn start_session(
        &self,
        workspace_root: PathBuf,
        event_tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) -> RuntimeFuture<'static, SharedProviderSession>;
}

#[derive(Clone, Default)]
pub struct ProviderRegistry {
    adapters: Arc<HashMap<String, Arc<dyn ProviderAdapter>>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_adapter(mut self, adapter: Arc<dyn ProviderAdapter>) -> Self {
        Arc::make_mut(&mut self.adapters).insert(adapter.metadata().id.clone(), adapter);
        self
    }

    pub fn with_codex(self) -> Self {
        self.with_adapter(Arc::new(CodexProviderAdapter))
    }

    pub fn metadata(&self, provider_id: &str) -> Option<ProviderMetadata> {
        self.adapters
            .get(provider_id)
            .map(|adapter| adapter.metadata())
    }

    pub fn provider_label(&self, provider_id: &str) -> String {
        self.metadata(provider_id)
            .map(|metadata| metadata.display_name)
            .unwrap_or_else(|| provider_id.to_string())
    }

    pub async fn start_session(
        &self,
        provider_id: &str,
        workspace_root: PathBuf,
        event_tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) -> RuntimeResult<SharedProviderSession> {
        let Some(adapter) = self.adapters.get(provider_id).cloned() else {
            return Err(RuntimeError::new(
                RuntimeErrorKind::Unsupported,
                format!("尚未注册 Provider：{provider_id}"),
            ));
        };
        adapter.start_session(workspace_root, event_tx).await
    }
}

pub fn codex_metadata() -> ProviderMetadata {
    ProviderMetadata {
        id: CODEX_PROVIDER_ID.into(),
        display_name: "Codex CLI".into(),
        mode: ProviderMode::NativeAcp,
        capabilities: ProviderCapabilities {
            streaming_text: true,
            tool_calls: true,
            session_resume: true,
            command_visibility: true,
            custom_system_prompt: true,
            working_directory_control: true,
            interruption: true,
        },
    }
}

pub struct CodexProviderAdapter;

impl ProviderAdapter for CodexProviderAdapter {
    fn metadata(&self) -> ProviderMetadata {
        codex_metadata()
    }

    fn start_session(
        &self,
        workspace_root: PathBuf,
        event_tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) -> RuntimeFuture<'static, SharedProviderSession> {
        Box::pin(async move {
            let session = CodexRuntimeSession::spawn(workspace_root, event_tx).await?;
            Ok(session)
        })
    }
}

pub struct CodexRuntimeSession {
    stdin: Mutex<ChildStdin>,
    child: Mutex<Child>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>,
    next_id: AtomicU64,
}

impl CodexRuntimeSession {
    pub async fn spawn(
        workspace_root: PathBuf,
        event_tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) -> RuntimeResult<SharedProviderSession> {
        let mut command = if cfg!(windows) {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", "codex", "app-server", "--listen", "stdio://"]);
            cmd
        } else {
            let mut cmd = Command::new("codex");
            cmd.args(["app-server", "--listen", "stdio://"]);
            cmd
        };

        command
            .current_dir(workspace_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|error| {
            RuntimeError::new(
                RuntimeErrorKind::Unavailable,
                format!("无法启动 Codex App Server：{error}"),
            )
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::Internal, "无法获取 Codex stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::Internal, "无法获取 Codex stdout")
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::Internal, "无法获取 Codex stderr")
        })?;

        let session = Arc::new(Self {
            stdin: Mutex::new(stdin),
            child: Mutex::new(child),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        });

        let stdout_session = session.clone();
        let stdout_events = event_tx.clone();
        tokio::spawn(async move {
            stdout_session.read_stdout(stdout, stdout_events).await;
        });

        let stderr_events = event_tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = stderr_events.send(RuntimeEvent::Stderr {
                    message: trimmed.to_string(),
                });
            }
        });

        let exit_session = session.clone();
        tokio::spawn(async move {
            let result = exit_session.child.lock().await.wait().await;
            let message = match result {
                Ok(status) => format!("Codex App Server 已退出：{status}"),
                Err(error) => format!("等待 Codex App Server 退出时失败：{error}"),
            };
            let _ = event_tx.send(RuntimeEvent::Exited { message });
        });

        session.initialize().await?;
        Ok(session)
    }

    async fn initialize(&self) -> RuntimeResult<()> {
        self.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "spotlight",
                    "title": "Spotlight",
                    "version": "0.1.0"
                },
                "capabilities": {
                    "experimentalApi": false
                }
            }),
        )
        .await
        .map(|_| ())
    }

    async fn request(&self, method: &str, params: Value) -> RuntimeResult<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let request = json!({
            "method": method,
            "id": id,
            "params": params
        });

        {
            let mut stdin = self.stdin.lock().await;
            stdin
                .write_all(request.to_string().as_bytes())
                .await
                .map_err(|error| {
                    RuntimeError::new(
                        RuntimeErrorKind::Internal,
                        format!("写入 Codex App Server 请求失败：{error}"),
                    )
                })?;
            stdin.write_all(b"\n").await.map_err(|error| {
                RuntimeError::new(
                    RuntimeErrorKind::Internal,
                    format!("写入 Codex App Server 换行失败：{error}"),
                )
            })?;
            stdin.flush().await.map_err(|error| {
                RuntimeError::new(
                    RuntimeErrorKind::Internal,
                    format!("刷新 Codex App Server stdin 失败：{error}"),
                )
            })?;
        }

        timeout(Duration::from_secs(60), rx)
            .await
            .map_err(|_| {
                RuntimeError::new(
                    RuntimeErrorKind::Timeout,
                    format!("等待 Codex {method} 响应超时"),
                )
            })?
            .map_err(|_| {
                RuntimeError::new(
                    RuntimeErrorKind::Internal,
                    format!("Codex {method} 响应通道已关闭"),
                )
            })?
            .map_err(|message| RuntimeError::new(RuntimeErrorKind::Protocol, message))
    }

    async fn read_stdout(
        self: Arc<Self>,
        stdout: ChildStdout,
        event_tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(payload) = serde_json::from_str::<Value>(trimmed) else {
                let _ = event_tx.send(RuntimeEvent::Error {
                    message: format!("无法解析 Codex 输出：{trimmed}"),
                });
                continue;
            };

            if let Some(id) = payload.get("id").and_then(Value::as_u64) {
                let tx = self.pending.lock().await.remove(&id);
                if let Some(tx) = tx {
                    if let Some(error) = payload.get("error") {
                        let _ = tx.send(Err(extract_error_message(error)));
                    } else {
                        let _ = tx.send(Ok(payload.get("result").cloned().unwrap_or(Value::Null)));
                    }
                }
                continue;
            }

            let Some(method) = payload.get("method").and_then(Value::as_str) else {
                continue;
            };
            let params = payload.get("params").cloned().unwrap_or(Value::Null);

            match method {
                "thread/started" => {
                    if let Some(thread_id) = params
                        .get("thread")
                        .and_then(|thread| thread.get("id"))
                        .and_then(Value::as_str)
                    {
                        let _ = event_tx.send(RuntimeEvent::ThreadStarted {
                            thread_id: thread_id.to_string(),
                        });
                    }
                }
                "turn/started" => {
                    if let Some(turn_id) = params
                        .get("turn")
                        .and_then(|turn| turn.get("id"))
                        .and_then(Value::as_str)
                    {
                        let _ = event_tx.send(RuntimeEvent::TurnStarted {
                            turn_id: turn_id.to_string(),
                        });
                    }
                }
                "turn/completed" => {
                    let turn_id = params
                        .get("turn")
                        .and_then(|turn| turn.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let status = params
                        .get("turn")
                        .and_then(|turn| turn.get("status"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string();
                    let _ = event_tx.send(RuntimeEvent::TurnCompleted { turn_id, status });
                }
                "item/agentMessage/delta" => {
                    if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                        let _ = event_tx.send(RuntimeEvent::AgentDelta {
                            delta: delta.to_string(),
                        });
                    }
                }
                "item/commandExecution/outputDelta" => {
                    if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                        let _ = event_tx.send(RuntimeEvent::CommandDelta {
                            delta: delta.to_string(),
                        });
                    }
                }
                "item/plan/delta" => {
                    if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                        let _ = event_tx.send(RuntimeEvent::PlanDelta {
                            delta: delta.to_string(),
                        });
                    }
                }
                "error" => {
                    let message = params
                        .get("error")
                        .and_then(|error| error.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("Codex 运行时返回了未知错误")
                        .to_string();
                    let _ = event_tx.send(RuntimeEvent::Error { message });
                }
                _ => {}
            }
        }
    }
}

impl ProviderSession for CodexRuntimeSession {
    fn provider_id(&self) -> &str {
        CODEX_PROVIDER_ID
    }

    fn start_thread<'a>(
        &'a self,
        cwd: &'a Path,
        developer_instructions: &'a str,
    ) -> RuntimeFuture<'a, String> {
        Box::pin(async move {
            let response = self
                .request(
                    "thread/start",
                    json!({
                        "cwd": cwd.to_string_lossy(),
                        "approvalPolicy": "never",
                        "sandbox": "danger-full-access",
                        "developerInstructions": developer_instructions,
                        "experimentalRawEvents": false
                    }),
                )
                .await?;

            extract_thread_id(&response, "thread/start")
        })
    }

    fn resume_thread<'a>(&'a self, thread_id: &'a str) -> RuntimeFuture<'a, String> {
        Box::pin(async move {
            let response = self
                .request(
                    "thread/resume",
                    json!({
                        "threadId": thread_id
                    }),
                )
                .await?;

            extract_thread_id(&response, "thread/resume")
        })
    }

    fn start_turn<'a>(
        &'a self,
        cwd: &'a Path,
        thread_id: &'a str,
        prompt: &'a str,
    ) -> RuntimeFuture<'a, String> {
        Box::pin(async move {
            let response = self
                .request(
                    "turn/start",
                    json!({
                        "threadId": thread_id,
                        "input": [{
                            "type": "text",
                            "text": prompt,
                            "text_elements": []
                        }],
                        "cwd": cwd.to_string_lossy(),
                        "approvalPolicy": "never",
                        "sandboxPolicy": {
                            "type": "dangerFullAccess"
                        }
                    }),
                )
                .await?;

            extract_turn_id(&response, "turn/start")
        })
    }

    fn interrupt_turn<'a>(&'a self, thread_id: &'a str, turn_id: &'a str) -> RuntimeFuture<'a, ()> {
        Box::pin(async move {
            self.request(
                "turn/interrupt",
                json!({
                    "threadId": thread_id,
                    "turnId": turn_id
                }),
            )
            .await
            .map(|_| ())
        })
    }
}

fn extract_thread_id(response: &Value, method: &str) -> RuntimeResult<String> {
    response
        .get("thread")
        .and_then(|thread| thread.get("id"))
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::Protocol,
                format!("Codex {method} 响应里缺少 thread.id"),
            )
        })
}

fn extract_turn_id(response: &Value, method: &str) -> RuntimeResult<String> {
    response
        .get("turn")
        .and_then(|turn| turn.get("id"))
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::Protocol,
                format!("Codex {method} 响应里缺少 turn.id"),
            )
        })
}

fn extract_error_message(error: &Value) -> String {
    error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_else(|| error.as_str().unwrap_or("Provider 返回了未知错误"))
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        codex_metadata, ProviderRegistry, RuntimeErrorKind, RuntimeEvent, CODEX_PROVIDER_ID,
    };
    use std::path::PathBuf;
    use tokio::{
        sync::mpsc,
        time::{timeout, Duration},
    };

    #[test]
    fn codex_metadata_matches_contract() {
        let metadata = codex_metadata();
        assert_eq!(metadata.id, CODEX_PROVIDER_ID);
        assert!(metadata.capabilities.streaming_text);
        assert!(metadata.capabilities.session_resume);
        assert!(metadata.capabilities.interruption);
    }

    #[tokio::test]
    async fn provider_registry_rejects_unknown_provider() {
        let registry = ProviderRegistry::new().with_codex();
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let error = match registry
            .start_session("claude", PathBuf::from("."), event_tx)
            .await
        {
            Ok(_) => panic!("unknown provider should fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind, RuntimeErrorKind::Unsupported);
    }

    #[tokio::test]
    #[ignore = "requires local Codex CLI auth and a working app-server install"]
    async fn real_codex_session_smoke_test() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("failed to resolve workspace root")
            .to_path_buf();

        let registry = ProviderRegistry::new().with_codex();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let session = registry
            .start_session(CODEX_PROVIDER_ID, workspace_root.clone(), event_tx)
            .await
            .expect("failed to spawn real codex session");

        let thread_id = session
            .start_thread(&workspace_root, "You are a concise smoke-test agent.")
            .await
            .expect("failed to start thread");
        assert!(!thread_id.is_empty());

        let turn_id = session
            .start_turn(
                &workspace_root,
                &thread_id,
                "Please reply with one short sentence confirming the app-server session is active.",
            )
            .await
            .expect("failed to start turn");
        assert!(!turn_id.is_empty());

        let mut saw_completion = false;
        let mut saw_error = None;

        while let Ok(Some(event)) = timeout(Duration::from_secs(20), event_rx.recv()).await {
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
            panic!("real codex smoke test received runtime error: {message}");
        }
        assert!(saw_completion, "did not observe a turn completion event");
    }
}
