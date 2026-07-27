//! MCP server — hand-rolled JSON-RPC over HTTP + SSE, matching the other Space
//! Apps (the `rmcp` crate is not used here).

use crate::db::CloneConfig;
use crate::process::{self, Mode};
use crate::scenes::{self, ReplaceRequest, Voice};
use crate::state::AppState;
use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;

/// Scene JSON is verbose; returning a whole project at once would swamp an
/// agent's context. Reads are windowed by default.
const DEFAULT_SCENE_LIMIT: i64 = 5;

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
            "serverInfo": { "name": "video-cloner-mcp", "version": "1.0.0" }
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

// ---- argument helpers ----

fn s(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn opt_s(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn int(args: &Value, key: &str) -> i64 {
    args.get(key)
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .unwrap_or(0)
}

fn opt_int(args: &Value, key: &str) -> Option<i64> {
    args.get(key)
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
}

fn opt_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| {
        v.as_bool()
            .or_else(|| v.as_str().map(|s| matches!(s, "1" | "true" | "yes")))
    })
}

/// Shared by the export tools: fetch a project with its scenes, or say why not.
fn load_export(
    db: &crate::db::Db,
    id: i64,
) -> Result<(crate::db::Project, Vec<crate::db::Scene>), String> {
    let project = match db.project(id) {
        Ok(Some(p)) => p,
        Ok(None) => return Err(format!("không tìm thấy dự án {id}")),
        Err(e) => return Err(e.to_string()),
    };
    let stored = db.scenes(id).map_err(|e| e.to_string())?;
    if stored.is_empty() {
        return Err("dự án chưa có đoạn nào để xuất — chạy vc_analyze trước".into());
    }
    Ok((project, stored))
}

async fn write_export_files(
    project: &crate::db::Project,
    stored: &[crate::db::Scene],
    now: &str,
    slug: &str,
) -> Result<Value, String> {
    let dir = crate::config::export_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("không tạo được thư mục {}: {e}", dir.display()))?;

    let bundle_path = dir.join(format!("{slug}.bundle.json"));
    let md_path = dir.join(format!("{slug}.md"));

    let bundle = crate::export::bundle(project, stored, now);
    tokio::fs::write(
        &bundle_path,
        serde_json::to_string_pretty(&bundle).unwrap_or_default(),
    )
    .await
    .map_err(|e| format!("ghi {} thất bại: {e}", bundle_path.display()))?;

    tokio::fs::write(&md_path, crate::export::markdown(project, stored, now))
        .await
        .map_err(|e| format!("ghi {} thất bại: {e}", md_path.display()))?;

    Ok(json!({
        "dir": dir.to_string_lossy(),
        "bundle": bundle_path.to_string_lossy(),
        "markdown": md_path.to_string_lossy(),
    }))
}

async fn write_wiki(path: &str, markdown: &str, project_name: &str) -> Result<(), String> {
    let url = format!(
        "{}/api/wiki/file",
        crate::config::senclaw_base_url().trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .put(&url)
        .json(&json!({
            "path": path,
            "content": markdown,
            "source": "video-cloner",
            "tags": ["video", "veo3", "kịch bản", "video-cloner"],
            "commitMsg": format!("video-cloner: kịch bản \"{project_name}\""),
        }))
        .send()
        .await
        .map_err(|e| format!("không gọi được wiki tại {url}: {e} — daemon SenClaw có chạy không?"))?;

    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        Err(format!(
            "wiki trả {status}: {}",
            scenes::truncate_chars(text.trim(), 300)
        ))
    }
}

