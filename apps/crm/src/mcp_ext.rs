//! MCP tools for the merged-in surfaces: organizations, the service catalogue,
//! the inbox, and proactive selling.
//!
//! These live outside `mcp.rs` for a mechanical reason: `tools_list()` there is a
//! single `json!` literal whose macro expansion already needs
//! `#![recursion_limit = "512"]`, and appending another twenty tools to it pushes
//! the expansion over. Two arrays concatenated cost nothing at runtime and keep
//! the limit where it is.

use serde_json::{json, Value};
use std::sync::Arc;

use crate::api::{now_ts, AppState};
use crate::mcp::{error_result, json_result};

/// Tool definitions, concatenated onto `mcp::tools_list()`.
pub fn tools_ext() -> Vec<Value> {
    let mut v = organizations();
    v.extend(services());
    v.extend(inbox());
    v.extend(sale());
    v.extend(dashboard());
    v
}

fn dashboard() -> Vec<Value> {
    vec![
        json!({
            "name": "crm_query",
            "description": "Ad-hoc analytics over the CRM — the most direct way to answer a 'how many / how much / broken down by' question without reading rows. Pick WHAT to measure (element), HOW (metric), what to SPLIT BY (grouping) and what to INCLUDE (filters); get back buckets with numbers. Use for 'doanh thu theo tổ chức', 'bao nhiêu lead mỗi giai đoạn', 'bán dịch vụ hay phần cứng nhiều hơn', 'khách mới 30 ngày qua'. Call crm_dashboard_schema first if unsure which fields an element has.",
            "inputSchema": { "type": "object", "properties": {
                "element":  { "type": "string", "enum": ["contact","organization","deal","service","task"], "description": "What is being counted — one row of this is one thing." },
                "metric":   { "type": "string", "enum": ["count","dealValue","dealQuantity"], "description": "count = how many. dealValue = summed money of related deals. dealQuantity = summed service line-item quantity. Not every element has every metric — see crm_dashboard_schema." },
                "grouping": { "type": "string", "description": "Field key to split by, e.g. 'stage', 'kind', 'role', 'organization'. Omit for a single total." },
                "filters":  { "type": "array", "description": "Each: {field, op, values[]}. ops — enum/text/relation: in|notIn|isNull|isNotNull; number/date: gt|gte|lt|lte|between|inLastDays. Dates are Unix seconds; inLastDays takes a day count.", "items": { "type": "object", "properties": {
                    "field":  { "type": "string" },
                    "op":     { "type": "string" },
                    "values": { "type": "array" }
                }, "required": ["field","op"] } }
            }, "required": ["element"] }
        }),
        json!({
            "name": "crm_dashboard_schema",
            "description": "Which elements exist, which metrics each supports, and which fields can be grouped or filtered (with their valid operators and value vocabularies). Read this before crm_query or crm_create_chart rather than guessing a field name — an unknown key is rejected, not ignored.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "crm_list_charts",
            "description": "The saved dashboard charts, each with its current numbers already computed. Use for 'dashboard đang có gì', 'xem biểu đồ'.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "crm_create_chart",
            "description": "Save a query as a chart on the dashboard. Same spec as crm_query plus a name and how to draw it. Fails with a reason if the combination is invalid — nothing broken gets saved.",
            "inputSchema": { "type": "object", "properties": {
                "name":     { "type": "string" },
                "element":  { "type": "string", "enum": ["contact","organization","deal","service","task"] },
                "metric":   { "type": "string", "enum": ["count","dealValue","dealQuantity"], "description": "Default count." },
                "grouping": { "type": "string" },
                "filters":  { "type": "array", "items": { "type": "object" } },
                "display_type": { "type": "string", "enum": ["verticalBarChart","horizontalBarChart","verticalBarChartWithLabels","horizontalBarChartWithLabels","doughnutChart","radarChart"], "description": "Default verticalBarChart." },
                "size":     { "type": "string", "enum": ["small","medium","large"], "description": "Grid width. Default medium." }
            }, "required": ["name","element"] }
        }),
        json!({
            "name": "crm_delete_chart",
            "description": "Remove a chart from the dashboard. Confirm with the user first.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        }),
    ]
}

fn organizations() -> Vec<Value> {
    vec![
        json!({
            "name": "crm_list_organizations",
            "description": "List / search organizations (accounts). Use for 'công ty nào', 'danh sách tổ chức', 'khách doanh nghiệp', 'list organizations'. Returns id, name, kind, website, domain, industry, plus contact/deal counts and open pipeline value.",
            "inputSchema": { "type": "object", "properties": {
                "q":     { "type": "string", "description": "Free-text over name/domain/industry/notes." },
                "kind":  { "type": "string", "enum": ["direct_customer","affiliated_company","partner","supplier","prospect"] },
                "limit": { "type": "number", "description": "Default 200, max 500." }
            } }
        }),
        json!({
            "name": "crm_get_organization",
            "description": "One organization by id, with its contacts and its deals. Start here for 'ai làm ở công ty X', 'công ty X có deal gì'.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        }),
        json!({
            "name": "crm_find_organization",
            "description": "Resolve an organization by exact name (case-insensitive). ALWAYS call this before crm_create_organization so the same company is not created twice.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string" }
            }, "required": ["name"] }
        }),
        json!({
            "name": "crm_create_organization",
            "description": "Create an organization. Resolve with crm_find_organization first. Use for 'thêm công ty', 'tạo tổ chức mới'.",
            "inputSchema": { "type": "object", "properties": {
                "name":     { "type": "string" },
                "kind":     { "type": "string", "enum": ["direct_customer","affiliated_company","partner","supplier","prospect"], "description": "Default direct_customer." },
                "website":  { "type": "string" },
                "domain":   { "type": "string" },
                "industry": { "type": "string" },
                "size":     { "type": "string" },
                "address":  { "type": "string" },
                "notes":    { "type": "string" },
                "tags":     { "type": "array", "items": { "type": "string" } }
            }, "required": ["name"] }
        }),
        json!({
            "name": "crm_update_organization",
            "description": "Patch an organization. Only the fields you pass change. Read it first and merge — never blind-overwrite notes or tags.",
            "inputSchema": { "type": "object", "properties": {
                "id":       { "type": "number" },
                "name":     { "type": "string" },
                "kind":     { "type": "string" },
                "website":  { "type": "string" },
                "domain":   { "type": "string" },
                "industry": { "type": "string" },
                "size":     { "type": "string" },
                "address":  { "type": "string" },
                "notes":    { "type": "string" },
                "tags":     { "type": "array", "items": { "type": "string" } }
            }, "required": ["id"] }
        }),
        json!({
            "name": "crm_delete_organization",
            "description": "Delete an organization. Its contacts and deals are UNLINKED, not deleted. Confirm with the user first.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        }),
        json!({
            "name": "crm_link_organization",
            "description": "Link a contact to an organization ('X làm ở công ty Y', 'gán khách vào công ty'). Pass organization_id, or organization_name to resolve-or-create. is_primary=true also updates the contact's company field.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id":       { "type": "number" },
                "organization_id":   { "type": "number" },
                "organization_name": { "type": "string", "description": "Used when organization_id is absent: resolves by name, creates if new." },
                "role_title":        { "type": "string", "description": "Their job title at this org." },
                "is_primary":        { "type": "boolean" }
            }, "required": ["customer_id"] }
        }),
        json!({
            "name": "crm_unlink_organization",
            "description": "Remove a contact ↔ organization link. Neither record is deleted.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id":     { "type": "number" },
                "organization_id": { "type": "number" }
            }, "required": ["customer_id", "organization_id"] }
        }),
        json!({
            "name": "crm_customer_organizations",
            "description": "Which organizations a contact belongs to, primary first.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" }
            }, "required": ["customer_id"] }
        }),
    ]
}

