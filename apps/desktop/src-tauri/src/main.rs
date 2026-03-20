use serde::Serialize;
use std::{
    env, fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};
use tauri::Manager;
use url::Url;

#[derive(Serialize)]
struct BackendStatus {
    backend_url: String,
    server_running: bool,
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

const BACKEND_URL: &str = "http://127.0.0.1:3000";
const BACKEND_ADDR: &str = "127.0.0.1:3000";

#[tauri::command]
fn app_status() -> Result<BackendStatus, String> {
    let tcp_connected = is_backend_running();
    let http_responding = backend_http_responding();
    let running = tcp_connected && http_responding;
    Ok(BackendStatus {
        backend_url: BACKEND_URL.to_string(),
        server_running: running,
        message: backend_status_message(tcp_connected, http_responding),
        platform: current_platform_label(),
    })
}

#[tauri::command]
fn probe_backend() -> Result<BackendProbe, String> {
    let tcp_connected = is_backend_running();
    let http_responding = backend_http_responding();

    Ok(BackendProbe {
        backend_url: BACKEND_URL.to_string(),
        tcp_connected,
        http_responding,
        message: backend_probe_message(tcp_connected, http_responding),
    })
}

#[tauri::command]
fn open_backend_in_browser() -> Result<(), String> {
    if !is_backend_running() {
        return Err("本机 Spotlight 服务未运行，请先单独启动服务端。".into());
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", BACKEND_URL])
            .spawn()
            .map_err(|error| format!("打开浏览器失败：{error}"))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(BACKEND_URL)
            .spawn()
            .map_err(|error| format!("打开浏览器失败：{error}"))?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(BACKEND_URL)
            .spawn()
            .map_err(|error| format!("打开浏览器失败：{error}"))?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("当前平台暂不支持自动打开浏览器".into())
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
        write_restart_helper_script(&desktop_root, &executable_path, std::process::id())?;
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
        .invoke_handler(tauri::generate_handler![
            app_status,
            probe_backend,
            open_backend_in_browser,
            rebuild_and_restart_desktop
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();

            thread::spawn(move || {
                for _ in 0..20 {
                    if backend_http_responding() {
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

fn is_backend_running() -> bool {
    TcpStream::connect_timeout(
        &BACKEND_ADDR
            .parse()
            .expect("backend socket address should be valid"),
        Duration::from_millis(350),
    )
    .is_ok()
}

fn backend_http_responding() -> bool {
    let mut stream = match TcpStream::connect_timeout(
        &BACKEND_ADDR
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

fn backend_status_message(tcp_connected: bool, http_responding: bool) -> String {
    match (tcp_connected, http_responding) {
        (true, true) => "桌面客户端已经连接到本机 Spotlight 服务。".into(),
        (true, false) => {
            "检测到 3000 端口可连接，但服务页面未正常响应。客户端暂不加载内嵌页。".into()
        }
        (false, _) => "本机 Spotlight 服务未运行。请单独启动服务端后，再回到客户端刷新连接状态。".into(),
    }
}

fn backend_probe_message(tcp_connected: bool, http_responding: bool) -> String {
    match (tcp_connected, http_responding) {
        (true, true) => "原生探测成功：3000 端口可连接，HTTP 已返回响应。".into(),
        (true, false) => "原生探测异常：3000 端口可连接，但 HTTP 没有正常返回。".into(),
        (false, _) => "原生探测失败：客户端进程无法连接到 127.0.0.1:3000。".into(),
    }
}

fn navigate_main_window_to_backend(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "未找到主窗口".to_string())?;

    let url = Url::parse(BACKEND_URL).map_err(|error| format!("解析后台地址失败：{error}"))?;

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
        build_windows_restart_script(pid, desktop_root, executable_path, &log_path)
    } else {
        build_unix_restart_script(pid, desktop_root, executable_path, &log_path)
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
    desktop_root: &Path,
    executable_path: &Path,
    log_path: &Path,
) -> String {
    let desktop_root = desktop_root.display().to_string().replace('\'', "''");
    let executable_path = executable_path.display().to_string().replace('\'', "''");
    let log_path = log_path.display().to_string().replace('\'', "''");

    format!(
        r#"$ErrorActionPreference = 'Stop'
$pidToWait = {pid}
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
"#
    )
}

fn build_unix_restart_script(
    pid: u32,
    desktop_root: &Path,
    executable_path: &Path,
    log_path: &Path,
) -> String {
    format!(
        r#"#!/bin/sh
set -eu
PID_TO_WAIT="{pid}"
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

log "开始执行 npm run tauri build -- --no-bundle"
cd "$DESKTOP_ROOT"
npm run tauri build -- --no-bundle >> "$LOG_PATH" 2>&1
log "构建完成，准备重新启动客户端"
"$EXE_PATH" >> "$LOG_PATH" 2>&1 &
log "客户端已重新启动"
"#,
        pid = pid,
        desktop_root = desktop_root.display(),
        executable_path = executable_path.display(),
        log_path = log_path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::{
        backend_probe_message, backend_status_message, build_windows_restart_script,
        find_workspace_root_from, release_executable_path,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn reports_connected_message_when_server_is_running() {
        assert_eq!(
            backend_status_message(true, true),
            "桌面客户端已经连接到本机 Spotlight 服务。"
        );
    }

    #[test]
    fn reports_manual_start_message_when_server_is_not_running() {
        assert_eq!(
            backend_status_message(false, false),
            "本机 Spotlight 服务未运行。请单独启动服务端后，再回到客户端刷新连接状态。"
        );
    }

    #[test]
    fn reports_partial_connection_message_when_http_is_not_ready() {
        assert_eq!(
            backend_status_message(true, false),
            "检测到 3000 端口可连接，但服务页面未正常响应。客户端暂不加载内嵌页。"
        );
    }

    #[test]
    fn reports_probe_success_message() {
        assert_eq!(
            backend_probe_message(true, true),
            "原生探测成功：3000 端口可连接，HTTP 已返回响应。"
        );
    }

    #[test]
    fn reports_probe_partial_failure_message() {
        assert_eq!(
            backend_probe_message(true, false),
            "原生探测异常：3000 端口可连接，但 HTTP 没有正常返回。"
        );
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
            Path::new("C:/repo/apps/desktop"),
            Path::new("C:/repo/apps/desktop/src-tauri/target/release/spotlight-desktop.exe"),
            Path::new("C:/temp/restart.log"),
        );

        assert!(script.contains("npm run tauri build -- --no-bundle"));
        assert!(script.contains("Start-Process -FilePath $exePath"));
        assert!(script.contains("$pidToWait = 42"));
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

    fn unique_temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("spotlight-desktop-test-{nanos}"))
    }
}