fn tools_list() -> Value {
    json!([
      {
        "name": "vc_status",
        "description": "Trạng thái Video Cloner: có bao nhiêu dự án, đã có Gemini API key chưa, dự án nào đang chạy. GỌI TOOL NÀY TRƯỚC TIÊN trong mọi phiên làm việc — nếu chưa có API key thì mọi lệnh phân tích đều sẽ hỏng, phải báo Sếp vào Cài đặt nhập key trước.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      {
        "name": "vc_presets",
        "description": "Danh sách phong cách, model, preset nhân vật và preset bối cảnh có sẵn. Dùng khi Sếp nói mơ hồ về phong cách để đưa ra lựa chọn cụ thể, thay vì tự bịa tên phong cách.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      {
        "name": "vc_project_list",
        "description": "Liệt kê các dự án sao chép video, mới nhất trước, kèm số scene đã tạo và dự án nào đang chạy.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "limit": { "type": "integer", "description": "Số dự án tối đa (mặc định 20)" }
          }
        }
      },
      {
        "name": "vc_project_get",
        "description": "Chi tiết một dự án: cấu hình sao chép hiện tại, số scene, danh sách nhân vật đã phát hiện (kèm ID và ai có lời thoại), và tiến trình gần nhất. Gọi trước khi sửa cấu hình hay thay tên nhân vật để biết ID thật.",
        "inputSchema": {
          "type": "object",
          "properties": { "project_id": { "type": "integer" } },
          "required": ["project_id"]
        }
      },
      {
        "name": "vc_project_config",
        "description": "Cập nhật cấu hình sao chép của một dự án (phong cách, model, mô tả nhân vật thay thế, lời thoại thay thế, bối cảnh, chế độ AI tự do sáng tạo, độ tương đồng hình ảnh). KHÔNG tạo video mới — chỉ đổi cấu hình cho các lần phân tích sau. Muốn áp dụng ngay thì gọi vc_analyze sau đó.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "project_id": { "type": "integer" },
            "name": { "type": "string" },
            "style": { "type": "string", "description": "Một trong các phong cách của vc_presets, hoặc phong cách tự do do Sếp mô tả" },
            "model": { "type": "string", "description": "gemini-3-flash-preview (nhanh) hoặc gemini-3-pro-preview (kỹ)" },
            "char_description": { "type": "string", "description": "Mô tả nhân vật thay thế cho nhân vật chính. Để rỗng nghĩa là giữ nhân vật gốc." },
            "custom_dialogue": { "type": "string", "description": "Lời thoại/câu viral muốn nhân vật nói. Để rỗng nghĩa là KHÔNG tạo lời thoại." },
            "bg_description": { "type": "string", "description": "Bối cảnh mới. Để rỗng nghĩa là AI tự nghĩ một bối cảnh hợp phong cách." },
            "auto_magic": { "type": "boolean", "description": "Bật = AI tự đổi cả nhân vật lẫn bối cảnh, bỏ qua char_description/bg_description và ép độ tương đồng về 0." },
            "visual_similarity": { "type": "integer", "description": "0-100. 100 = bám sát video gốc, 0 = sáng tạo tối đa. Đây là núm quyết định temperature." }
          },
          "required": ["project_id"]
        }
      },
      {
        "name": "vc_analyze",
        "description": "Chạy phân tích để sinh prompt JSON cho một đoạn video. CHẠY NỀN — trả về job_id NGAY, ĐỪNG chờ đồng bộ; một đoạn 8 giây thường mất vài phút. mode: \"start\" phân tích lại từ đầu và XOÁ mọi scene cũ; \"continue\" phân tích đoạn 8 giây tiếp theo và nối vào cuối; \"regenerate\" làm lại đoạn cuối cùng. Sau khi gọi, poll vc_job.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "project_id": { "type": "integer" },
            "mode": { "type": "string", "description": "start | continue | regenerate (mặc định start)" },
            "style": { "type": "string" },
            "model": { "type": "string" },
            "char_description": { "type": "string" },
            "custom_dialogue": { "type": "string" },
            "bg_description": { "type": "string" },
            "auto_magic": { "type": "boolean" },
            "visual_similarity": { "type": "integer" }
          },
          "required": ["project_id"]
        }
      },
      {
        "name": "vc_job",
        "description": "Trạng thái một tiến trình phân tích: queued | processing | completed | failed. Poll THƯA (30-60 giây một lần), không phải mỗi vài giây. Chỉ báo Sếp là xong khi thấy completed.",
        "inputSchema": {
          "type": "object",
          "properties": { "job_id": { "type": "integer" } },
          "required": ["job_id"]
        }
      },
      {
        "name": "vc_scenes",
        "description": "Đọc các prompt JSON đã tạo, theo cửa sổ. Mặc định chỉ trả 5 scene — mỗi scene rất dài, đọc cả dự án sẽ ngập context. Dùng offset/limit để đọc tiếp.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "project_id": { "type": "integer" },
            "offset": { "type": "integer" },
            "limit": { "type": "integer", "description": "Mặc định 5, tối đa 20" }
          },
          "required": ["project_id"]
        }
      },
      {
        "name": "vc_replace",
        "description": "Sửa hàng loạt trên MỌI scene cùng lúc: đổi tên nhân vật và/hoặc ép giọng nam/nữ. Sau khi đổi, tool tự đồng bộ lại voice_id, gender, audio_markers và voice_marker trong lời thoại của mọi scene — đây là lý do phải dùng tool này thay vì tự sửa từng scene, vì voice_id lệch giữa các đoạn sẽ khiến Veo 3 hiểu thành hai nhân vật khác nhau. Lấy character_id đúng từ vc_project_get.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "project_id": { "type": "integer" },
            "find": { "type": "string", "description": "Tên hoặc ID nhân vật cần đổi (cũng thay trong mọi câu văn có nhắc tên đó)" },
            "replace": { "type": "string", "description": "Tên mới" },
            "only_with_dialogue": { "type": "boolean", "description": "Chỉ đổi nếu nhân vật đó thật sự có lời thoại" },
            "voice_overrides": { "type": "object", "description": "Map character_id -> \"male\" | \"female\", ví dụ {\"CHAR_1\":\"male\"}" }
          },
          "required": ["project_id"]
        }
      },
      {
        "name": "vc_export",
        "description": "Xuất toàn bộ prompt dưới dạng văn bản dán thẳng vào Veo 3 (mỗi scene một dòng JSON, cách nhau một dòng trống). Mặc định chỉ trả thống kê + đoạn đầu; đặt full=true khi Sếp thật sự cần cả khối để dán.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "project_id": { "type": "integer" },
            "full": { "type": "boolean" }
          },
          "required": ["project_id"]
        }
      },
      {
        "name": "vc_history",
        "description": "Lịch sử một dự án: các lượt phân tích đã chạy (kèm model, temperature, số scene sinh ra, lỗi nếu có) và các điểm khôi phục đã tự lưu. Gọi trước khi định khôi phục, và khi cần giải thích cho Sếp vì sao kết quả hiện tại lại như vậy.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "project_id": { "type": "integer" },
            "limit": { "type": "integer", "description": "Số lượt chạy tối đa (mặc định 20)" }
          },
          "required": ["project_id"]
        }
      },
      {
        "name": "vc_restore",
        "description": "Khôi phục toàn bộ scene của dự án về một điểm đã lưu. App tự lưu điểm khôi phục TRƯỚC mỗi thao tác ghi đè (phân tích lại từ đầu, làm lại đoạn cuối, sửa hàng loạt), nên đổi tên nhầm hay lỡ tay chạy lại đều lùi về được. Bản thân việc khôi phục cũng được lưu lại nên quay ngược tiếp được. Lấy snapshot_id từ vc_history.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "project_id": { "type": "integer" },
            "snapshot_id": { "type": "integer" }
          },
          "required": ["project_id", "snapshot_id"]
        }
      },
      {
        "name": "vc_job_raw",
        "description": "Nội dung thô model trả về trong một lượt chạy, lưu nguyên vẹn không cắt. Dùng khi một lượt báo \"không có scene JSON nào đọc được\" để xem model thật sự đã trả về cái gì. Mặc định chỉ trả đoạn đầu.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "job_id": { "type": "integer" },
            "full": { "type": "boolean" }
          },
          "required": ["job_id"]
        }
      },
      {
        "name": "vc_export_bundle",
        "description": "Xuất kịch bản ra dạng máy đọc được để chuyển sang app khác: mỗi đoạn kèm image_prompt (mô tả khung hình) và video_prompt (diễn biến + âm thanh) đã được làm phẳng từ JSON Veo 3, cùng danh sách nhân vật và voice_id. Mặc định chỉ trả tóm tắt; đặt full=true khi thật sự cần cả khối. Muốn ghi ra file/wiki thì dùng vc_export_write.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "project_id": { "type": "integer" },
            "full": { "type": "boolean" }
          },
          "required": ["project_id"]
        }
      },
      {
        "name": "vc_export_write",
        "description": "Ghi kịch bản ra nơi app khác đọc được: \"file\" ghi bundle JSON + bản Markdown vào thư mục chia sẻ ~/.senclaw/exports/video-cloner, \"wiki\" đăng thành trang wiki SenClaw (git-backed, đọc lại bằng wiki_read/wiki_search), \"both\" làm cả hai. Dùng khi Sếp muốn lưu lại kịch bản hoặc chuyển tay sang công cụ khác.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "project_id": { "type": "integer" },
            "target": { "type": "string", "description": "file | wiki | both (mặc định both)" },
            "wiki_path": { "type": "string", "description": "Đường dẫn trang wiki, mặc định video-cloner/<tên-dự-án>.md" }
          },
          "required": ["project_id"]
        }
      },
      {
        "name": "vc_handoff_video_flow",
        "description": "Bàn giao thẳng kịch bản sang app video-flow để sinh video: tạo project, các nhân vật tham chiếu, video và toàn bộ scene bên đó. LUÔN chạy dry_run=true trước để Sếp duyệt nội dung sẽ tạo. Cần video-flow đang chạy. Đặt translate=true để dịch prompt hình ảnh sang tiếng Anh (video-flow đưa thẳng cho Veo 3 nên tiếng Anh cho kết quả tốt hơn; lời thoại vẫn giữ tiếng gốc). SAU KHI bàn giao, TUYỆT ĐỐI không gọi pipeline/create bên video-flow — nó xoá sạch scene vừa tạo; hãy dùng workflow/steps.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "project_id": { "type": "integer" },
            "orientation": { "type": "string", "description": "HORIZONTAL (mặc định) hoặc VERTICAL" },
            "translate": { "type": "boolean", "description": "Dịch image_prompt/video_prompt sang tiếng Anh" },
            "dry_run": { "type": "boolean", "description": "Chỉ dựng payload để xem, không tạo gì bên video-flow" },
            "target_url": { "type": "string", "description": "URL video-flow, mặc định http://127.0.0.1:4460" }
          },
          "required": ["project_id"]
        }
      },
      {
        "name": "vc_project_delete",
        "description": "Xoá một dự án cùng toàn bộ scene và file video của nó. KHÔNG hoàn tác được — phải được Sếp xác nhận rõ ràng trước khi gọi.",
        "inputSchema": {
          "type": "object",
          "properties": { "project_id": { "type": "integer" } },
          "required": ["project_id"]
        }
      }
    ])
}

