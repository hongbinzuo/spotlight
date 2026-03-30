use platform_core::BoardSnapshot;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::{
    env, fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
use tauri::Manager;
use url::Url;

#[derive(Serialize)]
struct BackendStatus {
    backend_url: String,
    server_running: bool,
    tcp_connected: bool,
    http_responding: bool,
    auto_launching: bool,
    last_launch_message: Option<String>,
    backend_state: String,
    message: String,
    platform: String,
}

#[derive(Serialize)]
struct BackendProbe {
    backend_url: String,
    tcp_connected: bool,
    http_responding: bool,
    message: String,
}

#[derive(Serialize)]
struct DesktopRestartPlan {
    message: String,
    script_path: String,
    log_path: String,
    workspace_root: String,
    desktop_root: String,
    executable_path: String,
}

#[derive(Default)]
struct BackendLaunchState {
    tracker: Mutex<BackendLaunchTracker>,
}

#[derive(Default)]
struct BackendLaunchTracker {
    last_attempt_at: Option<Instant>,
    last_message: Option<String>,
}

const BACKEND_LAUNCH_COOLDOWN: Duration = Duration::from_secs(8);

#[derive(Clone, Copy)]
struct BackendEndpoint {
    url: &'static str,
    addr: &'static str,
}

const BACKEND_ENDPOINTS: [BackendEndpoint; 2] = [
    BackendEndpoint {
        url: "http://127.0.0.1:3000",
        addr: "127.0.0.1:3000",
    },
    BackendEndpoint {
        url: "http://127.0.0.1:3001",
        addr: "127.0.0.1:3001",
    },
];

#[tauri::command]
fn app_status(launch_state: tauri::State<BackendLaunchState>) -> Result<BackendStatus, String> {
    let backend = preferred_backend_endpoint();
    let tcp_connected = is_backend_running(backend);
    let http_responding = backend_http_responding(backend);
    let running = tcp_connected && http_responding;
    let (backend_state, message) =
        derive_backend_status(&launch_state, backend, tcp_connected, http_responding);
    let (auto_launching, last_launch_message) = backend_launch_snapshot(&launch_state);
    Ok(BackendStatus {
        backend_url: backend.url.to_string(),
        server_running: running,
        tcp_connected,
        http_responding,
        auto_launching,
        last_launch_message,
        backend_state: backend_state.into(),
        message,
        platform: current_platform_label(),
    })
}

#[tauri::command]
fn probe_backend() -> Result<BackendProbe, String> {
    let backend = preferred_backend_endpoint();
    let tcp_connected = is_backend_running(backend);
    let http_responding = backend_http_responding(backend);

    Ok(BackendProbe {
        backend_url: backend.url.to_string(),
        tcp_connected,
        http_responding,
        message: backend_probe_message(backend, tcp_connected, http_responding),
    })
}

#[tauri::command]
fn board_snapshot() -> Result<BoardSnapshot, String> {
    let backend = preferred_backend_endpoint();
    if !backend_http_responding(backend) {
        return Err("本机 Spotlight 服务尚未就绪，暂时无法读取治理快照。".into());
    }

    fetch_backend_json(backend, "/api/v1/board")
}

#[tauri::command]
fn open_backend_in_browser(url: Option<String>) -> Result<(), String> {
    let backend = preferred_backend_endpoint();
    if !is_backend_running(backend) {
        return Err("本机 Spotlight 服务未运行，请先单独启动服务端。".into());
    }

    let target_url = resolve_backend_browser_url(backend, url)?;

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", target_url.as_str()])
            .spawn()
            .map_err(|error| format!("打开浏览器失败：{error}"))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(target_url.as_str())
            .spawn()
            .map_err(|error| format!("打开浏览器失败：{error}"))?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(target_url.as_str())
            .spawn()
            .map_err(|error| format!("打开浏览器失败：{error}"))?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("当前平台暂不支持自动打开浏览器".into())
}

fn resolve_backend_browser_url(
    backend: BackendEndpoint,
    url: Option<String>,
) -> Result<String, String> {
    let raw = url.unwrap_or_else(|| backend.url.to_string());
    let parsed = Url::parse(&raw).map_err(|error| format!("解析浏览器地址失败：{error}"))?;
    let backend = Url::parse(backend.url).map_err(|error| format!("解析后台地址失败：{error}"))?;

    if parsed.scheme() != backend.scheme()
        || parsed.host_str() != backend.host_str()
        || parsed.port_or_known_default() != backend.port_or_known_default()
    {
        return Err("浏览器打开地址必须指向本机 Spotlight 服务。".into());
    }

    Ok(parsed.to_string())
}

#[tauri::command]
fn rebuild_and_restart_desktop(app: tauri::AppHandle) -> Result<DesktopRestartPlan, String> {
    let workspace_root = find_workspace_root()?;
    let desktop_root = workspace_root.join("apps").join("desktop");
    let current_exe =
        env::current_exe().map_err(|error| format!("无法解析当前客户端路径：{error}"))?;
    let executable_path = release_executable_path(&desktop_root, &current_exe);

    if !desktop_root.join("package.json").exists() {
        return Err(format!("未找到桌面工程目录：{}", desktop_root.display()));
    }

    let (script_path, log_path) =
        write_restart_helper_script(
            &workspace_root,
            &desktop_root,
            &executable_path,
            std::process::id(),
        )?;
    spawn_restart_helper(&script_path)?;

    let plan = DesktopRestartPlan {
        message: "已交给外部 helper 处理客户端重编译和重启，当前窗口即将退出".into(),
        script_path: script_path.display().to_string(),
        log_path: log_path.display().to_string(),
        workspace_root: workspace_root.display().to_string(),
        desktop_root: desktop_root.display().to_string(),
        executable_path: executable_path.display().to_string(),
    };

    let app_handle = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        app_handle.exit(0);
    });

    Ok(plan)
}

