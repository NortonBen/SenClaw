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
    log_path: PathBuf,
    /// The app's allowlisting egress proxy, when it runs sandboxed with
    /// `network: hosts`. Held here so it lives exactly as long as the process it
    /// was opened for: dropping this record stops the proxy.
    proxy: Option<std::sync::Arc<crate::sandbox::proxy::HostProxy>>,
    /// Wall-clock start, so the monitor can say "up 4m" — and, more usefully,
    /// "up 6s" for an app that is crash-looping while the row still reads
    /// "running".
    started_at: std::time::SystemTime,
    /// Whether this launch was confined, and how. Recorded at spawn because the
    /// stored settings can be edited afterwards, and what is *running* is what
    /// the monitor must report.
    isolation: String,
}

/// What the monitor knows about one app's process, taken under the lock and
/// handed back as a plain value so nothing borrows the launcher's map.
pub struct RuntimeInfo {
    pub pid: u32,
    pub pgid: i32,
    pub port: u16,
    pub log_path: PathBuf,
    pub started_at: std::time::SystemTime,
    pub isolation: String,
    pub proxy: Option<(u16, crate::sandbox::proxy::ProxyStats)>,
}

/// Tracks server-app processes launched on behalf of Space Apps, keyed by app id.
pub struct SpaceMcpLauncher {
    children: Mutex<HashMap<String, ChildProc>>,
    /// How many times each app has been (re)launched this daemon run. A number
    /// that keeps climbing on its own is the signature of a crash loop, which
    /// otherwise looks exactly like a healthy app in every other view.
    launches: Mutex<HashMap<String, u32>>,
    /// Per-app spawn lock so concurrent callers (proxy lazy-spawn + supervisor +
    /// user restart) never double-launch the same app.
    start_locks: Mutex<HashMap<String, std::sync::Arc<Mutex<()>>>>,
    /// Apps served by a process this daemon did not start and could not take
    /// back. Remembered so the supervisor attempts the reclaim once per daemon
    /// run instead of once every tick — the answer will not change by itself.
    adopted: Mutex<std::collections::HashSet<String>>,
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
            launches: Mutex::new(HashMap::new()),
            start_locks: Mutex::new(HashMap::new()),
            adopted: Mutex::new(std::collections::HashSet::new()),
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
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
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
            let health_path = runtime
                .get("healthPath")
                .and_then(Value::as_str)
                .unwrap_or("/health");

            let tracked = {
                let mut children = self.children.lock().await;
                children
                    .get_mut(&app_id)
                    .map(|p| matches!(p.child.try_wait(), Ok(None)))
                    .unwrap_or(false)
            };
            // Healthy if the fixed port answers its health endpoint; for a
            // dynamic port, if the tracked child is still alive.
            let healthy = if port > 0 {
                self.is_healthy(&health_url(port, health_path)).await
            } else {
                tracked
            };
            // A port that answers with nothing tracked behind it is a process
            // from a previous daemon. It serves fine, which is exactly why it
            // used to go unnoticed for weeks — running old code, from an old
            // directory, outside whatever sandbox the settings now ask for. So
            // it is treated as work to do, once: `ensure_server_running` takes
            // the port back when the process is verifiably this app's, and
            // `adopted` stops us retrying every tick when it is not.
            let untracked_stranger =
                healthy && !tracked && !self.adopted.lock().await.contains(&app_id);
            if healthy && !untracked_stranger {
                continue;
            }

            if untracked_stranger {
                tracing::warn!(
                    "[space-mcp] supervisor: '{app_id}' is served by a process this daemon did \
                     not start → reclaiming"
                );
            } else {
                tracing::warn!("[space-mcp] supervisor: app '{app_id}' is DOWN → respawning");
            }
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
        // Import tool aliases declared in the manifest (`mcp.toolAliases`).
        // They land DISABLED — the user must approve each one in
        // Plugins → Alias before it takes effect.
        sync_app_tool_aliases(db, app_id, &name, &mcp);
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

