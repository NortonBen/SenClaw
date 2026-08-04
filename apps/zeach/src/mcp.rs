//! `zeach-mcp` — hand-rolled JSON-RPC MCP over HTTP + SSE.
//!
//! The `rmcp` crate is not used here; this mirrors the shape every other Rust
//! Space App uses (`apps/social/src/mcp.rs`) so the daemon's auto-registration
//! (`src/gateway/ui_server/space_mcp.rs`) picks it up unchanged.
//!
//! This is requirement #3 of the design: other agents, apps and skills call
//! `zeach_search` instead of each re-implementing retrieval.
//!
//! P0 ships the retrieval half of the surface. `zeach_ask`, `zeach_deep`,
//! `zeach_report`, `zeach_verify`, corpus and monitor tools land in P1–P4.

use crate::model::SourceKind;
use crate::pipeline::{self, SearchRequest};
use crate::sources::mcp_source::{FieldMap, McpSource, McpSourceSpec, McpTarget};
use crate::sources::SourceOrigin;
use crate::state::AppState;
use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

pub const SERVER_NAME: &str = "zeach-mcp";

#[derive(Deserialize)]
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
        yield Ok(Event::default().event("endpoint").data("/api/mcp/message".to_string()));
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

fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

pub async fn mcp_message(
    State(state): State<AppState>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<Value> {
    let reply = |result: Value| -> Json<Value> {
        let resp = json!({ "jsonrpc": "2.0", "id": req.id, "result": result });
        let _ = state.mcp_tx.send(resp.to_string());
        Json(resp)
    };

    match req.method.as_str() {
        "initialize" => reply(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER_NAME, "version": "1.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => {
            Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} }))
        }
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

// ---- argument helpers ------------------------------------------------------

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn arg_usize(args: &Value, key: &str) -> Option<usize> {
    args.get(key).and_then(Value::as_u64).map(|v| v as usize)
}

fn arg_str_list(args: &Value, key: &str) -> Option<Vec<String>> {
    let arr = args.get(key)?.as_array()?;
    let out: Vec<String> = arr
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Resolve `app_id` or `rpc_url` to a concrete JSON-RPC endpoint.
async fn resolve_target(state: &AppState, args: &Value) -> Result<String, String> {
    if let Some(url) = arg_str(args, "rpc_url") {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("`rpc_url` phải bắt đầu bằng http:// hoặc https://".into());
        }
        return Ok(url);
    }
    let Some(app_id) = arg_str(args, "app_id") else {
        return Err("cần `app_id` hoặc `rpc_url`".into());
    };
    let apps = state
        .core
        .transports
        .apps
        .discover()
        .await
        .map_err(|e| format!("không hỏi được daemon về danh sách app: {e}"))?;
    match apps.get(&app_id) {
        Some(p) => Ok(p.rpc_url()),
        None => Err(format!(
            "không có app `{app_id}`. Các app có MCP: {}",
            apps.keys().cloned().collect::<Vec<_>>().join(", ")
        )),
    }
}

/// Build an [`McpSourceSpec`] from loose tool arguments.
fn spec_from_args(args: &Value) -> Result<McpSourceSpec, String> {
    let id = arg_str(args, "id").ok_or("thiếu tham số `id`")?;
    let tool = arg_str(args, "tool").ok_or("thiếu tham số `tool`")?;

    let target = match (arg_str(args, "app_id"), arg_str(args, "rpc_url")) {
        (Some(_), Some(_)) => return Err("chỉ được dùng MỘT trong `app_id` hoặc `rpc_url`".into()),
        (Some(app_id), None) => McpTarget::App { app_id },
        (None, Some(rpc_url)) => McpTarget::Url { rpc_url },
        (None, None) => return Err("cần `app_id` hoặc `rpc_url`".into()),
    };

    let kind = match arg_str(args, "kind").as_deref() {
        None => SourceKind::Custom,
        Some(k) => serde_json::from_value(json!(k)).map_err(|_| {
            format!("`kind` không hợp lệ: `{k}` (web|internal|social|docs|code|custom)")
        })?,
    };

    let map: FieldMap = match args.get("map") {
        Some(v) if !v.is_null() => {
            serde_json::from_value(v.clone()).map_err(|e| format!("`map` không hợp lệ: {e}"))?
        }
        _ => FieldMap::default(),
    };

    Ok(McpSourceSpec {
        label: arg_str(args, "label").unwrap_or_else(|| id.clone()),
        id,
        kind,
        weight: args.get("weight").and_then(Value::as_f64).unwrap_or(1.0) as f32,
        target,
        tool,
        query_arg: arg_str(args, "query_arg").unwrap_or_else(|| "query".into()),
        limit_arg: arg_str(args, "limit_arg"),
        extra_args: args
            .get("extra_args")
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({})),
        map,
    })
}

