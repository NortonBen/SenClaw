//! Space App runtime: launch "server" apps and auto-register their MCP.
//!
//! A Space App manifest may declare a `runtime.kind == "server"` block with a
//! `start` command (e.g. `npm start`). On install and on daemon startup,
//! SenClaw will:
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

use anyhow::{Context, Result, anyhow};
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
    log_path: PathBuf,
}

/// Tracks server-app processes launched on behalf of Space Apps, keyed by app id.
pub struct SpaceMcpLauncher {
    children: Mutex<HashMap<String, ChildProc>>,
    /// Per-app spawn lock so concurrent callers (proxy lazy-spawn + supervisor +
    /// user restart) never double-launch the same app.
    start_locks: Mutex<HashMap<String, std::sync::Arc<Mutex<()>>>>,
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
            start_locks: Mutex::new(HashMap::new()),
            http,
        }
    }

    /// Get-or-create the per-app spawn lock.
    async fn start_lock(&self, app_id: &str) -> std::sync::Arc<Mutex<()>> {
        self.start_locks
            .lock()
            .await
            .entry(app_id.to_string())
            .or_insert_with(|| std::sync::Arc::new(Mutex::new(())))
            .clone()
    }

    /// Fully stop the tracked process for an app: signal the whole process
    /// group (SIGTERM → SIGKILL) and reap it. No-op if not tracked. Used by the
    /// supervisor to drop a dead child before respawning.
    pub async fn restart_app(&self, app_id: &str) {
        let tracked = self.children.lock().await.remove(app_id);
        if let Some(proc) = tracked {
            tracing::info!("[space-mcp] killing process group for app {}", app_id);
            kill_child_group(proc).await;
        }
    }

    /// Full user-facing restart: kill the tracked process group, reclaim the
    /// port (including orphans from a previous botched restart that still hold
    /// it), wait for it to free, then respawn + re-register. Starts the app even
    /// if it wasn't running. Returns the registered MCP name, if any.
    pub async fn restart_and_respawn(
        &self,
        db: &Db,
        manager: &McpManager,
        app_id: &str,
        app_dir: &Path,
        manifest: &Value,
        base_url: &str,
    ) -> Result<Option<String>> {
        // Serialize against proxy lazy-spawns / supervisor for this app.
        let lock = self.start_lock(app_id).await;
        let _guard = lock.lock().await;

        // 1. Kill the tracked child's whole group and reap it.
        let mut ports_to_free: Vec<u16> = Vec::new();
        if let Some(proc) = self.children.lock().await.remove(app_id) {
            ports_to_free.push(proc.port);
            kill_child_group(proc).await;
        }
        // 2. Also reclaim the last persisted port — this is where an orphaned
        //    grandchild (npm/next-server) from an earlier kill still listens.
        if let Some(p) = manifest
            .get("runtime")
            .and_then(|r| r.get("port"))
            .and_then(Value::as_u64)
        {
            let p = p as u16;
            if p > 0 && !ports_to_free.contains(&p) {
                ports_to_free.push(p);
            }
        }
        for p in ports_to_free {
            kill_port_listeners(p).await;
            // Wait (≤2s) until the port is actually free before respawning.
            for _ in 0..20 {
                if port_is_free(p) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        // 3. Respawn + persist the running origin + re-register the MCP.
        self.run_and_register(db, manager, app_id, app_dir, manifest, base_url)
            .await
    }

    /// Ensure the app is up and healthy, spawning it if necessary, and return
    /// the live port. Persists the running origin so the proxy targets the right
    /// process. Serialized per app to avoid double-spawns from concurrent proxy
    /// requests. Intended for the proxy's lazy self-heal path.
    pub async fn ensure_running(
        &self,
        db: &Db,
        app_id: &str,
        app_dir: &Path,
        manifest: &Value,
        base_url: &str,
    ) -> Result<u16> {
        if !is_server_runtime(manifest) {
            return Err(anyhow!("app '{app_id}' has no server runtime to start"));
        }
        let lock = self.start_lock(app_id).await;
        let _guard = lock.lock().await;

        let runtime = manifest.get("runtime").cloned().unwrap_or(Value::Null);
        let port = self
            .ensure_server_running(app_id, app_dir, &runtime, base_url)
            .await?;
        // Persist current origin/port so subsequent proxy hits go to this process.
        let mut m = manifest.clone();
        if let Some(rt) = m.get_mut("runtime").and_then(|v| v.as_object_mut()) {
            rt.insert(
                "url".into(),
                Value::String(format!("http://127.0.0.1:{port}")),
            );
            rt.insert("port".into(), Value::from(port));
        }
        update_app_manifest(db, app_id, &m);
        Ok(port)
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
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
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

    /// Health-check every enabled server app and respawn any that is down or
    /// stopped responding. Called on an interval by the daemon's Space-App
    /// supervisor loop — this is what keeps a crashed/killed app (or one that
    /// served a broken deploy) automatically coming back.
    pub async fn supervise(&self, db: &Db, manager: &McpManager, apps_dir: &Path, base_url: &str) {
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
            Err(_) => return,
        };

        for (app_id, manifest) in apps {
            if !is_server_runtime(&manifest) {
                continue;
            }
            let runtime = manifest.get("runtime").cloned().unwrap_or(Value::Null);
            let port = runtime.get("port").and_then(Value::as_u64).unwrap_or(0) as u16;
            let health_path = runtime.get("healthPath").and_then(Value::as_str).unwrap_or("/health");

            // Healthy if the fixed port answers its health endpoint; for a
            // dynamic port, if the tracked child is still alive.
            let healthy = if port > 0 {
                self.is_healthy(&health_url(port, health_path)).await
            } else {
                let mut children = self.children.lock().await;
                children
                    .get_mut(&app_id)
                    .map(|p| matches!(p.child.try_wait(), Ok(None)))
                    .unwrap_or(false)
            };
            if healthy {
                continue;
            }

            tracing::warn!("[space-mcp] supervisor: app '{app_id}' is DOWN → respawning");
            // Serialize with proxy lazy-spawns / user restarts for this app.
            let lock = self.start_lock(&app_id).await;
            let _guard = lock.lock().await;
            // Drop any dead tracked child so ensure_server_running spawns fresh.
            self.restart_app(&app_id).await;
            let app_dir = app_install_dir(&manifest, apps_dir, &app_id);
            match self
                .run_and_register(db, manager, &app_id, &app_dir, &manifest, base_url)
                .await
            {
                Ok(_) => tracing::info!("[space-mcp] supervisor: respawned '{app_id}'"),
                Err(e) => tracing::warn!("[space-mcp] supervisor: respawn '{app_id}' failed: {e}"),
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
        if !mcp
            .get("autoRegister")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
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
        let log_path = app_runtime_log_path(app_dir);
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create log dir for app '{app_id}'"))?;
        }
        let mut log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("open runtime log for app '{app_id}'"))?;
        use std::io::Write as _;
        let _ = writeln!(
            log_file,
            "\n===== {} launching {app_id}: {start} (PORT={port}) =====",
            chrono::Utc::now().to_rfc3339()
        );
        let stdout = log_file
            .try_clone()
            .with_context(|| format!("clone stdout log for app '{app_id}'"))?;
        let stderr = log_file
            .try_clone()
            .with_context(|| format!("clone stderr log for app '{app_id}'"))?;

        // Spawn the start command via the platform shell. On unix it gets its
        // own process group so we can kill the whole tree (npm -> next-server)
        // on shutdown; on Windows we fall back to killing the direct child.
        #[cfg(unix)]
        let mut cmd = {
            let mut c = Command::new("sh");
            c.arg("-c").arg(start);
            c
        };
        #[cfg(not(unix))]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(start);
            c
        };
        cmd.current_dir(app_dir)
            .env("PORT", port.to_string())
            .env("SENCLAW_BASE_URL", base_url)
            .env("SENCLAW_SPACE_APP_ID", app_id)
            .env("SENCLAW_SPACE_LOG_FILE", &log_path)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        let child = cmd
            .spawn()
            .with_context(|| format!("spawn '{start}' for app '{app_id}'"))?;
        let pgid = child.id().map(|i| i as i32).unwrap_or(0);
        self.children.lock().await.insert(
            app_id.to_string(),
            ChildProc {
                child,
                pgid,
                port,
                log_path: log_path.clone(),
            },
        );
        tracing::info!(
            "[space-mcp] launched '{app_id}': {start} (PORT={port}, log={})",
            log_path.display()
        );

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
            let log = proc.log_path.display().to_string();
            kill_child_group(proc).await;
            tracing::info!("[space-mcp] stopped server process for '{app_id}' (uninstall, log={log})");
        }
    }

    /// Kill every launched server process group. Call on graceful shutdown.
    pub async fn shutdown(&self) {
        let procs: Vec<(String, ChildProc)> =
            self.children.lock().await.drain().collect();
        for (app_id, proc) in procs {
            let log = proc.log_path.display().to_string();
            kill_child_group(proc).await;
            tracing::info!("[space-mcp] stopped server process for '{app_id}' (log={log})");
        }
    }
}

