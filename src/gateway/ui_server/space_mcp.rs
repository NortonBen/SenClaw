//! Space App runtime: launch "server" apps and auto-register their MCP.
//!
//! A Space App manifest may declare a `runtime.kind == "server"` block with a
//! `start` command (e.g. `npm start`). On install and on daemon startup,
//! SemaClaw will:
//!   1. launch the app's start command from its install directory with an
//!      assigned `PORT` (so one process serves the UI + `/mcp` route + API),
//!   2. wait for the app's health endpoint,
//!   3. record the running origin into the stored manifest (`runtime.url`) so
//!      the Web UI iframe loads it,
//!   4. auto-register the declared MCP (`mcp.autoRegister`) pointing at the
//!      running origin (`mcp.url` or origin + `mcp.path`).
//!
//! The launched process is tracked per app and killed (whole process group) on
//! daemon shutdown. Legacy apps that declare only an `mcp` block with an
//! absolute `url` (no server runtime) are still auto-registered without launch.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use rusqlite::params;
use serde_json::Value;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::db::Db;
use crate::mcp::config::{ExternalMcpServerConfig, McpScopeType, McpTransportType};
use crate::mcp::manager::McpManager;

struct ChildProc {
    child: Child,
    /// Process-group id (== leader pid) so we can signal the whole tree.
    pgid: i32,
    port: u16,
}

/// Tracks server-app processes launched on behalf of Space Apps, keyed by app id.
pub struct SpaceMcpLauncher {
    children: Mutex<HashMap<String, ChildProc>>,
    http: reqwest::Client,
}

impl Default for SpaceMcpLauncher {
    fn default() -> Self {
        Self::new()
    }
}

