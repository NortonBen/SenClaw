//! HTTP API for the Warehouse app. Every handler funnels through small
//! `*_value` helpers that the MCP server ([`crate::mcp`]) reuses, so REST and
//! agent tools always behave identically. All inventory data stays in the
//! local SQLite DB; the only outbound call is the LLM bridge for `/analyze`.

use crate::db::{Db, LineIn};
use crate::llm;
use crate::stock;
use app_space_sdk::SpaceClient;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub sc: SpaceClient,
    /// Fan-out of MCP JSON-RPC responses to any connected SSE client.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> AppState {
    let db = Arc::new(Db::open_default().expect("open warehouse db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    AppState {
        db,
        sc: SpaceClient::from_env(),
        mcp_tx,
    }
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/dashboard", get(dashboard))
        .route("/products", get(list_products).post(add_product))
        .route("/products/:id", get(get_product).post(update_product))
        .route("/warehouses", get(list_warehouses).post(add_warehouse))
        .route("/warehouses/:id", post(update_warehouse))
        .route("/partners", get(list_partners).post(add_partner))
        .route("/moves", get(list_moves).post(create_move))
        .route("/moves/:id", get(get_move))
        .route("/moves/:id/delete", post(delete_move))
        .route("/stock", get(stock_onhand))
        .route("/stock/card", get(stock_card))
        .route("/report/inout", get(report_inout))
        .route("/insight/products", get(product_insight))
        .route("/analyze", post(analyze))
        .route("/analyze/products", post(analyze_products))
        .route("/activity", get(activity))
        // MCP (HTTP + SSE), same shape as the other Space Apps.
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

// ---- status / dashboard ----

pub(crate) fn status_value(s: &AppState) -> Value {
    let products = s.db.list_products(None, None, Some("active"), false);
    let low = products.iter().filter(|p| p["low_stock"] == true).count();
    let stock_value: f64 = products
        .iter()
        .map(|p| p["stock_value"].as_f64().unwrap_or(0.0))
        .sum();
    json!({
        "ok": true,
        "app": "warehouse",
        "products_active": products.len(),
        "stock_value": stock::round2(stock_value),
        "low_stock_count": low,
    })
}

async fn status(State(s): State<AppState>) -> Json<Value> {
    Json(status_value(&s))
}

pub(crate) fn dashboard_value(s: &AppState) -> Value {
    s.db.dashboard(&stock::today())
}

async fn dashboard(State(s): State<AppState>) -> Json<Value> {
    Json(dashboard_value(&s))
}

// ---- products ----

#[derive(Deserialize, Default)]
pub(crate) struct ProductIn {
    #[serde(default)]
    pub sku: String,
    pub name: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub barcode: String,
    #[serde(default)]
    pub cost_price: f64,
    #[serde(default)]
    pub sell_price: f64,
    #[serde(default)]
    pub min_stock: f64,
    #[serde(default)]
    pub note: String,
}

pub(crate) fn add_product_value(s: &AppState, b: &ProductIn) -> Value {
    match s.db.add_product(
        &b.sku,
        &b.name,
        &b.unit,
        &b.category,
        &b.barcode,
        b.cost_price,
        b.sell_price,
        b.min_stock,
        &b.note,
    ) {
        Ok(id) => {
            s.db.log(
                "product",
                &format!("thêm sản phẩm \"{}\"", b.name.trim()),
                &id.to_string(),
            );
            match s.db.get_product(id) {
                Some(p) => json!({ "ok": true, "product": p }),
                None => json!({ "ok": true, "id": id }),
            }
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn add_product(State(s): State<AppState>, Json(b): Json<ProductIn>) -> Json<Value> {
    Json(add_product_value(&s, &b))
}

#[derive(Deserialize)]
struct ProductQuery {
    q: Option<String>,
    category: Option<String>,
    status: Option<String>,
    low_stock: Option<bool>,
}

pub(crate) fn list_products_value(
    s: &AppState,
    q: Option<&str>,
    category: Option<&str>,
    status: Option<&str>,
    low_only: bool,
) -> Value {
    json!({ "products": s.db.list_products(q, category, status, low_only) })
}

async fn list_products(State(s): State<AppState>, Query(q): Query<ProductQuery>) -> Json<Value> {
    Json(list_products_value(
        &s,
        q.q.as_deref(),
        q.category.as_deref(),
        q.status.as_deref(),
        q.low_stock.unwrap_or(false),
    ))
}

/// One product with per-kho breakdown and its recent thẻ kho rows.
pub(crate) fn get_product_value(s: &AppState, id: i64) -> Value {
    let Some(p) = s.db.get_product(id) else {
        return json!({ "error": format!("sản phẩm #{id} không tồn tại") });
    };
    json!({
        "product": p,
        "by_warehouse": s.db.stock_onhand(Some(id), None),
        "card": s.db.stock_card(id, None, 50),
    })
}

async fn get_product(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(get_product_value(&s, id))
}

pub(crate) fn update_product_value(s: &AppState, id: i64, patch: &Value) -> Value {
    match s.db.update_product(id, patch) {
        Ok(()) => {
            s.db.log(
                "product",
                &format!("cập nhật sản phẩm #{id}"),
                &id.to_string(),
            );
            match s.db.get_product(id) {
                Some(p) => json!({ "ok": true, "product": p }),
                None => json!({ "ok": true }),
            }
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn update_product(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(patch): Json<Value>,
) -> Json<Value> {
    Json(update_product_value(&s, id, &patch))
}

// ---- warehouses ----

#[derive(Deserialize)]
pub(crate) struct WarehouseIn {
    pub name: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub note: String,
}

pub(crate) fn add_warehouse_value(s: &AppState, b: &WarehouseIn) -> Value {
    match s.db.add_warehouse(&b.name, &b.location, &b.note) {
        Ok(id) => {
            s.db.log(
                "warehouse",
                &format!("thêm kho \"{}\"", b.name.trim()),
                &id.to_string(),
            );
            json!({ "ok": true, "warehouse_id": id })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn add_warehouse(State(s): State<AppState>, Json(b): Json<WarehouseIn>) -> Json<Value> {
    Json(add_warehouse_value(&s, &b))
}

#[derive(Deserialize)]
struct StatusQuery {
    status: Option<String>,
}

pub(crate) fn list_warehouses_value(s: &AppState, status: Option<&str>) -> Value {
    json!({ "warehouses": s.db.list_warehouses(status) })
}

async fn list_warehouses(State(s): State<AppState>, Query(q): Query<StatusQuery>) -> Json<Value> {
    Json(list_warehouses_value(&s, q.status.as_deref()))
}

pub(crate) fn update_warehouse_value(s: &AppState, id: i64, patch: &Value) -> Value {
    match s.db.update_warehouse(id, patch) {
        Ok(()) => {
            s.db.log("warehouse", &format!("cập nhật kho #{id}"), &id.to_string());
            json!({ "ok": true })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn update_warehouse(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(patch): Json<Value>,
) -> Json<Value> {
    Json(update_warehouse_value(&s, id, &patch))
}

// ---- partners ----

#[derive(Deserialize)]
pub(crate) struct PartnerIn {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub note: String,
}

pub(crate) fn add_partner_value(s: &AppState, b: &PartnerIn) -> Value {
    let kind = if b.kind.is_empty() {
        "supplier"
    } else {
        &b.kind
    };
    match s
        .db
        .add_partner(&b.name, kind, &b.phone, &b.address, &b.note)
    {
        Ok(id) => {
            s.db.log(
                "partner",
                &format!("thêm đối tác \"{}\" ({kind})", b.name.trim()),
                &id.to_string(),
            );
            json!({ "ok": true, "partner_id": id })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn add_partner(State(s): State<AppState>, Json(b): Json<PartnerIn>) -> Json<Value> {
    Json(add_partner_value(&s, &b))
}

#[derive(Deserialize)]
struct PartnerQuery {
    kind: Option<String>,
}

pub(crate) fn list_partners_value(s: &AppState, kind: Option<&str>) -> Value {
    json!({ "partners": s.db.list_partners(kind) })
}

async fn list_partners(State(s): State<AppState>, Query(q): Query<PartnerQuery>) -> Json<Value> {
    Json(list_partners_value(&s, q.kind.as_deref()))
}

// ---- moves ----

#[derive(Deserialize)]
pub(crate) struct MoveLineIn {
    pub product_id: i64,
    pub qty: f64,
    #[serde(default)]
    pub unit_price: f64,
}

#[derive(Deserialize)]
pub(crate) struct MoveIn {
    /// receipt | issue | transfer | adjust
    pub kind: String,
    pub warehouse_id: i64,
    #[serde(default)]
    pub to_warehouse_id: Option<i64>,
    #[serde(default)]
    pub partner_id: Option<i64>,
    #[serde(default)]
    pub move_date: String,
    #[serde(default)]
    pub note: String,
    pub lines: Vec<MoveLineIn>,
}

pub(crate) fn create_move_value(s: &AppState, b: &MoveIn) -> Value {
    let lines: Vec<LineIn> = b
        .lines
        .iter()
        .map(|l| LineIn {
            product_id: l.product_id,
            qty: l.qty,
            unit_price: l.unit_price,
        })
        .collect();
    match s.db.create_move(
        &b.kind,
        b.warehouse_id,
        b.to_warehouse_id,
        b.partner_id,
        &b.move_date,
        &b.note,
        &lines,
    ) {
        Ok(m) => {
            s.db.log(
                "move",
                &format!(
                    "tạo phiếu {} ({} dòng)",
                    m["code"].as_str().unwrap_or("?"),
                    lines.len()
                ),
                &m["code"].as_str().unwrap_or("").to_string(),
            );
            json!({ "ok": true, "move": m })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn create_move(State(s): State<AppState>, Json(b): Json<MoveIn>) -> Json<Value> {
    Json(create_move_value(&s, &b))
}

#[derive(Deserialize)]
struct MoveQuery {
    kind: Option<String>,
    warehouse_id: Option<i64>,
    product_id: Option<i64>,
    date_from: Option<String>,
    date_to: Option<String>,
    limit: Option<i64>,
}

pub(crate) fn list_moves_value(
    s: &AppState,
    kind: Option<&str>,
    warehouse_id: Option<i64>,
    product_id: Option<i64>,
    date_from: Option<&str>,
    date_to: Option<&str>,
    limit: i64,
) -> Value {
    json!({ "moves": s.db.list_moves(kind, warehouse_id, product_id, date_from, date_to, limit) })
}

async fn list_moves(State(s): State<AppState>, Query(q): Query<MoveQuery>) -> Json<Value> {
    Json(list_moves_value(
        &s,
        q.kind.as_deref(),
        q.warehouse_id,
        q.product_id,
        q.date_from.as_deref(),
        q.date_to.as_deref(),
        q.limit.unwrap_or(100),
    ))
}

pub(crate) fn get_move_value(s: &AppState, id: i64) -> Value {
    match s.db.get_move(id) {
        Some(m) => json!({ "move": m }),
        None => json!({ "error": format!("phiếu #{id} không tồn tại") }),
    }
}

async fn get_move(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(get_move_value(&s, id))
}

pub(crate) fn delete_move_value(s: &AppState, id: i64) -> Value {
    match s.db.delete_move(id) {
        Ok(v) => {
            s.db.log(
                "move",
                &format!("xoá phiếu {}", v["deleted"].as_str().unwrap_or("?")),
                &id.to_string(),
            );
            v
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn delete_move(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(delete_move_value(&s, id))
}

// ---- stock ----

#[derive(Deserialize)]
struct StockQuery {
    product_id: Option<i64>,
    warehouse_id: Option<i64>,
}

pub(crate) fn stock_onhand_value(
    s: &AppState,
    product_id: Option<i64>,
    warehouse_id: Option<i64>,
) -> Value {
    let rows = s.db.stock_onhand(product_id, warehouse_id);
    let total_value: f64 = rows
        .iter()
        .map(|r| r["value"].as_f64().unwrap_or(0.0))
        .sum();
    json!({ "stock": rows, "total_value": stock::round2(total_value) })
}

async fn stock_onhand(State(s): State<AppState>, Query(q): Query<StockQuery>) -> Json<Value> {
    Json(stock_onhand_value(&s, q.product_id, q.warehouse_id))
}

#[derive(Deserialize)]
struct CardQuery {
    product_id: i64,
    warehouse_id: Option<i64>,
    limit: Option<i64>,
}

pub(crate) fn stock_card_value(
    s: &AppState,
    product_id: i64,
    warehouse_id: Option<i64>,
    limit: i64,
) -> Value {
    if s.db.get_product(product_id).is_none() {
        return json!({ "error": format!("sản phẩm #{product_id} không tồn tại") });
    }
    json!({ "card": s.db.stock_card(product_id, warehouse_id, limit) })
}

async fn stock_card(State(s): State<AppState>, Query(q): Query<CardQuery>) -> Json<Value> {
    Json(stock_card_value(
        &s,
        q.product_id,
        q.warehouse_id,
        q.limit.unwrap_or(200),
    ))
}

// ---- reports / AI ----

#[derive(Deserialize)]
struct InoutQuery {
    months: Option<i64>,
}

pub(crate) fn report_inout_value(s: &AppState, months: i64) -> Value {
    json!({ "inout": s.db.report_inout(months) })
}

async fn report_inout(State(s): State<AppState>, Query(q): Query<InoutQuery>) -> Json<Value> {
    Json(report_inout_value(&s, q.months.unwrap_or(12)))
}

/// Hiệu suất sản phẩm + phân loại tiềm năng/bán chậm/tồn đọng (rule-based).
pub(crate) fn product_insight_value(s: &AppState, days: i64) -> Value {
    s.db.product_performance(&stock::today(), days)
}

#[derive(Deserialize)]
struct InsightQuery {
    days: Option<i64>,
}

async fn product_insight(State(s): State<AppState>, Query(q): Query<InsightQuery>) -> Json<Value> {
    Json(product_insight_value(&s, q.days.unwrap_or(90)))
}

#[derive(Deserialize, Default)]
pub(crate) struct AnalyzeIn {
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub days: Option<i64>,
}

pub(crate) async fn analyze_value(s: &AppState, question: &str) -> Value {
    let dash = dashboard_value(s);
    let (text, model) = llm::analyze(&s.sc, &dash, question).await;
    s.db.log("ai", "phân tích tồn kho", "");
    json!({ "analysis": text, "model": model })
}

async fn analyze(State(s): State<AppState>, body: Option<Json<AnalyzeIn>>) -> Json<Value> {
    let b = body.map(|Json(x)| x).unwrap_or_default();
    Json(analyze_value(&s, &b.question).await)
}

/// AI đánh giá danh mục: sản phẩm tiềm năng nên nhập thêm, hàng tồn đọng cần xử lý.
pub(crate) async fn analyze_products_value(s: &AppState, question: &str, days: i64) -> Value {
    let perf = product_insight_value(s, days);
    let (text, model) = llm::analyze_products(&s.sc, &perf, question).await;
    s.db.log("ai", "phân tích danh mục sản phẩm", "");
    json!({ "analysis": text, "model": model, "performance": perf })
}

async fn analyze_products(State(s): State<AppState>, body: Option<Json<AnalyzeIn>>) -> Json<Value> {
    let b = body.map(|Json(x)| x).unwrap_or_default();
    Json(analyze_products_value(&s, &b.question, b.days.unwrap_or(90)).await)
}

async fn activity(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "activity": s.db.recent_activity(50) }))
}
