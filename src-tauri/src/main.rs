// SenClaw desktop app — Tauri 2.0 shell that embeds the SenClaw daemon
// in-process and exposes a compact chat window from the menu bar / tray.
//
// The diagnostics window (label "diagnostics") is loaded from the bundled
// frontendDist and works WITHOUT the daemon, so the user can manage port
// conflicts and read logs even when the daemon failed to start.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};
use tracing_subscriber::prelude::*;

const UI_PORT: u16 = 18788;
const WS_PORT: u16 = 18789;
const CHAT_WINDOW: &str = "chat";
const MAIN_WINDOW: &str = "main";
const DIAG_WINDOW: &str = "diagnostics";
const LOG_BUFFER_CAP: usize = 2000;

type LogBuffer = Arc<Mutex<VecDeque<String>>>;

#[derive(Default)]
struct DaemonHandle {
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    last_error: Option<String>,
    started_at: Option<std::time::SystemTime>,
}

struct AppState {
    logs: LogBuffer,
    daemon: Mutex<DaemonHandle>,
    daemon_alive: AtomicBool,
}

fn main() {
    let _ = dotenvy::dotenv();

    let logs: LogBuffer = Arc::new(Mutex::new(VecDeque::with_capacity(LOG_BUFFER_CAP)));
    init_tracing(logs.clone());

    let state = Arc::new(AppState {
        logs,
        daemon: Mutex::new(DaemonHandle::default()),
        daemon_alive: AtomicBool::new(false),
    });

    let state_for_setup = state.clone();

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            app_status,
            app_logs,
            kill_port,
            restart_daemon,
            open_window,
        ])
        .setup(move |app| {
            // Menu-bar-only on macOS (no Dock icon).
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Positioner: lets us anchor the chat popover under the tray icon.
            app.handle().plugin(tauri_plugin_positioner::init())?;

            configure_env(app);
            build_tray(app)?;

            // Hide windows instead of quitting when closed; the app keeps
            // living in the menu bar until "Quit".
            for label in [CHAT_WINDOW, MAIN_WINDOW, DIAG_WINDOW] {
                if let Some(win) = app.get_webview_window(label) {
                    let w = win.clone();
                    win.on_window_event(move |event| {
                        if let WindowEvent::CloseRequested { api, .. } = event {
                            api.prevent_close();
                            let _ = w.hide();
                        }
                    });
                }
            }

            // OpenClaw-style popover: the chat window auto-hides on blur.
            if let Some(chat) = app.get_webview_window(CHAT_WINDOW) {
                let c = chat.clone();
                chat.on_window_event(move |event| {
                    if let WindowEvent::Focused(false) = event {
                        let _ = c.hide();
                    }
                });
            }

            // Spawn the embedded daemon and arm a watcher to reload the
            // (hidden) http-served windows once the UI port is up.
            spawn_daemon(app.handle().clone(), state_for_setup.clone());

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                wait_for_port(UI_PORT).await;
                for label in [CHAT_WINDOW, MAIN_WINDOW] {
                    if let Some(win) = handle.get_webview_window(label) {
                        let _ = win.eval("window.location.reload()");
                    }
                }
                // App launch UX: auto-open the full chat window so the user
                // lands in chat instead of a hidden menu-bar app. The chat
                // popover stays hidden until they click the tray icon.
                show_window_named(&handle, MAIN_WINDOW);
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running SenClaw app");
}

// ===== Tracing — capture logs into both stderr and an in-memory ring buffer.

fn init_tracing(buf: LogBuffer) {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let buffer_layer = tracing_subscriber::fmt::layer()
        .with_writer(BufMakeWriter(buf))
        .with_ansi(false);
    tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(buffer_layer)
        .init();
}

#[derive(Clone)]
struct BufMakeWriter(LogBuffer);

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufMakeWriter {
    type Writer = BufWriter;
    fn make_writer(&'a self) -> Self::Writer {
        BufWriter {
            buf: self.0.clone(),
            partial: Vec::new(),
        }
    }
}

struct BufWriter {
    buf: LogBuffer,
    partial: Vec<u8>,
}

impl std::io::Write for BufWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.partial.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for BufWriter {
    fn drop(&mut self) {
        if self.partial.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(&self.partial).to_string();
        if let Ok(mut q) = self.buf.lock() {
            for line in text.split_inclusive('\n') {
                let cleaned = line.trim_end_matches('\n').to_string();
                if cleaned.is_empty() {
                    continue;
                }
                q.push_back(cleaned);
                while q.len() > LOG_BUFFER_CAP {
                    q.pop_front();
                }
            }
        }
    }
}

// ===== Daemon supervision.

fn spawn_daemon(handle: tauri::AppHandle, state: Arc<AppState>) {
    {
        let mut d = state.daemon.lock().unwrap();
        d.last_error = None;
        d.started_at = Some(std::time::SystemTime::now());
    }
    state.daemon_alive.store(true, Ordering::SeqCst);
    let state2 = state.clone();
    let handle2 = handle.clone();
    let task = tauri::async_runtime::spawn(async move {
        let cfg = build_config();
        let outcome = senclaw::run_daemon(cfg).await;
        state2.daemon_alive.store(false, Ordering::SeqCst);
        match outcome {
            Ok(()) => {
                tracing::warn!("[senclaw-app] daemon returned (clean exit)");
                let mut d = state2.daemon.lock().unwrap();
                d.last_error = Some("daemon returned (clean exit)".into());
            }
            Err(e) => {
                tracing::error!("[senclaw-app] daemon exited: {e:#}");
                let mut d = state2.daemon.lock().unwrap();
                d.last_error = Some(format!("{e:#}"));
                // Don't auto-open Diagnostics — user opens it from the tray
                // when they want to investigate. The error is stored on the
                // status panel and surfaced there.
                let _ = handle2;
            }
        }
    });
    let mut d = state.daemon.lock().unwrap();
    d.task = Some(task);
}

/// Build `Config` exactly like the CLI `Start` branch (env + persisted overrides).
fn build_config() -> senclaw::config::Config {
    let mut cfg = senclaw::config::Config::from_env();
    let gcp = cfg.paths.global_config_path.clone();
    cfg.apply_persisted_overrides(&gcp);
    cfg
}

/// Point the embedded daemon at the bundled CLI binary (for MCP subprocesses)
/// and the bundled `web/dist` (for the UI), unless already set in the env.
fn configure_env(app: &tauri::App) {
    let resource_dir = app.path().resource_dir().ok();

    if std::env::var_os("SENCLAW_BIN").is_none() {
        let bin_name = if cfg!(windows) {
            "senclaw.exe"
        } else {
            "senclaw"
        };
        let mut candidates = Vec::new();
        if let Some(res) = &resource_dir {
            candidates.push(res.join("binaries").join(bin_name));
            candidates.push(res.join(bin_name));
        }
        // Dev: sibling of the app binary (target/debug/senclaw).
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join(bin_name));
            }
        }
        if let Some(p) = candidates.into_iter().find(|p| p.exists()) {
            std::env::set_var("SENCLAW_BIN", p);
        }
    }

    if std::env::var_os("SENCLAW_WEB_DIST").is_none() {
        if let Some(res) = &resource_dir {
            for cand in [res.join("web").join("dist"), res.join("web/dist")] {
                if cand.exists() {
                    std::env::set_var("SENCLAW_WEB_DIST", cand);
                    break;
                }
            }
        }
    }
}