        // Fixed port already healthy, but nothing tracked is behind it — so it is
        // a process from a previous daemon (or a manual run). Adopting it used to
        // be the answer and it was the wrong one: the app then runs whatever code
        // it was started with, from wherever it was started, with none of the
        // sandbox settings applied, for as long as the machine stays up.
        //
        // Take the port back when the process is verifiably this app's, and start
        // it again properly. When it is not ours, adopt as before — that port
        // belongs to someone else's server and killing it would be hostile.
        if fixed_port > 0 && self.is_healthy(&health_url(fixed_port, health_path)).await {
            match reclaim_app_port(app_id, app_dir, fixed_port).await {
                Reclaim::Freed => {
                    tracing::info!(
                        "[space-mcp] '{app_id}': took :{fixed_port} back from an untracked \
                         process, launching a fresh one"
                    );
                }
                Reclaim::NotOurs => {
                    tracing::info!("[space-mcp] '{app_id}' already serving on :{fixed_port}");
                    self.adopted.lock().await.insert(app_id.to_string());
                    return Ok(fixed_port);
                }
                Reclaim::Failed => {
                    tracing::warn!(
                        "[space-mcp] '{app_id}': could not free :{fixed_port}; using the process \
                         that is there (its sandbox state is unknown)"
                    );
                    self.adopted.lock().await.insert(app_id.to_string());
                    return Ok(fixed_port);
                }
            }
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

        // How this app is confined, if the user asked for it in Plugins → Space
        // Apps. `plan` returns the plain `sh -c <start>` when the app is not
        // sandboxed, so there is one spawn path either way. A failure here is
        // deliberately fatal: the user asked for a sandbox, and launching the app
        // unconfined instead would be the one outcome nobody wants.
        let sb_cfg = crate::sandbox::app_policy::current(app_id);
        let launch = crate::sandbox::app_launch::plan(
            app_id,
            app_dir,
            start,
            port,
            daemon_port(base_url),
            &sb_cfg,
        )
        .await
        .with_context(|| format!("prepare the sandbox for app '{app_id}'"))?;
        let _ = writeln!(log_file, "{}", launch.summary());
        if let Some(note) = &launch.note {
            tracing::warn!("[space-mcp] '{app_id}' sandbox: {note}");
        }

        // Spawn through the platform shell (wrapped by `sandbox-exec` / `bwrap`
        // when confined). On unix it gets its own process group so we can kill
        // the whole tree (npm -> next-server) on shutdown; on Windows we fall
        // back to killing the direct child.
        let mut cmd = Command::new(&launch.argv[0]);
        cmd.args(&launch.argv[1..]);
        cmd.current_dir(app_dir)
            .env("PORT", port.to_string())
            .env("SENCLAW_BASE_URL", base_url)
            .env("SENCLAW_SPACE_APP_ID", app_id)
            .env("SENCLAW_SPACE_LOG_FILE", &log_path)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .kill_on_drop(true);
        for (k, v) in &launch.env {
            cmd.env(k, v);
        }
        #[cfg(unix)]
        cmd.process_group(0);
        let child = cmd
            .spawn()
            .with_context(|| format!("spawn '{start}' for app '{app_id}'"))?;
        let pgid = child.id().map(|i| i as i32).unwrap_or(0);
        // Ours again.
        self.adopted.lock().await.remove(app_id);
        *self
            .launches
            .lock()
            .await
            .entry(app_id.to_string())
            .or_insert(0) += 1;
        self.children.lock().await.insert(
            app_id.to_string(),
            ChildProc {
                child,
                pgid,
                port,
                log_path: log_path.clone(),
                proxy: launch.proxy.clone(),
                started_at: std::time::SystemTime::now(),
                isolation: if launch.enforced {
                    launch.isolation.clone()
                } else {
                    "none".to_string()
                },
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
            tracing::info!(
                "[space-mcp] stopped server process for '{app_id}' (uninstall, log={log})"
            );
        }
    }

    /// Everything the monitor needs about the process this daemon launched for
    /// `app_id`. `None` when nothing is tracked — which for a server app means
    /// it is not running, whatever the manifest says.
    pub async fn runtime_info(&self, app_id: &str) -> Option<RuntimeInfo> {
        let children = self.children.lock().await;
        let proc = children.get(app_id)?;
        Some(RuntimeInfo {
            pid: proc.child.id().unwrap_or(0),
            pgid: proc.pgid,
            port: proc.port,
            log_path: proc.log_path.clone(),
            started_at: proc.started_at,
            isolation: proc.isolation.clone(),
            proxy: proc.proxy.as_ref().map(|p| (p.port, p.stats())),
        })
    }

    /// How many times this app has been launched since the daemon started.
    pub async fn launch_count(&self, app_id: &str) -> u32 {
        self.launches.lock().await.get(app_id).copied().unwrap_or(0)
    }

    /// Live sandbox facts for one app: the port of its allowlist proxy and what
    /// that proxy has refused so far. `None` when the app is not running, or is
    /// running without per-site egress.
    ///
    /// The refusal list is the reason this is exposed at all: "the app is broken"
    /// and "the app wanted `x.com` and did not have it" look identical from the
    /// outside otherwise.
    pub async fn proxy_status(&self, app_id: &str) -> Option<(u16, crate::sandbox::proxy::ProxyStats)> {
        let children = self.children.lock().await;
        let proxy = children.get(app_id)?.proxy.as_ref()?;
        Some((proxy.port, proxy.stats()))
    }

    /// Push an edited allowlist into a *running* app's proxy, so adding a site
    /// takes effect without restarting the app.
    pub async fn set_proxy_hosts(&self, app_id: &str, hosts: Vec<String>) -> bool {
        let children = self.children.lock().await;
        match children.get(app_id).and_then(|c| c.proxy.as_ref()) {
            Some(p) => {
                p.set_hosts(hosts);
                true
            }
            None => false,
        }
    }

    /// Kill every launched server process group. Call on graceful shutdown.
    ///
    /// Signals all of them first, then waits **once** — not app by app. The
    /// desktop app allows ~800 ms between SIGTERM and SIGKILL, and a machine
    /// with a few dozen apps would never finish a per-app grace period inside
    /// that window; every app that did not get its turn survived the daemon.
    pub async fn shutdown(&self) {
        let procs: Vec<(String, ChildProc)> = self.children.lock().await.drain().collect();
        if procs.is_empty() {
            return;
        }
        let n = procs.len();
        #[cfg(unix)]
        for (_, proc) in &procs {
            if proc.pgid > 0 {
                // Negative pid → the whole group (sh + npm + node, …).
                unsafe {
                    libc::kill(-proc.pgid, libc::SIGTERM);
                }
            }
        }
        // One grace period for everyone, well inside the window we are given.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let mut left = 0usize;
        for (app_id, mut proc) in procs {
            let log = proc.log_path.display().to_string();
            if matches!(proc.child.try_wait(), Ok(Some(_))) {
                tracing::info!("[space-mcp] stopped server process for '{app_id}' (log={log})");
                continue;
            }
            #[cfg(unix)]
            if proc.pgid > 0 {
                unsafe {
                    libc::kill(-proc.pgid, libc::SIGKILL);
                }
            }
            let _ = proc.child.start_kill();
            let _ = proc.child.try_wait();
            left += 1;
            tracing::info!("[space-mcp] killed server process for '{app_id}' (log={log})");
        }
        tracing::info!("[space-mcp] shutdown: {n} app(s) stopped ({left} needed SIGKILL)");
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
/// What happened when we tried to take back a port an untracked process holds.
#[derive(Debug, PartialEq)]
pub(crate) enum Reclaim {
    /// It was this app's process, it is gone, the port is free.
    Freed,
    /// Something else is on that port. Not ours to kill.
    NotOurs,
    /// It is ours but would not die, or the port stayed busy.
    Failed,
}

/// A process's working directory, read with `lsof`. `None` when it cannot be
/// determined — which the caller must treat as "cannot verify", never as "yes".
pub(crate) async fn process_cwd(pid: i32) -> Option<PathBuf> {
    if cfg!(windows) || pid <= 0 {
        return None;
    }
    let out = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::process::Command::new("lsof")
            .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
            .stdin(std::process::Stdio::null())
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    // `-Fn` prints one field per line; the cwd path is the line starting with n.
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix('n').map(PathBuf::from))
}

/// pids listening on `port`.
pub(crate) async fn pids_on_port(port: u16) -> Vec<i32> {
    if cfg!(windows) || port == 0 {
        return Vec::new();
    }
    let Ok(Ok(out)) = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::process::Command::new("lsof")
            .args(["-ti", &format!("tcp:{port}"), "-sTCP:LISTEN"])
            .stdin(std::process::Stdio::null())
            .output(),
    )
    .await
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse::<i32>().ok())
        .collect()
}

