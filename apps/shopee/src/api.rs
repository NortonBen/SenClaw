//! HTTP API for the Shopee app. Draft-first: sending a chat reply queues a
//! draft; only `POST /drafts/:id/approve` (or `autonomy=live`) actually calls
//! Shopee's `send_message`. The MCP server ([`crate::mcp`]) and the heartbeat
//! ([`crate::engine`]) reuse the same helpers so an agent can never bypass the
//! human-approval default.

use crate::db::Db;
use crate::llm;
use crate::shopee::{self, Client, Config};
use app_space_sdk::SpaceClient;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub sc: SpaceClient,
    /// Fan-out of MCP JSON-RPC responses to any connected SSE client.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> AppState {
    let db = Arc::new(Db::open_default().expect("open shopee db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    AppState {
        db,
        sc: SpaceClient::from_env(),
        mcp_tx,
    }
}

/// Assemble a signed client from the stored settings, or `None` if partner
/// credentials aren't configured yet.
pub(crate) fn client_from_settings(db: &Db) -> Option<Client> {
    let partner_id = db.get_setting("partner_id")?.parse::<i64>().ok()?;
    let partner_key = db.get_setting("partner_key")?;
    if partner_key.is_empty() {
        return None;
    }
    let shop_id = db
        .get_setting("shop_id")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let host = db
        .get_setting("host")
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| shopee::DEFAULT_HOST.into());
    Some(Client::new(Config {
        host,
        partner_id,
        partner_key,
        shop_id,
    }))
}

/// The configured shop id, or `None` if not authorized yet.
pub(crate) fn shop_id(db: &Db) -> Option<i64> {
    db.get_setting("shop_id")
        .and_then(|x| x.parse::<i64>().ok())
        .filter(|x| *x != 0)
}

/// Get a fresh access token for the configured shop, refreshing if stale.
pub(crate) async fn fresh_token(db: &Db, client: &Client, shop_id: i64) -> Result<String, String> {
    let tok = db
        .get_token(shop_id)
        .ok_or_else(|| "shop chưa authorize".to_string())?;
    if !tok.is_stale() {
        return Ok(tok.access_token);
    }
    let refreshed = client
        .refresh_token(&tok.refresh_token, shop_id)
        .await
        .map_err(|e| format!("refresh token thất bại: {e}"))?;
    db.save_token(
        shop_id,
        &refreshed.access_token,
        &refreshed.refresh_token,
        refreshed.expire_in,
    )
    .map_err(|e| e.to_string())?;
    db.log("token", "refreshed access token", &shop_id.to_string());
    Ok(refreshed.access_token)
}

