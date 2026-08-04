//! Skill-agent tool system — port of `internal/tool` (tool.go, executor.go,
//! file.go, http_tools.go, repo_tools.go, sandbox.go). One `Registry` bundles
//! every tool; skill agents call `execute(name, args)` from their ReAct loop.
//!
//! Security posture mirrors the Go side: exec allowlist + no shell + arg
//! sandboxing + ffmpeg protocol rejection; file tools confined to a path
//! sandbox (data dir + temp); HTTP tools with an SSRF guard; repo tools with a
//! table allowlist.

use crate::db::{self, Db};
use crate::state::Core;
use serde_json::{json, Map, Value};
use std::net::IpAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// ToolSpec + Registry
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, serde::Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct Registry {
    db: Db,
    sandbox: Sandbox,
    exec_allowlist: Vec<String>,
    exec_timeout: Duration,
    http_allow_private: bool,
}

/// Build the standard registry for one agent (Go: main.go tool wiring).
/// Sandbox roots = canonical data dir + the OS temp dir.
pub fn registry(core: &Arc<Core>) -> Registry {
    let data = crate::config::data_dir();
    let roots = vec![data, std::env::temp_dir()];
    Registry {
        db: core.db.clone(),
        sandbox: Sandbox::new(&roots),
        exec_allowlist: crate::config::exec_allowlist(),
        exec_timeout: Duration::from_secs(crate::config::exec_timeout_secs()),
        http_allow_private: crate::config::http_tools_allow_private(),
    }
}

