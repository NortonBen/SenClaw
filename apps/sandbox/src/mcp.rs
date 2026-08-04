//! MCP server — hand-rolled JSON-RPC over HTTP + SSE, matching the other Space
//! Apps (the `rmcp` crate is not used here).
//!
//! Tools are prefixed `sbx_`. Agents reach them as `mcp__sandbox-mcp__sbx_*`.

use std::collections::BTreeMap;
use std::convert::Infallible;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::Json;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;
use crate::{caps, code, files, monitor, mounts, runner};

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

pub async fn mcp_sse(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.mcp_tx.subscribe();
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("endpoint").data("/api/mcp/message"));
        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().event("message").data(msg));
        }
    };
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

fn text_result(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}

fn json_result(v: Value) -> Value {
    text_result(serde_json::to_string_pretty(&v).unwrap_or_default())
}

fn err(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

pub async fn mcp_message(
    State(state): State<AppState>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<Value> {
    // Results go back in the HTTP response only — never mirrored onto the SSE
    // fan-out (that would leak every caller's payload to every client).
    let reply = |result: Value| -> Json<Value> {
        Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": result }))
    };
    match req.method.as_str() {
        "initialize" => reply(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "sandbox-mcp", "version": "1.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} })),
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or_default();
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params["arguments"].clone();
            reply(call_tool(&state, &name, &args).await)
        }
        _ => Json(json!("ok")),
    }
}

fn s(args: &Value, k: &str) -> String {
    args[k].as_str().unwrap_or("").trim().to_string()
}

fn opt(args: &Value, k: &str) -> Option<String> {
    let v = s(args, k);
    (!v.is_empty()).then_some(v)
}

fn opt_int(args: &Value, k: &str) -> Option<i64> {
    args[k].as_i64()
}

fn flag(args: &Value, k: &str) -> bool {
    args[k].as_bool().unwrap_or(false)
}

fn str_list(args: &Value, k: &str) -> Vec<String> {
    args[k]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn env_map(args: &Value) -> BTreeMap<String, String> {
    args["env"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

/// A run, shaped for an agent: the fields that decide what to do next come
/// first, and the isolation actually applied is always included so the agent
/// can tell the user what protected them.
fn run_summary(run: &crate::db::Run) -> Value {
    json!({
        "runId": run.id,
        "ok": run.exit_code == Some(0) && !run.timed_out,
        "exitCode": run.exit_code,
        "timedOut": run.timed_out,
        "truncated": run.truncated,
        "durationMs": run.duration_ms,
        "isolation": run.isolation,
        "network": run.network,
        "stdout": run.stdout,
        "stderr": run.stderr,
    })
}

fn tools_list() -> Value {
    let langs = code::languages().join(", ");
    json!([
        {
            "name": "sbx_capabilities",
            "description": "Check what kind of sandbox this machine can actually run: docker (needs a live daemon) or direct execution confined by the operating system (macOS Seatbelt / Linux bubblewrap / Windows AppContainer). Call it before creating a sandbox if unsure, and again right after the user has started Docker.",
            "inputSchema": obj(json!({
                "refresh": { "type": "boolean", "description": "Re-measure instead of using the cached result (default false)." }
            }), &[])
        },
        {
            "name": "sbx_run",
            "description": format!("Run a snippet in a throwaway sandbox and delete it afterwards. This is the tool for almost every 'run this Python for me' request. The network is OFF by default. Languages: {langs}."),
            "inputSchema": obj(json!({
                "language": { "type": "string", "description": format!("One of: {langs}") },
                "code": { "type": "string", "description": "The source code to run." },
                "backend": { "type": "string", "enum": ["direct", "docker"], "description": "Leave empty to pick automatically based on what this machine supports." },
                "network": { "type": "boolean", "description": "Allow network access (default false)." },
                "timeoutMs": { "type": "integer", "description": "Run deadline in ms; default 30000, maximum 600000." }
            }), &["language", "code"])
        },
        {
            "name": "sbx_create",
            "description": "Create a long-lived sandbox for several commands in a row (files and installed packages persist between runs). Use it for multi-step work; for a single snippet use sbx_run instead.",
            "inputSchema": obj(json!({
                "name": { "type": "string" },
                "backend": { "type": "string", "enum": ["direct", "docker"] },
                "image": { "type": "string", "description": "Docker image, docker backend only (default python:3.12-slim)." },
                "network": { "type": "boolean", "description": "Default false. Must be on to install packages." },
                "cpus": { "type": "number" },
                "memoryMb": { "type": "integer" },
                "timeoutMs": { "type": "integer", "description": "Default deadline for each run in this sandbox." },
                "listenPorts": { "type": "array", "items": { "type": "integer" }, "description": "Ports the sandbox may serve on, reachable at 127.0.0.1:<port>." },
                "connectPorts": { "type": "array", "items": { "type": "integer" }, "description": "The only remote ports it may dial out to, e.g. [443]." },
                "fsMode": { "type": "string", "enum": ["strict", "allowlist", "open"], "description": "Disk READ isolation. strict = only the sandbox and mounted folders (default). allowlist = plus the folders declared in settings. open = the whole disk. Leave empty to use the default from settings." }
            }), &[])
        },
        {
            "name": "sbx_list",
            "description": "List existing sandboxes with their status, backend and resource limits.",
            "inputSchema": obj(json!({}), &[])
        },
        {
            "name": "sbx_exec",
            "description": "Run a shell command in an existing sandbox. The command reaches the shell on stdin, so quotes inside it survive untouched.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "command": { "type": "string", "description": "Shell command; may span several lines." },
                "timeoutMs": { "type": "integer" },
                "env": { "type": "object", "description": "Extra environment variables for this run." }
            }), &["sandboxId", "command"])
        },
        {
            "name": "sbx_run_in",
            "description": format!("Run a snippet inside an existing sandbox, keeping its state. Languages: {langs}."),
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "language": { "type": "string" },
                "code": { "type": "string" },
                "timeoutMs": { "type": "integer" },
                "env": { "type": "object" }
            }), &["sandboxId", "language", "code"])
        },
        {
            "name": "sbx_install",
            "description": "Install packages into a sandbox with pip, npm or apt. The sandbox must have the network on — use sbx_update to turn it on first.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "manager": { "type": "string", "enum": ["pip", "npm", "apt"] },
                "packages": { "type": "array", "items": { "type": "string" } },
                "timeoutMs": { "type": "integer", "description": "Default 300000, because installs are slow." }
            }), &["sandboxId", "manager", "packages"])
        },
        {
            "name": "sbx_update",
            "description": "Change a sandbox: network on/off, CPU/RAM limits, run deadline. On the docker backend, changing the network or resources recreates the container (files are kept).",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "name": { "type": "string" },
                "network": { "type": "boolean" },
                "cpus": { "type": "number" },
                "memoryMb": { "type": "integer" },
                "timeoutMs": { "type": "integer" }
            }), &["sandboxId"])
        },
        {
            "name": "sbx_delete",
            "description": "Delete a sandbox. Files are KEPT by default; pass purge=true to delete them as well.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "purge": { "type": "boolean", "description": "Also delete the files. Not recoverable." }
            }), &["sandboxId"])
        },
        {
            "name": "sbx_files",
            "description": "List files in the sandbox by relative path.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "path": { "type": "string", "description": "Relative to the sandbox root; empty means the root." }
            }), &["sandboxId"])
        },
        {
            "name": "sbx_file_read",
            "description": "Read a text file from the sandbox.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "path": { "type": "string" }
            }), &["sandboxId", "path"])
        },
        {
            "name": "sbx_file_write",
            "description": "Write a text file into the sandbox (parent folders are created). Use it to hand data to the code.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "path": { "type": "string" },
                "content": { "type": "string" }
            }), &["sandboxId", "path", "content"])
        },
        {
            "name": "sbx_stats",
            "description": "How much CPU and RAM the sandbox is using, with the processes running inside it (pid, %CPU, RAM, elapsed, command). Use it when the user asks whether something is still running, why the machine feels slow, or before deciding what to stop.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" }
            }), &["sandboxId"])
        },
        {
            "name": "sbx_kill",
            "description": "Stop processes in a sandbox. Omit `pid` to stop EVERYTHING it is running. Only processes belonging to that sandbox can be stopped — nothing else on the machine.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "pid": { "type": "integer", "description": "Omit to stop everything. Take a pid from sbx_stats." }
            }), &["sandboxId"])
        },
        {
            "name": "sbx_mount",
            "description": "Mount a real folder from this machine into a sandbox so the code can read and write actual data. It is READ-WRITE by default — pass readOnly=true when reading is enough, and prefer readOnly whenever the code is not yet trusted. The home directory, system directories and credential folders cannot be mounted.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "source": { "type": "string", "description": "Absolute path of the folder on the real machine." },
                "target": { "type": "string", "description": "Folder name inside the sandbox. Empty means the source folder's own name." },
                "readOnly": { "type": "boolean", "description": "Read-only (default false)." }
            }), &["sandboxId", "source"])
        },
        {
            "name": "sbx_unmount",
            "description": "Unmount a folder from a sandbox. This only removes the link; it does NOT delete anything in the real folder.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "target": { "type": "string", "description": "The folder name inside the sandbox, as given when mounting." }
            }), &["sandboxId", "target"])
        },
        {
            "name": "sbx_fs_mode",
            "description": "Change a sandbox's disk READ isolation: `strict` (only the sandbox and its mounts), `allowlist` (plus the folders declared in settings), `open` (the whole disk). Takes effect on the next run. Not applicable to the docker backend — a container is already fully isolated.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "fsMode": { "type": "string", "enum": ["strict", "allowlist", "open"] }
            }), &["sandboxId", "fsMode"])
        },
        {
            "name": "sbx_settings",
            "description": "Read or change the app defaults: the read-isolation new sandboxes start with, the folders readable in `allowlist` mode, and the default network/CPU/RAM/deadline. Call it with no arguments just to read them.",
            "inputSchema": obj(json!({
                "defaultFsMode": { "type": "string", "enum": ["strict", "allowlist", "open"] },
                "allowlist": { "type": "array", "items": { "type": "string" }, "description": "Absolute paths. REPLACES the whole list rather than adding to it." },
                "defaultNetwork": { "type": "boolean" },
                "defaultMemoryMb": { "type": "integer" },
                "defaultCpus": { "type": "number" },
                "defaultTimeoutMs": { "type": "integer" }
            }), &[])
        },
        {
            "name": "sbx_ports",
            "description": "Open specific ports for a sandbox while the rest of the network stays closed. `listen` = ports the sandbox may serve on, reachable from this machine at 127.0.0.1:<port> — this is how you run an app inside a sandbox. `connect` = the only remote ports it may dial out to, so `connect:[443]` means HTTPS and nothing else. Sending empty lists closes everything again. On macOS both directions are enforced exactly; on docker and Linux opening a port grants the sandbox a network, and the reply says so.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "listen": { "type": "array", "items": { "type": "integer" }, "description": "Ports the sandbox may bind (1024 and above). REPLACES the current list." },
                "connect": { "type": "array", "items": { "type": "integer" }, "description": "Remote ports it may connect out to, e.g. [443]. REPLACES the current list." }
            }), &["sandboxId"])
        },
        {
            "name": "sbx_trace",
            "description": "Turn activity tracing on or off, for testing: it records file reads and writes, process launches, and which addresses were contacted. OFF by default. Turn it on, run the code again, then read the result with sbx_events. NOTE: this is an observation tool for testing, NOT security evidence — the hook runs inside the sandbox, so code that deliberately hides can evade it. What actually stops hostile code is the sandbox itself (read/write/network isolation), not this.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "enabled": { "type": "boolean" }
            }), &["sandboxId", "enabled"])
        },
        {
            "name": "sbx_events",
            "description": "Read the traced events: which files were read or written, which processes were launched, which addresses were contacted (including hostnames looked up). Filter with `kind` = file | proc | net, or with `runId` for a single run.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "runId": { "type": "string", "description": "Take it from `runId` in a run result. Empty means all runs." },
                "kind": { "type": "string", "enum": ["file", "proc", "net"], "description": "Empty means every kind." },
                "limit": { "type": "integer", "description": "Default 200." }
            }), &["sandboxId"])
        },
        {
            "name": "sbx_runs",
            "description": "Run history: command, exit code, duration, and the isolation actually applied.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string", "description": "Leave empty for all of them." },
                "limit": { "type": "integer", "description": "Default 20." }
            }), &[])
        }
    ])
}