/// Draft-first enqueue used by REST, MCP, and the heartbeat. Composes the reply
/// (LLM if `content` is empty), stores a pending draft, and — only in
/// `autonomy=live` — sends it immediately. Returns a JSON summary.
pub(crate) async fn enqueue_or_send(
    s: &AppState,
    conversation_id: &str,
    to_id: i64,
    to_name: &str,
    content: Option<String>,
    customer_msg: &str,
    context: &str,
    source: &str,
) -> Value {
    let (content, model) = match content {
        Some(c) if !c.trim().is_empty() => (c, String::new()),
        _ => {
            let shop = s.db.get_setting("shop_id").unwrap_or_default();
            llm::compose_reply(&s.sc, &shop, customer_msg, context).await
        }
    };
    let draft_id = match s
        .db
        .add_draft(conversation_id, to_id, to_name, &content, source, &model)
    {
        Ok(id) => id,
        Err(e) => return json!({ "error": e.to_string() }),
    };
    if s.db.get_setting("autonomy").as_deref() == Some("live") {
        return send_draft(s, draft_id).await;
    }
    json!({ "ok": true, "draft_id": draft_id, "status": "pending", "content": content })
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/settings", get(get_settings).post(set_settings))
        .route("/account", get(account))
        .route("/oauth/link", get(oauth_link))
        .route("/oauth/callback", get(oauth_callback))
        .route("/orders", get(orders))
        .route("/orders/detail", get(order_detail))
        .route("/chat/conversations", get(conversations))
        .route("/chat/reply", post(chat_reply))
        .route("/drafts", get(list_drafts))
        .route("/drafts/:id/approve", post(approve_draft))
        .route("/drafts/:id/reject", post(reject_draft))
        .route("/activity", get(activity))
        .route("/products", get(products))
        .route("/products/info", get(product_info))
        .route("/products/stock", post(update_stock_h))
        .route("/products/price", post(update_price_h))
        .route("/engine/tick", post(engine_tick))
        // MCP (HTTP + SSE), same shape as the other Space Apps.
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn status(State(s): State<AppState>) -> Json<Value> {
    Json(status_value(&s))
}

pub(crate) fn status_value(s: &AppState) -> Value {
    let connected = client_from_settings(&s.db).is_some()
        && shop_id(&s.db)
            .map(|id| s.db.get_token(id).is_some())
            .unwrap_or(false);
    json!({
        "ok": true,
        "app": "shopee",
        "connected": connected,
        "autonomy": s.db.get_setting("autonomy").unwrap_or_else(|| "draft".into()),
        "pending_drafts": s.db.list_drafts("pending").len(),
    })
}

async fn get_settings(State(s): State<AppState>) -> Json<Value> {
    Json(s.db.settings_public())
}

#[derive(Deserialize)]
struct SettingsIn {
    partner_id: Option<String>,
    partner_key: Option<String>,
    shop_id: Option<String>,
    host: Option<String>,
    autonomy: Option<String>,
}

async fn set_settings(State(s): State<AppState>, Json(body): Json<SettingsIn>) -> Json<Value> {
    if let Some(v) = body.partner_id {
        let _ = s.db.set_setting("partner_id", &v);
    }
    if let Some(v) = body.partner_key {
        if !v.is_empty() {
            let _ = s.db.set_setting("partner_key", &v);
        }
    }
    if let Some(v) = body.shop_id {
        let _ = s.db.set_setting("shop_id", &v);
    }
    if let Some(v) = body.host {
        let _ = s.db.set_setting("host", &v);
    }
    if let Some(v) = body.autonomy {
        // observe | draft | live only.
        let v = match v.as_str() {
            "observe" | "draft" | "live" => v,
            _ => "draft".into(),
        };
        let _ = s.db.set_setting("autonomy", &v);
    }
    Json(s.db.settings_public())
}

async fn account(State(s): State<AppState>) -> Json<Value> {
    let Some(client) = client_from_settings(&s.db) else {
        return Json(json!({ "error": "chưa cấu hình partner_id/partner_key" }));
    };
    let Some(sid) = shop_id(&s.db) else {
        return Json(json!({ "error": "chưa authorize shop" }));
    };
    match fresh_token(&s.db, &client, sid).await {
        Ok(tok) => match client.get_shop_info(&tok).await {
            Ok(v) => Json(json!({ "shop_id": sid, "shop": v })),
            Err(e) => Json(json!({ "error": e.to_string() })),
        },
        Err(e) => Json(json!({ "error": e })),
    }
}

#[derive(Deserialize)]
struct LinkQuery {
    redirect: String,
}

async fn oauth_link(State(s): State<AppState>, Query(q): Query<LinkQuery>) -> Json<Value> {
    match client_from_settings(&s.db) {
        Some(client) => Json(json!({ "url": client.authorize_link(&q.redirect) })),
        None => Json(json!({ "error": "chưa cấu hình partner_id/partner_key" })),
    }
}

async fn oauth_callback(
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<Value> {
    let Some(code) = q.get("code") else {
        return Json(json!({ "error": "thiếu code" }));
    };
    let Some(sid) = q.get("shop_id").and_then(|x| x.parse::<i64>().ok()) else {
        return Json(json!({ "error": "thiếu shop_id" }));
    };
    let Some(client) = client_from_settings(&s.db) else {
        return Json(json!({ "error": "chưa cấu hình partner credentials" }));
    };
    match client.token_by_code(code, sid).await {
        Ok(tr) => {
            let _ = s.db.set_setting("shop_id", &sid.to_string());
            if let Err(e) =
                s.db.save_token(sid, &tr.access_token, &tr.refresh_token, tr.expire_in)
            {
                return Json(json!({ "error": e.to_string() }));
            }
            s.db.log("oauth", "authorized shop", &sid.to_string());
            Json(json!({ "ok": true, "shop_id": sid }))
        }
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

/// Recent orders (last 14 days). Reused by the MCP tool.
pub(crate) async fn orders_value(s: &AppState) -> Value {
    let (Some(client), Some(sid)) = (client_from_settings(&s.db), shop_id(&s.db)) else {
        return json!({ "error": "chưa kết nối shop" });
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    match fresh_token(&s.db, &client, sid).await {
        Ok(tok) => match client.get_order_list(&tok, now - 14 * 86400, now).await {
            Ok(v) => v,
            Err(e) => json!({ "error": e.to_string() }),
        },
        Err(e) => json!({ "error": e }),
    }
}

async fn orders(State(s): State<AppState>) -> Json<Value> {
    Json(orders_value(&s).await)
}

/// Full detail for one or more order_sn. Reused by REST + MCP + reply grounding.
pub(crate) async fn order_detail_value(s: &AppState, order_sns: &[String]) -> Value {
    let (Some(client), Some(sid)) = (client_from_settings(&s.db), shop_id(&s.db)) else {
        return json!({ "error": "chưa kết nối shop" });
    };
    match fresh_token(&s.db, &client, sid).await {
        Ok(tok) => match client.get_order_detail(&tok, order_sns).await {
            Ok(v) => v,
            Err(e) => json!({ "error": e.to_string() }),
        },
        Err(e) => json!({ "error": e }),
    }
}

/// A compact, human-readable summary of an order for grounding an LLM reply.
/// Best-effort: returns "" if the order can't be fetched, so the caller falls
/// back to an ungrounded draft rather than failing.
pub(crate) async fn order_context(s: &AppState, order_sn: &str) -> String {
    if order_sn.trim().is_empty() {
        return String::new();
    }
    let v = order_detail_value(s, &[order_sn.to_string()]).await;
    let order = v
        .get("order_list")
        .and_then(|x| x.as_array())
        .and_then(|a| a.first());
    let Some(o) = order else {
        return String::new();
    };
    let status = o
        .get("order_status")
        .and_then(|x| x.as_str())
        .unwrap_or("?");
    let total = o.get("total_amount").map(val_str).unwrap_or_default();
    let tracking = o
        .get("tracking_number")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let items: Vec<String> = o
        .get("item_list")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|it| {
                    it.get("item_name")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    let mut ctx = format!("Đơn {order_sn}: trạng thái {status}");
    if !total.is_empty() {
        ctx.push_str(&format!(", tổng {total}"));
    }
    if !items.is_empty() {
        ctx.push_str(&format!(", sản phẩm: {}", items.join(", ")));
    }
    if !tracking.is_empty() {
        ctx.push_str(&format!(", mã vận đơn {tracking}"));
    }
    ctx
}

fn val_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

#[derive(Deserialize)]
struct OrderDetailQuery {
    sn: Option<String>,
}

async fn order_detail(State(s): State<AppState>, Query(q): Query<OrderDetailQuery>) -> Json<Value> {
    let sns: Vec<String> =
        q.sn.unwrap_or_default()
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect();
    if sns.is_empty() {
        return Json(json!({ "error": "thiếu ?sn=<order_sn,...>" }));
    }
    Json(order_detail_value(&s, &sns).await)
}

/// Buyer↔seller conversations. Reused by the MCP tool and the heartbeat.
pub(crate) async fn conversations_value(s: &AppState) -> Value {
    let (Some(client), Some(sid)) = (client_from_settings(&s.db), shop_id(&s.db)) else {
        return json!({ "error": "chưa kết nối shop" });
    };
    match fresh_token(&s.db, &client, sid).await {
        Ok(tok) => match client.get_conversation_list(&tok).await {
            Ok(v) => v,
            Err(e) => json!({ "error": e.to_string() }),
        },
        Err(e) => json!({ "error": e }),
    }
}

async fn conversations(State(s): State<AppState>) -> Json<Value> {
    Json(conversations_value(&s).await)
}

#[derive(Deserialize)]
struct ReplyIn {
    conversation_id: String,
    to_id: i64,
    to_name: Option<String>,
    content: Option<String>,
    customer_msg: Option<String>,
    context: Option<String>,
    /// If set, the order's real status/items are fetched and prepended to the
    /// context so the AI reply is grounded in the actual order.
    order_sn: Option<String>,
}

async fn chat_reply(State(s): State<AppState>, Json(body): Json<ReplyIn>) -> Json<Value> {
    let context = grounded_context(&s, body.order_sn.as_deref(), body.context.as_deref()).await;
    Json(
        enqueue_or_send(
            &s,
            &body.conversation_id,
            body.to_id,
            &body.to_name.unwrap_or_default(),
            body.content,
            &body.customer_msg.unwrap_or_default(),
            &context,
            "user",
        )
        .await,
    )
}

/// Prepend real order data (if an order_sn is given) to any caller-provided
/// context. Shared by the REST reply and the MCP draft tool.
pub(crate) async fn grounded_context(
    s: &AppState,
    order_sn: Option<&str>,
    extra: Option<&str>,
) -> String {
    let order = match order_sn {
        Some(sn) if !sn.trim().is_empty() => order_context(s, sn).await,
        _ => String::new(),
    };
    match (order.is_empty(), extra.unwrap_or("").trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => order,
        (true, false) => extra.unwrap_or("").to_string(),
        (false, false) => format!("{order}\n{}", extra.unwrap_or("")),
    }
}

async fn list_drafts(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "pending": s.db.list_drafts("pending") }))
}

async fn approve_draft(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(send_draft(&s, id).await)
}

async fn reject_draft(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    let _ = s.db.decide_draft(id, "rejected", "");
    Json(json!({ "ok": true, "status": "rejected" }))
}

/// The single publish gate: actually call Shopee `send_message` for a draft.
pub(crate) async fn send_draft(s: &AppState, draft_id: i64) -> Value {
    let Some(draft) = s.db.get_draft(draft_id) else {
        return json!({ "error": "draft không tồn tại" });
    };
    if draft.content.trim().is_empty() {
        return json!({ "error": "draft rỗng, không gửi" });
    }
    let (Some(client), Some(sid)) = (client_from_settings(&s.db), shop_id(&s.db)) else {
        return json!({ "error": "chưa kết nối shop" });
    };
    let tok = match fresh_token(&s.db, &client, sid).await {
        Ok(t) => t,
        Err(e) => return json!({ "error": e }),
    };
    match client.send_message(&tok, draft.to_id, &draft.content).await {
        Ok(_) => {
            let _ = s.db.decide_draft(draft_id, "sent", "");
            s.db.log(
                "chat",
                &format!("đã gửi trả lời khách {}", draft.to_name),
                &draft.conversation_id,
            );
            json!({ "ok": true, "draft_id": draft_id, "status": "sent" })
        }
        Err(e) => {
            let _ = s.db.decide_draft(draft_id, "error", &e.to_string());
            json!({ "error": e.to_string(), "draft_id": draft_id })
        }
    }
}

async fn activity(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "activity": s.db.recent_activity(50) }))
}