fn main() {
    tauri::Builder::default()
        .manage(BackendLaunchState::default())
        .invoke_handler(tauri::generate_handler![
            app_status,
            probe_backend,
            board_snapshot,
            open_backend_in_browser,
            rebuild_and_restart_desktop
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();

            thread::spawn(move || {
                for _ in 0..20 {
                    if backend_http_responding(preferred_backend_endpoint()) {
                        let _ = navigate_main_window_to_backend(&app_handle);
                        return;
                    }

                    thread::sleep(Duration::from_millis(500));
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Spotlight desktop");
}

fn preferred_backend_endpoint() -> BackendEndpoint {
    for endpoint in BACKEND_ENDPOINTS {
        if backend_http_responding(endpoint) {
            return endpoint;
        }
    }
    for endpoint in BACKEND_ENDPOINTS {
        if is_backend_running(endpoint) {
            return endpoint;
        }
    }
    BACKEND_ENDPOINTS[0]
}

fn launchable_backend_endpoint() -> Option<BackendEndpoint> {
    BACKEND_ENDPOINTS
        .into_iter()
        .find(|endpoint| !is_backend_running(*endpoint))
}

fn backend_port(endpoint: BackendEndpoint) -> u16 {
    Url::parse(endpoint.url)
        .ok()
        .and_then(|url| url.port_or_known_default())
        .unwrap_or(3000)
}

fn is_backend_running(endpoint: BackendEndpoint) -> bool {
    TcpStream::connect_timeout(
        &endpoint
            .addr
            .parse()
            .expect("backend socket address should be valid"),
        Duration::from_millis(350),
    )
    .is_ok()
}

fn backend_http_responding(endpoint: BackendEndpoint) -> bool {
    let mut stream = match TcpStream::connect_timeout(
        &endpoint
            .addr
            .parse()
            .expect("backend socket address should be valid"),
        Duration::from_secs(1),
    ) {
        Ok(stream) => stream,
        Err(_) => return false,
    };

    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));

    if stream
        .write_all(b"HEAD / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }

    let mut response = [0_u8; 64];
    match stream.read(&mut response) {
        Ok(size) if size > 0 => {
            response[..size].starts_with(b"HTTP/1.1") || response[..size].starts_with(b"HTTP/1.0")
        }
        _ => false,
    }
}

fn current_platform_label() -> String {
    if cfg!(target_os = "windows") {
        "Windows".into()
    } else if cfg!(target_os = "macos") {
        "macOS".into()
    } else if cfg!(target_os = "linux") {
        "Linux".into()
    } else {
        "Unknown".into()
    }
}

fn derive_backend_status<'a>(
    launch_state: &'a BackendLaunchState,
    backend: BackendEndpoint,
    tcp_connected: bool,
    http_responding: bool,
) -> (&'a str, String) {
    if tcp_connected && http_responding {
        clear_backend_launch_tracker(launch_state);
        return ("ready", backend_status_message(backend, true, true));
    }

    if tcp_connected {
        if let Some(launch_endpoint) = launchable_backend_endpoint() {
            if recently_attempted_backend_launch(launch_state) {
                return ("starting", backend_http_starting_message(launch_endpoint));
            }
            return request_backend_launch(launch_state, launch_endpoint);
        }
        if recently_attempted_backend_launch(launch_state) {
            return ("starting", backend_http_starting_message(backend));
        }
        return ("partial", backend_status_message(backend, true, false));
    }

    request_backend_launch(launch_state, preferred_backend_endpoint())
}

fn request_backend_launch<'a>(
    launch_state: &'a BackendLaunchState,
    endpoint: BackendEndpoint,
) -> (&'a str, String) {
    {
        let tracker = launch_state
            .tracker
            .lock()
            .expect("backend launch tracker lock should not be poisoned");
        if tracker
            .last_attempt_at
            .is_some_and(|attempted_at| attempted_at.elapsed() < BACKEND_LAUNCH_COOLDOWN)
        {
            return (
                "starting",
                tracker
                    .last_message
                    .clone()
                    .unwrap_or_else(|| backend_launch_waiting_message(endpoint)),
            );
        }
    }

    let workspace_root = match find_workspace_root() {
        Ok(root) => root,
        Err(error) => {
            return (
                "offline",
                format!(
                    "无法定位 Spotlight 工作区，暂时不能自动拉起本地服务。请先手动运行 `cargo run -p spotlight-server`。{error}"
                ),
            );
        }
    };

    let candidates = backend_launch_candidates(&workspace_root);
    let Some(server_binary) = candidates.iter().find(|path| path.exists()).cloned() else {
        let checked_paths = candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("、");
        return (
            "offline",
            format!(
                "未找到本地 `spotlight-server` 可执行文件。已检查：{checked_paths}。请先运行 `cargo build -p spotlight-server`，或手动启动服务端。"
            ),
        );
    };

    match spawn_backend_process(&server_binary, &workspace_root, endpoint) {
        Ok(()) => {
            let message = backend_launch_message(&server_binary, endpoint);
            let mut tracker = launch_state
                .tracker
                .lock()
                .expect("backend launch tracker lock should not be poisoned");
            tracker.last_attempt_at = Some(Instant::now());
            tracker.last_message = Some(message.clone());
            ("starting", message)
        }
        Err(error) => (
            "offline",
            format!(
                "自动拉起本地 Spotlight 服务失败：{error}。请先手动运行 `cargo run -p spotlight-server`。"
            ),
        ),
    }
}

fn clear_backend_launch_tracker(launch_state: &BackendLaunchState) {
    let mut tracker = launch_state
        .tracker
        .lock()
        .expect("backend launch tracker lock should not be poisoned");
    tracker.last_attempt_at = None;
    tracker.last_message = None;
}

fn backend_launch_snapshot(launch_state: &BackendLaunchState) -> (bool, Option<String>) {
    let tracker = launch_state
        .tracker
        .lock()
        .expect("backend launch tracker lock should not be poisoned");
    let auto_launching = tracker
        .last_attempt_at
        .is_some_and(|attempted_at| attempted_at.elapsed() < BACKEND_LAUNCH_COOLDOWN);
    (auto_launching, tracker.last_message.clone())
}

fn recently_attempted_backend_launch(launch_state: &BackendLaunchState) -> bool {
    launch_state
        .tracker
        .lock()
        .expect("backend launch tracker lock should not be poisoned")
        .last_attempt_at
        .is_some_and(|attempted_at| attempted_at.elapsed() < BACKEND_LAUNCH_COOLDOWN)
}

fn backend_status_message(
    backend: BackendEndpoint,
    tcp_connected: bool,
    http_responding: bool,
) -> String {
    match (tcp_connected, http_responding) {
        (true, true) => "桌面客户端已经连接到本机 Spotlight 服务。".into(),
        (true, false) => {
            format!(
                "检测到 {} 可连接，但服务页面未正常响应。客户端暂不加载内嵌页。",
                backend.addr
            )
        }
        (false, _) => {
            "本机 Spotlight 服务未运行。请单独启动服务端后，再回到客户端刷新连接状态。".into()
        }
    }
}

fn backend_http_starting_message(backend: BackendEndpoint) -> String {
    format!(
        "已经拉起本地 Spotlight 服务进程，正在等待 {} 的 HTTP 接口就绪。客户端会在服务准备好后自动连接。",
        backend.addr
    )
}

fn backend_launch_waiting_message(backend: BackendEndpoint) -> String {
    format!(
        "正在尝试自动拉起本地 Spotlight 服务（目标 {}），请稍候，客户端会自动重连。",
        backend.addr
    )
}

fn backend_launch_message(server_binary: &Path, backend: BackendEndpoint) -> String {
    format!(
        "正在自动拉起本地 Spotlight 服务：{}。目标地址 {}，客户端会在服务就绪后自动连接。",
        server_binary.display(),
        backend.addr
    )
}

fn backend_probe_message(
    backend: BackendEndpoint,
    tcp_connected: bool,
    http_responding: bool,
) -> String {
    match (tcp_connected, http_responding) {
        (true, true) => format!("原生探测成功：{} 可连接，HTTP 已返回响应。", backend.addr),
        (true, false) => format!(
            "原生探测异常：{} 可连接，但 HTTP 没有正常返回。",
            backend.addr
        ),
        (false, _) => format!("原生探测失败：客户端进程无法连接到 {}。", backend.addr),
    }
}

fn fetch_backend_json<T: DeserializeOwned>(
    backend: BackendEndpoint,
    path: &str,
) -> Result<T, String> {
    let response = fetch_backend_response(backend, path)?;
    let body = parse_http_response_body(&response)?;
    serde_json::from_slice::<T>(&body).map_err(|error| format!("解析后台 JSON 响应失败：{error}"))
}

fn fetch_backend_response(backend: BackendEndpoint, path: &str) -> Result<Vec<u8>, String> {
    let mut stream = TcpStream::connect_timeout(
        &backend
            .addr
            .parse()
            .expect("backend socket address should be valid"),
        Duration::from_secs(2),
    )
    .map_err(|error| format!("连接本地 Spotlight 服务失败：{error}"))?;

    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("请求本地 Spotlight 服务失败：{error}"))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("读取本地 Spotlight 服务响应失败：{error}"))?;

    Ok(response)
}