/// Take back an app's port from a process this daemon does not track — but
/// **only if that process is demonstrably the app's**.
///
/// This is the fix for a real state: the daemon used to die without stopping its
/// children (SIGTERM was not handled), so every restart left the previous
/// generation running. The next daemon found the ports healthy and adopted them,
/// which meant apps ran for weeks as orphans, from stale install directories,
/// with none of the sandbox settings applied.
///
/// The check is the process's working directory, because that is what the
/// launcher sets and what every runtime inherits (`sh -c "npm start"` → node in
/// the app dir). A process we cannot place inside the app's own directory is
/// left alone: a port collision with the user's own dev server must not turn the
/// daemon into something that kills it on every boot.
pub(crate) async fn reclaim_app_port(app_id: &str, app_dir: &Path, port: u16) -> Reclaim {
    let pids = pids_on_port(port).await;
    if pids.is_empty() {
        return Reclaim::Freed;
    }
    let want = app_dir.canonicalize().unwrap_or_else(|_| app_dir.to_path_buf());
    let mut killed = false;
    for pid in pids {
        let cwd = process_cwd(pid).await;
        let ours = cwd
            .as_ref()
            .map(|c| {
                let c = c.canonicalize().unwrap_or_else(|_| c.clone());
                c.starts_with(&want)
            })
            .unwrap_or(false);
        if !ours {
            tracing::warn!(
                "[space-mcp] port {port} for '{app_id}' is held by pid {pid} (cwd={:?}), which is \
                 not this app — leaving it alone and adopting instead",
                cwd
            );
            return Reclaim::NotOurs;
        }
        #[cfg(unix)]
        unsafe {
            // The whole group: the listener is often a grandchild (npm → node).
            let pgid = libc::getpgid(pid);
            if pgid > 0 {
                libc::kill(-pgid, libc::SIGTERM);
            } else {
                libc::kill(pid, libc::SIGTERM);
            }
        }
        killed = true;
        tracing::info!("[space-mcp] '{app_id}': reclaiming :{port} from orphan pid {pid}");
    }
    if !killed {
        return Reclaim::NotOurs;
    }
    // Give it a moment, then insist.
    for i in 0..20 {
        if port_is_free(port) {
            return Reclaim::Freed;
        }
        if i == 5 {
            #[cfg(unix)]
            for pid in pids_on_port(port).await {
                unsafe {
                    let pgid = libc::getpgid(pid);
                    libc::kill(if pgid > 0 { -pgid } else { pid }, libc::SIGKILL);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Reclaim::Failed
}

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

    /// Every test here that takes an ephemeral port shares this.
    ///
    /// `pick_free_port` binds, reads the number and releases — so two tests
    /// running in parallel can be handed the *same* port, and then one of them
    /// sees a socket it did not open. That is a genuine flake (caught in one run
    /// out of ten), not a code bug, and serialising is what fixes it.
    static PORT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[test]
    fn port_is_free_reflects_binding() {
        let _guard = PORT_LOCK.blocking_lock();
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

    /// A real listener holding a real port, with a working directory we choose —
    /// the two facts `reclaim_app_port` decides on.
    async fn spawn_listener(cwd: &Path, port: u16) -> tokio::process::Child {
        let script = format!(
            "import socket,time
s=socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(('127.0.0.1',{port}))
s.listen()
time.sleep(60)"
        );
        let mut cmd = tokio::process::Command::new("python3");
        cmd.arg("-c")
            .arg(script)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        // Its own process group, exactly like a launched app — without this the
        // group kill under test takes the test runner down with it, which is
        // also the reason the daemon must never signal a group it did not create.
        #[cfg(unix)]
        cmd.process_group(0);
        let child = cmd.spawn().expect("spawn a listener");
        for _ in 0..40 {
            if !port_is_free(port) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        child
    }

    fn a_free_port() -> u16 {
        pick_free_port().expect("a free port")
    }

    #[tokio::test]
    async fn a_process_is_only_killed_when_it_is_demonstrably_the_app() {
        let _guard = PORT_LOCK.lock().await;
        // The safety property: a port collision with something that is not this
        // app — the user's own dev server — must not turn the daemon into
        // something that kills it on every boot.
        let app_dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let port = a_free_port();
        let mut foreign = spawn_listener(elsewhere.path(), port).await;
        if port_is_free(port) {
            return; // python3 unavailable on this machine; nothing to assert
        }

        let outcome = reclaim_app_port("demo", app_dir.path(), port).await;
        assert_eq!(outcome, Reclaim::NotOurs);
        assert!(
            matches!(foreign.try_wait(), Ok(None)),
            "a process outside the app directory must survive"
        );
        let _ = foreign.kill().await;
    }

    #[tokio::test]
    async fn an_orphan_of_this_app_is_killed_and_its_port_freed() {
        let _guard = PORT_LOCK.lock().await;
        // The state this exists to clean up: a previous daemon's child still
        // holding the app's port days later.
        let app_dir = tempfile::tempdir().unwrap();
        let port = a_free_port();
        let mut orphan = spawn_listener(app_dir.path(), port).await;
        if port_is_free(port) {
            return; // no python3 here
        }

        let outcome = reclaim_app_port("demo", app_dir.path(), port).await;
        assert_eq!(outcome, Reclaim::Freed);
        assert!(port_is_free(port), "the port must be usable again");
        assert!(
            !matches!(orphan.try_wait(), Ok(None)),
            "the orphan must be gone, not merely signalled"
        );
    }

    #[tokio::test]
    async fn a_free_port_needs_no_reclaiming() {
        let _guard = PORT_LOCK.lock().await;
        let app_dir = tempfile::tempdir().unwrap();
        assert_eq!(
            reclaim_app_port("demo", app_dir.path(), a_free_port()).await,
            Reclaim::Freed
        );
    }

    #[tokio::test]
    async fn the_working_directory_of_a_live_process_is_readable() {
        // `process_cwd` returning None must mean "cannot verify", so this pins
        // the happy path it is judged against.
        let dir = tempfile::tempdir().unwrap();
        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 5")
            .current_dir(dir.path())
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap() as i32;
        let cwd = process_cwd(pid).await;
        let _ = child.kill().await;
        if let Some(cwd) = cwd {
            let want = dir.path().canonicalize().unwrap();
            assert_eq!(cwd.canonicalize().unwrap(), want);
        }
    }

    #[test]
    fn the_daemon_port_is_taken_from_the_url_the_app_is_given() {
        // A sandboxed app gets exactly this one loopback port for the AI bridge,
        // so reading the wrong number means either a broken bridge or an open
        // door to some other local service.
        assert_eq!(daemon_port("http://127.0.0.1:18788"), 18788);
        assert_eq!(daemon_port("http://127.0.0.1:18788/"), 18788);
        assert_eq!(daemon_port("http://localhost:9000/api"), 9000);
        // No port in the URL: fall back to the documented default rather than
        // guessing something reachable.
        assert_eq!(daemon_port("http://127.0.0.1"), 18788);
        assert_eq!(daemon_port(""), 18788);
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

/// SenClaw's own UI port, read off the base URL the app is given. A sandboxed
/// app that may use the AI bridge needs exactly this port on loopback and
/// nothing else, so it is derived from the same string the app dials rather than
/// re-read from the environment (where it could disagree).
fn daemon_port(base_url: &str) -> u16 {
    base_url
        .rsplit_once(':')
        .and_then(|(_, p)| {
            p.trim_end_matches('/')
                .split('/')
                .next()
                .and_then(|n| n.parse::<u16>().ok())
        })
        .unwrap_or(18788)
}

fn pick_free_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
}

/// Sync the app's declared tool aliases (`mcp.toolAliases`) into the
/// `mcp_tool_aliases` table. Idempotent: re-imports refresh target/description
/// but never touch `enabled` (the user's opt-in from Plugins → Alias), and
/// aliases the manifest no longer declares are pruned. Runs on every path that
/// registers the app's MCP — install, update, boot, supervisor respawn.
fn sync_app_tool_aliases(db: &Db, app_id: &str, server_name: &str, mcp: &Value) {
    let declared = crate::tools::tool_alias::parse_declared_aliases(server_name, mcp);
    let keep: Vec<String> = declared.iter().map(|d| d.alias.clone()).collect();
    for d in &declared {
        if let Err(e) =
            db.import_app_tool_alias(app_id, &d.alias, &d.target, d.description.as_deref())
        {
            tracing::warn!("[space-mcp] {app_id}: import tool alias '{}' failed: {e:#}", d.alias);
        }
    }
    match db.prune_app_tool_aliases(app_id, &keep) {
        Ok(n) if n > 0 => {
            tracing::info!("[space-mcp] {app_id}: pruned {n} stale tool alias(es)");
        }
        Err(e) => tracing::warn!("[space-mcp] {app_id}: prune tool aliases failed: {e:#}"),
        _ => {}
    }
    if !keep.is_empty() {
        tracing::info!(
            "[space-mcp] {app_id}: imported {} declared tool alias(es) (disabled until approved in Plugins → Alias)",
            keep.len()
        );
    }
    crate::tools::tool_alias::reload_from_db(db);
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
