use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    sync::Arc,
};

use axum::http::StatusCode;
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{mpsc, oneshot, Mutex},
    time::{timeout, Duration},
};

type AppResult<T> = Result<T, (StatusCode, String)>;

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
    ) -> AppResult<Arc<Self>> {
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
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("无法启动 Codex App Server：{error}"),
            )
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "无法获取 Codex stdin".into(),
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "无法获取 Codex stdout".into(),
            )
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "无法获取 Codex stderr".into(),
            )
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

    pub async fn start_thread(
        &self,
        cwd: &Path,
        developer_instructions: &str,
    ) -> AppResult<String> {
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

        response
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
            .map(|value| value.to_string())
            .ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Codex thread/start 响应里缺少 thread.id".into(),
                )
            })
    }

    pub async fn start_turn(&self, cwd: &Path, thread_id: &str, prompt: &str) -> AppResult<String> {
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

        response
            .get("turn")
            .and_then(|turn| turn.get("id"))
            .and_then(Value::as_str)
            .map(|value| value.to_string())
            .ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Codex turn/start 响应里缺少 turn.id".into(),
                )
            })
    }

    pub async fn interrupt_turn(&self, thread_id: &str, turn_id: &str) -> AppResult<()> {
        self.request(
            "turn/interrupt",
            json!({
                "threadId": thread_id,
                "turnId": turn_id
            }),
        )
        .await
        .map(|_| ())
    }

    async fn initialize(&self) -> AppResult<()> {
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

    async fn request(&self, method: &str, params: Value) -> AppResult<Value> {
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
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("写入 Codex App Server 请求失败：{error}"),
                    )
                })?;
            stdin.write_all(b"\n").await.map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("写入 Codex App Server 换行失败：{error}"),
                )
            })?;
            stdin.flush().await.map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("刷新 Codex App Server stdin 失败：{error}"),
                )
            })?;
        }

        timeout(Duration::from_secs(60), rx)
            .await
            .map_err(|_| {
                (
                    StatusCode::GATEWAY_TIMEOUT,
                    format!("等待 Codex {method} 响应超时"),
                )
            })?
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Codex {method} 响应通道已关闭"),
                )
            })?
            .map_err(|message| (StatusCode::BAD_GATEWAY, message))
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

fn extract_error_message(error: &Value) -> String {
    error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_else(|| error.as_str().unwrap_or("Codex 返回了未知错误"))
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{CodexRuntimeSession, RuntimeEvent};
    use std::path::PathBuf;
    use tokio::{
        sync::mpsc,
        time::{timeout, Duration},
    };

    #[tokio::test]
    #[ignore = "requires local Codex CLI auth and a working app-server install"]
    async fn real_codex_session_smoke_test() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("failed to resolve workspace root")
            .to_path_buf();

        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let session = CodexRuntimeSession::spawn(workspace_root.clone(), event_tx)
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