/// Chunk, index and store one document. Shared by the MCP tool and the
/// multipart upload endpoint so both enforce the same rules.
pub fn ingest_text(
    state: &AppState,
    name: &str,
    mime: &str,
    raw: &[u8],
    text: &str,
) -> Result<Value, String> {
    use sha2::{Digest, Sha256};

    let chunks = crate::corpus::chunk(text);
    if chunks.is_empty() {
        return Err("tài liệu không có nội dung nào để lập chỉ mục".into());
    }
    let sha = hex::encode(Sha256::digest(raw));

    // Re-uploading the same bytes must not silently double every hit for that
    // document — duplicated chunks would look like independent corroboration.
    if let Ok(Some((id, existing))) = state.core.db.document_by_hash(&sha) {
        return Ok(json!({
            "ok": true, "duplicate": true, "doc_id": id,
            "message": format!("nội dung này đã có sẵn dưới tên `{existing}` — không thêm lại")
        }));
    }

    match state
        .core
        .db
        .add_document(name, mime, raw.len(), &sha, &chunks)
    {
        Ok((doc_id, n)) => Ok(json!({
            "ok": true, "doc_id": doc_id, "name": name, "chunks": n, "bytes": raw.len()
        })),
        Err(e) => Err(format!("lưu tài liệu thất bại: {e}")),
    }
}

/// Save a report into the local wiki. The wiki is MCP-only (no REST write
/// surface), so this goes through `agent.run` with a one-tool allowlist — the
/// documented way to reach an MCP tool that has no HTTP endpoint (`bridge.rs`).
/// Best-effort: any failure is surfaced to the caller, never fatal to the run.
async fn save_report_to_wiki(
    state: &AppState,
    title: &str,
    markdown: &str,
) -> Result<String, String> {
    let system = "Bạn là trợ lý lưu trữ. Dùng công cụ wiki_write để lưu ĐÚNG nội dung Markdown được cung cấp, \
        KHÔNG chỉnh sửa hay tóm tắt nội dung. Chọn một đường dẫn hợp lý dưới thư mục 'zeach/'.";
    let prompt = format!(
        "Hãy lưu báo cáo nghiên cứu sau vào wiki bằng công cụ wiki_write (đặt dưới 'zeach/', đặt gắn thẻ 'zeach' và 'report').\n\n\
         Tiêu đề: {title}\n\n----- NỘI DUNG MARKDOWN (giữ nguyên) -----\n{markdown}"
    );
    state
        .core
        .transports
        .bridge
        .agent_run(
            system,
            &prompt,
            &["mcp__senclaw-wiki__wiki_write".to_string()],
            Duration::from_secs(120),
        )
        .await
        .map_err(|e| e.to_string())
}

// ---- tool catalog ----------------------------------------------------------