impl Registry {
    pub fn specs(&self) -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: "execute_cmd".into(),
                description: "Execute an allowed command (e.g. ffmpeg, ffprobe). Only commands in the server allowlist are permitted; file arguments are confined to the tool sandbox and network/remote protocol arguments are rejected.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "cmd": {"type": "string", "description": "Command name, must be in the server allowlist"},
                        "args": {"type": "array", "items": {"type": "string"}, "description": "Command arguments"},
                        "cwd": {"type": "string", "description": "Working directory (optional, must be within the sandbox)"}
                    },
                    "required": ["cmd"]
                }),
            },
            ToolSpec {
                name: "file_read".into(),
                description: "Read a text file and return its content as a string.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {"path": {"type": "string", "description": "Absolute or relative file path"}},
                    "required": ["path"]
                }),
            },
            ToolSpec {
                name: "file_write".into(),
                description: "Write text content to a file (creates parent directories as needed).".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File path to write"},
                        "content": {"type": "string", "description": "Text content to write"},
                        "append": {"type": "boolean", "description": "Append instead of overwrite (default false)"}
                    },
                    "required": ["path", "content"]
                }),
            },
            ToolSpec {
                name: "file_list".into(),
                description: "List files and directories inside a directory.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Directory path to list"},
                        "recursive": {"type": "boolean", "description": "Recurse into sub-directories (default false)"}
                    },
                    "required": ["path"]
                }),
            },
            ToolSpec {
                name: "file_read_image".into(),
                description: "Read an image file and return its base64-encoded content with MIME type.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {"path": {"type": "string", "description": "Absolute or relative image file path"}},
                    "required": ["path"]
                }),
            },
            ToolSpec {
                name: "http_get".into(),
                description: "Perform an HTTP GET request and return the response body.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "URL to GET"},
                        "headers": {"type": "object", "description": "Optional request headers as key-value pairs"}
                    },
                    "required": ["url"]
                }),
            },
            ToolSpec {
                name: "http_post".into(),
                description: "Perform an HTTP POST request with a JSON or text body.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "URL to POST"},
                        "body": {"type": "string", "description": "Request body (JSON string or plain text)"},
                        "headers": {"type": "object", "description": "Optional request headers"}
                    },
                    "required": ["url"]
                }),
            },
            ToolSpec {
                name: "repo_get".into(),
                description: "Fetch a single record by id from a repo table.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "table": {"type": "string", "description": "Table name (project, character, video, scene, request, dag_parents, dag_tasks)"},
                        "id": {"type": "string", "description": "Record id"}
                    },
                    "required": ["table", "id"]
                }),
            },
            ToolSpec {
                name: "repo_list".into(),
                description: "List records from a repo table with optional equality filters.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "table": {"type": "string", "description": "Table name"},
                        "filters": {"type": "object", "description": "Key-value equality filters (optional)"},
                        "limit": {"type": "integer", "description": "Max rows to return (default 100)"}
                    },
                    "required": ["table"]
                }),
            },
            ToolSpec {
                name: "repo_create".into(),
                description: "Insert a new record into a repo table. An id is auto-generated if not provided.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "table": {"type": "string", "description": "Table name"},
                        "fields": {"type": "object", "description": "Field key-value pairs"}
                    },
                    "required": ["table", "fields"]
                }),
            },
            ToolSpec {
                name: "repo_update".into(),
                description: "Update fields on an existing record by id.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "table": {"type": "string", "description": "Table name"},
                        "id": {"type": "string", "description": "Record id"},
                        "fields": {"type": "object", "description": "Fields to update"}
                    },
                    "required": ["table", "id", "fields"]
                }),
            },
        ]
    }

    pub async fn execute(&self, name: &str, args: Value) -> Result<Value, String> {
        let obj = args.as_object().cloned().unwrap_or_default();
        match name {
            "execute_cmd" => self.execute_cmd(&obj).await,
            "file_read" => self.file_read(&obj),
            "file_write" => self.file_write(&obj),
            "file_list" => self.file_list(&obj),
            "file_read_image" => self.file_read_image(&obj),
            "http_get" => self.http_request(&obj, false).await,
            "http_post" => self.http_request(&obj, true).await,
            "repo_get" => self.repo_get(&obj),
            "repo_list" => self.repo_list(&obj),
            "repo_create" => self.repo_create(&obj),
            "repo_update" => self.repo_update(&obj),
            other => Err(format!("tool {other:?} not registered")),
        }
    }

    // ---- execute_cmd (executor.go) ----

    async fn execute_cmd(&self, input: &Map<String, Value>) -> Result<Value, String> {
        let cmd = sval(input, "cmd");
        if cmd.is_empty() {
            return Err("execute_cmd: cmd is required".to_string());
        }
        check_exec_allowlist(&self.exec_allowlist, &cmd)?;

        let mut args: Vec<String> = Vec::new();
        if let Some(Value::Array(raw)) = input.get("args") {
            for a in raw {
                args.push(match a {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                });
            }
        }
        for a in &args {
            validate_exec_arg(a, &self.sandbox).map_err(|e| format!("execute_cmd: {e}"))?;
        }

        let mut c = tokio::process::Command::new(&cmd);
        c.args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let cwd = sval(input, "cwd");
        if !cwd.is_empty() {
            let resolved = self
                .sandbox
                .resolve(&cwd)
                .map_err(|e| format!("execute_cmd: cwd: {e}"))?;
            c.current_dir(resolved);
        }

        let fut = c.output();
        let out = if self.exec_timeout > Duration::ZERO {
            tokio::time::timeout(self.exec_timeout, fut)
                .await
                .map_err(|_| {
                    format!("execute_cmd {cmd}: timed out after {:?}", self.exec_timeout)
                })?
        } else {
            fut.await
        }
        .map_err(|e| format!("execute_cmd {cmd}: {e}"))?;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&out.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&out.stderr).to_string(),
            "exit_code": out.status.code().unwrap_or(-1),
        }))
    }

    // ---- file tools (file.go) ----

    fn file_read(&self, input: &Map<String, Value>) -> Result<Value, String> {
        let safe = self
            .sandbox
            .resolve(&sval(input, "path"))
            .map_err(|e| format!("file_read: {e}"))?;
        let data = std::fs::read(&safe).map_err(|e| format!("file_read: {e}"))?;
        Ok(json!({
            "content": String::from_utf8_lossy(&data).to_string(),
            "size": data.len(),
        }))
    }

    fn file_write(&self, input: &Map<String, Value>) -> Result<Value, String> {
        let safe = self
            .sandbox
            .resolve(&sval(input, "path"))
            .map_err(|e| format!("file_write: {e}"))?;
        let content = sval_raw(input, "content");
        if let Some(parent) = safe.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("file_write: mkdir: {e}"))?;
        }
        let append = input
            .get("append")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&safe)
            .map_err(|e| format!("file_write: {e}"))?;
        f.write_all(content.as_bytes())
            .map_err(|e| format!("file_write: {e}"))?;
        Ok(json!({ "bytes_written": content.len(), "path": safe.to_string_lossy() }))
    }

    fn file_list(&self, input: &Map<String, Value>) -> Result<Value, String> {
        let dir = self
            .sandbox
            .resolve(&sval(input, "path"))
            .map_err(|e| format!("file_list: {e}"))?;
        let recursive = input
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let mut entries: Vec<Value> = Vec::new();
        walk_dir(&dir, recursive, &mut entries).map_err(|e| format!("file_list: {e}"))?;
        Ok(json!({ "entries": entries, "count": entries.len() }))
    }

    fn file_read_image(&self, input: &Map<String, Value>) -> Result<Value, String> {
        let safe = self
            .sandbox
            .resolve(&sval(input, "path"))
            .map_err(|e| format!("file_read_image: {e}"))?;
        let data = std::fs::read(&safe).map_err(|e| format!("file_read_image: {e}"))?;
        let ext = safe
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        Ok(json!({
            "base64": base64_encode(&data),
            "mime_type": mime_by_ext(&ext),
            "size": data.len(),
            "path": safe.to_string_lossy(),
        }))
    }

    // ---- http tools (http_tools.go) ----

    async fn http_request(&self, input: &Map<String, Value>, post: bool) -> Result<Value, String> {
        let tool = if post { "http_post" } else { "http_get" };
        let url = sval(input, "url");
        if url.is_empty() {
            return Err(format!("{tool}: url is required"));
        }
        let parsed = reqwest::Url::parse(&url).map_err(|e| format!("{tool}: {e}"))?;
        if !self.http_allow_private {
            ssrf_check(&parsed)
                .await
                .map_err(|e| format!("{tool}: {e}"))?;
        }

        let client = http_client(self.http_allow_private);
        let mut req = if post {
            let body = sval_raw(input, "body");
            client
                .post(parsed)
                .header("Content-Type", "application/json")
                .body(body)
        } else {
            client.get(parsed)
        };
        if let Some(Value::Object(headers)) = input.get("headers") {
            for (k, v) in headers {
                let vs = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                req = req.header(k.as_str(), vs);
            }
        }
        let resp = req.send().await.map_err(|e| format!("{tool}: {e}"))?;
        let status = resp.status().as_u16();
        let mut headers = Map::new();
        for (k, v) in resp.headers() {
            headers.insert(k.to_string(), json!(v.to_str().unwrap_or("")));
        }
        let body = resp
            .text()
            .await
            .map_err(|e| format!("{tool}: read response: {e}"))?;
        Ok(json!({ "status_code": status, "body": body, "headers": headers }))
    }

    // ---- repo tools (repo_tools.go) ----

    fn repo_get(&self, input: &Map<String, Value>) -> Result<Value, String> {
        let table = sval(input, "table");
        check_table(&table)?;
        let id = sval(input, "id");
        let row = self.db.get(&table, &id).map_err(|e| e.to_string())?;
        Ok(row.map(Value::Object).unwrap_or(Value::Null))
    }

    fn repo_list(&self, input: &Map<String, Value>) -> Result<Value, String> {
        let table = sval(input, "table");
        check_table(&table)?;
        let cols =
            db::table_columns(&table).ok_or_else(|| format!("repo: unknown table {table:?}"))?;
        let mut conds: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(Value::Object(filters)) = input.get("filters") {
            for (k, v) in filters {
                // Column names come from the LLM — allowlist them (the Go code
                // concatenated raw keys; here we refuse unknown columns).
                if !cols.contains(&k.as_str()) {
                    return Err(format!(
                        "repo_list: unknown filter column {k:?} for table {table:?}"
                    ));
                }
                conds.push(format!("{k} = ?{}", params.len() + 1));
                params.push(to_sql_box(v));
            }
        }
        let mut limit = input.get("limit").and_then(|v| v.as_i64()).unwrap_or(0);
        if limit <= 0 {
            limit = 100;
        }
        let mut sql = format!("SELECT * FROM {table}");
        if !conds.is_empty() {
            sql.push_str(&format!(" WHERE {}", conds.join(" AND ")));
        }
        sql.push_str(&format!(" ORDER BY rowid DESC LIMIT {limit}"));
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let rows = self.db.query(&sql, &refs).map_err(|e| e.to_string())?;
        Ok(
            json!({ "count": rows.len(), "rows": rows.into_iter().map(Value::Object).collect::<Vec<_>>() }),
        )
    }

    fn repo_create(&self, input: &Map<String, Value>) -> Result<Value, String> {
        let table = sval(input, "table");
        check_table(&table)?;
        let fields = match input.get("fields") {
            Some(Value::Object(f)) if !f.is_empty() => f.clone(),
            _ => return Err("repo_create: fields is required".to_string()),
        };
        let id = self
            .db
            .insert(&table, &fields)
            .map_err(|e| format!("repo_create: {e}"))?;
        let row = self.db.get(&table, &id).map_err(|e| e.to_string())?;
        Ok(row.map(Value::Object).unwrap_or(Value::Null))
    }

    fn repo_update(&self, input: &Map<String, Value>) -> Result<Value, String> {
        let table = sval(input, "table");
        check_table(&table)?;
        let id = sval(input, "id");
        let fields = match input.get("fields") {
            Some(Value::Object(f)) if !f.is_empty() => f.clone(),
            _ => return Err("repo_update: fields is required".to_string()),
        };
        self.db
            .update(&table, &id, &fields)
            .map_err(|e| format!("repo_update: {e}"))?;
        let row = self.db.get(&table, &id).map_err(|e| e.to_string())?;
        Ok(row.map(Value::Object).unwrap_or(Value::Null))
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn sval(m: &Map<String, Value>, k: &str) -> String {
    m.get(k)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Untrimmed string value (file content, request body).
fn sval_raw(m: &Map<String, Value>, k: &str) -> String {
    m.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn to_sql_box(v: &Value) -> Box<dyn rusqlite::ToSql> {
    match v {
        Value::Null => Box::new(None::<String>),
        Value::Bool(b) => Box::new(*b as i64),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Box::new(i)
            } else {
                Box::new(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => Box::new(s.clone()),
        other => Box::new(other.to_string()),
    }
}

fn check_exec_allowlist(allowlist: &[String], cmd: &str) -> Result<(), String> {
    if allowlist.iter().any(|a| a == cmd) {
        Ok(())
    } else {
        Err(format!("execute_cmd: {cmd:?} is not in the allowlist"))
    }
}

const ALLOWED_REPO_TABLES: &[&str] = &[
    "project",
    "character",
    "video",
    "scene",
    "request",
    "dag_parents",
    "dag_tasks",
];

fn check_table(table: &str) -> Result<(), String> {
    if ALLOWED_REPO_TABLES.contains(&table) {
        Ok(())
    } else {
        Err(format!("repo: table {table:?} is not permitted"))
    }
}

fn walk_dir(dir: &Path, recursive: bool, out: &mut Vec<Value>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let is_dir = p.is_dir();
        if !recursive && is_dir {
            // Go's WalkDir + SkipDir behavior: non-recursive mode lists files only.
            continue;
        }
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(json!({
            "name": e.file_name().to_string_lossy(),
            "path": p.to_string_lossy(),
            "is_dir": is_dir,
            "size": size,
        }));
        if recursive && is_dir {
            walk_dir(&p, recursive, out)?;
        }
    }
    Ok(())
}

// ---- exec argument validation (executor.go) ----

/// ffmpeg/ffprobe input "protocols" that enable SSRF or arbitrary host reads.
const RISKY_PROTOCOLS: &[&str] = &[
    "http", "https", "ftp", "ftps", "sftp", "ssh", "tcp", "udp", "rtp", "rtmp", "rtmps", "rtsp",
    "srt", "tls", "unix", "gopher", "telnet", "hls", "file", "concat", "pipe", "data", "subfile",
    "crypto", "cache", "tee", "md5", "async",
];

/// Reject ffmpeg protocol URIs and path-style arguments that escape the
/// sandbox. Flags, plain options, filter strings and in-sandbox paths pass.
pub(crate) fn validate_exec_arg(arg: &str, sb: &Sandbox) -> Result<(), String> {
    if arg.is_empty() || arg.starts_with('-') {
        return Ok(());
    }
    if let Some(i) = arg.find(':') {
        if i > 0 {
            let scheme = arg[..i].to_lowercase();
            if is_scheme_token(&scheme) && RISKY_PROTOCOLS.contains(&scheme.as_str()) {
                return Err(format!("blocked protocol {scheme:?} in argument {arg:?}"));
            }
        }
    }
    if looks_like_path(arg) {
        sb.resolve(arg)?;
    }
    Ok(())
}

/// Whether s is a bare URI scheme token (letters/digits/+-.), so "http" is a
/// protocol but "scale=1280" (from "scale=1280:720") is not.
fn is_scheme_token(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|r| {
            r.is_ascii_lowercase() || r.is_ascii_digit() || r == '+' || r == '-' || r == '.'
        })
}

/// Whether arg is a filesystem path that should be sandbox-checked: absolute,
/// home-relative, or containing parent-directory traversal.
fn looks_like_path(arg: &str) -> bool {
    if arg.is_empty() || arg.starts_with('-') {
        return false;
    }
    arg.starts_with('/')
        || arg.starts_with('~')
        || arg.starts_with("./")
        || arg.starts_with("../")
        || arg.contains("/../")
        || arg.ends_with("/..")
}

// ---- SSRF guard (http_tools.go) ----

fn http_client(allow_private: bool) -> &'static reqwest::Client {
    // Two static clients: the guarded one never follows redirects (a redirect
    // to an internal address would bypass the pre-resolution check).
    static GUARDED: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    static OPEN: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    let build = |redirects: bool| {
        let mut b = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10));
        if !redirects {
            b = b.redirect(reqwest::redirect::Policy::none());
        }
        b.build().expect("build http tool client")
    };
    if allow_private {
        OPEN.get_or_init(|| build(true))
    } else {
        GUARDED.get_or_init(|| build(false))
    }
}

