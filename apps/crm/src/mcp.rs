//! MCP server (HTTP + SSE) that exposes CRM operations to SenClaw agents. All
//! side effects go through the same `AppState.db` the REST API uses, so nothing
//! can drift between the UI and the agent's view.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::api::{now_ts, AppState};
use crate::db::{ChannelCreate, ChannelPatch, CustomerCreate, CustomerPatch, DealCreate, DealPatch, RelationshipCreate, TaskCreate};

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

pub async fn mcp_sse(
    State(state): State<Arc<AppState>>,
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
    State(state): State<Arc<AppState>>,
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
            "serverInfo": { "name": "crm-mcp", "version": "1.0.0" }
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

fn tools_list() -> Value {
    json!([
        {
            "name": "crm_list_customers",
            "description": "List / search customers in the SenClaw CRM. Use for 'khách hàng của tôi', 'tìm khách tên X', 'khách công ty Y', or 'khách nào có tag Z'. Returns id, name, email, phone, company, tags, status, last-interaction date, and total interaction count.",
            "inputSchema": { "type": "object", "properties": {
                "q":      { "type": "string", "description": "Free-text search over name/email/phone/company/notes/tags." },
                "tag":    { "type": "string", "description": "Return only customers carrying this exact tag." },
                "status": { "type": "string", "description": "Pipeline status filter, e.g. 'lead', 'customer', 'lost'." },
                "limit":  { "type": "number", "description": "Max rows (default 50, max 500)." }
            } }
        },
        {
            "name": "crm_get_customer",
            "description": "Fetch one customer by id — the FULL profile (avatar, contact, notes, tags) plus the 20 most recent interactions. Start here for 'ai là khách X' / 'thông tin về khách hàng #123'.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        },
        {
            "name": "crm_find_by_email",
            "description": "Look up a customer by their email address (case-insensitive). Use before creating a new customer to avoid duplicates.",
            "inputSchema": { "type": "object", "properties": {
                "email": { "type": "string" }
            }, "required": ["email"] }
        },
        {
            "name": "crm_create_customer",
            "description": "Create a new customer record. Only 'name' is required. 'avatar_url' can be an https URL or a base64 data URL (data:image/png;base64,...). 'tags' is a free-form array. Returns the created row (with its new id).",
            "inputSchema": { "type": "object", "properties": {
                "name":       { "type": "string" },
                "email":      { "type": "string" },
                "phone":      { "type": "string" },
                "company":    { "type": "string" },
                "title":      { "type": "string", "description": "Job title / role." },
                "avatar_url": { "type": "string" },
                "notes":      { "type": "string" },
                "tags":       { "type": "array", "items": { "type": "string" } },
                "status":     { "type": "string", "description": "Pipeline status. Default 'lead'." },
                "source":     { "type": "string", "description": "Where they came from (referral, website, event...)." },
                "address":    { "type": "string" },
                "birthday":   { "type": "string", "description": "YYYY-MM-DD or a free-form label." }
            }, "required": ["name"] }
        },
        {
            "name": "crm_update_customer",
            "description": "Patch an existing customer. Any omitted field is left untouched. Pass an empty string to clear a scalar field. Pass a replacement array to overwrite tags.",
            "inputSchema": { "type": "object", "properties": {
                "id":         { "type": "number" },
                "name":       { "type": "string" },
                "email":      { "type": "string" },
                "phone":      { "type": "string" },
                "company":    { "type": "string" },
                "title":      { "type": "string" },
                "avatar_url": { "type": "string" },
                "notes":      { "type": "string" },
                "tags":       { "type": "array", "items": { "type": "string" } },
                "status":     { "type": "string" },
                "source":     { "type": "string" },
                "address":    { "type": "string" },
                "birthday":   { "type": "string" }
            }, "required": ["id"] }
        },
        {
            "name": "crm_delete_interaction",
            "description": "Delete a single logged touchpoint by id (a specific call/email/meeting/note/task/update entry). Use for 'xoá tương tác #123', 'gỡ log gọi nhầm'. Confirm with the user first.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        },
        {
            "name": "crm_delete_task",
            "description": "Delete a task by id — a hard delete different from crm_complete_task (which marks it done but keeps the record). Confirm with the user first.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        },
        {
            "name": "crm_sync_calendar",
            "description": "Push open tasks and upcoming birthdays from the CRM to the Space Calendar app (a generic calendar Space App at localhost:4392). Upsert semantics — events the user edited or deleted on the calendar side are preserved. Reverse updates from Space Calendar (edit time / delete) flow back into the CRM via /api/sync/callback. Returns { pushed_tasks, pushed_birthdays, targets, warnings, note }. Use for 'đồng bộ lịch', 'push việc sang lịch', 'sync task to calendar'.",
            "inputSchema": { "type": "object", "properties": {
                "space_calendar": { "type": "boolean", "description": "Push to Space Calendar. Default true." }
            } }
        },
        {
            "name": "crm_delete_customer",
            "description": "Delete a customer and every interaction logged against them. Irreversible — confirm with the user first.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        },
        {
            "name": "crm_list_channels",
            "description": "List every extra contact channel of a customer: additional phone numbers, secondary emails, and social handles (zalo, facebook, linkedin, instagram, x, tiktok, youtube, github, telegram, whatsapp, signal, line, wechat, skype, viber, discord, messenger, website). Use when the user asks 'khách này có Zalo/Facebook không', 'các số điện thoại của khách X', 'social của khách'.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" }
            }, "required": ["customer_id"] }
        },
        {
            "name": "crm_add_channel",
            "description": "Add a contact channel to a customer. `kind` in phone|email|zalo|facebook|linkedin|instagram|x|tiktok|youtube|github|telegram|whatsapp|signal|line|wechat|skype|viber|discord|messenger|website. `value` is the raw handle/number/URL. `label` is optional user shorthand ('Công việc', 'Cá nhân', 'Vợ'). Use for 'thêm SĐT thứ hai cho khách X', 'lưu Zalo của khách Y', 'khách Z có Facebook mới'.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" },
                "kind":        { "type": "string" },
                "value":       { "type": "string" },
                "label":       { "type": "string" }
            }, "required": ["customer_id", "kind", "value"] }
        },
        {
            "name": "crm_update_channel",
            "description": "Patch an existing channel by id — change kind/value/label. Any omitted field left as-is.",
            "inputSchema": { "type": "object", "properties": {
                "id":    { "type": "number" },
                "kind":  { "type": "string" },
                "value": { "type": "string" },
                "label": { "type": "string" }
            }, "required": ["id"] }
        },
        {
            "name": "crm_delete_channel",
            "description": "Remove a contact channel by id. Irreversible — confirm first.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        },
        {
            "name": "crm_add_interaction",
            "description": "Log a touchpoint with a customer: call, email, meeting, or a free-form note. Use for 'ghi lại vừa gọi cho khách X', 'lưu meeting với khách Y'. Timestamp defaults to now.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" },
                "kind":        { "type": "string", "enum": ["call", "email", "meeting", "note", "task"], "description": "Interaction type. Default 'note'." },
                "summary":     { "type": "string", "description": "One-line summary of what happened." },
                "details":     { "type": "string", "description": "Longer body text (optional)." },
                "occurred_at": { "type": "number", "description": "Unix seconds. Default = now." }
            }, "required": ["customer_id", "summary"] }
        },
        {
            "name": "crm_list_interactions",
            "description": "List the interactions logged against a customer, newest first.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" },
                "limit":       { "type": "number", "description": "Max rows (default 50)." }
            }, "required": ["customer_id"] }
        },
        {
            "name": "crm_all_tags",
            "description": "List every tag currently in use across all customers, sorted alphabetically.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "crm_stats",
            "description": "Dashboard summary: total customers, total interactions, and a count per pipeline status.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "crm_summarize",
            "description": "Produce a concise AI briefing for a customer: who they are, latest activity, and the next recommended step. Grounded in the stored profile + recent interactions. Requires the daemon's LLM.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        },
        {
            "name": "crm_add_deal",
            "description": "Add a new sales opportunity (deal) to a customer. Stages: qualifying, proposal, negotiation, won, lost. Use for 'khách X quan tâm gói Y giá Z', 'add opportunity to customer'.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id":       { "type": "number" },
                "title":             { "type": "string", "description": "Short deal name (e.g. 'Yearly plan')." },
                "amount":            { "type": "number", "description": "Deal value. Default 0." },
                "currency":          { "type": "string", "description": "3-letter code. Default VND." },
                "stage":             { "type": "string", "enum": ["qualifying","proposal","negotiation","won","lost"], "description": "Default 'qualifying'." },
                "probability":       { "type": "number", "description": "0-100. Default 50." },
                "expected_close_at": { "type": "number", "description": "Expected close, Unix seconds." },
                "notes":             { "type": "string" }
            }, "required": ["customer_id", "title"] }
        },
        {
            "name": "crm_move_deal",
            "description": "Change a deal's stage (or any other field) — the typical 'chuyển deal sang won/lost' operation. Setting stage=won|lost stamps closed_at automatically.",
            "inputSchema": { "type": "object", "properties": {
                "id":                { "type": "number" },
                "stage":             { "type": "string", "enum": ["qualifying","proposal","negotiation","won","lost"] },
                "amount":            { "type": "number" },
                "probability":       { "type": "number" },
                "expected_close_at": { "type": "number" },
                "title":             { "type": "string" },
                "notes":             { "type": "string" }
            }, "required": ["id"] }
        },
        {
            "name": "crm_list_deals",
            "description": "List deals for the pipeline. Optional stage filter. Includes customer_name so the agent can talk about them by name without a second lookup.",
            "inputSchema": { "type": "object", "properties": {
                "stage":       { "type": "string", "enum": ["qualifying","proposal","negotiation","won","lost"] },
                "customer_id": { "type": "number", "description": "If set, returns only deals for this customer." }
            } }
        },
        {
            "name": "crm_delete_deal",
            "description": "Delete a deal. Irreversible — confirm with the user first.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        },
        {
            "name": "crm_add_task",
            "description": "Create a follow-up task / reminder. Optionally link it to a customer. Use for 'nhắc tôi gọi lại khách X ngày mai', 'follow-up với khách Y tuần sau'.",
            "inputSchema": { "type": "object", "properties": {
                "title":       { "type": "string" },
                "customer_id": { "type": "number", "description": "Optional — a task can be unattached." },
                "details":     { "type": "string" },
                "due_at":      { "type": "number", "description": "Due time, Unix seconds." }
            }, "required": ["title"] }
        },
        {
            "name": "crm_complete_task",
            "description": "Mark a task done (or re-open it). Use `done=false` to re-open.",
            "inputSchema": { "type": "object", "properties": {
                "id":   { "type": "number" },
                "done": { "type": "boolean", "description": "Default true." }
            }, "required": ["id"] }
        },
        {
            "name": "crm_list_tasks",
            "description": "List tasks. By default only open tasks are returned, ordered by due date ascending. Pass `open_only=false` to include finished ones.",
            "inputSchema": { "type": "object", "properties": {
                "open_only":   { "type": "boolean", "description": "Default true." },
                "customer_id": { "type": "number", "description": "If set, only tasks for this customer." },
                "limit":       { "type": "number", "description": "Default 50." }
            } }
        },
        {
            "name": "crm_upcoming",
            "description": "Personal-CRM feed: tasks due AND customer birthdays coming up in the next N days (default 14). Use for 'sắp tới có gì', 'khách nào sinh nhật tuần này', 'việc gì sắp đến hạn'.",
            "inputSchema": { "type": "object", "properties": {
                "days": { "type": "number", "description": "Look-ahead window in days. Default 14." }
            } }
        },
        {
            "name": "crm_add_relationship",
            "description": "Link two customers with a directional relationship. Kinds: referred_by, introduced_by, colleague_of, spouse_of, family_of, friend_of, reports_to, partner_of, supplier_of, competitor_of, contact_of. Semantics: `from_id --(kind)--> to_id` reads as 'from is <kind> to'. E.g. add_relationship(from=Anna, to=Tuấn, kind=referred_by) = 'Anna was referred by Tuấn'. Duplicate (from,to,kind) triples upsert.",
            "inputSchema": { "type": "object", "properties": {
                "from_id":    { "type": "number" },
                "to_id":      { "type": "number" },
                "kind":       { "type": "string", "enum": ["referred_by","introduced_by","colleague_of","spouse_of","family_of","friend_of","reports_to","partner_of","supplier_of","competitor_of","contact_of"] },
                "note":       { "type": "string" },
                "confidence": { "type": "number", "description": "0-1. Default 1.0." }
            }, "required": ["from_id", "to_id", "kind"] }
        },
        {
            "name": "crm_list_relationships",
            "description": "List every relationship involving a customer (either endpoint), OR all relationships across the whole CRM if `customer_id` is omitted. Includes both endpoints' names.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" }
            } }
        },
        {
            "name": "crm_delete_relationship",
            "description": "Remove a single relationship by id.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        },
        {
            "name": "crm_ai_path",
            "description": "AI-driven connection search between TWO customers. Goes BEYOND the explicit BFS path — reasons about shared interests, common industries/markets, hobbies, mediating people who could bridge, cities, events. Returns a summary + list of connections (type: shared_interest|common_market|possible_bridge|explicit_path|weak_tie|shared_person, detail, strength). Includes the BFS shortest path as grounding when it exists. Use for 'kết nối tiềm năng giữa X và Y', 'điểm chung của A và B', 'A và B có thể gặp nhau qua đâu'. Requires the daemon's LLM.",
            "inputSchema": { "type": "object", "properties": {
                "from": { "type": "number" },
                "to":   { "type": "number" }
            }, "required": ["from", "to"] }
        },
        {
            "name": "crm_find_common",
            "description": "AI-driven common-ground search: for the focus customer, ask the LLM to identify every meaningful theme they share with OTHER customers in the CRM — industry, product, project, market, hobby, mediating person, event. Returns a list of themes each with `theme`, `why` (1 sentence), and `customer_ids` who share it, plus a de-duped `highlight_ids` list for the graph. Use for 'khách nào có điểm chung với X', 'ai làm cùng ngành với X', 'gợi ý kết nối chéo'. Requires the daemon's LLM.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        },
        {
            "name": "crm_similar_customers",
            "description": "Rank OTHER customers by similarity to the given one. Signals combined: Jaccard on tags, same company, Jaccard on 1-hop neighbours in the relationship graph, and shared extracted mentions. Returns each match with a numeric score and Vietnamese reasons like 'chung tag #vip, cùng công ty Shop Co, cùng biết Tuấn Anh'. Use for 'ai giống khách X', 'gợi ý khách tương tự', 'tìm khách hàng có hồ sơ tương đồng'.",
            "inputSchema": { "type": "object", "properties": {
                "id":    { "type": "number" },
                "limit": { "type": "number", "description": "Default 8, max 50." }
            }, "required": ["id"] }
        },
        {
            "name": "crm_find_path",
            "description": "BFS shortest path between two customers through the (undirected) relationship graph. Returns the ordered id path plus the nodes/edges along it. Use for 'kết nối giữa khách A và B', 'ai giới thiệu ai qua chuỗi'.",
            "inputSchema": { "type": "object", "properties": {
                "from": { "type": "number" },
                "to":   { "type": "number" }
            }, "required": ["from", "to"] }
        },
        {
            "name": "crm_expand_network",
            "description": "Subgraph reachable from a focus customer within N hops. Use for 'mở rộng mạng lưới của khách X 2 hop'.",
            "inputSchema": { "type": "object", "properties": {
                "focus": { "type": "number" },
                "hops":  { "type": "number", "description": "Default 1." }
            }, "required": ["focus"] }
        },
        {
            "name": "crm_customer_network",
            "description": "Return a customer's direct connections as a graph: the customer, every neighbour they relate to (via any kind), and the edges. Use for 'ai đang liên kết với khách X', 'mạng lưới của khách Y'.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" }
            }, "required": ["customer_id"] }
        },
        {
            "name": "crm_search",
            "description": "Full-text search across the WHOLE CRM (customer profiles, interactions, extracted mentions) via FTS5 with Vietnamese diacritic folding — 'khach' matches 'khách', 'anna' matches 'Anna Nguyễn'. Returns hits with entity_type (customer|interaction|mention), entity_id, customer_id + name, and a 12-word snippet. Use for 'tìm ai nói về X', 'ai đã từng nhắc đến sản phẩm Y', 'khách nào có ghi chú Z'.",
            "inputSchema": { "type": "object", "properties": {
                "q":     { "type": "string" },
                "limit": { "type": "number", "description": "Default 20, max 100." }
            }, "required": ["q"] }
        },
        {
            "name": "crm_extract_graph",
            "description": "Ask the LLM to READ a customer's profile + notes + interactions and extract every OTHER person mentioned, plus the relationship implied. Each extraction is saved as a mention; if the mentioned name matches an existing customer, a directional relationship (source='ai') is created automatically. Use for 'phân tích mạng lưới khách X', 'ai đã giới thiệu ai cho khách Y', 'trích mối quan hệ'. Requires the daemon's LLM.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" }
            }, "required": ["customer_id"] }
        },
        {
            "name": "crm_list_mentions",
            "description": "List AI-extracted mentions of people who aren't yet full customers. Use for 'ai đã được trích ra chưa có trong CRM', 'ai giới thiệu chưa map được'.",
            "inputSchema": { "type": "object", "properties": {
                "unresolved_only": { "type": "boolean", "description": "Default false; true skips mentions already linked to a customer." },
                "limit":           { "type": "number", "description": "Default 50." }
            } }
        },
        {
            "name": "crm_aggregate_report",
            "description": "Generate an AI executive briefing across the WHOLE CRM: totals, pipeline by stage, top open deals, most active customers, recent activity, upcoming birthdays, overdue tasks. Grounded in stored data. Use for 'tổng hợp CRM hôm nay', 'báo cáo tổng quan khách hàng', 'thống kê CRM', 'CRM summary'. Requires the daemon's LLM.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "crm_recent_activity",
            "description": "Global interaction feed across ALL customers, newest first. Use for 'gần đây có gì mới', 'khách nào vừa tương tác'.",
            "inputSchema": { "type": "object", "properties": {
                "limit": { "type": "number", "description": "Default 50." }
            } }
        }
    ])
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    match name {
        "crm_list_customers" => {
            let q = args["q"].as_str().map(str::to_string);
            let tag = args["tag"].as_str().map(str::to_string);
            let status = args["status"].as_str().map(str::to_string);
            let limit = args["limit"].as_i64().unwrap_or(50);
            match state.db.list_customers(q.as_deref(), tag.as_deref(), status.as_deref(), limit) {
                Ok(list) => json_result(json!({ "count": list.len(), "customers": list })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_get_customer" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            match state.db.get_customer(id) {
                Ok(Some(c)) => {
                    let interactions = state.db.list_interactions(id, 20).unwrap_or_default();
                    json_result(json!({ "customer": c, "interactions": interactions }))
                }
                Ok(None) => error_result(format!("customer {id} not found")),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_find_by_email" => {
            let email = args["email"].as_str().unwrap_or("").trim();
            if email.is_empty() {
                return error_result("email is required".into());
            }
            match state.db.find_by_email(email) {
                Ok(Some(c)) => json_result(json!({ "customer": c })),
                Ok(None) => json_result(json!({ "customer": null })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_create_customer" => {
            let create: CustomerCreate = match serde_json::from_value(args.clone()) {
                Ok(v) => v,
                Err(e) => return error_result(format!("bad arguments: {e}")),
            };
            match state.db.create_customer(&create, now_ts()) {
                Ok(id) => match state.db.get_customer(id) {
                    Ok(Some(c)) => json_result(json!({ "customer": c })),
                    _ => error_result("created but could not read back".into()),
                },
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_update_customer" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            let patch: CustomerPatch = match serde_json::from_value(args.clone()) {
                Ok(v) => v,
                Err(e) => return error_result(format!("bad arguments: {e}")),
            };
            match state.db.update_customer(id, &patch, now_ts()) {
                Ok(()) => match state.db.get_customer(id) {
                    Ok(Some(c)) => json_result(json!({ "customer": c })),
                    _ => error_result("updated but could not read back".into()),
                },
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_delete_interaction" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            match state.db.delete_interaction(id, now_ts()) {
                Ok(()) => json_result(json!({ "ok": true, "id": id })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_delete_task" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            match state.db.delete_task(id) {
                Ok(()) => json_result(json!({ "ok": true, "id": id })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_sync_calendar" => {
            let space_calendar = args["space_calendar"].as_bool().unwrap_or(true);
            let body = crate::api::sync_calendar_impl(&state.db, space_calendar).await;
            match body {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }
        "crm_delete_customer" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            match state.db.delete_customer(id) {
                Ok(()) => json_result(json!({ "ok": true, "id": id })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_list_channels" => {
            let Some(cid) = args["customer_id"].as_i64() else {
                return error_result("customer_id is required".into());
            };
            match state.db.list_channels(cid) {
                Ok(list) => json_result(json!({ "count": list.len(), "channels": list })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_add_channel" => {
            let Some(cid) = args["customer_id"].as_i64() else {
                return error_result("customer_id is required".into());
            };
            let create: ChannelCreate = match serde_json::from_value(args.clone()) {
                Ok(v) => v,
                Err(e) => return error_result(format!("bad arguments: {e}")),
            };
            match state.db.add_channel(cid, &create, now_ts()) {
                Ok(id) => json_result(json!({ "ok": true, "id": id })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_update_channel" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            let patch: ChannelPatch = match serde_json::from_value(args.clone()) {
                Ok(v) => v,
                Err(e) => return error_result(format!("bad arguments: {e}")),
            };
            match state.db.update_channel(id, &patch, now_ts()) {
                Ok(()) => json_result(json!({ "ok": true, "id": id })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_delete_channel" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            match state.db.delete_channel(id) {
                Ok(_) => json_result(json!({ "ok": true, "id": id })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_add_interaction" => {
            let Some(cid) = args["customer_id"].as_i64() else {
                return error_result("customer_id is required".into());
            };
            let summary = args["summary"].as_str().unwrap_or("").trim();
            if summary.is_empty() {
                return error_result("summary is required".into());
            }
            let kind = args["kind"].as_str().unwrap_or("note");
            let details = args["details"].as_str().unwrap_or("");
            let now = now_ts();
            let occurred = args["occurred_at"].as_i64().unwrap_or(now);
            match state.db.add_interaction(cid, kind, summary, details, occurred, now) {
                Ok(id) => {
                    let list = state.db.list_interactions(cid, 500).unwrap_or_default();
                    let created = list.into_iter().find(|i| i.id == id);
                    json_result(json!({ "interaction": created }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_list_interactions" => {
            let Some(cid) = args["customer_id"].as_i64() else {
                return error_result("customer_id is required".into());
            };
            let limit = args["limit"].as_i64().unwrap_or(50);
            match state.db.list_interactions(cid, limit) {
                Ok(list) => json_result(json!({ "count": list.len(), "interactions": list })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_all_tags" => match state.db.all_tags() {
            Ok(tags) => json_result(json!({ "tags": tags })),
            Err(e) => error_result(e.to_string()),
        },
        "crm_stats" => match state.db.stats() {
            Ok(s) => json_result(s),
            Err(e) => error_result(e.to_string()),
        },
        "crm_add_deal" => {
            let create: DealCreate = match serde_json::from_value(args.clone()) {
                Ok(v) => v,
                Err(e) => return error_result(format!("bad arguments: {e}")),
            };
            match state.db.create_deal(&create, now_ts()) {
                Ok(id) => {
                    let deal = state.db.list_deals(None).unwrap_or_default().into_iter().find(|d| d.id == id);
                    json_result(json!({ "deal": deal }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_move_deal" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            let patch: DealPatch = match serde_json::from_value(args.clone()) {
                Ok(v) => v,
                Err(e) => return error_result(format!("bad arguments: {e}")),
            };
            match state.db.update_deal(id, &patch, now_ts()) {
                Ok(()) => {
                    let deal = state.db.list_deals(None).unwrap_or_default().into_iter().find(|d| d.id == id);
                    json_result(json!({ "deal": deal }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_list_deals" => {
            let stage = args["stage"].as_str().map(str::to_string);
            let cid = args["customer_id"].as_i64();
            let deals = if let Some(cid) = cid {
                state.db.deals_of_customer(cid)
            } else {
                state.db.list_deals(stage.as_deref())
            };
            match deals {
                Ok(list) => json_result(json!({ "count": list.len(), "deals": list })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_delete_deal" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            match state.db.delete_deal(id) {
                Ok(()) => json_result(json!({ "ok": true, "id": id })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_add_task" => {
            let create: TaskCreate = match serde_json::from_value(args.clone()) {
                Ok(v) => v,
                Err(e) => return error_result(format!("bad arguments: {e}")),
            };
            match state.db.create_task(&create, now_ts()) {
                Ok(id) => {
                    let task = state.db.list_tasks(false, 1000).unwrap_or_default().into_iter().find(|t| t.id == id);
                    json_result(json!({ "task": task }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_complete_task" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            let done = args["done"].as_bool().unwrap_or(true);
            match state.db.set_task_done(id, done, now_ts()) {
                Ok(()) => json_result(json!({ "ok": true, "id": id, "done": done })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_list_tasks" => {
            let cid = args["customer_id"].as_i64();
            let open_only = args["open_only"].as_bool().unwrap_or(true);
            let limit = args["limit"].as_i64().unwrap_or(50);
            let tasks = if let Some(cid) = cid {
                state.db.tasks_of_customer(cid)
            } else {
                state.db.list_tasks(open_only, limit)
            };
            match tasks {
                Ok(list) => json_result(json!({ "count": list.len(), "tasks": list })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_upcoming" => {
            let days = args["days"].as_i64().unwrap_or(14);
            match state.db.upcoming(now_ts(), days) {
                Ok(v) => json_result(v),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_add_relationship" => {
            let create: RelationshipCreate = match serde_json::from_value(args.clone()) {
                Ok(v) => v,
                Err(e) => return error_result(format!("bad arguments: {e}")),
            };
            match state.db.add_relationship(&create, now_ts()) {
                Ok(id) => {
                    let rel = state.db.all_relationships().unwrap_or_default().into_iter().find(|r| r.id == id);
                    json_result(json!({ "relationship": rel }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_list_relationships" => {
            let cid = args["customer_id"].as_i64();
            let rels = if let Some(cid) = cid {
                state.db.relationships_of(cid)
            } else {
                state.db.all_relationships()
            };
            match rels {
                Ok(list) => json_result(json!({ "count": list.len(), "relationships": list })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_delete_relationship" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            match state.db.delete_relationship(id) {
                Ok(()) => json_result(json!({ "ok": true, "id": id })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_ai_path" => {
            let Some(from_id) = args["from"].as_i64() else {
                return error_result("from is required".into());
            };
            let Some(to_id) = args["to"].as_i64() else {
                return error_result("to is required".into());
            };
            let a = match state.db.get_customer(from_id) {
                Ok(Some(c)) => c,
                Ok(None) => return error_result(format!("customer {from_id} not found")),
                Err(e) => return error_result(e.to_string()),
            };
            let b = match state.db.get_customer(to_id) {
                Ok(Some(c)) => c,
                Ok(None) => return error_result(format!("customer {to_id} not found")),
                Err(e) => return error_result(e.to_string()),
            };
            let a_ctx = state.db.compact_context(from_id).unwrap_or_default();
            let b_ctx = state.db.compact_context(to_id).unwrap_or_default();
            let path_ids = state.db.find_path(from_id, to_id).ok().flatten();
            let path_names: Option<Vec<String>> = path_ids.as_ref().and_then(|ids| {
                state.db.graph_nodes().ok().map(|nodes| {
                    let by_id: std::collections::HashMap<i64, String> = nodes
                        .iter()
                        .filter_map(|n| Some((n.get("id")?.as_i64()?, n.get("name")?.as_str()?.to_string())))
                        .collect();
                    ids.iter().map(|i| by_id.get(i).cloned().unwrap_or_default()).collect()
                })
            });
            match crate::llm::path_ai(&a, &a_ctx, &b, &b_ctx, path_names.as_deref()).await {
                Ok((out, model)) => json_result(json!({
                    "from": from_id, "to": to_id, "model": model,
                    "summary": out.summary,
                    "connections": out.connections.iter().map(|c| json!({
                        "type": c.r#type, "detail": c.detail, "strength": c.strength,
                    })).collect::<Vec<_>>(),
                    "bfs_path_ids": path_ids,
                    "bfs_path_names": path_names,
                })),
                Err(e) => error_result(e),
            }
        }
        "crm_find_common" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            let focus = match state.db.get_customer(id) {
                Ok(Some(c)) => c,
                Ok(None) => return error_result(format!("customer {id} not found")),
                Err(e) => return error_result(e.to_string()),
            };
            let focus_ctx = state.db.compact_context(id).unwrap_or_default();
            let others_meta = state.db.list_customers(None, None, None, 2000).unwrap_or_default();
            let others: Vec<(i64, String, String)> = others_meta
                .into_iter()
                .filter(|c| c.id != id)
                .map(|c| {
                    let ctx = state.db.compact_context(c.id).unwrap_or_default();
                    (c.id, c.name, ctx)
                })
                .collect();
            match crate::llm::find_common_themes(id, &focus.name, &focus_ctx, &others).await {
                Ok((themes, model)) => {
                    let mut highlight = std::collections::BTreeSet::<i64>::new();
                    for t in &themes {
                        for cid in &t.customer_ids {
                            highlight.insert(*cid);
                        }
                    }
                    json_result(json!({
                        "focus_id": id,
                        "model": model,
                        "themes": themes.iter().map(|t| json!({
                            "theme": t.theme,
                            "why": t.why,
                            "customer_ids": t.customer_ids,
                        })).collect::<Vec<_>>(),
                        "highlight_ids": highlight.iter().copied().collect::<Vec<_>>(),
                    }))
                }
                Err(e) => error_result(e),
            }
        }
        "crm_similar_customers" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            let limit = args["limit"].as_i64().unwrap_or(8);
            match state.db.similar_customers(id, limit) {
                Ok(list) => {
                    let items: Vec<Value> = list
                        .into_iter()
                        .map(|(c, score, reasons)| json!({ "customer": c, "score": score, "reasons": reasons }))
                        .collect();
                    json_result(json!({ "count": items.len(), "similar": items }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_find_path" => {
            let Some(from) = args["from"].as_i64() else {
                return error_result("from is required".into());
            };
            let Some(to) = args["to"].as_i64() else {
                return error_result("to is required".into());
            };
            match state.db.find_path(from, to) {
                Ok(Some(path)) => {
                    let names = state.db.graph_nodes().ok().and_then(|nodes| {
                        let by_id: std::collections::HashMap<i64, String> = nodes
                            .iter()
                            .filter_map(|n| Some((n.get("id")?.as_i64()?, n.get("name")?.as_str()?.to_string())))
                            .collect();
                        Some(path.iter().map(|i| by_id.get(i).cloned().unwrap_or_default()).collect::<Vec<_>>())
                    }).unwrap_or_default();
                    json_result(json!({ "found": true, "hops": path.len() as i64 - 1, "path_ids": path, "path_names": names }))
                }
                Ok(None) => json_result(json!({ "found": false, "hops": -1 })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_expand_network" => {
            let Some(focus) = args["focus"].as_i64() else {
                return error_result("focus is required".into());
            };
            let hops = args["hops"].as_i64().unwrap_or(1);
            match state.db.subgraph_within(focus, hops) {
                Ok((nodes, edges)) => json_result(json!({ "focus": focus, "hops": hops, "nodes": nodes, "edges": edges })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_customer_network" => {
            let Some(cid) = args["customer_id"].as_i64() else {
                return error_result("customer_id is required".into());
            };
            let rels = state.db.relationships_of(cid).unwrap_or_default();
            let mut neighbour_ids = std::collections::BTreeSet::new();
            neighbour_ids.insert(cid);
            for r in &rels {
                neighbour_ids.insert(r.from_id);
                neighbour_ids.insert(r.to_id);
            }
            let all_nodes = state.db.graph_nodes().unwrap_or_default();
            let nodes: Vec<Value> = all_nodes
                .into_iter()
                .filter(|n| n.get("id").and_then(|v| v.as_i64()).map(|i| neighbour_ids.contains(&i)).unwrap_or(false))
                .collect();
            json_result(json!({ "focus": cid, "nodes": nodes, "edges": rels }))
        }
        "crm_search" => {
            let q = args["q"].as_str().unwrap_or("").trim();
            if q.is_empty() {
                return error_result("q is required".into());
            }
            let limit = args["limit"].as_i64().unwrap_or(20);
            match state.db.search(q, limit) {
                Ok(hits) => json_result(json!({ "q": q, "count": hits.len(), "hits": hits })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_extract_graph" => {
            let Some(cid) = args["customer_id"].as_i64() else {
                return error_result("customer_id is required".into());
            };
            let c = match state.db.get_customer(cid) {
                Ok(Some(c)) => c,
                Ok(None) => return error_result(format!("customer {cid} not found")),
                Err(e) => return error_result(e.to_string()),
            };
            let interactions = state.db.list_interactions(cid, 30).unwrap_or_default();
            let (people, model) = match crate::llm::extract_graph(&c, &interactions).await {
                Ok(v) => v,
                Err(e) => return error_result(e),
            };
            let now = now_ts();
            let all_customers = state.db.list_customers(None, None, None, 5000).unwrap_or_default();
            let mut saved = 0usize;
            let mut linked = Vec::new();
            for p in &people {
                if p.name.trim().is_empty() {
                    continue;
                }
                let name_lc = p.name.to_lowercase();
                let resolved = all_customers.iter().find(|cc| cc.name.to_lowercase() == name_lc).map(|cc| cc.id).or_else(|| {
                    let toks: Vec<String> = name_lc.split_whitespace().map(str::to_string).collect();
                    all_customers.iter().find(|cc| {
                        let n = cc.name.to_lowercase();
                        toks.iter().all(|t| n.contains(t))
                    }).map(|cc| cc.id)
                });
                let kind = if p.kind.trim().is_empty() { "contact_of" } else { p.kind.trim() };
                let role_guess = if p.role_guess.trim().is_empty() { "contact" } else { p.role_guess.trim() };
                let _ = state.db.add_mention(cid, &p.name, role_guess, kind, &p.context, p.confidence, resolved, now);
                saved += 1;
                if let Some(r) = resolved {
                    if r != cid {
                        linked.push(json!({ "name": p.name, "customer_id": r, "kind": kind }));
                    }
                }
            }
            json_result(json!({
                "model": model,
                "extracted": people.len(),
                "mentions_saved": saved,
                "linked": linked,
            }))
        }
        "crm_list_mentions" => {
            let unresolved_only = args["unresolved_only"].as_bool().unwrap_or(false);
            let limit = args["limit"].as_i64().unwrap_or(50);
            match state.db.list_mentions(unresolved_only, limit) {
                Ok(list) => json_result(json!({ "count": list.len(), "mentions": list })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_aggregate_report" => {
            let stats = match state.db.stats() {
                Ok(v) => v,
                Err(e) => return error_result(e.to_string()),
            };
            let top_deals = state.db.top_open_deals(5).unwrap_or_default();
            let top_active = state.db.top_active_customers(5).unwrap_or_default();
            let recent = state.db.recent_activity(8).unwrap_or_default();
            let upcoming = state.db.upcoming(now_ts(), 14).unwrap_or(json!({}));
            let overdue = state.db.overdue_tasks(now_ts(), 5).unwrap_or_default();
            let snap = crate::llm::ReportSnapshot {
                stats: &stats,
                top_deals: &top_deals,
                top_active_customers: &top_active,
                recent_activity: &recent,
                upcoming: &upcoming,
                overdue_tasks: &overdue,
            };
            match crate::llm::aggregate_report(&snap).await {
                Ok((text, model)) => json_result(json!({
                    "text": text,
                    "model": model,
                    "customers": stats.get("customers"),
                    "open_deals": stats.get("open_deals"),
                    "pipeline_value": stats.get("pipeline_value"),
                })),
                Err(e) => error_result(e),
            }
        }
        "crm_recent_activity" => {
            let limit = args["limit"].as_i64().unwrap_or(50);
            match state.db.recent_activity(limit) {
                Ok(items) => json_result(json!({ "count": items.len(), "items": items })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_summarize" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id is required".into());
            };
            let c = match state.db.get_customer(id) {
                Ok(Some(c)) => c,
                Ok(None) => return error_result(format!("customer {id} not found")),
                Err(e) => return error_result(e.to_string()),
            };
            let interactions = state.db.list_interactions(id, 20).unwrap_or_default();
            match crate::llm::summarize(&c, &interactions).await {
                Ok((text, model)) => json_result(json!({ "text": text, "model": model })),
                Err(e) => error_result(e),
            }
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}