pub fn tools_list() -> Vec<Value> {
    vec![
        json!({
            "name": "zeach_search",
            "description": "Tìm kiếm liên nguồn (web, knowledge graph, wiki, …) và trả về danh sách bằng chứng đã khử trùng lặp và xếp hạng bằng Reciprocal Rank Fusion. KHÔNG dùng LLM nên nhanh và rẻ — đây là công cụ mặc định cho các agent/app khác cần tìm thông tin. Mỗi kết quả kèm provenance (nguồn nào tìm ra nó, hạng bao nhiêu) và số loại nguồn độc lập đã xác nhận. Kết quả trả về LUÔN kèm phần `sources` liệt kê nguồn nào chạy được, nguồn nào lỗi/hết giờ/bị bỏ qua — hãy đọc nó trước khi kết luận 'không có thông tin'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query":   { "type": "string", "description": "Câu truy vấn." },
                    "sources": { "type": "array", "items": { "type": "string" },
                                 "description": "Giới hạn ở các nguồn này (id lấy từ zeach_sources). Bỏ trống = mọi nguồn đang bật." },
                    "limit":   { "type": "integer", "description": "Số bằng chứng tối đa trả về (mặc định 20)." },
                    "lang":    { "type": "string", "description": "Mã ngôn ngữ ưu tiên, ví dụ 'vi' hoặc 'en'." },
                    "depth":   { "type": "integer", "description": "1 = chỉ đoạn trích (mặc định). 2 = tải thêm toàn văn cho các kết quả web đầu bảng (chậm hơn nhiều)." }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "zeach_ask",
            "description": "Tìm kiếm RỒI rút ra các khẳng định nguyên tử, mỗi khẳng định gắn với bằng chứng cụ thể và được xếp hạng tin cậy bằng cách ĐẾM số nguồn độc lập (không phải do mô hình tự chấm). Khi các nguồn mâu thuẫn, kết quả trả về `disputed` kèm cả hai phía chứ KHÔNG tự chọn một bên. Chậm và tốn hơn zeach_search vì có gọi LLM — dùng khi cần câu trả lời có kiểm chứng, còn chỉ cần danh sách kết quả thì dùng zeach_search. LƯU Ý: điểm tin cậy đo độ chứng thực của nguồn, không đo tính đúng sai.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query":   { "type": "string" },
                    "sources": { "type": "array", "items": { "type": "string" } },
                    "limit":   { "type": "integer", "description": "Số bằng chứng đưa vào phân tích (mặc định 20)." },
                    "lang":    { "type": "string" },
                    "depth":   { "type": "integer", "description": "2 = tải toàn văn các kết quả web đầu bảng trước khi rút khẳng định (chính xác hơn, chậm hơn)." }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "zeach_research",
            "description": "Nghiên cứu chuyên sâu ĐA NGUỒN rồi trả về một BÁO CÁO có trích dẫn — đây là năng lực chính của Zeach. Khác với zeach_search (chỉ trả danh sách kết quả), công cụ này: (1) tách câu hỏi thành nhiều truy vấn con để bao phủ nhiều khía cạnh; (2) gom bằng chứng từ MỌI nguồn đang bật (web, knowledge graph, wiki, tài liệu, mạng xã hội và bất kỳ MCP nào khác đã đăng ký); (3) rút các khẳng định nguyên tử và KIỂM CHỨNG CHÉO bằng cách ĐẾM số nguồn độc lập — chỉ điều được ≥2 nguồn độc lập xác nhận mới đạt mức 'nhiều nguồn'; khi mâu thuẫn thì nêu CẢ HAI phía chứ không tự chọn; (4) tổng hợp thành báo cáo Markdown có trích dẫn [n] tra ngược được về từng nguồn. depth='deep' chạy thêm một vòng truy chứng cho các khẳng định yếu/nhạy cảm. CÓ HAI CHỐT KIỂM ĐỊNH: tư liệu lạc chủ đề bị loại trước khi rút khẳng định (nằm ở `off_topic`), và báo cáo được chấm lại so với câu hỏi trước khi trả (`review`). Đọc trường `status`: 'ok' = trả lời đúng câu hỏi; 'off_topic' = báo cáo KHÔNG trả lời được câu hỏi (đừng dùng làm câu trả lời); 'insufficient' = không có tư liệu nào đúng chủ đề, hệ thống từ chối tổng hợp. Chậm và tốn LLM — dùng khi cần câu trả lời tổng hợp đáng tin; tra nhanh thì dùng zeach_search.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query":         { "type": "string", "description": "Câu hỏi nghiên cứu." },
                    "depth":         { "type": "string", "description": "'quick' = một lượt, không đào sâu trang. 'standard' (mặc định) = vài truy vấn con + đào sâu các trang web đầu bảng. 'deep' = fan-out rộng + một vòng truy chứng bổ sung cho các khẳng định yếu/nhạy cảm." },
                    "sources":       { "type": "array", "items": { "type": "string" }, "description": "Giới hạn ở các nguồn này (id lấy từ zeach_sources). Bỏ trống = mọi nguồn đang bật." },
                    "lang":          { "type": "string", "description": "Ngôn ngữ ưu tiên, ví dụ 'vi' hoặc 'en'." },
                    "max_evidence":  { "type": "integer", "description": "Số bằng chứng tối đa đưa vào tổng hợp (mặc định 24)." },
                    "save_wiki":     { "type": "boolean", "description": "Nếu true, ghi báo cáo vào wiki để tái dùng về sau." },
                    "save_knowledge":{ "type": "boolean", "description": "Nếu true, lưu báo cáo vào knowledge graph." }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "zeach_report",
            "description": "Đọc lại BÁO CÁO đã tổng hợp của một lần nghiên cứu (kèm khẳng định đã kiểm chứng, các mâu thuẫn, và nhật ký nguồn). Bỏ trống `run_id` để liệt kê các báo cáo gần đây.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "run_id": { "type": "string", "description": "run_id trả về từ zeach_research. Bỏ trống để liệt kê." },
                    "limit":  { "type": "integer", "description": "Khi liệt kê: số báo cáo tối đa (mặc định 30)." }
                }
            }
        }),
        json!({
            "name": "zeach_claims",
            "description": "Đọc lại các khẳng định và mâu thuẫn đã rút ra của một lần chạy trước.",
            "inputSchema": {
                "type": "object",
                "properties": { "run_id": { "type": "string" } },
                "required": ["run_id"]
            }
        }),
        json!({
            "name": "zeach_sources",
            "description": "Liệt kê mọi nguồn tìm kiếm cùng tình trạng thực tế (ready / degraded / unavailable kèm lý do), trọng số và giới hạn. Dùng công cụ này khi một lần tìm kiếm trả về ít kết quả bất thường — nó phân biệt 'không có gì để tìm' với 'nguồn đó đang hỏng'.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "zeach_source_config",
            "description": "Bật/tắt một nguồn hoặc chỉnh trọng số, số kết quả tối đa, thời gian chờ. Thay đổi được lưu lại qua các lần khởi động.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source_id":   { "type": "string" },
                    "enabled":     { "type": "boolean" },
                    "weight":      { "type": "number", "description": "Trọng số tin cậy dùng khi hợp nhất hạng (0–10, mặc định 1)." },
                    "max_results": { "type": "integer" },
                    "timeout_ms":  { "type": "integer" }
                },
                "required": ["source_id"]
            }
        }),
        json!({
            "name": "zeach_runs",
            "description": "Liệt kê các lần tìm kiếm gần đây (id, câu hỏi, số bằng chứng, thời gian chạy).",
            "inputSchema": {
                "type": "object",
                "properties": { "limit": { "type": "integer", "description": "Mặc định 30." } }
            }
        }),
        json!({
            "name": "zeach_run",
            "description": "Đọc lại toàn bộ một lần tìm kiếm đã lưu: bằng chứng, provenance, và kết quả của từng nguồn. Dùng để xem lại mà không phải tìm kiếm lại từ đầu.",
            "inputSchema": {
                "type": "object",
                "properties": { "run_id": { "type": "string" } },
                "required": ["run_id"]
            }
        }),
        json!({
            "name": "zeach_mcp_tools",
            "description": "Liệt kê các công cụ của một MCP bất kỳ (một Space App đã cài, hoặc một URL JSON-RPC). Dùng TRƯỚC zeach_source_add để biết tên công cụ và tên tham số thật, thay vì đoán.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id":  { "type": "string", "description": "Id của Space App đã cài, ví dụ 'youtube', 'social', 'crm'." },
                    "rpc_url": { "type": "string", "description": "Hoặc URL JSON-RPC trực tiếp (http/https)." }
                }
            }
        }),
        json!({
            "name": "zeach_source_add",
            "description": "Biến MỘT CÔNG CỤ MCP BẤT KỲ thành một nguồn tìm kiếm — không cần viết code. Nguồn mới lập tức tham gia mọi lần zeach_search. Trình ánh xạ tự dò mảng kết quả và các trường title/url/snippet; chỉ khai báo `map` khi tự dò sai. Dùng `url_template` khi công cụ trả về id thay vì URL (ví dụ YouTube trả videoId). Dùng `extra_args` cho tham số bắt buộc mà truy vấn không cung cấp được (ví dụ social_search cần platform + handle).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id":         { "type": "string", "description": "Tên nguồn, ví dụ 'crm' hoặc 'social:threads'. Không được trùng nguồn có sẵn." },
                    "label":      { "type": "string", "description": "Tên hiển thị." },
                    "app_id":     { "type": "string", "description": "Space App đích (dùng cái này HOẶC rpc_url)." },
                    "rpc_url":    { "type": "string", "description": "URL JSON-RPC của MCP ngoài." },
                    "tool":       { "type": "string", "description": "Tên công cụ cần gọi, ví dụ 'youtube_search'." },
                    "query_arg":  { "type": "string", "description": "Tên tham số nhận câu truy vấn (mặc định 'query')." },
                    "limit_arg":  { "type": "string", "description": "Tên tham số giới hạn số kết quả, nếu công cụ có. Bỏ trống nếu không có — gửi tham số lạ sẽ lỗi." },
                    "extra_args": { "type": "object", "description": "Tham số cố định gửi kèm mọi lần gọi." },
                    "kind":       { "type": "string", "description": "web | internal | social | docs | code | custom. Ảnh hưởng tới việc đếm nguồn độc lập." },
                    "weight":     { "type": "number", "description": "Trọng số tin cậy (mặc định 1)." },
                    "map":        { "type": "object", "description": "Ánh xạ trường: list_path, title, url, url_template, snippet, published_at." }
                },
                "required": ["id", "tool"]
            }
        }),
        json!({
            "name": "zeach_source_remove",
            "description": "Gỡ một nguồn MCP do người dùng đăng ký. Không gỡ được nguồn có sẵn (web/knowledge/wiki) — hãy dùng zeach_source_config để tắt.",
            "inputSchema": {
                "type": "object",
                "properties": { "source_id": { "type": "string" } },
                "required": ["source_id"]
            }
        }),
        json!({
            "name": "zeach_source_templates",
            "description": "Các nguồn CÓ THỂ thêm nhưng cần bạn cung cấp thêm thông tin, kèm lý do vì sao không thể tự thêm. Ví dụ: mạng xã hội cần platform + handle vì nó tìm bằng phiên đăng nhập của một tài khoản cụ thể.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "zeach_sync",
            "description": "Quét lại các Space App đã cài và đăng ký nguồn cho những app hỗ trợ. Gọi sau khi cài/bật thêm app. Trả về app nào được thêm, app nào không và vì sao.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "zeach_corpus_add",
            "description": "Thêm một tài liệu dạng VĂN BẢN vào kho tài liệu của app (tự cắt đoạn và lập chỉ mục FTS tiếng Việt). Sau đó nội dung này tham gia mọi lần zeach_search như một nguồn. Muốn tải tệp PDF/DOCX thì dùng giao diện web hoặc POST /api/corpus.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Tên tài liệu để trích dẫn về sau." },
                    "text": { "type": "string", "description": "Toàn bộ nội dung văn bản." }
                },
                "required": ["name", "text"]
            }
        }),
        json!({
            "name": "zeach_corpus_list",
            "description": "Liệt kê tài liệu đã tải lên (tên, dung lượng, số đoạn đã lập chỉ mục).",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "zeach_corpus_remove",
            "description": "Xoá một tài liệu cùng toàn bộ đoạn và chỉ mục của nó.",
            "inputSchema": {
                "type": "object",
                "properties": { "doc_id": { "type": "string" } },
                "required": ["doc_id"]
            }
        }),
        json!({
            "name": "zeach_status",
            "description": "Tình trạng app: số run/bằng chứng đã lưu và tình trạng từng nguồn.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
    ]
}