/// Resolve the URL host and reject internal addresses. The Go version hooked
/// the dialer (post-DNS); here we pre-resolve and check every candidate IP.
async fn ssrf_check(url: &reqwest::Url) -> Result<(), String> {
    let host = url.host_str().ok_or("ssrf guard: url has no host")?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port = url.port_or_known_default().unwrap_or(80);
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_blocked_ip(&ip) {
            return Err(blocked_msg(&ip));
        }
        return Ok(());
    }
    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("ssrf guard: unresolvable address {host:?}: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("ssrf guard: unresolvable address {host:?}"));
    }
    for a in addrs {
        if is_blocked_ip(&a.ip()) {
            return Err(blocked_msg(&a.ip()));
        }
    }
    Ok(())
}

fn blocked_msg(ip: &IpAddr) -> String {
    format!("ssrf guard: blocked internal address {ip} (set FLOWKIT_TOOL_HTTP_ALLOW_PRIVATE=1 to allow)")
}

/// Whether an IP is in a range tools must not reach: loopback, private,
/// link-local (incl. 169.254.169.254 cloud metadata), unspecified.
pub(crate) fn is_blocked_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_unspecified()
                || v4.is_link_local()
                || v4.is_broadcast()
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_ip(&IpAddr::V4(mapped));
            }
            let seg0 = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || (seg0 & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (seg0 & 0xffc0) == 0xfe80 // link-local fe80::/10
        }
    }
}

