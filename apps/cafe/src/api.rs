//! REST API cho Web UI + phần lõi `*_value` dùng chung với MCP.
//! Quy ước envelope: thành công `{"ok":true,...}` / `{"<collection>":[...]}`,
//! lỗi `{"error":"..."}` — luôn HTTP 200, client tự kiểm tra `error`.

use crate::db::{Db, PurchaseLineIn, RecipeItemIn, SaleLineIn};
use crate::{calc, llm};
use app_space_sdk::SpaceClient;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub sc: SpaceClient,
    /// Fan-out phản hồi JSON-RPC của MCP cho mọi client SSE đang nối.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> AppState {
    let db = Arc::new(Db::open_default().expect("open cafe db"));
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
        .route("/ingredients", get(list_ingredients).post(add_ingredient))
        .route("/ingredients/:id", get(get_ingredient).post(update_ingredient))
        .route("/ingredients/:id/card", get(stock_card))
        .route("/stock/adjust", post(adjust_stock))
        .route("/purchases", get(list_purchases).post(create_purchase))
        .route("/purchases/:id", get(get_purchase))
        .route("/report/purchases", get(report_purchases))
        .route("/purchase-suggest", get(purchase_suggest))
        .route("/menu", get(list_menu).post(add_menu))
        .route("/menu/:id", get(get_menu).post(update_menu))
        .route("/menu/:id/recipe", post(set_recipe))
        .route("/sales", get(list_sales).post(create_sale))
        .route("/sales/:id", get(get_sale))
        .route("/sales/:id/void", post(void_sale))
        .route("/report/revenue", get(report_revenue))
        .route("/report/inventory", get(report_inventory))
        .route("/forecast/sales", get(forecast_sales))
        .route("/forecast/ingredients", get(forecast_ingredients))
        .route("/analyze", post(analyze))
        .route("/menu-suggest", post(menu_suggest))
        // MCP (HTTP + SSE), cùng khuôn với các Space App khác.
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

// ------------------------------------------------------------------ inputs

#[derive(Deserialize, Default)]
pub struct IngredientIn {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub min_stock: f64,
    #[serde(default)]
    pub note: String,
}

#[derive(Deserialize, Default)]
pub struct AdjustIn {
    #[serde(default)]
    pub ingredient_id: i64,
    pub delta: Option<f64>,
    pub set_qty: Option<f64>,
    #[serde(default)]
    pub reason: String,
}

#[derive(Deserialize)]
pub struct PLineIn {
    pub ingredient_id: i64,
    pub qty: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub unit_price: f64,
}

#[derive(Deserialize, Default)]
pub struct PurchaseIn {
    #[serde(default)]
    pub supplier: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub lines: Vec<PLineIn>,
}

#[derive(Deserialize, Default)]
pub struct MenuIn {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub price: f64,
    #[serde(default)]
    pub instructions: String,
}

#[derive(Deserialize)]
pub struct RItemIn {
    pub ingredient_id: i64,
    pub qty: f64,
}

#[derive(Deserialize, Default)]
pub struct RecipeIn {
    #[serde(default)]
    pub items: Vec<RItemIn>,
}

#[derive(Deserialize, Default)]
pub struct SLineIn {
    #[serde(default)]
    pub menu_id: i64,
    #[serde(default)]
    pub qty: f64,
    pub unit_price: Option<f64>,
}

#[derive(Deserialize, Default)]
pub struct SaleIn {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub lines: Vec<SLineIn>,
}

#[derive(Deserialize, Default)]
pub struct VoidIn {
    #[serde(default)]
    pub reason: String,
}

#[derive(Deserialize, Default)]
pub struct AnalyzeIn {
    #[serde(default)]
    pub question: String,
}

#[derive(Deserialize, Default)]
pub struct SuggestIn {
    #[serde(default)]
    pub idea: String,
    pub target_margin_pct: Option<f64>,
}

/// Query chung cho các endpoint GET (mỗi handler chỉ đọc field nó cần).
#[derive(Deserialize, Default)]
pub struct ListQuery {
    pub q: Option<String>,
    pub low_only: Option<bool>,
    pub include_inactive: Option<bool>,
    pub category: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub supplier: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub group_by: Option<String>,
    pub days: Option<i64>,
}

// ------------------------------------------------------------- core values

