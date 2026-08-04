//! REST for organizations, the service catalogue, and deal line items.

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::api::{bad, not_found, now_ts, server, ApiError, AppState};
use crate::db_org::{OrganizationInput, OrganizationPatch, ServiceInput, ServicePatch};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/organizations", get(list_orgs).post(create_org))
        .route(
            "/organizations/:id",
            get(get_org).patch(update_org).delete(delete_org),
        )
        .route("/organizations/:id/contacts", get(org_contacts))
        .route("/organizations/:id/deals", get(org_deals))
        .route(
            "/customers/:id/organizations",
            get(customer_orgs).post(link_org),
        )
        .route("/customers/:id/organizations/:org_id", delete(unlink_org))
        .route("/services", get(list_services).post(create_service))
        .route(
            "/services/:id",
            get(get_service)
                .patch(update_service)
                .delete(delete_service),
        )
        .route(
            "/deals/:id/services",
            get(deal_services).post(attach_service),
        )
        .route("/deals/:id/services/:service_id", delete(detach_service))
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub q: Option<String>,
    pub kind: Option<String>,
    #[serde(default)]
    pub active_only: bool,
    pub limit: Option<i64>,
}

fn clamp(limit: Option<i64>, default: i64) -> i64 {
    limit.unwrap_or(default).clamp(1, 500)
}

// ---- organizations ----

async fn list_orgs(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let orgs =
        s.db.list_organizations(q.q.as_deref(), q.kind.as_deref(), clamp(q.limit, 200))
            .map_err(server)?;
    Ok(Json(json!({ "organizations": orgs })))
}

async fn get_org(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let org =
        s.db.get_organization(id)
            .map_err(server)?
            .ok_or_else(|| not_found(format!("organization {id} not found")))?;
    let contacts = s.db.contacts_of_org(id).map_err(server)?;
    let deals = s.db.deals_of_organization(id).map_err(server)?;
    Ok(Json(
        json!({ "organization": org, "contacts": contacts, "deals": deals }),
    ))
}

async fn create_org(
    State(s): State<Arc<AppState>>,
    Json(input): Json<OrganizationInput>,
) -> Result<Json<Value>, ApiError> {
    let id = s.db.create_organization(&input, now_ts()).map_err(bad)?;
    let org = s.db.get_organization(id).map_err(server)?;
    Ok(Json(json!({ "organization": org })))
}

async fn update_org(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(patch): Json<OrganizationPatch>,
) -> Result<Json<Value>, ApiError> {
    s.db.update_organization(id, &patch, now_ts())
        .map_err(bad)?;
    let org = s.db.get_organization(id).map_err(server)?;
    Ok(Json(json!({ "organization": org })))
}

async fn delete_org(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_organization(id).map_err(not_found)?;
    Ok(Json(json!({ "ok": true })))
}

async fn org_contacts(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({ "contacts": s.db.contacts_of_org(id).map_err(server)? }),
    ))
}

async fn org_deals(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({ "deals": s.db.deals_of_organization(id).map_err(server)? }),
    ))
}

// ---- person ↔ org ----

async fn customer_orgs(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({ "organizations": s.db.orgs_of_customer(id).map_err(server)? }),
    ))
}

#[derive(Deserialize)]
struct LinkInput {
    organization_id: Option<i64>,
    /// Convenience for agents and the type-ahead: name an org and it is resolved
    /// (or created) rather than making the caller do a lookup round-trip first.
    organization_name: Option<String>,
    #[serde(default)]
    role_title: String,
    #[serde(default)]
    is_primary: bool,
}

async fn link_org(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<LinkInput>,
) -> Result<Json<Value>, ApiError> {
    let now = now_ts();
    let org_id = match (input.organization_id, input.organization_name.as_deref()) {
        (Some(oid), _) => oid,
        (None, Some(name)) if !name.trim().is_empty() => {
            match s.db.find_organization_by_name(name).map_err(server)? {
                Some(oid) => oid,
                None => {
                    s.db.create_organization(
                        &OrganizationInput {
                            name: name.trim().to_string(),
                            ..Default::default()
                        },
                        now,
                    )
                    .map_err(bad)?
                }
            }
        }
        _ => return Err(bad("organization_id or organization_name is required")),
    };
    s.db.link_customer_org(id, org_id, &input.role_title, input.is_primary, now)
        .map_err(bad)?;
    Ok(Json(
        json!({ "organizations": s.db.orgs_of_customer(id).map_err(server)? }),
    ))
}

async fn unlink_org(
    State(s): State<Arc<AppState>>,
    Path((id, org_id)): Path<(i64, i64)>,
) -> Result<Json<Value>, ApiError> {
    s.db.unlink_customer_org(id, org_id, now_ts())
        .map_err(not_found)?;
    Ok(Json(
        json!({ "organizations": s.db.orgs_of_customer(id).map_err(server)? }),
    ))
}

// ---- services ----

async fn list_services(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let services =
        s.db.list_services(
            q.q.as_deref(),
            q.kind.as_deref(),
            q.active_only,
            clamp(q.limit, 200),
        )
        .map_err(server)?;
    Ok(Json(json!({ "services": services })))
}

async fn get_service(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let service =
        s.db.get_service(id)
            .map_err(server)?
            .ok_or_else(|| not_found(format!("service {id} not found")))?;
    Ok(Json(json!({ "service": service })))
}

async fn create_service(
    State(s): State<Arc<AppState>>,
    Json(input): Json<ServiceInput>,
) -> Result<Json<Value>, ApiError> {
    let id = s.db.create_service(&input, now_ts()).map_err(bad)?;
    Ok(Json(
        json!({ "service": s.db.get_service(id).map_err(server)? }),
    ))
}

async fn update_service(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(patch): Json<ServicePatch>,
) -> Result<Json<Value>, ApiError> {
    s.db.update_service(id, &patch, now_ts()).map_err(bad)?;
    Ok(Json(
        json!({ "service": s.db.get_service(id).map_err(server)? }),
    ))
}

async fn delete_service(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    // Refuses when the entry priced a real deal — surfaced as 400, not 404, so
    // the UI can show the "deactivate instead" reason verbatim.
    s.db.delete_service(id).map_err(bad)?;
    Ok(Json(json!({ "ok": true })))
}

// ---- deal line items ----

async fn deal_services(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let items = s.db.services_of_deal(id).map_err(server)?;
    let quantity = s.db.deal_service_quantity(id).map_err(server)?;
    let total: f64 = items.iter().map(|i| i.line_total).sum();
    Ok(Json(
        json!({ "services": items, "quantity": quantity, "total": total }),
    ))
}

#[derive(Deserialize)]
struct AttachInput {
    service_id: i64,
    #[serde(default)]
    quantity: Option<f64>,
    #[serde(default)]
    unit_amount: Option<f64>,
    #[serde(default)]
    note: String,
}

async fn attach_service(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<AttachInput>,
) -> Result<Json<Value>, ApiError> {
    s.db.attach_service(
        id,
        input.service_id,
        input.quantity.unwrap_or(1.0),
        input.unit_amount,
        &input.note,
        now_ts(),
    )
    .map_err(bad)?;
    let items = s.db.services_of_deal(id).map_err(server)?;
    Ok(Json(json!({ "services": items })))
}

async fn detach_service(
    State(s): State<Arc<AppState>>,
    Path((id, service_id)): Path<(i64, i64)>,
) -> Result<Json<Value>, ApiError> {
    s.db.detach_service(id, service_id, now_ts())
        .map_err(not_found)?;
    Ok(Json(
        json!({ "services": s.db.services_of_deal(id).map_err(server)? }),
    ))
}