// ===== Tray.

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let open_chat = MenuItemBuilder::with_id("open_chat", "Open Chat").build(app)?;
    let open_dash = MenuItemBuilder::with_id("open_dashboard", "Open Full Window").build(app)?;
    let fullscreen =
        MenuItemBuilder::with_id("toggle_fullscreen", "Toggle Fullscreen").build(app)?;
    let diagnostics =
        MenuItemBuilder::with_id("open_diagnostics", "Diagnostics…").build(app)?;
    let open_browser = MenuItemBuilder::with_id("open_browser", "Open in Browser").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[
            &open_chat,
            &open_dash,
            &fullscreen,
            &diagnostics,
            &open_browser,
            &quit,
        ])
        .build()?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open_chat" => popup_chat(app),
            "open_dashboard" => show_window_named(app, MAIN_WINDOW),
            "toggle_fullscreen" => toggle_fullscreen(app),
            "open_diagnostics" => show_window_named(app, DIAG_WINDOW),
            "open_browser" => open_in_browser_or_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                popup_chat(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn show_window_named(app: &tauri::AppHandle, label: &str) {
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.show();
        let _ = win.unminimize();
        if label == MAIN_WINDOW {
            let _ = win.maximize();
        }
        let _ = win.set_focus();
    }
}

fn toggle_fullscreen(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window(MAIN_WINDOW) {
        let _ = win.show();
        let on = win.is_fullscreen().unwrap_or(false);
        let _ = win.set_fullscreen(!on);
        let _ = win.set_focus();
    }
}