// ---- Sandbox (sandbox.go) ----

/// Restricts filesystem tool access to a set of root directories. A Sandbox
/// with no roots is disabled (allows any path).
pub struct Sandbox {
    roots: Vec<PathBuf>,
}

impl Sandbox {
    pub fn new(roots: &[PathBuf]) -> Sandbox {
        let mut clean = Vec::new();
        for r in roots {
            if r.as_os_str().is_empty() {
                continue;
            }
            let abs = absolutize(r);
            let real = abs.canonicalize().unwrap_or(abs);
            clean.push(real);
        }
        Sandbox { roots: clean }
    }

    /// Cleaned absolute path for the request, or an error when it falls outside
    /// every configured root. Existing-portion symlinks are resolved first so a
    /// symlink cannot escape the sandbox.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        if path.trim().is_empty() {
            return Err("path is required".to_string());
        }
        let abs = clean_path(&absolutize(Path::new(path)));
        if self.roots.is_empty() {
            return Ok(abs);
        }
        let real = resolve_existing_symlinks(&abs);
        for root in &self.roots {
            if real == *root || real.starts_with(root) {
                return Ok(abs);
            }
        }
        Err(format!("path {path:?} is outside the tool sandbox"))
    }
}

fn absolutize(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    }
}

/// Lexical path normalization ("." removed, ".." collapsed).
fn clean_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(comp.as_os_str());
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve symlinks on the longest existing ancestor of `abs` and re-append the
/// non-existent remainder, so writes to not-yet-created files are checked
/// against the real location of their parent directory.
fn resolve_existing_symlinks(abs: &Path) -> PathBuf {
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = abs.to_path_buf();
    loop {
        if let Ok(real) = cur.canonicalize() {
            let mut out = real;
            for c in rest.iter().rev() {
                out.push(c);
            }
            return out;
        }
        let (name, parent) = (
            cur.file_name().map(|s| s.to_os_string()),
            cur.parent().map(|p| p.to_path_buf()),
        );
        match (name, parent) {
            (Some(name), Some(parent)) if parent != cur => {
                rest.push(name);
                cur = parent;
            }
            _ => return abs.to_path_buf(),
        }
    }
}

