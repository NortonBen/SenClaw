//! REST for the dynamic dashboard: chart CRUD, the builder's schema, and the
//! endpoint that actually runs a spec.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::api::{bad, emit, not_found, now_ts, server, ApiError, AppState};
use crate::db_dashboard::{ChartInput, ChartPatch, Filter};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/dashboard/schema", get(schema))
        .route("/dashboard/charts", get(list_charts).post(create_chart))
        .route("/dashboard/charts/reorder", post(reorder))
        .route(
            "/dashboard/charts/:id",
            get(get_chart).patch(update_chart).delete(delete_chart),
        )
        .route("/dashboard/charts/:id/data", get(chart_data))
        .route("/dashboard/preview", post(preview))
        .route("/dashboard/values", get(field_values))
}

/// The registry the chart builder renders its dropdowns from. Serving it keeps
/// the UI from carrying a second, drifting copy of which metrics and groupings
/// each element supports.
async fn schema() -> Json<Value> {
    Json(crate::db_dashboard::schema_json())
}

/// Every chart with its data already resolved, so the dashboard paints in one
/// round-trip instead of N+1.
async fn list_charts(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let charts = s.db.list_charts().map_err(server)?;
    let out: Vec<Value> = charts
        .into_iter()
        .map(|c| {
            let data =
                s.db.run_chart(&c.element, &c.metric, &c.grouping, &c.filters);
            match data {
                Ok(d) => json!({ "chart": c, "data": d }),
                // A chart that no longer compiles renders as a card with an
                // error rather than failing the whole dashboard.
                Err(e) => json!({ "chart": c, "error": e.to_string() }),
            }
        })
        .collect();
    Ok(Json(json!({ "charts": out })))
}

async fn get_chart(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let c =
        s.db.get_chart(id)
            .map_err(server)?
            .ok_or_else(|| not_found(format!("chart {id} not found")))?;
    Ok(Json(json!({ "chart": c })))
}

async fn chart_data(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let c =
        s.db.get_chart(id)
            .map_err(server)?
            .ok_or_else(|| not_found(format!("chart {id} not found")))?;
    let d =
        s.db.run_chart(&c.element, &c.metric, &c.grouping, &c.filters)
            .map_err(bad)?;
    Ok(Json(json!({ "data": d })))
}

async fn create_chart(
    State(s): State<Arc<AppState>>,
    Json(input): Json<ChartInput>,
) -> Result<Json<Value>, ApiError> {
    // create_chart validates by compiling the spec, so a bad combination fails
    // here with a reason instead of saving a permanently broken card.
    let id = s.db.create_chart(&input, now_ts()).map_err(bad)?;
    emit(&s.events, "chart", json!({ "id": id, "action": "created" }));
    Ok(Json(
        json!({ "chart": s.db.get_chart(id).map_err(server)? }),
    ))
}

async fn update_chart(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(patch): Json<ChartPatch>,
) -> Result<Json<Value>, ApiError> {
    s.db.update_chart(id, &patch, now_ts()).map_err(bad)?;
    emit(&s.events, "chart", json!({ "id": id, "action": "updated" }));
    Ok(Json(
        json!({ "chart": s.db.get_chart(id).map_err(server)? }),
    ))
}

async fn delete_chart(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_chart(id).map_err(not_found)?;
    emit(&s.events, "chart", json!({ "id": id, "action": "deleted" }));
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ReorderInput {
    ids: Vec<i64>,
}

async fn reorder(
    State(s): State<Arc<AppState>>,
    Json(input): Json<ReorderInput>,
) -> Result<Json<Value>, ApiError> {
    s.db.reorder_charts(&input.ids, now_ts()).map_err(bad)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct PreviewInput {
    element: String,
    #[serde(default = "count")]
    metric: String,
    #[serde(default)]
    grouping: String,
    #[serde(default)]
    filters: Vec<Filter>,
}
fn count() -> String {
    "count".into()
}

/// Run an unsaved spec. This is what makes the builder live: the operator sees
/// the real numbers before committing, and an invalid combination reports why
/// rather than saving and failing later.
async fn preview(
    State(s): State<Arc<AppState>>,
    Json(input): Json<PreviewInput>,
) -> Result<Json<Value>, ApiError> {
    let d =
        s.db.run_chart(
            &input.element,
            &input.metric,
            &input.grouping,
            &input.filters,
        )
        .map_err(bad)?;
    Ok(Json(json!({ "data": d })))
}

#[derive(Deserialize)]
struct ValuesQuery {
    element: String,
    field: String,
}

/// Candidate values for a filter. Fixed vocabularies come from the registry;
/// open sets (industry, source, currency) come from the data.
async fn field_values(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ValuesQuery>,
) -> Result<Json<Value>, ApiError> {
    let vals = s.db.chart_field_values(&q.element, &q.field).map_err(bad)?;
    Ok(Json(json!({ "values": vals })))
}
