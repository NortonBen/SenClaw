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
            "description": "Kiểm tra máy này chạy được kiểu sandbox nào: docker (cần daemon đang chạy) hay chạy trực tiếp có rào chắn của hệ điều hành (macOS Seatbelt / Linux bubblewrap). Gọi trước khi tạo sandbox nếu chưa chắc, và gọi lại sau khi người dùng vừa mở Docker.",
            "inputSchema": obj(json!({
                "refresh": { "type": "boolean", "description": "Đo lại thay vì dùng kết quả đã lưu (mặc định false)." }
            }), &[])
        },
        {
            "name": "sbx_run",
            "description": format!("Chạy nhanh một đoạn mã trong sandbox dùng-một-lần rồi xoá sandbox đó đi. Đây là công cụ nên dùng cho hầu hết yêu cầu kiểu 'chạy thử đoạn Python này'. Mạng TẮT mặc định. Ngôn ngữ: {langs}."),
            "inputSchema": obj(json!({
                "language": { "type": "string", "description": format!("Một trong: {langs}") },
                "code": { "type": "string", "description": "Mã nguồn cần chạy." },
                "backend": { "type": "string", "enum": ["direct", "docker"], "description": "Bỏ trống để tự chọn theo khả năng của máy." },
                "network": { "type": "boolean", "description": "Cho phép truy cập mạng (mặc định false)." },
                "timeoutMs": { "type": "integer", "description": "Hạn chạy, mặc định 30000, tối đa 600000." }
            }), &["language", "code"])
        },
        {
            "name": "sbx_create",
            "description": "Tạo một sandbox tồn tại lâu dài để chạy nhiều lệnh nối tiếp nhau (file và gói đã cài được giữ lại giữa các lần chạy). Dùng khi công việc cần nhiều bước; việc chạy một đoạn mã lẻ thì dùng sbx_run.",
            "inputSchema": obj(json!({
                "name": { "type": "string" },
                "backend": { "type": "string", "enum": ["direct", "docker"] },
                "image": { "type": "string", "description": "Docker image, chỉ dùng cho backend docker (mặc định python:3.12-slim)." },
                "network": { "type": "boolean", "description": "Mặc định false. Phải bật thì mới cài được gói." },
                "cpus": { "type": "number" },
                "memoryMb": { "type": "integer" },
                "timeoutMs": { "type": "integer", "description": "Hạn mặc định cho mỗi lần chạy trong sandbox này." },
                "fsMode": { "type": "string", "enum": ["strict", "allowlist", "open"], "description": "Mức cách ly ĐỌC đĩa. strict = chỉ thấy sandbox + thư mục đã gắn (mặc định). allowlist = thêm các thư mục khai trong cài đặt. open = đọc được cả đĩa. Bỏ trống để dùng mặc định trong cài đặt." }
            }), &[])
        },
        {
            "name": "sbx_list",
            "description": "Liệt kê các sandbox đang có kèm trạng thái, backend và giới hạn tài nguyên.",
            "inputSchema": obj(json!({}), &[])
        },
        {
            "name": "sbx_exec",
            "description": "Chạy một lệnh shell trong sandbox đã có. Lệnh được đưa vào shell qua stdin nên dấu nháy trong lệnh giữ nguyên.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "command": { "type": "string", "description": "Lệnh shell (có thể nhiều dòng)." },
                "timeoutMs": { "type": "integer" },
                "env": { "type": "object", "description": "Biến môi trường thêm cho lần chạy này." }
            }), &["sandboxId", "command"])
        },
        {
            "name": "sbx_run_in",
            "description": format!("Chạy một đoạn mã trong sandbox đã có, giữ nguyên trạng thái sandbox. Ngôn ngữ: {langs}."),
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
            "description": "Cài gói vào sandbox bằng pip, npm hoặc apt. Sandbox phải đang bật mạng — nếu chưa thì dùng sbx_update để bật.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "manager": { "type": "string", "enum": ["pip", "npm", "apt"] },
                "packages": { "type": "array", "items": { "type": "string" } },
                "timeoutMs": { "type": "integer", "description": "Mặc định 300000 vì cài gói lâu." }
            }), &["sandboxId", "manager", "packages"])
        },
        {
            "name": "sbx_update",
            "description": "Đổi cấu hình sandbox: bật/tắt mạng, đổi giới hạn CPU/RAM, đổi hạn thời gian. Với backend docker, đổi mạng hoặc tài nguyên sẽ tạo lại container (file trong sandbox vẫn còn).",
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
            "description": "Xoá sandbox. Mặc định GIỮ lại file; đặt purge=true nếu muốn xoá luôn toàn bộ file trong sandbox.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "purge": { "type": "boolean", "description": "Xoá luôn file. Không khôi phục được." }
            }), &["sandboxId"])
        },
        {
            "name": "sbx_files",
            "description": "Liệt kê file trong sandbox theo đường dẫn tương đối.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "path": { "type": "string", "description": "Tương đối với gốc sandbox; bỏ trống là gốc." }
            }), &["sandboxId"])
        },
        {
            "name": "sbx_file_read",
            "description": "Đọc nội dung một file văn bản trong sandbox.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "path": { "type": "string" }
            }), &["sandboxId", "path"])
        },
        {
            "name": "sbx_file_write",
            "description": "Ghi một file văn bản vào sandbox (tự tạo thư mục cha). Dùng để đưa dữ liệu vào cho đoạn mã xử lý.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "path": { "type": "string" },
                "content": { "type": "string" }
            }), &["sandboxId", "path", "content"])
        },
        {
            "name": "sbx_stats",
            "description": "Xem sandbox đang dùng bao nhiêu CPU và RAM, kèm danh sách tiến trình đang chạy bên trong (pid, %CPU, RAM, thời gian chạy, lệnh). Dùng khi người dùng hỏi 'nó có còn chạy không', 'sao máy chậm thế', hoặc trước khi quyết định dừng cái gì.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" }
            }), &["sandboxId"])
        },
        {
            "name": "sbx_kill",
            "description": "Dừng tiến trình trong sandbox. Bỏ trống `pid` để dừng TẤT CẢ những gì sandbox đang chạy. Chỉ dừng được tiến trình thuộc chính sandbox đó — không dừng được tiến trình khác trên máy.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "pid": { "type": "integer", "description": "Bỏ trống = dừng toàn bộ. Lấy pid từ sbx_stats." }
            }), &["sandboxId"])
        },
        {
            "name": "sbx_mount",
            "description": "Gắn một thư mục có thật trên máy vào trong sandbox, để mã trong sandbox đọc/ghi dữ liệu thật. Mặc định gắn cho phép GHI — đặt readOnly=true khi chỉ cần đọc, và nên mặc định chọn readOnly nếu mã nguồn chưa đáng tin. Không gắn được thư mục nhà, thư mục hệ thống, hay các thư mục chứa khoá bí mật.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "source": { "type": "string", "description": "Đường dẫn tuyệt đối của thư mục trên máy thật." },
                "target": { "type": "string", "description": "Tên thư mục bên trong sandbox. Bỏ trống thì lấy tên thư mục gốc." },
                "readOnly": { "type": "boolean", "description": "Chỉ đọc (mặc định false)." }
            }), &["sandboxId", "source"])
        },
        {
            "name": "sbx_unmount",
            "description": "Gỡ một thư mục đã gắn khỏi sandbox. Chỉ gỡ liên kết, KHÔNG xoá dữ liệu trong thư mục đó trên máy thật.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "target": { "type": "string", "description": "Tên thư mục trong sandbox, như khi gắn." }
            }), &["sandboxId", "target"])
        },
        {
            "name": "sbx_fs_mode",
            "description": "Đổi mức cách ly ĐỌC đĩa của một sandbox: `strict` (chỉ thấy sandbox và thư mục đã gắn), `allowlist` (thêm thư mục khai trong cài đặt), `open` (đọc được cả đĩa). Có hiệu lực ngay từ lần chạy kế tiếp. Không áp dụng cho backend docker — container vốn đã cách ly toàn bộ.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "fsMode": { "type": "string", "enum": ["strict", "allowlist", "open"] }
            }), &["sandboxId", "fsMode"])
        },
        {
            "name": "sbx_settings",
            "description": "Xem hoặc đổi cài đặt mặc định của app: mức cách ly đọc mặc định cho sandbox mới, danh sách thư mục cho phép đọc ở chế độ `allowlist`, và mặc định mạng/CPU/RAM/hạn giờ. Gọi không kèm tham số để chỉ xem.",
            "inputSchema": obj(json!({
                "defaultFsMode": { "type": "string", "enum": ["strict", "allowlist", "open"] },
                "allowlist": { "type": "array", "items": { "type": "string" }, "description": "Đường dẫn tuyệt đối. THAY THẾ toàn bộ danh sách cũ, không phải thêm vào." },
                "defaultNetwork": { "type": "boolean" },
                "defaultMemoryMb": { "type": "integer" },
                "defaultCpus": { "type": "number" },
                "defaultTimeoutMs": { "type": "integer" }
            }), &[])
        },
        {
            "name": "sbx_trace",
            "description": "Bật/tắt theo dõi hoạt động của sandbox, dùng khi kiểm thử: ghi lại các sự kiện đọc/ghi file, khởi tạo tiến trình, và kết nối mạng tới địa chỉ nào. Mặc định TẮT. Bật xong thì chạy lại mã, rồi xem bằng sbx_events. LƯU Ý: đây là công cụ quan sát cho kiểm thử, KHÔNG phải bằng chứng an ninh — hook chạy bên trong sandbox nên mã cố tình lẩn tránh thì né được; thứ chặn được mã độc là bản thân sandbox (cách ly đọc/ghi/mạng), không phải cái theo dõi này.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "enabled": { "type": "boolean" }
            }), &["sandboxId", "enabled"])
        },
        {
            "name": "sbx_events",
            "description": "Xem các sự kiện đã theo dõi được: file nào bị đọc/ghi, tiến trình nào được khởi tạo, kết nối mạng tới đâu (kèm tên miền tra cứu). Lọc theo `kind` = file | proc | net, hoặc theo `runId` để chỉ xem một lần chạy.",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string" },
                "runId": { "type": "string", "description": "Lấy từ `runId` trong kết quả chạy. Bỏ trống = tất cả." },
                "kind": { "type": "string", "enum": ["file", "proc", "net"], "description": "Bỏ trống = mọi loại." },
                "limit": { "type": "integer", "description": "Mặc định 200." }
            }), &["sandboxId"])
        },
        {
            "name": "sbx_runs",
            "description": "Xem lịch sử các lần chạy (lệnh, mã thoát, thời gian, mức cách ly đã áp dụng).",
            "inputSchema": obj(json!({
                "sandboxId": { "type": "string", "description": "Bỏ trống để xem tất cả." },
                "limit": { "type": "integer", "description": "Mặc định 20." }
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
                return err("cần `language` và `code`".into());
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
                    "note": "Container/tiến trình chỉ khởi động ở lần chạy đầu tiên.",
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
                return err("cần `command`".into());
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
                    "Đã xoá sandbox `{}`{}.",
                    sb.name,
                    if purge { " cùng toàn bộ file" } else { " (file vẫn còn trên đĩa)" }
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
                Ok(n) => text_result(format!("Đã ghi {n} byte vào `{path}`.")),
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
                    Ok(()) => text_result(format!("Đã dừng tiến trình {pid}.")),
                    Err(e) => err(e.to_string()),
                },
                None => match monitor::kill_all(&sb).await {
                    Ok(0) => text_result("Sandbox không có tiến trình nào đang chạy.".into()),
                    Ok(n) => text_result(format!("Đã dừng {n} nhóm tiến trình của sandbox.")),
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
                            Ok(_) => " Container đã được tạo lại để nhận thư mục mới.".into(),
                            Err(e) => format!(" LƯU Ý: tạo lại container thất bại: {e}"),
                        };
                    }
                    text_result(format!(
                        "Đã gắn `{}` vào sandbox tại `{}`{}.{note}",
                        m.source,
                        m.target,
                        if m.read_only { " (chỉ đọc)" } else { " (đọc-ghi)" }
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
                return err(format!("sandbox không có thư mục gắn tại `{target}`"));
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
                        "Đã gỡ `{target}` khỏi sandbox. Dữ liệu trên máy thật vẫn nguyên."
                    ))
                }
                Err(e) => err(e.to_string()),
            }
        }

        "sbx_fs_mode" => {
            let Some(mode) = crate::fsmode::FsMode::parse(&s(args, "fsMode")) else {
                return err(format!(
                    "chế độ `{}` không hợp lệ (strict, allowlist, open)",
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
                    "Sandbox này chạy bằng docker — container vốn đã cách ly toàn bộ đĩa, \
                     không cần đổi chế độ đọc."
                        .into(),
                );
            }
            match db.set_fs_mode(&id, mode) {
                Ok(sb) => text_result(format!(
                    "Sandbox `{}` giờ ở chế độ: {}",
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

        "sbx_trace" => {
            let on = flag(args, "enabled");
            match db.set_trace(&s(args, "sandboxId"), on) {
                Ok(sb) => text_result(format!(
                    "Theo dõi hoạt động của `{}`: {}.{}",
                    sb.name,
                    if on { "BẬT" } else { "TẮT" },
                    if on {
                        " Chạy lại mã rồi dùng sbx_events để xem. Đây là công cụ quan sát \
                          cho kiểm thử, không phải bằng chứng an ninh."
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
                    "Chưa có sự kiện nào. Sandbox đã bật theo dõi — hãy chạy mã rồi xem lại."
                        .into()
                } else {
                    format!(
                        "Sandbox `{}` đang TẮT theo dõi nên không có gì được ghi lại. \
                         Bật bằng sbx_trace rồi chạy lại mã.",
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

        other => err(format!("không có công cụ `{other}`")),
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