impl SpaceMcpLauncher {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            children: Mutex::new(HashMap::new()),
            http,
        }
    }

    /// Scan every enabled installed Space App and launch + auto-register the
    /// ones that declare a server runtime and/or `mcp.autoRegister`. Best-effort.
    pub async fn autoregister_installed(
        &self,
        db: &Db,
        manager: &McpManager,
        apps_dir: &Path,
        base_url: &str,
    ) {
        let apps: Vec<(String, Value)> = match db.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT id, manifest FROM space_apps WHERE enabled = 1")?;
            let rows = stmt
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
                .filter_map(|r| r.ok())
                .filter_map(|(id, m)| serde_json::from_str::<Value>(&m).ok().map(|v| (id, v)))
                .collect::<Vec<_>>();
            Ok(rows)
        }) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("[space-mcp] could not list space apps: {e}");
                return;
            }
        };

        for (app_id, manifest) in apps {
            let app_dir = app_install_dir(&manifest, apps_dir, &app_id);
            match self
                .run_and_register(db, manager, &app_id, &app_dir, &manifest, base_url)
                .await
            {
                Ok(Some(name)) => {
                    tracing::info!("[space-mcp] auto-registered '{name}' for app '{app_id}'")
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("[space-mcp] auto-register for app '{app_id}' failed: {e}")
                }
            }
        }
    }

    /// Launch (if a server runtime) and auto-register a single app's MCP.
    /// Updates the stored manifest with the running origin. Returns the
    /// registered MCP server name, or `None` when nothing to register.
    pub async fn run_and_register(
        &self,
        db: &Db,
        manager: &McpManager,
        app_id: &str,
        app_dir: &Path,
        manifest: &Value,
        base_url: &str,
    ) -> Result<Option<String>> {
        let mut manifest = manifest.clone();

        // Launch a server runtime, if declared, and record the running origin.
        let origin = if is_server_runtime(&manifest) {
            let runtime = manifest.get("runtime").cloned().unwrap_or(Value::Null);
            let port = self
                .ensure_server_running(app_id, app_dir, &runtime, base_url)
                .await
                .with_context(|| format!("launch server app '{app_id}'"))?;
            let origin = format!("http://127.0.0.1:{port}");
            // Persist the running origin so the iframe + detail page can reach it.
            if let Some(rt) = manifest.get_mut("runtime").and_then(|v| v.as_object_mut()) {
                rt.insert("url".into(), Value::String(origin.clone()));
                rt.insert("port".into(), Value::from(port));
            }
            update_app_manifest(db, app_id, &manifest);
            Some(origin)
        } else {
            None
        };

        // Auto-register the MCP server, if declared.
        let mcp = match manifest.get("mcp") {
            Some(v) if v.is_object() => v.clone(),
            _ => return Ok(None),
        };
        if !mcp.get("autoRegister").and_then(Value::as_bool).unwrap_or(false) {
            return Ok(None);
        }
        let name = mcp
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{app_id}-mcp"));
        let config = build_mcp_config(&name, &mcp, app_id, base_url, origin.as_deref())?;
        manager
            .add_or_update(config, McpScopeType::Project)
            .await
            .with_context(|| format!("register MCP '{name}'"))?;
        Ok(Some(name))
    }

    /// Ensure the app's server process is running and healthy; returns its port.
    /// Idempotent: an already-healthy server (tracked, manual, or orphaned on a
    /// fixed port) is reused rather than double-spawned.
    async fn ensure_server_running(
        &self,
        app_id: &str,
        app_dir: &Path,
        runtime: &Value,
        base_url: &str,
    ) -> Result<u16> {
        let start = runtime
            .get("start")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("runtime.start is required for a server app"))?;
        let health_path = runtime
            .get("healthPath")
            .and_then(Value::as_str)
            .unwrap_or("/health");
        let fixed_port = runtime.get("port").and_then(Value::as_u64).unwrap_or(0) as u16;

        // Reuse a tracked, still-alive child.
        {
            let mut children = self.children.lock().await;
            if let Some(proc) = children.get_mut(app_id) {
                if matches!(proc.child.try_wait(), Ok(None)) {
                    let port = proc.port;
                    if self.is_healthy(&health_url(port, health_path)).await {
                        return Ok(port);
                    }
                } else {
                    children.remove(app_id);
                }
            }
        }

        // Fixed port already healthy (orphan or manual run)? Reuse it.
        if fixed_port > 0 && self.is_healthy(&health_url(fixed_port, health_path)).await {
            tracing::info!("[space-mcp] '{app_id}' already serving on :{fixed_port}");
            return Ok(fixed_port);
        }

        let port = if fixed_port > 0 {
            fixed_port
        } else {
            pick_free_port().ok_or_else(|| anyhow!("no free port for app '{app_id}'"))?
        };

        // Spawn the start command in its own process group so we can kill the
        // whole tree (npm -> next-server) on shutdown.
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(start)
            .current_dir(app_dir)
            .env("PORT", port.to_string())
            .env("SENCLAW_BASE_URL", base_url)
            .env("SENCLAW_SPACE_APP_ID", app_id)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .kill_on_drop(true);
        let child = cmd
            .spawn()
            .with_context(|| format!("spawn '{start}' for app '{app_id}'"))?;
        let pgid = child.id().map(|i| i as i32).unwrap_or(0);
        self.children
            .lock()
            .await
            .insert(app_id.to_string(), ChildProc { child, pgid, port });
        tracing::info!("[space-mcp] launched '{app_id}': {start} (PORT={port})");

        // Wait for health (server boot can take a few seconds).
        let url = health_url(port, health_path);
        for _ in 0..120 {
            if self.is_healthy(&url).await {
                return Ok(port);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(anyhow!("server app '{app_id}' not healthy at {url}"))
    }

    async fn is_healthy(&self, url: &str) -> bool {
        matches!(self.http.get(url).send().await, Ok(r) if r.status().is_success())
    }

    /// Stop one app's server process (on uninstall).
    pub async fn stop_app(&self, app_id: &str) {
        if let Some(proc) = self.children.lock().await.remove(app_id) {
            if proc.pgid > 0 {
                unsafe {
                    libc::kill(-proc.pgid, libc::SIGTERM);
                }
            }
            let mut child = proc.child;
            let _ = child.start_kill();
            tracing::info!("[space-mcp] stopped server process for '{app_id}' (uninstall)");
        }
    }

    /// Kill every launched server process group. Call on graceful shutdown.
    pub async fn shutdown(&self) {
        let mut children = self.children.lock().await;
        for (app_id, proc) in children.drain() {
            if proc.pgid > 0 {
                // Signal the whole process group (npm + next-server children).
                unsafe {
                    libc::kill(-proc.pgid, libc::SIGTERM);
                }
            }
            let mut child = proc.child;
            let _ = child.start_kill();
            tracing::info!("[space-mcp] stopped server process for '{app_id}'");
        }
    }
}