/// Signal a launched app's whole process group (SIGTERM, then SIGKILL after a
/// grace period) and reap the direct child so no zombie/port-holder is left.
async fn kill_child_group(proc: ChildProc) {
    let pgid = proc.pgid;
    let mut child = proc.child;
    #[cfg(unix)]
    if pgid > 0 {
        // Negative pid → the whole group (sh + npm + next-server, …).
        unsafe {
            libc::kill(-pgid, libc::SIGTERM);
        }
    }
    // Give it up to ~2s to exit cleanly on SIGTERM.
    for _ in 0..20 {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    #[cfg(unix)]
    if pgid > 0 {
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
}

/// True if nothing is currently bound to `127.0.0.1:port`.
fn port_is_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Best-effort SIGKILL of any process still LISTENing on `port`. Reclaims a
/// port held by an orphaned grandchild (npm/next-server) that survived an
/// earlier kill of only the `sh` group leader. Unix-only; no-op elsewhere.
async fn kill_port_listeners(port: u16) {
    if port == 0 {
        return;
    }
    #[cfg(unix)]
    {
        let out = tokio::process::Command::new("lsof")
            .args(["-ti", &format!("tcp:{port}"), "-sTCP:LISTEN"])
            .output()
            .await;
        if let Ok(out) = out {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if let Ok(pid) = line.trim().parse::<i32>() {
                    tracing::info!("[space-mcp] reclaiming port {port}: killing pid {pid}");
                    unsafe {
                        libc::kill(pid, libc::SIGKILL);
                    }
                }
            }
        }
    }
    #[cfg(not(unix))]
    let _ = port;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_is_free_reflects_binding() {
        // A port we hold a listener on is not free; once released it is.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!port_is_free(port), "held port must report busy");
        drop(listener);
        assert!(port_is_free(port), "released port must report free");
    }

    #[test]
    fn is_server_runtime_requires_kind_and_start() {
        let ok = serde_json::json!({"runtime": {"kind": "server", "start": "npm start"}});
        assert!(is_server_runtime(&ok));
        let no_start = serde_json::json!({"runtime": {"kind": "server"}});
        assert!(!is_server_runtime(&no_start));
        let static_app = serde_json::json!({"runtime": {"kind": "static"}});
        assert!(!is_server_runtime(&static_app));
        assert!(!is_server_runtime(&serde_json::json!({})));
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

pub fn app_runtime_log_path(app_dir: &Path) -> PathBuf {
    app_dir.join(".senclaw").join("runtime.log")
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
    let transport_str = mcp
        .get("transport")
        .and_then(Value::as_str)
        .unwrap_or("http");
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