/// Run a read-only product call with a fresh token, shared by REST + MCP.
async fn with_token<F, Fut>(s: &AppState, f: F) -> Value
where
    F: FnOnce(Client, String) -> Fut,
    Fut: std::future::Future<Output = Result<Value, String>>,
{
    let (Some(client), Some(sid)) = (client_from_settings(&s.db), shop_id(&s.db)) else {
        return json!({ "error": "chưa kết nối shop" });
    };
    match fresh_token(&s.db, &client, sid).await {
        Ok(tok) => match f(client, tok).await {
            Ok(v) => v,
            Err(e) => json!({ "error": e }),
        },
        Err(e) => json!({ "error": e }),
    }
}

pub(crate) async fn products_value(s: &AppState, status: &str) -> Value {
    let status = status.to_string();
    with_token(s, |client, tok| async move {
        client
            .get_item_list(&tok, 0, 50, &status)
            .await
            .map_err(|e| e.to_string())
    })
    .await
}

pub(crate) async fn product_info_value(s: &AppState, ids: &[i64]) -> Value {
    let ids = ids.to_vec();
    with_token(s, |client, tok| async move {
        client
            .get_item_base_info(&tok, &ids)
            .await
            .map_err(|e| e.to_string())
    })
    .await
}

pub(crate) async fn update_stock_value(s: &AppState, item_id: i64, stock: i64) -> Value {
    let v = with_token(s, |client, tok| async move {
        client
            .update_stock(&tok, item_id, stock)
            .await
            .map_err(|e| e.to_string())
    })
    .await;
    if v.get("error").is_none() {
        s.db.log(
            "product",
            &format!("cập nhật tồn kho item {item_id} = {stock}"),
            &item_id.to_string(),
        );
    }
    v
}

