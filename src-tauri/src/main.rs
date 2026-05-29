// SemaClaw desktop app — Tauri 2.0 shell that embeds the SenClaw daemon
// in-process and exposes a compact chat window from the menu bar / tray.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

use std::time::Duration;

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

const UI_PORT: u16 = 18788;
const CHAT_WINDOW: &str = "chat";
const MAIN_WINDOW: &str = "main";

fn main() {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            // Menu-bar-only on macOS (no Dock icon).
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            configure_env(app);
            build_tray(app)?;

            // Hide windows instead of quitting when closed; the app keeps
            // living in the menu bar until "Quit".
            for label in [CHAT_WINDOW, MAIN_WINDOW] {
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

            // Spawn the embedded daemon, then reload the (hidden) windows once
            // the UI server is accepting connections.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tauri::async_runtime::spawn(async move {
                    let cfg = build_config();
                    if let Err(e) = senclaw::run_daemon(cfg).await {
                        tracing::error!("[senclaw-app] daemon exited: {e}");
                    }
                });
                wait_for_port(UI_PORT).await;
                for label in [CHAT_WINDOW, MAIN_WINDOW] {
                    if let Some(win) = handle.get_webview_window(label) {
                        let _ = win.eval("window.location.reload()");
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running SemaClaw app");
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
        let bin_name = if cfg!(windows) { "senclaw.exe" } else { "senclaw" };
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

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let open_chat = MenuItemBuilder::with_id("open_chat", "Open Chat").build(app)?;
    let open_dash = MenuItemBuilder::with_id("open_dashboard", "Open Full Window").build(app)?;
    let open_browser =
        MenuItemBuilder::with_id("open_browser", "Open in Browser").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&open_chat, &open_dash, &open_browser, &quit])
        .build()?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open_chat" => show_window(app, CHAT_WINDOW),
            "open_dashboard" => show_window(app, MAIN_WINDOW),
            "open_browser" => open_in_browser_or_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_chat(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn show_window(app: &tauri::AppHandle, label: &str) {
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

fn toggle_chat(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window(CHAT_WINDOW) {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
        } else {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}

fn open_in_browser_or_window(app: &tauri::AppHandle) {
    let url = format!("http://127.0.0.1:{UI_PORT}");
    if let Err(e) = open_in_browser(&url) {
        tracing::warn!("[senclaw-app] failed to open browser: {e}");
        // Fall back to the in-app full window.
        show_window(app, MAIN_WINDOW);
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

/// Poll until the embedded UI server is accepting TCP connections.
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