fn is_server_runtime(manifest: &Value) -> bool {
    manifest
        .get("runtime")
        .and_then(|r| r.get("kind"))
        .and_then(Value::as_str)
        == Some("server")
        && manifest
            .get("runtime")
            .and_then(|r| r.get("start"))
            .and_then(Value::as_str)
            .is_some()
}

/// Where the app's files live: an explicit `install.localPath`, else
/// `<apps_dir>/<app_id>`.
fn app_install_dir(manifest: &Value, apps_dir: &Path, app_id: &str) -> PathBuf {
    manifest
        .get("install")
        .and_then(|i| i.get("localPath"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| apps_dir.join(app_id))
}

fn health_url(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}")
}

fn pick_free_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
}

fn update_app_manifest(db: &Db, app_id: &str, manifest: &Value) {
    let raw = serde_json::to_string(manifest).unwrap_or_default();
    let now = chrono::Utc::now().timestamp_millis();
    let _ = db.with_conn(|conn| {
        conn.execute(
            "UPDATE space_apps SET manifest=?1, last_seen_at=?2 WHERE id=?3",
            params![raw, now, app_id],
        )?;
        Ok(())
    });
}

/// Map a manifest `mcp` block onto an `ExternalMcpServerConfig`. For a server
/// app the URL is composed from the running `origin` + `mcp.path` unless an
/// absolute `mcp.url` is given.
fn build_mcp_config(
    name: &str,
    mcp: &Value,
    app_id: &str,
    base_url: &str,
    origin: Option<&str>,
) -> Result<ExternalMcpServerConfig> {
    let transport_str = mcp.get("transport").and_then(Value::as_str).unwrap_or("http");
    let transport = match transport_str {
        "stdio" => McpTransportType::Stdio,
        "sse" => McpTransportType::Sse,
        "http" => McpTransportType::Http,
        other => return Err(anyhow!("unknown mcp transport '{other}'")),
    };

    let str_field = |k: &str| mcp.get(k).and_then(Value::as_str).map(str::to_string);
    let str_array = |k: &str| {
        mcp.get(k).and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
    };
    let str_map = |k: &str| -> HashMap<String, String> {
        mcp.get(k)
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    };

    // Resolve the URL: absolute mcp.url wins; else origin + mcp.path.
    let url = match (str_field("url"), origin) {
        (Some(u), _) if u.starts_with("http") => Some(u),
        (_, Some(origin)) => {
            let path = mcp.get("path").and_then(Value::as_str).unwrap_or("/mcp");
            Some(format!("{}{}", origin.trim_end_matches('/'), path))
        }
        (other, None) => other,
    };

    let mut env = str_map("env");
    env.insert("SENCLAW_SPACE_APP_ID".into(), app_id.to_string());
    env.insert("SENCLAW_BASE_URL".into(), base_url.to_string());

    let (command, args) = if matches!(transport, McpTransportType::Stdio) {
        (str_field("command"), str_array("args").unwrap_or_default())
    } else {
        (None, vec![])
    };

    let config = ExternalMcpServerConfig {
        name: name.to_string(),
        transport,
        description: str_field("description"),
        enabled: true,
        use_tools: str_array("use_tools"),
        command,
        args,
        env,
        url,
        headers: str_map("headers"),
    };
    config.validate().map_err(|e| anyhow!(e))?;
    Ok(config)
}