fn services() -> Vec<Value> {
    vec![
        json!({
            "name": "crm_list_services",
            "description": "List / search the service + hardware catalogue. Use for 'bảng giá', 'bên mình bán gì', 'danh sách dịch vụ', 'list services'. Returns name, kind, amount, currency, pricing_model and how many deals use each.",
            "inputSchema": { "type": "object", "properties": {
                "q":           { "type": "string" },
                "kind":        { "type": "string", "enum": ["service","hardware"] },
                "active_only": { "type": "boolean" },
                "limit":       { "type": "number", "description": "Default 200, max 500." }
            } }
        }),
        json!({
            "name": "crm_get_service",
            "description": "One catalogue entry by id.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        }),
        json!({
            "name": "crm_create_service",
            "description": "Add a catalogue entry ('thêm dịch vụ', 'thêm sản phẩm', 'tạo gói'). pricing_model says how the amount is charged.",
            "inputSchema": { "type": "object", "properties": {
                "name":          { "type": "string" },
                "kind":          { "type": "string", "enum": ["service","hardware"], "description": "Default service." },
                "amount":        { "type": "number" },
                "currency":      { "type": "string", "description": "Default VND." },
                "pricing_model": { "type": "string", "enum": ["fixed","hourly","daily","monthly","yearly"], "description": "Default fixed." },
                "unit":          { "type": "string" },
                "sku":           { "type": "string" },
                "description":   { "type": "string" }
            }, "required": ["name"] }
        }),
        json!({
            "name": "crm_update_service",
            "description": "Patch a catalogue entry. Set active=false to retire one without deleting it — deals already priced with it keep their line item.",
            "inputSchema": { "type": "object", "properties": {
                "id":            { "type": "number" },
                "name":          { "type": "string" },
                "kind":          { "type": "string" },
                "amount":        { "type": "number" },
                "currency":      { "type": "string" },
                "pricing_model": { "type": "string" },
                "unit":          { "type": "string" },
                "sku":           { "type": "string" },
                "description":   { "type": "string" },
                "active":        { "type": "boolean" }
            }, "required": ["id"] }
        }),
        json!({
            "name": "crm_delete_service",
            "description": "Delete a catalogue entry. FAILS if it prices any deal — deactivate it with crm_update_service {active:false} instead.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        }),
        json!({
            "name": "crm_attach_service",
            "description": "Add a catalogue entry to a deal as a line item ('thêm dịch vụ vào deal', 'deal này gồm những gì'). unit_amount defaults to the current catalogue price and is then frozen on the line. The deal's total value is recomputed from its line items.",
            "inputSchema": { "type": "object", "properties": {
                "deal_id":     { "type": "number" },
                "service_id":  { "type": "number" },
                "quantity":    { "type": "number", "description": "Default 1." },
                "unit_amount": { "type": "number", "description": "Override the catalogue price for this deal only." },
                "note":        { "type": "string" }
            }, "required": ["deal_id", "service_id"] }
        }),
        json!({
            "name": "crm_detach_service",
            "description": "Remove a line item from a deal. The deal's total is recomputed.",
            "inputSchema": { "type": "object", "properties": {
                "deal_id":    { "type": "number" },
                "service_id": { "type": "number" }
            }, "required": ["deal_id", "service_id"] }
        }),
        json!({
            "name": "crm_deal_services",
            "description": "The line items on a deal, with quantity, unit amount, line totals and the deal total.",
            "inputSchema": { "type": "object", "properties": {
                "deal_id": { "type": "number" }
            }, "required": ["deal_id"] }
        }),
        json!({
            "name": "crm_revenue_breakdown",
            "description": "Revenue analytics: deal value by organization, deal value by service kind (service vs hardware), and organization counts by type. Use for 'doanh thu theo công ty', 'bán dịch vụ hay phần cứng nhiều hơn'.",
            "inputSchema": { "type": "object", "properties": {
                "limit": { "type": "number", "description": "Top N organizations, default 20." }
            } }
        }),
    ]
}

fn inbox() -> Vec<Value> {
    vec![
        json!({
            "name": "crm_list_conversations",
            "description": "List inbox threads across every connected channel. Use for 'hộp thư', 'ai đang nhắn', 'tin chưa đọc', 'inbox'. customer_id=0 on a thread means nobody is linked to it yet.",
            "inputSchema": { "type": "object", "properties": {
                "status":      { "type": "string", "enum": ["open","snoozed","closed"] },
                "kind":        { "type": "string", "enum": ["telegram","zalo","facebook","tiktok","websocket"] },
                "customer_id": { "type": "number" },
                "q":           { "type": "string" },
                "limit":       { "type": "number", "description": "Default 100." }
            } }
        }),
        json!({
            "name": "crm_get_conversation",
            "description": "One thread with its transcript and the linked contact profile.",
            "inputSchema": { "type": "object", "properties": {
                "id":    { "type": "number" },
                "limit": { "type": "number", "description": "Max messages, default 200." }
            }, "required": ["id"] }
        }),
        json!({
            "name": "crm_link_conversation",
            "description": "Attach an unlinked thread to a contact. Also records the platform identity on that contact, so future messages from them resolve automatically.",
            "inputSchema": { "type": "object", "properties": {
                "conversation_id": { "type": "number" },
                "customer_id":     { "type": "number" }
            }, "required": ["conversation_id", "customer_id"] }
        }),
        json!({
            "name": "crm_list_inbox_channels",
            "description": "The connected channel accounts (our Telegram bot, Zalo OA, ...) with health status. Credentials are redacted.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
    ]
}

fn sale() -> Vec<Value> {
    vec![
        json!({
            "name": "sale_list_leads",
            "description": "Customers through the sales lens: stage, temperature (cold/warm/hot/churned), lead score, unsubscribe flag. Use for 'lead nóng', 'khách nào sắp chốt', 'danh sách lead'.",
            "inputSchema": { "type": "object", "properties": {
                "stage":       { "type": "string", "enum": ["new_lead","engaged","qualified","consult_scheduled","consult_done","closed_won","churned"] },
                "temperature": { "type": "string", "enum": ["cold","warm","hot","churned"] },
                "q":           { "type": "string" },
                "limit":       { "type": "number", "description": "Default 200." }
            } }
        }),
        json!({
            "name": "sale_get_lead",
            "description": "Customer 360 through the sales lens: profile + organizations + sales state + transcript + agent reasoning replay + scheduled follow-ups.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" }
            }, "required": ["customer_id"] }
        }),
        json!({
            "name": "sale_next_action",
            "description": "Run ONE proactive turn for a customer: build context from the CRM, draft a message, and push it through the guardrail. May end in sent, queued-for-review, or blocked. Use for 'chăm khách này', 'follow up khách X'.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" },
                "intent":      { "type": "string", "description": "welcome_and_value | share_value_content | soft_offer_consultation | re_engage_soft | check_in_value | winback_offer | reply_to_customer. Default share_value_content." },
                "channel":     { "type": "string", "description": "Channel kind to deliver over. Defaults to whatever identity the contact has." }
            }, "required": ["customer_id"] }
        }),
        json!({
            "name": "sale_draft_message",
            "description": "Draft a message WITHOUT sending it — for previewing wording. No guardrail decision is recorded.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" },
                "intent":      { "type": "string" }
            }, "required": ["customer_id"] }
        }),
        json!({
            "name": "sale_send",
            "description": "THE ONLY SEND PATH. Every outbound message goes through the guardrail here: unsubscribed customers are blocked outright, more than N touches in 24h queues for review, and price/contract wording queues for human review. Never try to reach a customer any other way.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" },
                "text":        { "type": "string" },
                "channel":     { "type": "string", "description": "Channel kind, e.g. telegram." },
                "is_reply":    { "type": "boolean", "description": "true when answering the customer. Replies are held to a stricter risky-wording threshold than proactive messages." }
            }, "required": ["customer_id", "text"] }
        }),
        json!({
            "name": "sale_update_stage",
            "description": "Move a customer along the nurture pipeline, and/or set temperature and lead score. Distinct from crm_move_deal, which moves one opportunity.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" },
                "stage":       { "type": "string", "enum": ["new_lead","engaged","qualified","consult_scheduled","consult_done","closed_won","churned"] },
                "temperature": { "type": "string", "enum": ["cold","warm","hot","churned"] },
                "lead_score":  { "type": "number", "description": "0..100." }
            }, "required": ["customer_id"] }
        }),
        json!({
            "name": "sale_escalate",
            "description": "Hand a case to a human: complaint, pricing demand, asked-for-a-person, hot lead, question you cannot ground in stored context. Escalate rather than guess.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" },
                "reason":      { "type": "string", "description": "complaint | pricing_request | asked_for_human | hot_lead | complex_question" },
                "draft":       { "type": "string", "description": "What you would have said, for the human to adapt." },
                "context":     { "type": "string", "description": "JSON snapshot of why." }
            }, "required": ["customer_id", "reason"] }
        }),
        json!({
            "name": "sale_list_inbox",
            "description": "The human queues: risky drafts awaiting approval, and escalated cases. Use for 'hàng chờ duyệt', 'có gì cần tôi xử lý'.",
            "inputSchema": { "type": "object", "properties": {
                "kind":   { "type": "string", "enum": ["review","escalation"] },
                "status": { "type": "string", "description": "review: pending|approved|rejected|edited|all. escalation: open|resolved|all." },
                "limit":  { "type": "number" }
            }, "required": ["kind"] }
        }),
        json!({
            "name": "sale_approve_review",
            "description": "Approve a queued draft and send it. Pass `edited` to send different words instead. The risky-wording rule is waived (a human read it) but unsubscribe and rate limits still apply.",
            "inputSchema": { "type": "object", "properties": {
                "review_id": { "type": "number" },
                "edited":    { "type": "string" },
                "by":        { "type": "string", "description": "Who approved. Default 'operator'." }
            }, "required": ["review_id"] }
        }),
        json!({
            "name": "sale_reject_review",
            "description": "Reject a queued draft. Nothing is sent.",
            "inputSchema": { "type": "object", "properties": {
                "review_id": { "type": "number" },
                "by":        { "type": "string" }
            }, "required": ["review_id"] }
        }),
        json!({
            "name": "sale_resolve_escalation",
            "description": "Mark an escalated case handled.",
            "inputSchema": { "type": "object", "properties": {
                "escalation_id": { "type": "number" },
                "by":            { "type": "string" }
            }, "required": ["escalation_id"] }
        }),
        json!({
            "name": "sale_list_sequences",
            "description": "Available follow-up sequences (welcome, nurture, re_engage, winback) and their steps.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "sale_start_sequence",
            "description": "Enrol a customer in a follow-up sequence. The scheduler drives each step; wording is generated fresh per step, not templated.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id":  { "type": "number" },
                "sequence_key": { "type": "string", "enum": ["welcome","nurture","re_engage","winback"] }
            }, "required": ["customer_id", "sequence_key"] }
        }),
        json!({
            "name": "sale_schedule_followup",
            "description": "Schedule one ad-hoc follow-up in N hours ('nhắc chăm khách này sau 3 ngày').",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" },
                "delay_hours": { "type": "number" },
                "intent":      { "type": "string" }
            }, "required": ["customer_id", "delay_hours", "intent"] }
        }),
        json!({
            "name": "sale_unsubscribe",
            "description": "Record that a customer asked to stop being contacted. This blocks every outbound message to them, permanently and without override.",
            "inputSchema": { "type": "object", "properties": {
                "customer_id": { "type": "number" },
                "on":          { "type": "boolean", "description": "Default true." }
            }, "required": ["customer_id"] }
        }),
        json!({
            "name": "sale_pipeline_report",
            "description": "Sales funnel by stage, win rate, hot leads, pending reviews, open escalations, unsubscribes and token spend.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
    ]
}