pub(crate) fn status_value(s: &AppState) -> Value {
    let today = calc::today();
    let d = s.db.dashboard(&today);
    json!({
        "ok": true,
        "app": "cafe",
        "menu_count": d["menu_count"],
        "ingredient_count": d["ingredient_count"],
        "today_orders": d["today"]["orders"],
        "today_revenue": d["today"]["revenue"],
        "low_stock_count": d["low_stock"].as_array().map(|a| a.len()).unwrap_or(0),
        "stock_value": d["stock_value"],
    })
}

pub(crate) fn dashboard_value(s: &AppState) -> Value {
    s.db.dashboard(&calc::today())
}

pub(crate) fn add_ingredient_value(s: &AppState, b: &IngredientIn) -> Value {
    match s.db.add_ingredient(&b.name, &b.unit, b.min_stock, &b.note) {
        Ok(id) => match s.db.get_ingredient(id) {
            Some(i) => json!({ "ok": true, "ingredient": i }),
            None => json!({ "ok": true, "id": id }),
        },
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn update_ingredient_value(s: &AppState, id: i64, patch: &Value) -> Value {
    match s.db.update_ingredient(id, patch) {
        Ok(i) => json!({ "ok": true, "ingredient": i }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn list_ingredients_value(
    s: &AppState,
    q: Option<&str>,
    low_only: bool,
    include_inactive: bool,
) -> Value {
    json!({ "ingredients": s.db.list_ingredients(q, low_only, include_inactive) })
}

pub(crate) fn get_ingredient_value(s: &AppState, id: i64) -> Value {
    match s.db.get_ingredient(id) {
        Some(i) => json!({ "ingredient": i }),
        None => json!({ "error": format!("nguyên liệu #{id} không tồn tại") }),
    }
}

pub(crate) fn adjust_stock_value(s: &AppState, b: &AdjustIn) -> Value {
    match s.db.adjust_stock(b.ingredient_id, b.delta, b.set_qty, &b.reason) {
        Ok(v) => v,
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn stock_card_value(
    s: &AppState,
    id: i64,
    from: Option<&str>,
    to: Option<&str>,
    limit: i64,
) -> Value {
    match s.db.stock_card(id, from, to, limit) {
        Ok(v) => v,
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn create_purchase_value(s: &AppState, b: &PurchaseIn) -> Value {
    let lines: Vec<PurchaseLineIn> = b
        .lines
        .iter()
        .map(|l| PurchaseLineIn {
            ingredient_id: l.ingredient_id,
            qty: l.qty,
            unit: l.unit.clone(),
            unit_price: l.unit_price,
        })
        .collect();
    match s.db.create_purchase(&b.supplier, &b.date, &b.note, &lines) {
        Ok(p) => json!({ "ok": true, "purchase": p }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn list_purchases_value(
    s: &AppState,
    from: Option<&str>,
    to: Option<&str>,
    supplier: Option<&str>,
    limit: i64,
) -> Value {
    json!({ "purchases": s.db.list_purchases(from, to, supplier, limit) })
}

pub(crate) fn get_purchase_value(s: &AppState, id: i64) -> Value {
    match s.db.get_purchase(id) {
        Some(p) => json!({ "purchase": p }),
        None => json!({ "error": format!("phiếu nhập #{id} không tồn tại") }),
    }
}

pub(crate) fn report_purchases_value(s: &AppState, from: &str, to: &str, group_by: &str) -> Value {
    s.db.report_purchases(from, to, group_by)
}

pub(crate) fn purchase_suggest_value(s: &AppState, days: i64) -> Value {
    s.db.purchase_suggest(&calc::today(), days)
}

pub(crate) fn add_menu_value(s: &AppState, b: &MenuIn) -> Value {
    match s.db.add_menu(&b.name, &b.category, b.price, &b.instructions) {
        Ok(id) => match s.db.get_menu(id) {
            Some(m) => json!({ "ok": true, "menu": m }),
            None => json!({ "ok": true, "id": id }),
        },
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn update_menu_value(s: &AppState, id: i64, patch: &Value) -> Value {
    match s.db.update_menu(id, patch) {
        Ok(m) => json!({ "ok": true, "menu": m }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn list_menu_value(
    s: &AppState,
    q: Option<&str>,
    category: Option<&str>,
    include_inactive: bool,
) -> Value {
    json!({ "menu": s.db.list_menu(q, category, include_inactive) })
}

pub(crate) fn get_menu_value(s: &AppState, id: i64) -> Value {
    match s.db.get_menu(id) {
        Some(m) => json!({ "menu": m }),
        None => json!({ "error": format!("món #{id} không tồn tại") }),
    }
}

pub(crate) fn set_recipe_value(s: &AppState, menu_id: i64, items: &[RItemIn]) -> Value {
    let items: Vec<RecipeItemIn> = items
        .iter()
        .map(|i| RecipeItemIn {
            ingredient_id: i.ingredient_id,
            qty: i.qty,
        })
        .collect();
    match s.db.set_recipe(menu_id, &items) {
        Ok(m) => json!({ "ok": true, "menu": m }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn create_sale_value(s: &AppState, b: &SaleIn) -> Value {
    let lines: Vec<SaleLineIn> = b
        .lines
        .iter()
        .map(|l| SaleLineIn {
            menu_id: l.menu_id,
            qty: l.qty,
            unit_price: l.unit_price,
        })
        .collect();
    match s.db.create_sale(&b.date, &b.note, &lines) {
        Ok(v) => json!({ "ok": true, "sale": v }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn list_sales_value(
    s: &AppState,
    from: Option<&str>,
    to: Option<&str>,
    status: Option<&str>,
    limit: i64,
) -> Value {
    json!({ "sales": s.db.list_sales(from, to, status, limit) })
}

pub(crate) fn get_sale_value(s: &AppState, id: i64) -> Value {
    match s.db.get_sale(id) {
        Some(v) => json!({ "sale": v }),
        None => json!({ "error": format!("đơn #{id} không tồn tại") }),
    }
}

pub(crate) fn void_sale_value(s: &AppState, id: i64, reason: &str) -> Value {
    match s.db.void_sale(id, reason) {
        Ok(v) => json!({ "ok": true, "sale": v }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn report_revenue_value(s: &AppState, from: &str, to: &str, group_by: &str) -> Value {
    s.db.report_revenue(from, to, group_by)
}

pub(crate) fn report_inventory_value(s: &AppState) -> Value {
    s.db.report_inventory()
}

pub(crate) fn forecast_sales_value(s: &AppState, days: i64) -> Value {
    s.db.forecast_sales(&calc::today(), days)
}

pub(crate) fn forecast_ingredients_value(s: &AppState, days: i64) -> Value {
    s.db.forecast_ingredients(&calc::today(), days)
}

pub(crate) async fn analyze_value(s: &AppState, question: &str) -> Value {
    let today = calc::today();
    let mut ctx = s.db.dashboard(&today);
    // Cho AI thêm bảng doanh thu theo món 30 ngày để nhận xét món lãi tốt/kém.
    ctx["revenue_30d_by_item"] = s.db.report_revenue(&calc::date_add(&today, -29), &today, "item");
    ctx["forecast_7d"] = s.db.forecast_sales(&today, 7);
    let (analysis, model) = llm::analyze(&s.sc, &ctx, question).await;
    json!({ "analysis": analysis, "model": model })
}

pub(crate) async fn menu_suggest_value(
    s: &AppState,
    idea: &str,
    target_margin_pct: Option<f64>,
) -> Value {
    let ctx = json!({
        "ingredients": s.db.list_ingredients(None, false, false),
        "menu": s.db.list_menu(None, None, false),
    });
    let (suggestion, model) = llm::menu_suggest(&s.sc, idea, &ctx, target_margin_pct).await;
    json!({ "suggestion": suggestion, "model": model })
}

// ----------------------------------------------------------------- handlers

async fn status(State(s): State<AppState>) -> Json<Value> {
    Json(status_value(&s))
}

async fn dashboard(State(s): State<AppState>) -> Json<Value> {
    Json(dashboard_value(&s))
}

async fn list_ingredients(State(s): State<AppState>, Query(q): Query<ListQuery>) -> Json<Value> {
    Json(list_ingredients_value(
        &s,
        q.q.as_deref(),
        q.low_only.unwrap_or(false),
        q.include_inactive.unwrap_or(false),
    ))
}

async fn add_ingredient(State(s): State<AppState>, Json(b): Json<IngredientIn>) -> Json<Value> {
    Json(add_ingredient_value(&s, &b))
}

async fn get_ingredient(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(get_ingredient_value(&s, id))
}

async fn update_ingredient(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(patch): Json<Value>,
) -> Json<Value> {
    Json(update_ingredient_value(&s, id, &patch))
}

async fn stock_card(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<ListQuery>,
) -> Json<Value> {
    Json(stock_card_value(
        &s,
        id,
        q.from.as_deref(),
        q.to.as_deref(),
        q.limit.unwrap_or(200),
    ))
}

async fn adjust_stock(State(s): State<AppState>, Json(b): Json<AdjustIn>) -> Json<Value> {
    Json(adjust_stock_value(&s, &b))
}

async fn list_purchases(State(s): State<AppState>, Query(q): Query<ListQuery>) -> Json<Value> {
    Json(list_purchases_value(
        &s,
        q.from.as_deref(),
        q.to.as_deref(),
        q.supplier.as_deref(),
        q.limit.unwrap_or(100),
    ))
}

async fn create_purchase(State(s): State<AppState>, Json(b): Json<PurchaseIn>) -> Json<Value> {
    Json(create_purchase_value(&s, &b))
}

async fn get_purchase(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(get_purchase_value(&s, id))
}

async fn report_purchases(State(s): State<AppState>, Query(q): Query<ListQuery>) -> Json<Value> {
    Json(report_purchases_value(
        &s,
        q.from.as_deref().unwrap_or(""),
        q.to.as_deref().unwrap_or(""),
        q.group_by.as_deref().unwrap_or("ingredient"),
    ))
}

async fn purchase_suggest(State(s): State<AppState>, Query(q): Query<ListQuery>) -> Json<Value> {
    Json(purchase_suggest_value(&s, q.days.unwrap_or(7)))
}

async fn list_menu(State(s): State<AppState>, Query(q): Query<ListQuery>) -> Json<Value> {
    Json(list_menu_value(
        &s,
        q.q.as_deref(),
        q.category.as_deref(),
        q.include_inactive.unwrap_or(false),
    ))
}

async fn add_menu(State(s): State<AppState>, Json(b): Json<MenuIn>) -> Json<Value> {
    Json(add_menu_value(&s, &b))
}

async fn get_menu(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(get_menu_value(&s, id))
}

async fn update_menu(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(patch): Json<Value>,
) -> Json<Value> {
    Json(update_menu_value(&s, id, &patch))
}

async fn set_recipe(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<RecipeIn>,
) -> Json<Value> {
    Json(set_recipe_value(&s, id, &b.items))
}

async fn list_sales(State(s): State<AppState>, Query(q): Query<ListQuery>) -> Json<Value> {
    Json(list_sales_value(
        &s,
        q.from.as_deref(),
        q.to.as_deref(),
        q.status.as_deref(),
        q.limit.unwrap_or(100),
    ))
}

async fn create_sale(State(s): State<AppState>, Json(b): Json<SaleIn>) -> Json<Value> {
    Json(create_sale_value(&s, &b))
}

async fn get_sale(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(get_sale_value(&s, id))
}

async fn void_sale(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    body: Option<Json<VoidIn>>,
) -> Json<Value> {
    let b = body.map(|Json(x)| x).unwrap_or_default();
    Json(void_sale_value(&s, id, &b.reason))
}

async fn report_revenue(State(s): State<AppState>, Query(q): Query<ListQuery>) -> Json<Value> {
    Json(report_revenue_value(
        &s,
        q.from.as_deref().unwrap_or(""),
        q.to.as_deref().unwrap_or(""),
        q.group_by.as_deref().unwrap_or("day"),
    ))
}

async fn report_inventory(State(s): State<AppState>) -> Json<Value> {
    Json(report_inventory_value(&s))
}

async fn forecast_sales(State(s): State<AppState>, Query(q): Query<ListQuery>) -> Json<Value> {
    Json(forecast_sales_value(&s, q.days.unwrap_or(7)))
}

async fn forecast_ingredients(
    State(s): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Json<Value> {
    Json(forecast_ingredients_value(&s, q.days.unwrap_or(7)))
}

async fn analyze(State(s): State<AppState>, body: Option<Json<AnalyzeIn>>) -> Json<Value> {
    let b = body.map(|Json(x)| x).unwrap_or_default();
    Json(analyze_value(&s, &b.question).await)
}

async fn menu_suggest(State(s): State<AppState>, body: Option<Json<SuggestIn>>) -> Json<Value> {
    let b = body.map(|Json(x)| x).unwrap_or_default();
    Json(menu_suggest_value(&s, &b.idea, b.target_margin_pct).await)
}