pub(crate) async fn update_price_value(s: &AppState, item_id: i64, price: f64) -> Value {
    let v = with_token(s, |client, tok| async move {
        client
            .update_price(&tok, item_id, price)
            .await
            .map_err(|e| e.to_string())
    })
    .await;
    if v.get("error").is_none() {
        s.db.log(
            "product",
            &format!("cập nhật giá item {item_id} = {price}"),
            &item_id.to_string(),
        );
    }
    v
}

#[derive(Deserialize)]
struct ProductQuery {
    status: Option<String>,
    ids: Option<String>,
}

async fn products(State(s): State<AppState>, Query(q): Query<ProductQuery>) -> Json<Value> {
    Json(products_value(&s, q.status.as_deref().unwrap_or("NORMAL")).await)
}

async fn product_info(State(s): State<AppState>, Query(q): Query<ProductQuery>) -> Json<Value> {
    let ids: Vec<i64> = q
        .ids
        .unwrap_or_default()
        .split(',')
        .filter_map(|x| x.trim().parse().ok())
        .collect();
    if ids.is_empty() {
        return Json(json!({ "error": "thiếu ?ids=<item_id,...>" }));
    }
    Json(product_info_value(&s, &ids).await)
}

#[derive(Deserialize)]
struct StockIn {
    item_id: i64,
    stock: i64,
}

async fn update_stock_h(State(s): State<AppState>, Json(b): Json<StockIn>) -> Json<Value> {
    Json(update_stock_value(&s, b.item_id, b.stock).await)
}

#[derive(Deserialize)]
struct PriceIn {
    item_id: i64,
    price: f64,
}

async fn update_price_h(State(s): State<AppState>, Json(b): Json<PriceIn>) -> Json<Value> {
    Json(update_price_value(&s, b.item_id, b.price).await)
}

async fn engine_tick(State(s): State<AppState>) -> Json<Value> {
    Json(crate::engine::tick(&s).await)
}