fn parse_http_response_body(response: &[u8]) -> Result<Vec<u8>, String> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "本地 Spotlight 服务响应格式不完整。".to_string())?;

    let header_bytes = &response[..header_end];
    let body_bytes = &response[header_end + 4..];
    let header_text = String::from_utf8_lossy(header_bytes);
    let mut header_lines = header_text.lines();
    let status_line = header_lines
        .next()
        .ok_or_else(|| "本地 Spotlight 服务响应缺少状态行。".to_string())?;
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "本地 Spotlight 服务响应状态行无效。".to_string())?
        .parse::<u16>()
        .map_err(|error| format!("解析本地 Spotlight 服务状态码失败：{error}"))?;

    let is_chunked = header_lines.any(|line| {
        line.to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
    });
    let payload = if is_chunked {
        decode_chunked_body(body_bytes)?
    } else {
        body_bytes.to_vec()
    };

    if !(200..300).contains(&status_code) {
        let preview = String::from_utf8_lossy(&payload);
        return Err(format!(
            "本地 Spotlight 服务返回 {status_code}：{}",
            truncate_for_error(&preview)
        ));
    }

    Ok(payload)
}

fn decode_chunked_body(body: &[u8]) -> Result<Vec<u8>, String> {
    let mut cursor = 0;
    let mut decoded = Vec::new();

    loop {
        let Some(size_end) = body[cursor..]
            .windows(2)
            .position(|window| window == b"\r\n")
        else {
            return Err("chunked 响应缺少大小行。".into());
        };

        let size_bytes = &body[cursor..cursor + size_end];
        let size_text = String::from_utf8_lossy(size_bytes);
        let size_hex = size_text
            .split(';')
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "chunked 响应块大小为空。".to_string())?;
        let chunk_size = usize::from_str_radix(size_hex, 16)
            .map_err(|error| format!("解析 chunked 响应块大小失败：{error}"))?;

        cursor += size_end + 2;
        if chunk_size == 0 {
            return Ok(decoded);
        }

        if body.len() < cursor + chunk_size + 2 {
            return Err("chunked 响应块内容不完整。".into());
        }

        decoded.extend_from_slice(&body[cursor..cursor + chunk_size]);
        cursor += chunk_size + 2;
    }
}