// ---- dispatch --------------------------------------------------------------

pub async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    match name {
        "zeach_search" => {
            let Some(query) = arg_str(args, "query") else {
                return error_result("thiếu tham số `query`".into());
            };
            let req = SearchRequest {
                query,
                sources: arg_str_list(args, "sources"),
                limit: arg_usize(args, "limit").unwrap_or(20).clamp(1, 100),
                lang: arg_str(args, "lang"),
                depth: arg_usize(args, "depth").unwrap_or(1).clamp(1, 2) as u8,
            };

            let registry = state.core.registry.read().await.clone();
            let out = pipeline::run(&registry, &state.core.transports, &req).await;

            let params = json!({
                "sources": req.sources, "limit": req.limit,
                "lang": req.lang, "depth": req.depth
            });
            let run_id = state.core.db.save_run(&out, &params, "cited").ok();

            let mut body = serde_json::to_value(&out).unwrap_or(json!({}));
            body["run_id"] = json!(run_id);
            json_result(body)
        }

        "zeach_ask" => {
            let Some(query) = arg_str(args, "query") else {
                return error_result("thiếu tham số `query`".into());
            };
            let req = SearchRequest {
                query: query.clone(),
                sources: arg_str_list(args, "sources"),
                limit: arg_usize(args, "limit").unwrap_or(20).clamp(1, 60),
                lang: arg_str(args, "lang"),
                depth: arg_usize(args, "depth").unwrap_or(1).clamp(1, 2) as u8,
            };

            let registry = state.core.registry.read().await.clone();
            let out = pipeline::run(&registry, &state.core.transports, &req).await;
            let params = json!({
                "sources": req.sources, "limit": req.limit,
                "lang": req.lang, "depth": req.depth
            });
            let run_id = state.core.db.save_run(&out, &params, "corroborate").ok();

            let mut body = serde_json::to_value(&out).unwrap_or(json!({}));
            body["run_id"] = json!(run_id);
            body["confidence_note"] = json!(crate::claims::CONFIDENCE_IS_PROVENANCE);

            if out.evidence.is_empty() {
                // No evidence means no claims — and saying "no claims" without
                // saying "because no source returned anything" is the exact
                // ambiguity this app exists to remove.
                body["claims"] = json!([]);
                body["claims_note"] = json!(
                    "Không có bằng chứng nào nên không rút được khẳng định. \
                     Xem `sources` để biết nguồn nào lỗi hay bị bỏ qua."
                );
                return json_result(body);
            }

            match crate::extract::extract_claims(
                &state.core.transports.bridge,
                &query,
                &out.evidence,
                Duration::from_secs(180),
            )
            .await
            {
                Err(e) => {
                    // The evidence is still worth returning; what failed was the
                    // analysis on top of it. Say which.
                    body["claims"] = json!([]);
                    body["claims_error"] = json!(format!(
                        "không rút được khẳng định ({e}) — phần bằng chứng bên dưới vẫn dùng được"
                    ));
                    json_result(body)
                }
                Ok((raw_claims, raw_contradictions)) => {
                    let mut claims = crate::claims::assess_all(&raw_claims, &out.evidence);
                    let contradictions =
                        crate::claims::validate_contradictions(&raw_contradictions, &claims);
                    crate::claims::mark_disputed(&mut claims, &contradictions);

                    if let Some(id) = &run_id {
                        if let Err(e) = state.core.db.save_claims(id, &claims, &contradictions) {
                            eprintln!("[zeach] không lưu được claims: {e}");
                        }
                    }
                    body["claims"] = serde_json::to_value(&claims).unwrap_or(json!([]));
                    body["contradictions"] =
                        serde_json::to_value(&contradictions).unwrap_or(json!([]));
                    json_result(body)
                }
            }
        }

        "zeach_research" => {
            let Some(query) = arg_str(args, "query") else {
                return error_result("thiếu tham số `query`".into());
            };
            let req = crate::research::ResearchRequest {
                query,
                sources: arg_str_list(args, "sources"),
                lang: arg_str(args, "lang"),
                depth: crate::research::Depth::parse(&arg_str(args, "depth").unwrap_or_default()),
                max_evidence: arg_usize(args, "max_evidence").unwrap_or(24).clamp(4, 60),
            };

            let registry = state.core.registry.read().await.clone();
            let mut out = crate::research::run(&registry, &state.core.transports, &req).await;

            // Persist run + verified claims + report so zeach_report reads it back.
            let params = json!({
                "depth": out.depth.as_str(), "sources": req.sources,
                "lang": req.lang, "max_evidence": req.max_evidence, "rounds": out.rounds
            });
            let so = out.as_search_outcome();
            let run_id = state.core.db.save_run(&so, &params, "research").ok();
            let mut saved: Vec<Value> = Vec::new();
            if let Some(id) = &run_id {
                if let Err(e) = state
                    .core
                    .db
                    .save_claims(id, &out.claims, &out.contradictions)
                {
                    eprintln!("[zeach] không lưu được claims: {e}");
                }
                let body_json = json!({ "title": out.report_title, "llm": out.report_llm });
                match state.core.db.save_report(
                    id,
                    &out.report_title,
                    &out.report_markdown,
                    &body_json,
                ) {
                    Ok((rep_id, ver)) => {
                        saved.push(json!({ "target": "db", "report_id": rep_id, "version": ver }))
                    }
                    Err(e) => out.warnings.push(format!("không lưu được báo cáo: {e}")),
                }
            }

            // Optional export — best-effort, recorded, never fatal to the run.
            if args
                .get("save_knowledge")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                let text = format!("{}\n\n{}", out.report_title, out.report_markdown);
                match state
                    .core
                    .transports
                    .bridge
                    .knowledge_save(&text, None, Some("zeach"), Duration::from_secs(60))
                    .await
                {
                    Ok(_) => saved.push(json!({ "target": "knowledge", "status": "ok" })),
                    Err(e) => out
                        .warnings
                        .push(format!("lưu vào knowledge thất bại: {e}")),
                }
            }
            if args
                .get("save_wiki")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                match save_report_to_wiki(state, &out.report_title, &out.report_markdown).await {
                    Ok(detail) => {
                        saved.push(json!({ "target": "wiki", "status": "ok", "detail": detail }))
                    }
                    Err(e) => out.warnings.push(format!("lưu vào wiki thất bại: {e}")),
                }
            }

            let mut body = serde_json::to_value(&out).unwrap_or(json!({}));
            body["run_id"] = json!(run_id);
            body["saved"] = json!(saved);
            json_result(body)
        }

        "zeach_report" => match arg_str(args, "run_id") {
            Some(id) => match state.core.db.get_report(&id) {
                Ok(Some(r)) => json_result(r),
                Ok(None) => error_result(format!(
                    "chưa có báo cáo cho run `{id}` — chạy zeach_research trước"
                )),
                Err(e) => error_result(e.to_string()),
            },
            None => {
                let limit = arg_usize(args, "limit").unwrap_or(30).clamp(1, 200);
                match state.core.db.list_reports(limit) {
                    Ok(reports) => json_result(json!({ "reports": reports })),
                    Err(e) => error_result(e.to_string()),
                }
            }
        },

        "zeach_claims" => {
            let Some(id) = arg_str(args, "run_id") else {
                return error_result("thiếu tham số `run_id`".into());
            };
            match (
                state.core.db.run_claims(&id),
                state.core.db.run_contradictions(&id),
            ) {
                (Ok(claims), Ok(cts)) => json_result(json!({
                    "run_id": id, "claims": claims, "contradictions": cts,
                    "confidence_note": crate::claims::CONFIDENCE_IS_PROVENANCE
                })),
                (Err(e), _) | (_, Err(e)) => error_result(e.to_string()),
            }
        }

        "zeach_sources" => {
            let list = state.core.registry.read().await.describe().await;
            json_result(json!({ "sources": list }))
        }

        "zeach_source_config" => {
            let Some(id) = arg_str(args, "source_id") else {
                return error_result("thiếu tham số `source_id`".into());
            };
            let enabled = args.get("enabled").and_then(Value::as_bool);
            let weight = args.get("weight").and_then(Value::as_f64).map(|v| v as f32);
            let max_results = arg_usize(args, "max_results");
            let timeout_ms = args.get("timeout_ms").and_then(Value::as_u64);

            let ok = state.core.registry.write().await.set_config(
                &id,
                enabled,
                weight,
                max_results,
                timeout_ms,
            );
            if !ok {
                return error_result(format!("không có nguồn `{id}`"));
            }
            if let Err(e) =
                state
                    .core
                    .db
                    .save_source_config(&id, enabled, weight, max_results, timeout_ms)
            {
                return error_result(format!("lưu cấu hình thất bại: {e}"));
            }
            let list = state.core.registry.read().await.describe().await;
            json_result(json!({ "ok": true, "source": id, "sources": list }))
        }

        "zeach_runs" => {
            let limit = arg_usize(args, "limit").unwrap_or(30).clamp(1, 200);
            match state.core.db.list_runs(limit) {
                Ok(runs) => json_result(json!({ "runs": runs })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "zeach_run" => {
            let Some(id) = arg_str(args, "run_id") else {
                return error_result("thiếu tham số `run_id`".into());
            };
            match state.core.db.get_run(&id) {
                Ok(Some(run)) => json_result(run),
                Ok(None) => error_result(format!("không có run `{id}`")),
                Err(e) => error_result(e.to_string()),
            }
        }

        "zeach_mcp_tools" => {
            let target = match resolve_target(state, args).await {
                Ok(t) => t,
                Err(e) => return error_result(e),
            };
            match state
                .core
                .transports
                .apps
                .list_tools(&target, Duration::from_secs(15))
                .await
            {
                Ok(tools) => json_result(json!({ "rpc_url": target, "tools": tools })),
                Err(e) => error_result(format!("không gọi được MCP tại {target}: {e}")),
            }
        }

        "zeach_source_add" => {
            let spec = match spec_from_args(args) {
                Ok(s) => s,
                Err(e) => return error_result(e),
            };
            if let Err(e) = spec.validate() {
                return error_result(e);
            }
            if let Err(e) = state.core.db.save_mcp_source(&spec, true) {
                return error_result(format!("lưu nguồn thất bại: {e}"));
            }
            let id = spec.id.clone();
            state.core.registry.write().await.register(
                Arc::new(McpSource::new(spec, state.core.transports.apps.clone())),
                SourceOrigin::User,
            );

            // Probe immediately: a source that was accepted but cannot run is
            // far more useful to know about now than at the next search.
            let health = state
                .core
                .registry
                .read()
                .await
                .describe()
                .await
                .into_iter()
                .find(|s| s.id == id)
                .map(|s| s.health);
            json_result(json!({ "ok": true, "source": id, "health": health }))
        }

        "zeach_source_remove" => {
            let Some(id) = arg_str(args, "source_id") else {
                return error_result("thiếu tham số `source_id`".into());
            };
            if crate::sources::mcp_source::RESERVED_IDS.contains(&id.as_str()) {
                return error_result(format!(
                    "`{id}` là nguồn có sẵn, không gỡ được — dùng zeach_source_config để tắt"
                ));
            }
            match state.core.db.delete_mcp_source(&id) {
                Ok(true) => {
                    state.core.registry.write().await.remove(&id);
                    json_result(json!({ "ok": true, "removed": id }))
                }
                Ok(false) => error_result(format!("không có nguồn `{id}` do người dùng đăng ký")),
                Err(e) => error_result(e.to_string()),
            }
        }

        "zeach_source_templates" => {
            let mut templates: Vec<Value> = crate::sources::presets::templates()
                .into_iter()
                .map(|t| {
                    json!({
                        "id": t.id, "label": t.label, "app_id": t.app_id, "tool": t.tool,
                        "why": t.why,
                        "required_args": t.required_args.iter()
                            .map(|(k, hint)| json!({ "name": k, "hint": hint }))
                            .collect::<Vec<_>>(),
                    })
                })
                .collect();
            // Dynamic suggestions from the last rescan: search tools found by
            // rule that need arguments only the user can supply.
            for s in state.core.discovered_suggestions.read().await.iter() {
                if templates.iter().any(|t| t["app_id"] == json!(s.app_id)) {
                    continue; // curated template already covers this app
                }
                templates.push(json!({
                    "id": s.app_id, "label": s.app_name, "app_id": s.app_id, "tool": s.tool,
                    "why": "công cụ tìm kiếm của app này cần tham số bắt buộc ngoài truy vấn — \
                            thêm bằng form MCP bên dưới và điền các giá trị này vào Tham số cố định",
                    "required_args": s.required_args.iter()
                        .map(|(k, hint)| json!({ "name": k, "hint": hint }))
                        .collect::<Vec<_>>(),
                }));
            }
            json_result(json!({ "templates": templates }))
        }

        "zeach_sync" => {
            state.core.transports.apps.invalidate().await;
            let report = state.core.sync_mcp_sources().await;
            json_result(json!({ "ok": true, "sources": report }))
        }

        "zeach_corpus_add" => {
            let (Some(name), Some(text)) = (arg_str(args, "name"), arg_str(args, "text")) else {
                return error_result("cần cả `name` và `text`".into());
            };
            match ingest_text(state, &name, "text/plain", text.as_bytes(), &text) {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }

        "zeach_corpus_list" => match state.core.db.list_documents() {
            Ok(docs) => json_result(json!({ "documents": docs })),
            Err(e) => error_result(e.to_string()),
        },

        "zeach_corpus_remove" => {
            let Some(id) = arg_str(args, "doc_id") else {
                return error_result("thiếu tham số `doc_id`".into());
            };
            match state.core.db.delete_document(&id) {
                Ok(true) => json_result(json!({ "ok": true, "removed": id })),
                Ok(false) => error_result(format!("không có tài liệu `{id}`")),
                Err(e) => error_result(e.to_string()),
            }
        }

        "zeach_status" => {
            let stats = state.core.db.stats().unwrap_or(json!({}));
            let sources = state.core.registry.read().await.describe().await;
            json_result(json!({ "ok": true, "stats": stats, "sources": sources }))
        }

        other => error_result(format!("công cụ không tồn tại: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    /// Boot against a throwaway data dir so tests never touch `~/.senclaw`.
    /// The tempdir is leaked deliberately — it must outlive every test in the
    /// process, and the OS reclaims it.
    fn test_state() -> AppState {
        static DIR: OnceLock<()> = OnceLock::new();
        DIR.get_or_init(|| {
            let dir = tempfile::tempdir().expect("tempdir").keep();
            std::env::set_var("ZEACH_DATA_DIR", dir);
        });
        AppState {
            core: crate::state::Core::boot().expect("boot"),
            mcp_tx: tokio::sync::broadcast::channel(8).0,
        }
    }

    /// Drift guard: every advertised tool must have a dispatch arm, or agents
    /// get "tool not found" for something `tools/list` promised.
    #[tokio::test]
    async fn every_listed_tool_has_a_dispatch_arm() {
        let state = test_state();
        for tool in tools_list() {
            let name = tool["name"].as_str().unwrap();
            let result = call_tool(&state, name, &json!({})).await;
            let text = result["content"][0]["text"].as_str().unwrap_or_default();
            assert!(
                !text.contains("công cụ không tồn tại"),
                "{name} is listed but not dispatched"
            );
        }
    }

    #[tokio::test]
    async fn a_missing_required_argument_is_a_tool_error_not_a_panic() {
        let state = test_state();
        let out = call_tool(&state, "zeach_search", &json!({})).await;
        assert_eq!(out["isError"], true);
        assert!(out["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("query"));
    }

    #[test]
    fn arg_helpers_reject_blank_and_empty_values() {
        let args = json!({ "a": "  ", "b": "x", "list": [], "list2": ["a", " "] });
        assert_eq!(arg_str(&args, "a"), None);
        assert_eq!(arg_str(&args, "b"), Some("x".into()));
        assert_eq!(arg_str_list(&args, "list"), None);
        assert_eq!(arg_str_list(&args, "list2"), Some(vec!["a".to_string()]));
    }
}