// ---- base64 (hand-rolled — no extra crate) ----

pub fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18 & 63) as usize] as char);
        out.push(TABLE[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn mime_by_ext(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_sandbox() -> Sandbox {
        Sandbox::new(&[std::env::temp_dir()])
    }

    #[test]
    fn exec_allowlist_rejects_unknown_commands() {
        let list = vec!["ffmpeg".to_string(), "ffprobe".to_string()];
        assert!(check_exec_allowlist(&list, "ffmpeg").is_ok());
        assert!(check_exec_allowlist(&list, "ffprobe").is_ok());
        let err = check_exec_allowlist(&list, "rm").unwrap_err();
        assert!(err.contains("not in the allowlist"), "{err}");
        assert!(check_exec_allowlist(&list, "bash").is_err());
        assert!(check_exec_allowlist(&list, "").is_err());
    }

    #[tokio::test]
    async fn registry_execute_cmd_rejects_disallowed() {
        let core = std::sync::Arc::new(crate::state::Core {
            db: crate::db::Db::open_memory().unwrap(),
            dash: crate::dashws::DashHub::new(),
            ext: crate::extbridge::ExtBridge::new(),
            souls_dir: std::env::temp_dir(),
            playbooks_dir: std::env::temp_dir(),
            media_dir: std::env::temp_dir(),
        });
        let reg = registry(&core);
        let err = reg
            .execute("execute_cmd", json!({"cmd": "definitely_not_allowed"}))
            .await
            .unwrap_err();
        assert!(err.contains("not in the allowlist"), "{err}");
    }

    #[test]
    fn exec_arg_validation_blocks_protocols_and_escapes() {
        let sb = test_sandbox();
        // flags and filter strings pass
        assert!(validate_exec_arg("-i", &sb).is_ok());
        assert!(validate_exec_arg("scale=1280:720", &sb).is_ok());
        // risky protocols rejected
        assert!(validate_exec_arg("http://evil/x.mp4", &sb).is_err());
        assert!(validate_exec_arg("concat:a.mp4|b.mp4", &sb).is_err());
        assert!(validate_exec_arg("file:/etc/passwd", &sb).is_err());
        assert!(validate_exec_arg("pipe:0", &sb).is_err());
        // out-of-sandbox absolute path rejected
        assert!(validate_exec_arg("/etc/passwd", &sb).is_err());
        // in-sandbox path passes
        let inside = std::env::temp_dir().join("clip.mp4");
        assert!(validate_exec_arg(inside.to_str().unwrap(), &sb).is_ok());
    }

    #[test]
    fn ssrf_host_classification() {
        let blocked = [
            "127.0.0.1",
            "10.0.0.5",
            "172.16.3.4",
            "192.168.1.1",
            "169.254.169.254", // cloud metadata
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:127.0.0.1",
        ];
        for h in blocked {
            let ip: IpAddr = h.parse().unwrap();
            assert!(is_blocked_ip(&ip), "{h} should be blocked");
        }
        let allowed = [
            "8.8.8.8",
            "1.1.1.1",
            "142.250.72.196",
            "2607:f8b0:4004:c07::64",
        ];
        for h in allowed {
            let ip: IpAddr = h.parse().unwrap();
            assert!(!is_blocked_ip(&ip), "{h} should be allowed");
        }
    }

    #[test]
    fn sandbox_confines_paths() {
        let sb = test_sandbox();
        assert!(sb.resolve("/etc/passwd").is_err());
        let tmp = std::env::temp_dir().join("vf-test.txt");
        assert!(sb.resolve(tmp.to_str().unwrap()).is_ok());
        // traversal out of the sandbox is caught after normalization
        let sneaky = std::env::temp_dir().join("../../etc/passwd");
        assert!(sb.resolve(sneaky.to_str().unwrap()).is_err());
        assert!(sb.resolve("").is_err());
    }

    #[test]
    fn base64_roundtrip_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn repo_table_allowlist() {
        assert!(check_table("scene").is_ok());
        assert!(check_table("media").is_err());
        assert!(check_table("app_kv").is_err());
        assert!(check_table("scene; DROP TABLE scene").is_err());
    }
}