fn truncate_for_error(message: &str) -> String {
    let total = message.chars().count();
    if total <= 160 {
        return message.to_string();
    }

    format!(
        "{}...(已截断，共 {total} 字符)",
        message.chars().take(160).collect::<String>()
    )
}

fn navigate_main_window_to_backend(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "未找到主窗口".to_string())?;

    let backend = preferred_backend_endpoint();
    let url = Url::parse(backend.url).map_err(|error| format!("解析后台地址失败：{error}"))?;

    window
        .navigate(url)
        .map_err(|error| format!("切换到后台页面失败：{error}"))?;

    Ok(())
}

fn find_workspace_root() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(current_dir) = env::current_dir() {
        candidates.push(current_dir);
    }
    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            candidates.push(parent.to_path_buf());
        }
    }

    for candidate in candidates {
        if let Some(root) = find_workspace_root_from(&candidate) {
            return Ok(root);
        }
    }

    Err("无法定位 Spotlight 工作区根目录，无法自动重建客户端".into())
}

fn backend_launch_candidates(workspace_root: &Path) -> Vec<PathBuf> {
    let binary_name = server_binary_name();
    vec![
        workspace_root
            .join("target")
            .join("debug")
            .join(binary_name),
        workspace_root
            .join("target")
            .join("release")
            .join(binary_name),
        workspace_root
            .join("apps")
            .join("desktop")
            .join("src-tauri")
            .join("binaries")
            .join(binary_name),
    ]
}