async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    let db = &state.db;
    match name {
        "sbx_capabilities" => {
            let c = caps::probe(flag(args, "refresh")).await;
            json_result(json!({
                "os": c.os,
                "backends": c.backends,
                "recommended": c.default_backend(),
                "direct": c.direct,
                "docker": c.docker,
                "hostInterpreters": c.host_interpreters,
                "languages": code::languages(),
            }))
        }

        "sbx_run" => {
            let (lang, src) = (s(args, "language"), s(args, "code"));
            if lang.is_empty() || src.is_empty() {
                return err("`language` and `code` are required".into());
            }
            match runner::run_once(
                db,
                &lang,
                &src,
                opt(args, "backend"),
                flag(args, "network"),
                opt_int(args, "timeoutMs"),
            )
            .await
            {
                Ok((run, sb)) => json_result(json!({
                    "backend": sb.backend,
                    "run": run_summary(&run),
                })),
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_create" => {
            match runner::create(
                db,
                runner::CreateReq {
                    name: opt(args, "name"),
                    backend: opt(args, "backend"),
                    image: opt(args, "image"),
                    network: flag(args, "network"),
                    cpus: args["cpus"].as_f64(),
                    memory_mb: opt_int(args, "memoryMb"),
                    timeout_ms: opt_int(args, "timeoutMs"),
                    env: json!({}),
                    mounts: Vec::new(),
                    fs_mode: opt(args, "fsMode").as_deref().and_then(crate::fsmode::FsMode::parse),
                    ports: crate::ports::validate(
                        &args["listenPorts"].as_array().map(|a| a.iter().filter_map(|v| v.as_u64()).map(|v| v as u16).collect::<Vec<_>>()).unwrap_or_default(),
                        &args["connectPorts"].as_array().map(|a| a.iter().filter_map(|v| v.as_u64()).map(|v| v as u16).collect::<Vec<_>>()).unwrap_or_default(),
                    )
                    .unwrap_or_default(),
                },
            )
            .await
            {
                Ok(sb) => json_result(json!({
                    "sandboxId": sb.id,
                    "name": sb.name,
                    "backend": sb.backend,
                    "image": sb.image,
                    "network": sb.network,
                    "note": "The container/process only starts on the first run.",
                })),
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_list" => match db.list_sandboxes() {
            Ok(v) => json_result(json!({ "sandboxes": v })),
            Err(e) => err(e.to_string()),
        },

        "sbx_exec" => {
            let cmd = s(args, "command");
            if cmd.is_empty() {
                return err("`command` is required".into());
            }
            let sb = match db.sandbox(&s(args, "sandboxId")) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            match runner::exec(
                db,
                &sb,
                &cmd,
                opt_int(args, "timeoutMs"),
                env_map(args),
                "exec",
                None,
                &cmd,
                runner::shell_argv(&sb),
)
            .await
            {
                Ok(run) => json_result(run_summary(&run)),
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_run_in" => {
            let sb = match db.sandbox(&s(args, "sandboxId")) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            match runner::run_code(
                db,
                &sb,
                &s(args, "language"),
                &s(args, "code"),
                opt_int(args, "timeoutMs"),
                env_map(args),
            )
            .await
            {
                Ok(run) => json_result(run_summary(&run)),
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_install" => {
            let sb = match db.sandbox(&s(args, "sandboxId")) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            match runner::install(
                db,
                &sb,
                &s(args, "manager"),
                &str_list(args, "packages"),
                opt_int(args, "timeoutMs"),
            )
            .await
            {
                Ok(run) => json_result(run_summary(&run)),
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_update" => {
            let id = s(args, "sandboxId");
            let before = match db.sandbox(&id) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            let sb = match db.update_limits(
                &id,
                opt(args, "name").as_deref(),
                args["network"].as_bool(),
                args["cpus"].as_f64(),
                opt_int(args, "memoryMb"),
                opt_int(args, "timeoutMs"),
                None,
            ) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            // Same rule as the REST handler: docker limits are baked into
            // `docker run`, so a live container is recreated rather than left
            // running under limits the caller no longer sees.
            let mut restarted = false;
            if sb.backend == "docker"
                && before.status == "running"
                && (before.network != sb.network
                    || before.cpus != sb.cpus
                    || before.memory_mb != sb.memory_mb)
            {
                let _ = runner::stop(db, &sb).await;
                restarted = runner::ensure_started(db, &sb).await.is_ok();
            }
            json_result(json!({ "sandbox": sb, "containerRecreated": restarted }))
        }

        "sbx_delete" => {
            let sb = match db.sandbox(&s(args, "sandboxId")) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            let purge = flag(args, "purge");
            match runner::delete(db, &sb, purge).await {
                Ok(()) => text_result(format!(
                    "Deleted sandbox `{}`{}.",
                    sb.name,
                    if purge { " and all of its files" } else { " (files are still on disk)" }
                )),
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_files" => {
            let sb = match db.sandbox(&s(args, "sandboxId")) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            match files::list(&files::Scope::of(&sb), &s(args, "path")) {
                Ok(entries) => json_result(json!({ "entries": entries })),
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_file_read" => {
            let sb = match db.sandbox(&s(args, "sandboxId")) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            match files::read(&files::Scope::of(&sb), &s(args, "path")) {
                Ok(c) => text_result(c),
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_file_write" => {
            let sb = match db.sandbox(&s(args, "sandboxId")) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            let path = s(args, "path");
            match files::write(
                &files::Scope::of(&sb),
                &path,
                args["content"].as_str().unwrap_or(""),
            ) {
                Ok(n) => text_result(format!("Wrote {n} bytes to `{path}`.")),
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_stats" => {
            let sb = match db.sandbox(&s(args, "sandboxId")) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            json_result(serde_json::to_value(monitor::stats(&sb).await).unwrap_or_default())
        }

        "sbx_kill" => {
            let sb = match db.sandbox(&s(args, "sandboxId")) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            match args["pid"].as_u64() {
                Some(pid) => match monitor::kill_pid(&sb, pid as u32).await {
                    Ok(()) => text_result(format!("Stopped process {pid}.")),
                    Err(e) => err(e.to_string()),
                },
                None => match monitor::kill_all(&sb).await {
                    Ok(0) => text_result("The sandbox has no running processes.".into()),
                    Ok(n) => text_result(format!("Stopped {n} process group(s) belonging to the sandbox.")),
                    Err(e) => err(e.to_string()),
                },
            }
        }

        "sbx_mount" => {
            let id = s(args, "sandboxId");
            let sb = match db.sandbox(&id) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            let m = match mounts::validate(&s(args, "source"), &s(args, "target"), flag(args, "readOnly")) {
                Ok(m) => m,
                Err(e) => return err(e.to_string()),
            };
            let next = match mounts::add(&sb.mounts, m.clone()) {
                Ok(v) => v,
                Err(e) => return err(e.to_string()),
            };
            match db.set_mounts(&id, &next) {
                Ok(sb) => {
                    // A live container has its mounts fixed at `docker run`.
                    let mut note = String::new();
                    if sb.backend == "docker" && sb.status == "running" {
                        let _ = runner::stop(db, &sb).await;
                        note = match runner::ensure_started(db, &sb).await {
                            Ok(_) => " The container was recreated so the new folder is visible.".into(),
                            Err(e) => format!(" NOTE: recreating the container failed: {e}"),
                        };
                    }
                    text_result(format!(
                        "Mounted `{}` into the sandbox at `{}`{}.{note}",
                        m.source,
                        m.target,
                        if m.read_only { " (read-only)" } else { " (read-write)" }
                    ))
                }
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_unmount" => {
            let id = s(args, "sandboxId");
            let sb = match db.sandbox(&id) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            let target = s(args, "target");
            let next = mounts::remove(&sb.mounts, &target);
            if next.len() == sb.mounts.len() {
                return err(format!("the sandbox has no folder mounted at `{target}`"));
            }
            match db.set_mounts(&id, &next) {
                Ok(sb) => {
                    // Remove the symlink so the file browser stops showing a
                    // broken entry. This never touches the real folder.
                    let link = std::path::Path::new(&sb.workdir).join(&target);
                    if std::fs::symlink_metadata(&link)
                        .map(|m| m.is_symlink())
                        .unwrap_or(false)
                    {
                        let _ = std::fs::remove_file(&link);
                    }
                    if sb.backend == "docker" && sb.status == "running" {
                        let _ = runner::stop(db, &sb).await;
                        let _ = runner::ensure_started(db, &sb).await;
                    }
                    text_result(format!(
                        "Unmounted `{target}`. The real folder is untouched."
                    ))
                }
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_fs_mode" => {
            let Some(mode) = crate::fsmode::FsMode::parse(&s(args, "fsMode")) else {
                return err(format!(
                    "invalid mode `{}` (strict, allowlist, open)",
                    s(args, "fsMode")
                ));
            };
            let id = s(args, "sandboxId");
            let sb = match db.sandbox(&id) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            if sb.backend == "docker" {
                return text_result(
                    "This sandbox uses docker — a container already isolates the whole disk, so there is no read mode to change."
                        .into(),
                );
            }
            match db.set_fs_mode(&id, mode) {
                Ok(sb) => text_result(format!(
                    "Sandbox `{}` is now in mode: {}",
                    sb.name,
                    mode.label()
                )),
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_settings" => {
            let cur = crate::settings::load(db);
            let touched = ["defaultFsMode", "allowlist", "defaultNetwork", "defaultMemoryMb", "defaultCpus", "defaultTimeoutMs"]
                .iter()
                .any(|k| !args[*k].is_null());
            if !touched {
                return json_result(serde_json::to_value(&cur).unwrap_or_default());
            }
            let next = crate::settings::Settings {
                default_fs_mode: opt(args, "defaultFsMode")
                    .and_then(|v| crate::fsmode::FsMode::parse(&v))
                    .unwrap_or(cur.default_fs_mode),
                allowlist: if args["allowlist"].is_null() {
                    cur.allowlist.clone()
                } else {
                    str_list(args, "allowlist")
                },
                default_network: args["defaultNetwork"].as_bool().unwrap_or(cur.default_network),
                default_memory_mb: opt_int(args, "defaultMemoryMb").unwrap_or(cur.default_memory_mb),
                default_cpus: args["defaultCpus"].as_f64().unwrap_or(cur.default_cpus),
                default_timeout_ms: opt_int(args, "defaultTimeoutMs").unwrap_or(cur.default_timeout_ms),
            };
            match crate::settings::save(db, &next) {
                Ok(saved) => json_result(serde_json::to_value(saved).unwrap_or_default()),
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_ports" => {
            let id = s(args, "sandboxId");
            let before = match db.sandbox(&id) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            let nums = |k: &str| -> Vec<u16> {
                args[k]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_u64()).map(|v| v as u16).collect())
                    .unwrap_or_default()
            };
            let policy = match crate::ports::validate(&nums("listen"), &nums("connect")) {
                Ok(p) => p,
                Err(e) => return err(e.to_string()),
            };
            match db.set_ports(&id, &policy) {
                Ok(sb) => {
                    if sb.backend == "docker" && before.status == "running" {
                        let _ = runner::stop(db, &sb).await;
                        let _ = runner::ensure_started(db, &sb).await;
                    }
                    let isolation = caps::direct_caps(false).await.kind.as_str().to_string();
                    json_result(json!({
                        "listen": sb.ports.listen,
                        "connect": sb.ports.connect,
                        "reachableAt": sb.ports.listen.iter()
                            .map(|p| format!("127.0.0.1:{p}"))
                            .collect::<Vec<_>>(),
                        "note": crate::ports::note_for(&sb.backend, &isolation, &sb.ports),
                    }))
                }
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_trace" => {
            let on = flag(args, "enabled");
            match db.set_trace(&s(args, "sandboxId"), on) {
                Ok(sb) => text_result(format!(
                    "Activity tracing for `{}`: {}.{}",
                    sb.name,
                    if on { "ON" } else { "OFF" },
                    if on {
                        " Run the code again, then read sbx_events. This is an observation tool for testing, not security evidence."
                    } else {
                        ""
                    }
                )),
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_events" => {
            let id = s(args, "sandboxId");
            let sb = match db.sandbox(&id) {
                Ok(sb) => sb,
                Err(e) => return err(e.to_string()),
            };
            match db.list_events(
                &id,
                opt(args, "runId").as_deref(),
                opt(args, "kind").as_deref(),
                opt_int(args, "limit").unwrap_or(200),
            ) {
                Ok(events) if events.is_empty() => text_result(if sb.trace_enabled {
                    "No events yet. Tracing is on for this sandbox — run some code and look again."
                        .into()
                } else {
                    format!(
                        "Tracing is OFF for sandbox `{}`, so nothing was recorded. Turn it on with sbx_trace and run the code again.",
                        sb.name
                    )
                }),
                Ok(events) => json_result(json!({ "traceEnabled": sb.trace_enabled, "events": events })),
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_runs" => {
            let limit = opt_int(args, "limit").unwrap_or(20);
            match db.list_runs(opt(args, "sandboxId").as_deref(), limit) {
                Ok(runs) => json_result(json!({ "runs": runs })),
                Err(e) => err(e.to_string()),
            }
        }

        other => err(format!("no such tool `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_schema_and_a_description() {
        let tools = tools_list();
        let arr = tools.as_array().unwrap();
        assert!(!arr.is_empty());
        for t in arr {
            let name = t["name"].as_str().unwrap();
            assert!(name.starts_with("sbx_"), "`{name}` breaks the sbx_ prefix rule");
            assert!(
                t["description"].as_str().map(|d| d.len() > 30).unwrap_or(false),
                "`{name}` needs a description an agent can act on"
            );
            assert_eq!(t["inputSchema"]["type"], "object", "`{name}` schema");
        }
    }

    #[test]
    fn required_fields_all_exist_in_properties() {
        // A required field that is not declared is a schema an agent cannot
        // satisfy — it shows up as a validation error at call time, not here.
        for t in tools_list().as_array().unwrap() {
            let name = t["name"].as_str().unwrap();
            let props = t["inputSchema"]["properties"].as_object().unwrap();
            for r in t["inputSchema"]["required"].as_array().unwrap() {
                let r = r.as_str().unwrap();
                assert!(props.contains_key(r), "`{name}` requires `{r}` but never declares it");
            }
        }
    }

    #[test]
    fn tool_names_are_unique() {
        let names: Vec<_> = tools_list()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "duplicate tool name");
    }

    #[test]
    fn non_string_env_entries_are_dropped_not_stringified() {
        let m = env_map(&json!({ "env": { "A": "1", "B": 2 } }));
        assert_eq!(m.get("A").map(String::as_str), Some("1"));
        assert!(!m.contains_key("B"));
    }

    #[test]
    fn str_list_skips_blanks_and_non_strings() {
        assert_eq!(
            str_list(&json!({ "packages": ["a", "", "  ", 5, "b"] }), "packages"),
            vec!["a", "b"]
        );
    }
}