async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    let db = &state.core.db;

    match name {
        "vc_status" => {
            let projects = db.list_projects().unwrap_or_default();
            let running: Vec<i64> = projects
                .iter()
                .filter(|p| state.core.is_busy(p.id))
                .map(|p| p.id)
                .collect();
            let has_key = !db.gemini_api_key().trim().is_empty();
            json_result(json!({
                "ok": true,
                "projects": projects.len(),
                "has_api_key": has_key,
                "running_projects": running,
                "next": if has_key {
                    "sẵn sàng. Dùng vc_project_list để chọn dự án."
                } else {
                    "CHƯA CÓ GEMINI API KEY — báo Sếp mở Cài đặt của Video Cloner để nhập key, đừng chạy vc_analyze."
                },
            }))
        }

        "vc_presets" => json_result(json!({
            "styles": crate::presets::STYLES,
            "models": crate::presets::models(),
            "characters": crate::presets::character_presets(),
            "backgrounds": crate::presets::background_presets(),
        })),

        "vc_project_list" => {
            let limit = opt_int(args, "limit").unwrap_or(20).clamp(1, 100);
            match db.list_projects() {
                Ok(items) => {
                    let total = items.len();
                    let rows: Vec<Value> = items
                        .iter()
                        .take(limit as usize)
                        .map(|p| {
                            json!({
                                "project_id": p.id,
                                "name": p.name,
                                "style": p.style,
                                "model": p.model,
                                "scene_count": db.scene_count(p.id).unwrap_or(0),
                                "running": state.core.is_busy(p.id),
                                "updated_at": p.updated_at,
                            })
                        })
                        .collect();
                    json_result(json!({ "total": total, "shown": rows.len(), "projects": rows }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }

        "vc_project_get" => {
            let id = int(args, "project_id");
            match db.project(id) {
                Ok(Some(p)) => {
                    let values: Vec<Value> = db
                        .scenes(id)
                        .unwrap_or_default()
                        .iter()
                        .map(|s| s.json.clone())
                        .collect();
                    json_result(json!({
                        "project_id": p.id,
                        "name": p.name,
                        "video": { "filename": p.video_filename, "size": p.video_size },
                        "config": {
                            "style": p.style,
                            "model": p.model,
                            "char_description": p.char_description,
                            "custom_dialogue": p.custom_dialogue,
                            "bg_description": p.bg_description,
                            "auto_magic": p.auto_magic,
                            "visual_similarity": p.visual_similarity,
                            "has_char_image": p.has_char_image,
                        },
                        "scene_count": values.len(),
                        "characters": scenes::detect_characters(&values),
                        "running": state.core.is_busy(id),
                        "latest_job": db.latest_job(id).ok().flatten(),
                    }))
                }
                Ok(None) => error_result(format!("không tìm thấy dự án {id}")),
                Err(e) => error_result(e.to_string()),
            }
        }

        "vc_project_config" => {
            let id = int(args, "project_id");
            let Ok(Some(p)) = db.project(id) else {
                return error_result(format!("không tìm thấy dự án {id}"));
            };
            if let Some(name) = opt_s(args, "name") {
                if !name.trim().is_empty() {
                    let _ = db.set_project_name(id, name.trim());
                }
            }
            let cfg = crate::api::merge_config(&CloneConfig::from(&p), args);
            match db.update_project_config(id, &cfg) {
                Ok(()) => json_result(json!({
                    "ok": true,
                    "config": cfg,
                    "next": "gọi vc_analyze với mode \"start\" để áp dụng cấu hình mới cho toàn bộ video",
                })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "vc_analyze" => {
            let id = int(args, "project_id");
            let mode_str = opt_s(args, "mode").unwrap_or_else(|| "start".into());
            let Some(mode) = Mode::parse(&mode_str) else {
                return error_result(format!(
                    "mode không hợp lệ: {mode_str} (start | continue | regenerate)"
                ));
            };
            let Ok(Some(p)) = db.project(id) else {
                return error_result(format!("không tìm thấy dự án {id}"));
            };
            if db.gemini_api_key().trim().is_empty() {
                return error_result(
                    "chưa có Gemini API key — báo Sếp mở Cài đặt của Video Cloner để nhập key".into(),
                );
            }
            let count = db.scene_count(id).unwrap_or(0);
            if matches!(mode, Mode::Continue | Mode::Regenerate) && count == 0 {
                return error_result(
                    "dự án chưa có scene nào — chạy vc_analyze với mode \"start\" trước".into(),
                );
            }

            let cfg = crate::api::merge_config(&CloneConfig::from(&p), args);
            if let Err(e) = db.update_project_config(id, &cfg) {
                return error_result(e.to_string());
            }
            let p = db.project(id).ok().flatten().unwrap_or(p);

            match process::start(&state.core, &p, mode, cfg) {
                Ok(job_id) => json_result(json!({
                    "job_id": job_id,
                    "mode": mode.as_str(),
                    "status": "queued",
                    "next": "ĐỪNG chờ. Poll vc_job với job_id này sau 30-60 giây; một đoạn 8 giây thường mất vài phút.",
                })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "vc_job" => {
            let id = int(args, "job_id");
            match db.job(id) {
                Ok(Some(j)) => {
                    let total = db.scene_count(j.project_id).unwrap_or(0);
                    json_result(json!({
                        "job_id": j.id,
                        "project_id": j.project_id,
                        "kind": j.kind,
                        "status": j.status,
                        "scenes_added": j.scenes_added,
                        "total_scenes": total,
                        "error": j.error,
                        "next": match j.status.as_str() {
                            "completed" => "xong — đọc kết quả bằng vc_scenes (nhớ dùng limit) hoặc vc_export",
                            "failed" => "thất bại — đọc trường error. Nếu do model không trả JSON thì gọi lại vc_analyze cùng mode.",
                            _ => "đang chạy — poll lại sau 30-60 giây",
                        },
                    }))
                }
                Ok(None) => error_result(format!("không tìm thấy tiến trình {id}")),
                Err(e) => error_result(e.to_string()),
            }
        }

        "vc_scenes" => {
            let id = int(args, "project_id");
            let offset = opt_int(args, "offset").unwrap_or(0).max(0);
            let limit = opt_int(args, "limit").unwrap_or(DEFAULT_SCENE_LIMIT).clamp(1, 20);
            match db.scenes(id) {
                Ok(items) => {
                    let total = items.len() as i64;
                    let window: Vec<Value> = items
                        .iter()
                        .skip(offset as usize)
                        .take(limit as usize)
                        .map(|s| s.json.clone())
                        .collect();
                    let shown = window.len() as i64;
                    json_result(json!({
                        "project_id": id,
                        "total": total,
                        "offset": offset,
                        "shown": shown,
                        "has_more": offset + shown < total,
                        "scenes": window,
                    }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }

        "vc_replace" => {
            let id = int(args, "project_id");
            let Ok(Some(p)) = db.project(id) else {
                return error_result(format!("không tìm thấy dự án {id}"));
            };

            let find = s(args, "find");
            let replace = s(args, "replace");
            let mut voices: HashMap<String, Voice> = HashMap::new();
            if let Some(map) = args.get("voice_overrides").and_then(|v| v.as_object()) {
                for (char_id, v) in map {
                    let raw = v.as_str().unwrap_or_default();
                    match Voice::parse(raw) {
                        Some(voice) => {
                            voices.insert(char_id.clone(), voice);
                        }
                        None => {
                            return error_result(format!(
                                "giọng không hợp lệ cho {char_id}: \"{raw}\" (chỉ nhận male | female)"
                            ))
                        }
                    }
                }
            }
            if find.trim().is_empty() && voices.is_empty() {
                return error_result("cần ít nhất \"find\" hoặc \"voice_overrides\"".into());
            }

            let stored = match db.scenes(id) {
                Ok(s) => s,
                Err(e) => return error_result(e.to_string()),
            };
            let values: Vec<Value> = stored.iter().map(|x| x.json.clone()).collect();
            if values.is_empty() {
                return error_result("dự án chưa có scene nào để sửa".into());
            }

            let label = if find.trim().is_empty() {
                "trước khi đổi giọng hàng loạt".to_string()
            } else {
                format!("trước khi đổi \"{}\" → \"{}\"", find.trim(), replace.trim())
            };

            let outcome = match scenes::apply_replace(
                &values,
                &ReplaceRequest {
                    find,
                    replace,
                    only_with_dialogue: opt_bool(args, "only_with_dialogue").unwrap_or(false),
                    voice_overrides: voices,
                    style: p.style.clone(),
                },
            ) {
                Ok(o) => o,
                Err(e) => return error_result(e),
            };

            let snapshot_id = db.snapshot(id, "replace", &label).ok().flatten();
            let entries = crate::api::pair_with_jobs(&stored, &outcome.scenes);

            match db.replace_all_scenes(id, &entries) {
                Ok(()) => {
                    state
                        .core
                        .dash
                        .emit("scenes:updated", json!({ "project_id": id }));
                    json_result(json!({
                        "ok": true,
                        "scenes_updated": outcome.scenes.len(),
                        "replaced_text": outcome.replaced_text,
                        "voices_applied": outcome.voices_applied,
                        "characters": scenes::detect_characters(&outcome.scenes),
                        "snapshot_id": snapshot_id,
                        "next": "đã tự lưu điểm khôi phục trước khi sửa — nếu sai, gọi vc_restore với snapshot_id này",
                    }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }

        "vc_history" => {
            let id = int(args, "project_id");
            if matches!(db.project(id), Ok(None)) {
                return error_result(format!("không tìm thấy dự án {id}"));
            }
            let limit = opt_int(args, "limit").unwrap_or(20).clamp(1, 100);
            let jobs: Vec<Value> = db
                .list_jobs(id, limit)
                .unwrap_or_default()
                .iter()
                .map(|j| {
                    json!({
                        "job_id": j.id,
                        "kind": j.kind,
                        "status": j.status,
                        "scenes_added": j.scenes_added,
                        "model": j.model,
                        "temperature": j.temperature,
                        "error": j.error,
                        "created_at": j.created_at,
                    })
                })
                .collect();
            let snaps: Vec<Value> = db
                .list_snapshots(id)
                .unwrap_or_default()
                .iter()
                .map(|s| {
                    json!({
                        "snapshot_id": s.id,
                        "reason": s.reason,
                        "label": s.label,
                        "scene_count": s.scene_count,
                        "created_at": s.created_at,
                    })
                })
                .collect();
            json_result(json!({
                "project_id": id,
                "runs": jobs,
                "restore_points": snaps,
                "next": "khôi phục một bản cũ bằng vc_restore với snapshot_id tương ứng",
            }))
        }

        "vc_restore" => {
            let id = int(args, "project_id");
            let snapshot_id = int(args, "snapshot_id");

            let Ok(Some(meta)) = db.snapshot_meta(snapshot_id) else {
                return error_result(format!("không tìm thấy điểm khôi phục {snapshot_id}"));
            };
            if meta.project_id != id {
                return error_result(format!(
                    "điểm khôi phục {snapshot_id} thuộc dự án {} chứ không phải {id}",
                    meta.project_id
                ));
            }
            if state.core.is_busy(id) {
                return error_result(
                    "dự án đang chạy phân tích — chờ xong rồi hãy khôi phục".into(),
                );
            }

            let Ok(Some(scene_values)) = db.snapshot_scenes(snapshot_id) else {
                return error_result("điểm khôi phục không còn nội dung".into());
            };

            let undo = db
                .snapshot(id, "restore", &format!("trước khi khôi phục #{snapshot_id}"))
                .ok()
                .flatten();
            let entries: Vec<(i64, Value)> =
                scene_values.iter().map(|v| (0, v.clone())).collect();

            match db.replace_all_scenes(id, &entries) {
                Ok(()) => {
                    state
                        .core
                        .dash
                        .emit("scenes:updated", json!({ "project_id": id }));
                    json_result(json!({
                        "ok": true,
                        "restored_scenes": scene_values.len(),
                        "undo_snapshot_id": undo,
                        "next": "trạng thái ngay trước khi khôi phục cũng đã được lưu — quay lại được bằng undo_snapshot_id",
                    }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }

        "vc_job_raw" => {
            let job_id = int(args, "job_id");
            match db.job_raw(job_id) {
                Ok(Some(raw)) => {
                    let full = opt_bool(args, "full").unwrap_or(false);
                    if full {
                        json_result(json!({ "job_id": job_id, "raw": raw }))
                    } else {
                        json_result(json!({
                            "job_id": job_id,
                            "chars": raw.chars().count(),
                            "preview": scenes::truncate_chars(raw.trim(), 800),
                            "next": "gọi lại với full=true nếu cần cả khối để soi lỗi parse",
                        }))
                    }
                }
                Ok(None) => error_result(format!("không tìm thấy tiến trình {job_id}")),
                Err(e) => error_result(e.to_string()),
            }
        }

        "vc_export" => {
            let id = int(args, "project_id");
            let values: Vec<Value> = match db.scenes(id) {
                Ok(s) => s.iter().map(|x| x.json.clone()).collect(),
                Err(e) => return error_result(e.to_string()),
            };
            let text = scenes::export_text(&values);
            if opt_bool(args, "full").unwrap_or(false) {
                json_result(json!({ "scene_count": values.len(), "text": text }))
            } else {
                json_result(json!({
                    "scene_count": values.len(),
                    "chars": text.chars().count(),
                    "preview": scenes::truncate_chars(&text, 800),
                    "next": "gọi lại với full=true nếu Sếp cần cả khối để dán vào Veo 3",
                }))
            }
        }

        "vc_export_bundle" => {
            let id = int(args, "project_id");
            let (project, stored) = match load_export(db, id) {
                Ok(v) => v,
                Err(e) => return error_result(e),
            };
            let bundle = crate::export::bundle(&project, &stored, &crate::db::now());
            if opt_bool(args, "full").unwrap_or(false) {
                json_result(bundle)
            } else {
                json_result(json!({
                    "format": bundle["format"],
                    "version": bundle["version"],
                    "summary": bundle["summary"],
                    "cast": bundle["cast"],
                    "first_scene": bundle["scenes"].get(0).cloned().unwrap_or(Value::Null),
                    "next": "gọi lại với full=true để lấy toàn bộ, hoặc vc_export_write để ghi ra file/wiki, hoặc vc_handoff_video_flow để bàn giao thẳng",
                }))
            }
        }

        "vc_export_write" => {
            let id = int(args, "project_id");
            let target = opt_s(args, "target").unwrap_or_else(|| "both".into());
            let target = target.trim().to_lowercase();
            if !matches!(target.as_str(), "file" | "wiki" | "both") {
                return error_result(format!(
                    "target không hợp lệ: {target} (dùng file | wiki | both)"
                ));
            }

            let (project, stored) = match load_export(db, id) {
                Ok(v) => v,
                Err(e) => return error_result(e),
            };
            let now = crate::db::now();
            let slug = crate::export::slug(&project.name, project.id);
            let mut out = json!({ "ok": true, "scene_count": stored.len() });

            if target == "file" || target == "both" {
                match write_export_files(&project, &stored, &now, &slug).await {
                    Ok(v) => out["file"] = v,
                    Err(e) => return error_result(e),
                }
            }
            if target == "wiki" || target == "both" {
                let path = opt_s(args, "wiki_path")
                    .filter(|p| !p.trim().is_empty())
                    .unwrap_or_else(|| format!("video-cloner/{slug}.md"));
                let md = crate::export::markdown(&project, &stored, &now);
                match write_wiki(&path, &md, &project.name).await {
                    Ok(()) => out["wiki"] = json!({ "path": path }),
                    Err(e) => return error_result(e),
                }
            }
            json_result(out)
        }

        "vc_handoff_video_flow" => {
            let id = int(args, "project_id");
            let (project, stored) = match load_export(db, id) {
                Ok(v) => v,
                Err(e) => return error_result(e),
            };

            let orientation = opt_s(args, "orientation").unwrap_or_default();
            let mut plan = crate::handoff::plan(&project, &stored, &orientation);

            let mut translated = 0usize;
            if opt_bool(args, "translate").unwrap_or(false) {
                match crate::handoff::translate_plan(&mut plan).await {
                    Ok(n) => translated = n,
                    Err(e) => return error_result(format!("dịch sang tiếng Anh thất bại: {e}")),
                }
            }

            if opt_bool(args, "dry_run").unwrap_or(false) {
                return json_result(json!({
                    "dry_run": true,
                    "translated_scenes": translated,
                    "will_create": {
                        "project": plan["project"]["name"],
                        "entities": plan["entities"].as_array().map(|a| a.len()).unwrap_or(0),
                        "scenes": plan["scenes"].as_array().map(|a| a.len()).unwrap_or(0),
                        "orientation": plan["video"]["orientation"],
                    },
                    "first_scene": plan["scenes"].get(0).cloned().unwrap_or(Value::Null),
                    "next": "đưa Sếp duyệt rồi gọi lại với dry_run=false để tạo thật",
                }));
            }

            let base = opt_s(args, "target_url")
                .filter(|u| !u.trim().is_empty())
                .unwrap_or_else(crate::config::video_flow_url);

            if let Err(e) = crate::handoff::probe(&base).await {
                return error_result(format!("{e}. Hãy bật app video-flow trước khi bàn giao."));
            }

            match crate::handoff::push(&base, &plan).await {
                Ok(p) => json_result(json!({
                    "ok": true,
                    "target": base,
                    "video_flow_project_id": p.project_id,
                    "video_flow_video_id": p.video_id,
                    "entities_created": p.entity_count,
                    "scenes_created": p.scene_count,
                    "translated_scenes": translated,
                    "next": "bên video-flow: chạy vf_workflow_run (hoặc các bước steps/scene) để sinh ảnh và video. TUYỆT ĐỐI KHÔNG gọi vf_pipeline_create — script_parser của nó xoá sạch scene vừa bàn giao.",
                })),
                Err(e) => error_result(e),
            }
        }

        "vc_project_delete" => {
            let id = int(args, "project_id");
            let Ok(Some(p)) = db.project(id) else {
                return error_result(format!("không tìm thấy dự án {id}"));
            };
            match db.delete_project(id) {
                Ok(()) => {
                    let _ = tokio::fs::remove_file(&p.video_path).await;
                    if !p.char_image_path.is_empty() {
                        let _ = tokio::fs::remove_file(&p.char_image_path).await;
                    }
                    state
                        .core
                        .dash
                        .emit("project:deleted", json!({ "project_id": id }));
                    json_result(json!({ "ok": true, "deleted": id }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }

        other => error_result(format!("tool không tồn tại: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dashws::DashHub;
    use crate::db::Db;
    use crate::state::Core;
    use std::sync::Arc;

    fn state() -> AppState {
        // `Core::boot` touches the real data dir; tests build one in memory.
        let core = Core::for_test(Db::open_memory().unwrap(), DashHub::new());
        let (tx, _) = tokio::sync::broadcast::channel(8);
        AppState { core, mcp_tx: tx }
    }

    #[tokio::test]
    async fn every_advertised_tool_is_dispatchable() {
        let st = state();
        for tool in tools_list().as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            let out = call_tool(&st, name, &json!({})).await;
            let text = out["content"][0]["text"].as_str().unwrap_or_default();
            assert!(
                !text.contains("tool không tồn tại"),
                "{name} is advertised but not dispatchable"
            );
        }
    }

    #[tokio::test]
    async fn an_unknown_tool_is_rejected() {
        let st = state();
        let out = call_tool(&st, "vc_nope", &json!({})).await;
        assert_eq!(out["isError"], true);
    }

    #[tokio::test]
    async fn status_tells_the_agent_to_stop_when_no_api_key_is_set() {
        let st = state();
        let out = call_tool(&st, "vc_status", &json!({})).await;
        let text = out["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("CHƯA CÓ GEMINI API KEY"), "got: {text}");
    }

    #[tokio::test]
    async fn scenes_are_windowed_by_default() {
        let st = state();
        let id = st
            .core
            .db
            .create_project("p", "/tmp/v.mp4", "video/mp4", 1, "v.mp4", &CloneConfig::default())
            .unwrap();
        let many: Vec<Value> = (1..=12).map(|i| json!({ "scene_id": i.to_string() })).collect();
        st.core.db.append_scenes(id, &many, 1).unwrap();

        let out = call_tool(&st, "vc_scenes", &json!({ "project_id": id })).await;
        let text = out["content"][0]["text"].as_str().unwrap();
        let v: Value = serde_json::from_str(text).unwrap();
        assert_eq!(v["total"], 12);
        assert_eq!(v["shown"], DEFAULT_SCENE_LIMIT);
        assert_eq!(v["has_more"], true);
    }

    #[tokio::test]
    async fn continue_is_refused_before_any_scene_exists() {
        let st = state();
        st.core.db.set_setting("gemini_api_key", "k").unwrap();
        let id = st
            .core
            .db
            .create_project("p", "/tmp/v.mp4", "video/mp4", 1, "v.mp4", &CloneConfig::default())
            .unwrap();

        let out = call_tool(
            &st,
            "vc_analyze",
            &json!({ "project_id": id, "mode": "continue" }),
        )
        .await;
        assert_eq!(out["isError"], true);
    }

    #[tokio::test]
    async fn replace_rejects_an_unknown_voice_name() {
        let st = state();
        let id = st
            .core
            .db
            .create_project("p", "/tmp/v.mp4", "video/mp4", 1, "v.mp4", &CloneConfig::default())
            .unwrap();
        st.core.db.append_scenes(id, &[json!({"scene_id":"1"})], 1).unwrap();

        let out = call_tool(
            &st,
            "vc_replace",
            &json!({ "project_id": id, "voice_overrides": { "CHAR_1": "robot" } }),
        )
        .await;
        assert_eq!(out["isError"], true);
    }

    #[tokio::test]
    async fn export_does_not_dump_everything_unless_asked() {
        let st = state();
        let id = st
            .core
            .db
            .create_project("p", "/tmp/v.mp4", "video/mp4", 1, "v.mp4", &CloneConfig::default())
            .unwrap();
        st.core.db.append_scenes(id, &[json!({"scene_id":"1"})], 1).unwrap();

        let out = call_tool(&st, "vc_export", &json!({ "project_id": id })).await;
        let v: Value =
            serde_json::from_str(out["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(v.get("preview").is_some());
        assert!(v.get("text").is_none());
    }
}