fn server_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "spotlight-server.exe"
    } else {
        "spotlight-server"
    }
}

fn spawn_backend_process(
    server_binary: &Path,
    workspace_root: &Path,
    endpoint: BackendEndpoint,
) -> Result<(), String> {
    let mut command = Command::new(server_binary);
    command
        .current_dir(workspace_root)
        .env("SPOTLIGHT_SERVER_PORT", backend_port(endpoint).to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法启动 {}：{error}", server_binary.display()))
}

fn find_workspace_root_from(start: &Path) -> Option<PathBuf> {
    for candidate in start.ancestors() {
        let desktop_package = candidate.join("apps").join("desktop").join("package.json");
        let agents = candidate.join("AGENTS.md");
        if desktop_package.exists() && agents.exists() {
            return Some(candidate.to_path_buf());
        }
    }

    None
}

fn release_executable_path(desktop_root: &Path, current_exe: &Path) -> PathBuf {
    let file_name = current_exe
        .file_name()
        .map(|value| value.to_owned())
        .unwrap_or_else(|| "spotlight-desktop.exe".into());

    desktop_root
        .join("src-tauri")
        .join("target")
        .join("release")
        .join(file_name)
}

fn write_restart_helper_script(
    workspace_root: &Path,
    desktop_root: &Path,
    executable_path: &Path,
    pid: u32,
) -> Result<(PathBuf, PathBuf), String> {
    let temp_root = env::temp_dir().join("spotlight-desktop-restart");
    fs::create_dir_all(&temp_root)
        .map_err(|error| format!("无法创建客户端重启辅助目录：{error}"))?;

    let log_path = temp_root.join("rebuild-and-restart.log");
    let script_path = if cfg!(target_os = "windows") {
        temp_root.join("rebuild-and-restart.ps1")
    } else {
        temp_root.join("rebuild-and-restart.sh")
    };

    let content = if cfg!(target_os = "windows") {
        build_windows_restart_script(pid, workspace_root, desktop_root, executable_path, &log_path)
    } else {
        build_unix_restart_script(pid, workspace_root, desktop_root, executable_path, &log_path)
    };

    fs::write(&script_path, content)
        .map_err(|error| format!("无法写入客户端重启辅助脚本：{error}"))?;
    Ok((script_path, log_path))
}

fn spawn_restart_helper(script_path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                &script_path.display().to_string(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| format!("无法启动客户端重启 helper：{error}"))?;
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new("sh")
            .arg(script_path)
            .spawn()
            .map_err(|error| format!("无法启动客户端重启 helper：{error}"))?;
        Ok(())
    }
}