/// Show the chat popover anchored under the menu-bar icon (OpenClaw-style).
fn popup_chat(app: &tauri::AppHandle) {
    use tauri_plugin_positioner::{Position, WindowExt};
    if let Some(win) = app.get_webview_window(CHAT_WINDOW) {
        let _ = win.move_window(Position::TrayBottomCenter);
        let _ = win.show();
        let _ = win.set_focus();
    }
}

fn open_in_browser_or_window(app: &tauri::AppHandle) {
    let url = format!("http://127.0.0.1:{UI_PORT}");
    if let Err(e) = open_in_browser(&url) {
        tracing::warn!("[senclaw-app] failed to open browser: {e}");
        show_window_named(app, MAIN_WINDOW);
    }
}

fn open_in_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");

    cmd.arg(url).spawn().map(|_| ())
}

async fn wait_for_port(port: u16) {
    for _ in 0..600 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    tracing::warn!("[senclaw-app] UI server did not come up on port {port}");
}

// ===== Port helpers (macOS / Linux via lsof).

fn lsof_pid(port: u16) -> Option<u32> {
    let out = std::process::Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-F", "p"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix('p').and_then(|p| p.trim().parse().ok()))
}

fn proc_name(pid: u32) -> Option<String> {
    let out = std::process::Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        // Show just basename for readability.
        Some(s.rsplit('/').next().unwrap_or(&s).to_string())
    }
}

fn port_status(port: u16) -> serde_json::Value {
    let pid = lsof_pid(port);
    let self_pid = std::process::id();
    serde_json::json!({
        "port": port,
        "pid": pid,
        "process": pid.and_then(proc_name),
        "free": pid.is_none(),
        "self": pid == Some(self_pid),
    })
}

// ===== Tauri commands invoked from the diagnostics window.

#[tauri::command]
fn app_status(state: tauri::State<'_, Arc<AppState>>) -> serde_json::Value {
    let daemon = state.daemon.lock().unwrap();
    let running = state.daemon_alive.load(Ordering::SeqCst);
    let last_error = daemon.last_error.clone();
    let started_at = daemon.started_at.and_then(|t| {
        t.duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
    });
    drop(daemon);
    serde_json::json!({
        "self_pid": std::process::id(),
        "daemon": {
            "running": running,
            "last_error": last_error,
            "started_at_unix": started_at,
        },
        "ports": [port_status(UI_PORT), port_status(WS_PORT)],
    })
}

#[tauri::command]
fn app_logs(state: tauri::State<'_, Arc<AppState>>, limit: Option<usize>) -> Vec<String> {
    let q = state.logs.lock().unwrap();
    let n = limit.unwrap_or(500).min(q.len());
    q.iter().rev().take(n).rev().cloned().collect()
}

#[tauri::command]
fn kill_port(port: u16) -> Result<u32, String> {
    let pid = lsof_pid(port).ok_or_else(|| format!("port {port} is already free"))?;
    if pid == std::process::id() {
        return Err("refusing to kill own process".into());
    }
    let _ = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(800));
    if lsof_pid(port).is_some() {
        let _ = std::process::Command::new("kill")
            .arg("-KILL")
            .arg(pid.to_string())
            .status();
    }
    Ok(pid)
}

#[tauri::command]
async fn restart_daemon(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    {
        let mut d = state.daemon.lock().map_err(|e| e.to_string())?;
        if let Some(t) = d.task.take() {
            t.abort();
        }
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    spawn_daemon(app.clone(), state.inner().clone());
    Ok(())
}

#[tauri::command]
fn open_window(app: tauri::AppHandle, label: String) -> Result<(), String> {
    if label == CHAT_WINDOW {
        popup_chat(&app);
        return Ok(());
    }
    let w = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("unknown window {label}"))?;
    w.show().map_err(|e| e.to_string())?;
    let _ = w.unminimize();
    if label == MAIN_WINDOW {
        let _ = w.maximize();
    }
    let _ = w.set_focus();
    Ok(())
}
