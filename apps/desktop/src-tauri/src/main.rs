use serde::Serialize;
use std::{
    io::{Read, Write},
    net::TcpStream,
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

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_status,
            probe_backend,
            open_backend_in_browser
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
        Ok(size) if size > 0 => response[..size].starts_with(b"HTTP/1.1")
            || response[..size].starts_with(b"HTTP/1.0"),
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

#[cfg(test)]
mod tests {
    use super::{backend_probe_message, backend_status_message};

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
}