fn build_windows_restart_script(
    pid: u32,
    workspace_root: &Path,
    desktop_root: &Path,
    executable_path: &Path,
    log_path: &Path,
) -> String {
    let workspace_target = workspace_root
        .join("target")
        .display()
        .to_string()
        .replace('\'', "''");
    let desktop_target = desktop_root
        .join("src-tauri")
        .join("target")
        .display()
        .to_string()
        .replace('\'', "''");
    let desktop_root = desktop_root.display().to_string().replace('\'', "''");
    let executable_path = executable_path.display().to_string().replace('\'', "''");
    let log_path = log_path.display().to_string().replace('\'', "''");

    format!(
        r#"$ErrorActionPreference = 'Stop'
$pidToWait = {pid}
$workspaceTarget = '{workspace_target}'
$desktopTarget = '{desktop_target}'
$desktopRoot = '{desktop_root}'
$exePath = '{executable_path}'
$logPath = '{log_path}'

function Write-Log([string]$message) {{
  $timestamp = Get-Date -Format 'yyyy-MM-dd HH:mm:ss'
  Add-Content -Path $logPath -Value ('[' + $timestamp + '] ' + $message)
}}

Set-Content -Path $logPath -Value ''
Write-Log '准备等待旧客户端退出'

for ($i = 0; $i -lt 120; $i++) {{
  if (-not (Get-Process -Id $pidToWait -ErrorAction SilentlyContinue)) {{
    break
  }}
  Start-Sleep -Milliseconds 500
}}

if (Get-Process -Id $pidToWait -ErrorAction SilentlyContinue) {{
  Write-Log '旧客户端长时间未退出，停止本次重启流程'
  exit 1
}}

foreach ($targetPath in @($desktopTarget, $workspaceTarget)) {{
  if (Test-Path $targetPath) {{
    Write-Log ('清理旧 target 目录：' + $targetPath)
    Remove-Item -Recurse -Force $targetPath
  }}
}}

Write-Log '开始执行 npm run tauri build -- --no-bundle'
Set-Location $desktopRoot
npm run tauri build -- --no-bundle *>> $logPath
if ($LASTEXITCODE -ne 0) {{
  Write-Log ('桌面客户端重建失败，退出码：' + $LASTEXITCODE)
  exit $LASTEXITCODE
}}

Write-Log '构建完成，准备重新启动客户端'
Start-Process -FilePath $exePath -WorkingDirectory ([System.IO.Path]::GetDirectoryName($exePath))
Write-Log '客户端已重新启动'
"#,
        workspace_target = workspace_target,
        desktop_target = desktop_target,
    )
}