/// Dispatch for the tools above. Returns `None` when the name isn't ours, so
/// `mcp::call_tool` can fall through to its own match.
pub async fn call_tool_ext(state: &Arc<AppState>, name: &str, args: &Value) -> Option<Value> {
    let now = now_ts();
    let r = match name {
        // ---- organizations ----
        "crm_list_organizations" => {
            let q = args["q"].as_str();
            let kind = args["kind"].as_str();
            let limit = args["limit"].as_i64().unwrap_or(200).clamp(1, 500);
            match state.db.list_organizations(q, kind, limit) {
                Ok(list) => json_result(json!({ "count": list.len(), "organizations": list })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_get_organization" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match state.db.get_organization(id) {
                Ok(Some(org)) => {
                    let contacts = state.db.contacts_of_org(id).unwrap_or_default();
                    let deals = state.db.deals_of_organization(id).unwrap_or_default();
                    json_result(json!({ "organization": org, "contacts": contacts, "deals": deals }))
                }
                Ok(None) => error_result(format!("organization {id} not found")),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_find_organization" => {
            let name = args["name"].as_str().unwrap_or("");
            match state.db.find_organization_by_name(name) {
                Ok(Some(id)) => json_result(json!({ "found": true, "id": id })),
                Ok(None) => json_result(json!({ "found": false })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_create_organization" => match serde_json::from_value(args.clone()) {
            Ok(input) => match state.db.create_organization(&input, now) {
                Ok(id) => json_result(json!({ "created": true, "organization": state.db.get_organization(id).ok().flatten() })),
                Err(e) => error_result(e.to_string()),
            },
            Err(e) => error_result(format!("bad input: {e}")),
        },
        "crm_update_organization" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match serde_json::from_value(args.clone()) {
                Ok(patch) => match state.db.update_organization(id, &patch, now) {
                    Ok(()) => json_result(json!({ "updated": true, "organization": state.db.get_organization(id).ok().flatten() })),
                    Err(e) => error_result(e.to_string()),
                },
                Err(e) => error_result(format!("bad input: {e}")),
            }
        }
        "crm_delete_organization" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match state.db.delete_organization(id) {
                Ok(()) => json_result(json!({ "deleted": true })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_link_organization" => {
            let cid = args["customer_id"].as_i64().unwrap_or(0);
            let org_id = match args["organization_id"].as_i64() {
                Some(v) => Some(v),
                None => match args["organization_name"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    Some(name) => match state.db.find_organization_by_name(name) {
                        Ok(Some(id)) => Some(id),
                        Ok(None) => state
                            .db
                            .create_organization(
                                &crate::db_org::OrganizationInput {
                                    name: name.to_string(),
                                    ..Default::default()
                                },
                                now,
                            )
                            .ok(),
                        Err(_) => None,
                    },
                    None => None,
                },
            };
            match org_id {
                Some(oid) => {
                    let title = args["role_title"].as_str().unwrap_or("");
                    let primary = args["is_primary"].as_bool().unwrap_or(false);
                    match state.db.link_customer_org(cid, oid, title, primary, now) {
                        Ok(()) => json_result(json!({
                            "linked": true,
                            "organizations": state.db.orgs_of_customer(cid).unwrap_or_default()
                        })),
                        Err(e) => error_result(e.to_string()),
                    }
                }
                None => error_result("organization_id or organization_name is required".into()),
            }
        }
        "crm_unlink_organization" => {
            let cid = args["customer_id"].as_i64().unwrap_or(0);
            let oid = args["organization_id"].as_i64().unwrap_or(0);
            match state.db.unlink_customer_org(cid, oid, now) {
                Ok(()) => json_result(json!({ "unlinked": true })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_customer_organizations" => {
            let cid = args["customer_id"].as_i64().unwrap_or(0);
            match state.db.orgs_of_customer(cid) {
                Ok(list) => json_result(json!({ "organizations": list })),
                Err(e) => error_result(e.to_string()),
            }
        }

        // ---- services ----
        "crm_list_services" => {
            let q = args["q"].as_str();
            let kind = args["kind"].as_str();
            let active = args["active_only"].as_bool().unwrap_or(false);
            let limit = args["limit"].as_i64().unwrap_or(200).clamp(1, 500);
            match state.db.list_services(q, kind, active, limit) {
                Ok(list) => json_result(json!({ "count": list.len(), "services": list })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_get_service" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match state.db.get_service(id) {
                Ok(Some(s)) => json_result(json!({ "service": s })),
                Ok(None) => error_result(format!("service {id} not found")),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_create_service" => match serde_json::from_value(args.clone()) {
            Ok(input) => match state.db.create_service(&input, now) {
                Ok(id) => json_result(json!({ "created": true, "service": state.db.get_service(id).ok().flatten() })),
                Err(e) => error_result(e.to_string()),
            },
            Err(e) => error_result(format!("bad input: {e}")),
        },
        "crm_update_service" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match serde_json::from_value(args.clone()) {
                Ok(patch) => match state.db.update_service(id, &patch, now) {
                    Ok(()) => json_result(json!({ "updated": true, "service": state.db.get_service(id).ok().flatten() })),
                    Err(e) => error_result(e.to_string()),
                },
                Err(e) => error_result(format!("bad input: {e}")),
            }
        }
        "crm_delete_service" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match state.db.delete_service(id) {
                Ok(()) => json_result(json!({ "deleted": true })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_attach_service" => {
            let deal_id = args["deal_id"].as_i64().unwrap_or(0);
            let service_id = args["service_id"].as_i64().unwrap_or(0);
            let qty = args["quantity"].as_f64().unwrap_or(1.0);
            let unit = args["unit_amount"].as_f64();
            let note = args["note"].as_str().unwrap_or("");
            match state.db.attach_service(deal_id, service_id, qty, unit, note, now) {
                Ok(_) => json_result(json!({
                    "attached": true,
                    "services": state.db.services_of_deal(deal_id).unwrap_or_default()
                })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_detach_service" => {
            let deal_id = args["deal_id"].as_i64().unwrap_or(0);
            let service_id = args["service_id"].as_i64().unwrap_or(0);
            match state.db.detach_service(deal_id, service_id, now) {
                Ok(()) => json_result(json!({ "detached": true })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_deal_services" => {
            let deal_id = args["deal_id"].as_i64().unwrap_or(0);
            match state.db.services_of_deal(deal_id) {
                Ok(items) => {
                    let total: f64 = items.iter().map(|i| i.line_total).sum();
                    let quantity = state.db.deal_service_quantity(deal_id).unwrap_or(0.0);
                    json_result(json!({ "services": items, "quantity": quantity, "total": total }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_revenue_breakdown" => {
            let limit = args["limit"].as_i64().unwrap_or(20).clamp(1, 200);
            let by_org: Vec<Value> = state
                .db
                .value_by_organization(limit)
                .unwrap_or_default()
                .into_iter()
                .map(|(name, total)| json!({ "organization": name, "total": total }))
                .collect();
            let by_kind: Vec<Value> = state
                .db
                .value_by_service_kind()
                .unwrap_or_default()
                .into_iter()
                .map(|(kind, total)| json!({ "kind": kind, "total": total }))
                .collect();
            let org_kinds: Vec<Value> = state
                .db
                .org_kind_counts()
                .unwrap_or_default()
                .into_iter()
                .map(|(kind, n)| json!({ "kind": kind, "count": n }))
                .collect();
            json_result(json!({
                "byOrganization": by_org,
                "byServiceKind": by_kind,
                "organizationsByKind": org_kinds
            }))
        }

        // ---- inbox ----
        "crm_list_conversations" => {
            let limit = args["limit"].as_i64().unwrap_or(100).clamp(1, 500);
            match state.db.list_conversations(
                args["status"].as_str(),
                args["kind"].as_str(),
                args["customer_id"].as_i64(),
                args["q"].as_str(),
                limit,
            ) {
                Ok(list) => json_result(json!({ "count": list.len(), "conversations": list })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_get_conversation" => {
            let id = args["id"].as_i64().unwrap_or(0);
            let limit = args["limit"].as_i64().unwrap_or(200).clamp(1, 500);
            match state.db.get_conversation(id) {
                Ok(Some(conv)) => {
                    let messages = state.db.list_conv_messages(id, limit).unwrap_or_default();
                    let customer = if conv.customer_id != 0 {
                        state.db.get_customer(conv.customer_id).ok().flatten()
                    } else {
                        None
                    };
                    json_result(json!({ "conversation": conv, "messages": messages, "customer": customer }))
                }
                Ok(None) => error_result(format!("conversation {id} not found")),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_link_conversation" => {
            let conv_id = args["conversation_id"].as_i64().unwrap_or(0);
            let cid = args["customer_id"].as_i64().unwrap_or(0);
            match state.db.link_conversation(conv_id, cid, now) {
                Ok(()) => json_result(json!({ "linked": true })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_list_inbox_channels" => match state.db.list_channels_all() {
            Ok(list) => {
                let out: Vec<Value> = list
                    .into_iter()
                    .map(|c| {
                        json!({
                            "id": c.id, "kind": c.kind, "name": c.name,
                            "enabled": c.enabled, "last_status": c.last_status,
                            "last_error": c.last_error, "last_sync_at": c.last_sync_at,
                            "config": crate::db_inbox::redact_config(&c.config),
                        })
                    })
                    .collect();
                json_result(json!({ "channels": out }))
            }
            Err(e) => error_result(e.to_string()),
        },

        // ---- sale ----
        "sale_list_leads" => {
            let limit = args["limit"].as_i64().unwrap_or(200).clamp(1, 500);
            match state.db.list_leads(
                args["stage"].as_str(),
                args["temperature"].as_str(),
                args["q"].as_str(),
                limit,
            ) {
                Ok(list) => json_result(json!({ "count": list.len(), "leads": list })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "sale_get_lead" => {
            let cid = args["customer_id"].as_i64().unwrap_or(0);
            match state.db.sale_state(cid) {
                Ok(Some(lead)) => json_result(json!({
                    "lead": lead,
                    "customer": state.db.get_customer(cid).ok().flatten(),
                    "organizations": state.db.orgs_of_customer(cid).unwrap_or_default(),
                    "messages": state.db.recent_messages_of_customer(cid, 50).unwrap_or_default(),
                    "actions": state.db.list_actions(Some(cid), 30).unwrap_or_default(),
                    "runs": state.db.list_runs(Some(cid)).unwrap_or_default(),
                    "jobs": state.db.list_jobs(Some(cid), 20).unwrap_or_default(),
                })),
                Ok(None) => error_result(format!("customer {cid} not found")),
                Err(e) => error_result(e.to_string()),
            }
        }
        "sale_next_action" => {
            let cid = args["customer_id"].as_i64().unwrap_or(0);
            let intent = args["intent"].as_str().unwrap_or("share_value_content");
            match crate::sale::next_action(
                &state.db,
                &state.events,
                &state.channels,
                cid,
                intent,
                args["channel"].as_str(),
            )
            .await
            {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }
        "sale_draft_message" => {
            let cid = args["customer_id"].as_i64().unwrap_or(0);
            let intent = args["intent"].as_str().unwrap_or("share_value_content");
            match crate::sale::draft_message(&state.db, cid, intent).await {
                Ok(text) => json_result(json!({ "draft": text })),
                Err(e) => error_result(e),
            }
        }
        "sale_send" => {
            let cid = args["customer_id"].as_i64().unwrap_or(0);
            let text = args["text"].as_str().unwrap_or("");
            let channel = args["channel"].as_str().unwrap_or("telegram");
            let is_reply = args["is_reply"].as_bool().unwrap_or(false);
            match crate::sale::send(
                &state.db,
                &state.events,
                &state.channels,
                cid,
                channel,
                text,
                is_reply,
                false,
            )
            .await
            {
                Ok(out) => json_result(
                    json!({ "action": out.action(), "detail": out.detail(), "outcome": out }),
                ),
                Err(e) => error_result(e),
            }
        }
        "sale_update_stage" => {
            let cid = args["customer_id"].as_i64().unwrap_or(0);
            match state.db.update_sale_stage(
                cid,
                args["stage"].as_str(),
                args["temperature"].as_str(),
                args["lead_score"].as_i64(),
                now,
            ) {
                Ok(()) => json_result(json!({ "updated": true, "lead": state.db.sale_state(cid).ok().flatten() })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "sale_escalate" => {
            let cid = args["customer_id"].as_i64().unwrap_or(0);
            let reason = args["reason"].as_str().unwrap_or("complex_question");
            let draft = args["draft"].as_str().unwrap_or("");
            let context = args["context"].as_str().unwrap_or("{}");
            match state.db.create_escalation(cid, reason, context, draft, now) {
                Ok(id) => {
                    crate::api::emit(&state.events, "escalation", json!({ "id": id, "action": "created" }));
                    json_result(json!({ "escalated": true, "id": id }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "sale_list_inbox" => {
            let kind = args["kind"].as_str().unwrap_or("review");
            let limit = args["limit"].as_i64().unwrap_or(100).clamp(1, 500);
            let status = args["status"].as_str();
            let status = if status == Some("all") { None } else { status };
            if kind == "escalation" {
                let status = status.or(Some("open"));
                match state.db.list_escalations(status, limit) {
                    Ok(list) => json_result(json!({ "count": list.len(), "escalations": list })),
                    Err(e) => error_result(e.to_string()),
                }
            } else {
                let status = status.or(Some("pending"));
                match state.db.list_reviews(status, limit) {
                    Ok(list) => json_result(json!({ "count": list.len(), "reviews": list })),
                    Err(e) => error_result(e.to_string()),
                }
            }
        }
        "sale_approve_review" => {
            let id = args["review_id"].as_i64().unwrap_or(0);
            let by = args["by"].as_str().unwrap_or("operator");
            match crate::sale::approve_review(
                &state.db,
                &state.events,
                &state.channels,
                id,
                args["edited"].as_str(),
                by,
            )
            .await
            {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }
        "sale_reject_review" => {
            let id = args["review_id"].as_i64().unwrap_or(0);
            let by = args["by"].as_str().unwrap_or("operator");
            match state.db.resolve_review(id, "rejected", "", by, now) {
                Ok(()) => json_result(json!({ "rejected": true })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "sale_resolve_escalation" => {
            let id = args["escalation_id"].as_i64().unwrap_or(0);
            let by = args["by"].as_str().unwrap_or("operator");
            match state.db.resolve_escalation(id, by, now) {
                Ok(()) => json_result(json!({ "resolved": true })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "sale_list_sequences" => match state.db.list_sequences() {
            Ok(list) => json_result(json!({ "sequences": list })),
            Err(e) => error_result(e.to_string()),
        },
        "sale_start_sequence" => {
            let cid = args["customer_id"].as_i64().unwrap_or(0);
            let key = args["sequence_key"].as_str().unwrap_or("welcome");
            match crate::sale::start_sequence(&state.db, &state.events, cid, key).await {
                Ok(run_id) => json_result(json!({ "started": true, "run_id": run_id })),
                Err(e) => error_result(e),
            }
        }
        "sale_schedule_followup" => {
            let cid = args["customer_id"].as_i64().unwrap_or(0);
            let delay = args["delay_hours"].as_f64().unwrap_or(24.0);
            let intent = args["intent"].as_str().unwrap_or("share_value_content");
            let run_at = now + (delay * 3600.0) as i64;
            let payload = json!({ "intent": intent }).to_string();
            match state.db.enqueue_job(cid, "followup", run_at, &payload, now) {
                Ok(id) => json_result(json!({ "scheduled": true, "job_id": id, "run_at": run_at })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "sale_unsubscribe" => {
            let cid = args["customer_id"].as_i64().unwrap_or(0);
            let on = args["on"].as_bool().unwrap_or(true);
            match state.db.set_unsubscribed(cid, on, now) {
                Ok(()) => json_result(json!({ "unsubscribed": on })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "sale_pipeline_report" => match state.db.sale_stats() {
            Ok(v) => json_result(v),
            Err(e) => error_result(e.to_string()),
        },

        // ---- dashboard ----
        "crm_dashboard_schema" => json_result(crate::db_dashboard::schema_json()),
        "crm_query" => {
            let filters: Vec<crate::db_dashboard::Filter> =
                match serde_json::from_value(args["filters"].clone()) {
                    Ok(v) => v,
                    // Absent is fine (no filters); malformed is worth saying out
                    // loud rather than silently querying everything.
                    Err(_) if args["filters"].is_null() => vec![],
                    Err(e) => return Some(error_result(format!("bad filters: {e}"))),
                };
            let element = args["element"].as_str().unwrap_or("");
            let metric = args["metric"].as_str().unwrap_or("count");
            let grouping = args["grouping"].as_str().unwrap_or("");
            match state.db.run_chart(element, metric, grouping, &filters) {
                Ok(d) => json_result(json!(d)),
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_list_charts" => match state.db.list_charts() {
            Ok(list) => {
                let out: Vec<Value> = list
                    .into_iter()
                    .map(|c| {
                        match state.db.run_chart(&c.element, &c.metric, &c.grouping, &c.filters) {
                            Ok(d) => json!({ "chart": c, "data": d }),
                            Err(e) => json!({ "chart": c, "error": e.to_string() }),
                        }
                    })
                    .collect();
                json_result(json!({ "count": out.len(), "charts": out }))
            }
            Err(e) => error_result(e.to_string()),
        },
        "crm_create_chart" => {
            let filters: Vec<crate::db_dashboard::Filter> =
                serde_json::from_value(args["filters"].clone()).unwrap_or_default();
            let display_type = args["display_type"].as_str().unwrap_or("verticalBarChart");
            let input = crate::db_dashboard::ChartInput {
                name: args["name"].as_str().unwrap_or("").to_string(),
                element: args["element"].as_str().unwrap_or("").to_string(),
                metric: args["metric"].as_str().unwrap_or("count").to_string(),
                grouping: args["grouping"].as_str().unwrap_or("").to_string(),
                filters,
                display: json!({ "type": display_type, "showFilters": true }),
                size: args["size"].as_str().unwrap_or("medium").to_string(),
                is_template: false,
            };
            match state.db.create_chart(&input, now) {
                Ok(id) => {
                    crate::api::emit(&state.events, "chart", json!({ "id": id, "action": "created" }));
                    json_result(json!({ "created": true, "chart": state.db.get_chart(id).ok().flatten() }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "crm_delete_chart" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match state.db.delete_chart(id) {
                Ok(()) => {
                    crate::api::emit(&state.events, "chart", json!({ "id": id, "action": "deleted" }));
                    json_result(json!({ "deleted": true }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }

        _ => return None,
    };
    Some(r)
}