fn build_unix_restart_script(
    pid: u32,
    workspace_root: &Path,
    desktop_root: &Path,
    executable_path: &Path,
    log_path: &Path,
) -> String {
    format!(
        r#"#!/bin/sh
set -eu
PID_TO_WAIT="{pid}"
WORKSPACE_TARGET="{workspace_target}"
DESKTOP_TARGET="{desktop_target}"
DESKTOP_ROOT="{desktop_root}"
EXE_PATH="{executable_path}"
LOG_PATH="{log_path}"

log() {{
  printf '[%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$1" >> "$LOG_PATH"
}}

: > "$LOG_PATH"
log "准备等待旧客户端退出"

for _ in $(seq 1 120); do
  if ! kill -0 "$PID_TO_WAIT" 2>/dev/null; then
    break
  fi
  sleep 0.5
done

if kill -0 "$PID_TO_WAIT" 2>/dev/null; then
  log "旧客户端长时间未退出，停止本次重启流程"
  exit 1
fi

for target_path in "$DESKTOP_TARGET" "$WORKSPACE_TARGET"; do
  if [ -d "$target_path" ]; then
    log "清理旧 target 目录: $target_path"
    rm -rf "$target_path"
  fi
done

log "开始执行 npm run tauri build -- --no-bundle"
cd "$DESKTOP_ROOT"
npm run tauri build -- --no-bundle >> "$LOG_PATH" 2>&1
log "构建完成，准备重新启动客户端"
"$EXE_PATH" >> "$LOG_PATH" 2>&1 &
log "客户端已重新启动"
"#,
        pid = pid,
        workspace_target = workspace_root.join("target").display(),
        desktop_target = desktop_root.join("src-tauri").join("target").display(),
        desktop_root = desktop_root.display(),
        executable_path = executable_path.display(),
        log_path = log_path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::{
        backend_http_starting_message, backend_launch_candidates, backend_launch_message,
        backend_probe_message, backend_status_message, build_windows_restart_script,
        decode_chunked_body, find_workspace_root_from, parse_http_response_body,
        release_executable_path, resolve_backend_browser_url, server_binary_name,
        BACKEND_ENDPOINTS,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn reports_connected_message_when_server_is_running() {
        assert_eq!(
            backend_status_message(BACKEND_ENDPOINTS[0], true, true),
            "桌面客户端已经连接到本机 Spotlight 服务。"
        );
    }

    #[test]
    fn reports_manual_start_message_when_server_is_not_running() {
        assert_eq!(
            backend_status_message(BACKEND_ENDPOINTS[0], false, false),
            "本机 Spotlight 服务未运行。请单独启动服务端后，再回到客户端刷新连接状态。"
        );
    }

    #[test]
    fn reports_partial_connection_message_when_http_is_not_ready() {
        assert_eq!(
            backend_status_message(BACKEND_ENDPOINTS[0], true, false),
            "检测到 127.0.0.1:3000 可连接，但服务页面未正常响应。客户端暂不加载内嵌页。"
        );
    }

    #[test]
    fn reports_starting_message_after_auto_launch() {
        assert_eq!(
            backend_http_starting_message(BACKEND_ENDPOINTS[0]),
            "已经拉起本地 Spotlight 服务进程，正在等待 127.0.0.1:3000 的 HTTP 接口就绪。客户端会在服务准备好后自动连接。"
        );
    }

    #[test]
    fn reports_probe_success_message() {
        assert_eq!(
            backend_probe_message(BACKEND_ENDPOINTS[0], true, true),
            "原生探测成功：127.0.0.1:3000 可连接，HTTP 已返回响应。"
        );
    }

    #[test]
    fn reports_probe_partial_failure_message() {
        assert_eq!(
            backend_probe_message(BACKEND_ENDPOINTS[0], true, false),
            "原生探测异常：127.0.0.1:3000 可连接，但 HTTP 没有正常返回。"
        );
    }

    #[test]
    fn browser_url_can_include_focus_query_params() {
        let url = resolve_backend_browser_url(
            BACKEND_ENDPOINTS[0],
            Some("http://127.0.0.1:3000/?project_id=project-1&task_id=task-2".into()),
        )
        .unwrap();

        assert!(url.contains("project_id=project-1"));
        assert!(url.contains("task_id=task-2"));
    }

    #[test]
    fn browser_url_rejects_other_hosts() {
        let error =
            resolve_backend_browser_url(BACKEND_ENDPOINTS[0], Some("https://example.com/".into()))
                .unwrap_err();

        assert!(error.contains("Spotlight"));
    }

    #[test]
    fn can_find_workspace_root_from_nested_desktop_path() {
        let base = unique_temp_dir();
        let nested = base
            .join("apps")
            .join("desktop")
            .join("src-tauri")
            .join("target")
            .join("release");
        fs::create_dir_all(base.join("apps").join("desktop")).unwrap();
        fs::create_dir_all(&nested).unwrap();
        fs::write(base.join("AGENTS.md"), "# agents").unwrap();
        fs::write(base.join("apps").join("desktop").join("package.json"), "{}").unwrap();

        let root = find_workspace_root_from(&nested);
        assert_eq!(root, Some(base.clone()));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn windows_restart_script_contains_build_and_relaunch_steps() {
        let script = build_windows_restart_script(
            42,
            Path::new("C:/repo"),
            Path::new("C:/repo/apps/desktop"),
            Path::new("C:/repo/apps/desktop/src-tauri/target/release/spotlight-desktop.exe"),
            Path::new("C:/temp/restart.log"),
        );

        assert!(script.contains("Remove-Item -Recurse -Force"));
        assert!(script.contains("npm run tauri build -- --no-bundle"));
        assert!(script.contains("Start-Process -FilePath $exePath"));
        assert!(script.contains("$pidToWait = 42"));
        assert!(script.contains("C:/repo/target"));
        assert!(script.contains("C:/repo/apps/desktop/src-tauri/target"));
    }

    #[test]
    fn release_executable_points_to_release_target() {
        let path = release_executable_path(
            Path::new("C:/repo/apps/desktop"),
            Path::new("C:/repo/apps/desktop/src-tauri/target/debug/spotlight-desktop.exe"),
        );

        assert_eq!(
            path,
            PathBuf::from("C:/repo/apps/desktop/src-tauri/target/release/spotlight-desktop.exe")
        );
    }

    #[test]
    fn backend_launch_candidates_prefer_workspace_target_outputs() {
        let workspace_root = Path::new("C:/repo");
        let candidates = backend_launch_candidates(workspace_root);

        assert_eq!(
            candidates.first(),
            Some(
                &workspace_root
                    .join("target")
                    .join("debug")
                    .join(server_binary_name())
            )
        );
        assert_eq!(
            candidates.get(1),
            Some(
                &workspace_root
                    .join("target")
                    .join("release")
                    .join(server_binary_name())
            )
        );
    }

    #[test]
    fn backend_launch_message_mentions_binary_path() {
        let message = backend_launch_message(
            Path::new("C:/repo/target/debug/spotlight-server.exe"),
            BACKEND_ENDPOINTS[0],
        );
        assert!(message.contains("spotlight-server.exe"));
        assert!(message.contains("127.0.0.1:3000"));
        assert!(message.contains("自动连接"));
    }

    #[test]
    fn can_decode_chunked_http_body() {
        let decoded = decode_chunked_body(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n").unwrap();
        assert_eq!(decoded, b"Wikipedia");
    }

    #[test]
    fn parse_http_response_body_supports_chunked_payloads() {
        let payload = parse_http_response_body(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n11\r\n{\"ok\":true,\"n\":1}\r\n0\r\n\r\n",
        )
        .unwrap();

        assert_eq!(payload, br#"{"ok":true,"n":1}"#);
    }

    fn unique_temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("spotlight-desktop-test-{nanos}"))
    }
}
