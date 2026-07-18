//! Organizations (accounts) and the sellable catalogue, plus the two join tables
//! that connect them to people and deals.
//!
//! Shape, in one line each:
//!   - `organizations`          — a company. Contacts belong to it; deals are won at it.
//!   - `customer_organizations` — person ↔ org, many-to-many, one flagged primary.
//!   - `services`               — the catalogue: what we sell, at what price, on what model.
//!   - `deal_services`          — line items: which catalogue entries make up a deal.
//!
//! A deal's value is the sum of its line items when it has any, and falls back to
//! the flat `deals.amount` when it has none — so deals created before the
//! catalogue existed keep reporting the number they were created with.

use anyhow::{anyhow, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::Db;

#[derive(Serialize, Clone)]
pub struct Organization {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub website: String,
    pub domain: String,
    pub industry: String,
    pub size: String,
    pub address: String,
    pub logo_url: String,
    pub notes: String,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// Denormalized counts for the list view — cheaper than N+1 from the client.
    pub contact_count: i64,
    pub deal_count: i64,
    pub open_deal_value: f64,
}

#[derive(Deserialize, Default)]
pub struct OrganizationInput {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub website: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub industry: String,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub logo_url: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Every field optional — absent means "leave alone", which is what lets the
/// inline-edit UI PATCH a single cell without shipping the whole record.
#[derive(Deserialize, Default)]
pub struct OrganizationPatch {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub website: Option<String>,
    pub domain: Option<String>,
    pub industry: Option<String>,
    pub size: Option<String>,
    pub address: Option<String>,
    pub logo_url: Option<String>,
    pub notes: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Serialize, Clone)]
pub struct Service {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub amount: f64,
    pub currency: String,
    pub pricing_model: String,
    pub unit: String,
    pub sku: String,
    pub description: String,
    pub active: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub deal_count: i64,
}

#[derive(Deserialize, Default)]
pub struct ServiceInput {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub amount: f64,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub pricing_model: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub sku: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Deserialize, Default)]
pub struct ServicePatch {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub amount: Option<f64>,
    pub currency: Option<String>,
    pub pricing_model: Option<String>,
    pub unit: Option<String>,
    pub sku: Option<String>,
    pub description: Option<String>,
    pub active: Option<bool>,
}

/// One catalogue entry attached to one deal, with the price it was sold at.
#[derive(Serialize, Clone)]
pub struct DealService {
    pub id: i64,
    pub deal_id: i64,
    pub service_id: i64,
    pub name: String,
    pub kind: String,
    pub pricing_model: String,
    pub currency: String,
    pub quantity: f64,
    pub unit_amount: f64,
    pub line_total: f64,
    pub note: String,
    pub created_at: i64,
}

/// A person's membership of an organization, from the person's side.
#[derive(Serialize, Clone)]
pub struct OrgMembership {
    pub organization_id: i64,
    pub name: String,
    pub kind: String,
    pub logo_url: String,
    pub role_title: String,
    pub is_primary: bool,
}

/// A contact of an organization, from the org's side.
#[derive(Serialize, Clone)]
pub struct OrgContact {
    pub customer_id: i64,
    pub name: String,
    pub email: String,
    pub avatar_url: String,
    pub role: String,
    pub role_title: String,
    pub is_primary: bool,
}

fn normalize(value: &str, allowed: &[&str], fallback: &str) -> String {
    let v = value.trim().to_lowercase();
    if allowed.contains(&v.as_str()) {
        v
    } else {
        fallback.to_string()
    }
}

impl Db {
    // ---- organizations ----

    pub fn list_organizations(
        &self,
        q: Option<&str>,
        kind: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Organization>> {
        self.with(|c| {
            let like = q
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| format!("%{}%", s.to_lowercase()));
            let kind = kind.map(|s| s.trim()).filter(|s| !s.is_empty());
            let mut stmt = c.prepare(
                "SELECT o.*,
                        (SELECT COUNT(*) FROM customer_organizations m WHERE m.organization_id = o.id) AS contact_count,
                        (SELECT COUNT(*) FROM deals d WHERE d.organization_id = o.id) AS deal_count,
                        (SELECT COALESCE(SUM(d.amount), 0) FROM deals d
                          WHERE d.organization_id = o.id AND d.stage NOT IN ('won','lost','abandoned')) AS open_deal_value
                 FROM organizations o
                 WHERE (?1 IS NULL OR LOWER(o.name) LIKE ?1 OR LOWER(o.domain) LIKE ?1
                        OR LOWER(o.industry) LIKE ?1 OR LOWER(o.notes) LIKE ?1)
                   AND (?2 IS NULL OR o.kind = ?2)
                 ORDER BY o.name COLLATE NOCASE
                 LIMIT ?3",
            )?;
            let rows = stmt
                .query_map(params![like, kind, limit], Self::row_to_org)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn get_organization(&self, id: i64) -> Result<Option<Organization>> {
        self.with(|c| {
            let row = c
                .query_row(
                    "SELECT o.*,
                            (SELECT COUNT(*) FROM customer_organizations m WHERE m.organization_id = o.id) AS contact_count,
                            (SELECT COUNT(*) FROM deals d WHERE d.organization_id = o.id) AS deal_count,
                            (SELECT COALESCE(SUM(d.amount), 0) FROM deals d
                              WHERE d.organization_id = o.id AND d.stage NOT IN ('won','lost','abandoned')) AS open_deal_value
                     FROM organizations o WHERE o.id = ?1",
                    params![id],
                    Self::row_to_org,
                )
                .optional()?;
            Ok(row)
        })
    }

    /// Case-insensitive exact-name lookup. The dedupe hook for agents: resolve
    /// before creating, so "Bayer" typed twice doesn't become two accounts.
    pub fn find_organization_by_name(&self, name: &str) -> Result<Option<i64>> {
        self.with(|c| {
            let id = c
                .query_row(
                    "SELECT id FROM organizations WHERE LOWER(name) = LOWER(?1) LIMIT 1",
                    params![name.trim()],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?;
            Ok(id)
        })
    }

    pub fn create_organization(&self, input: &OrganizationInput, now: i64) -> Result<i64> {
        let name = input.name.trim();
        if name.is_empty() {
            return Err(anyhow!("organization name is required"));
        }
        let kind = normalize(&input.kind, crate::db::ORG_KINDS, "direct_customer");
        let tags = serde_json::to_string(&input.tags).unwrap_or_else(|_| "[]".into());
        let id = self.with(|c| {
            c.execute(
                "INSERT INTO organizations(name, kind, website, domain, industry, size, address,
                                           logo_url, notes, tags_json, created_at, updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
                params![
                    name,
                    kind,
                    input.website.trim(),
                    input.domain.trim().to_lowercase(),
                    input.industry.trim(),
                    input.size.trim(),
                    input.address.trim(),
                    input.logo_url.trim(),
                    input.notes,
                    tags,
                    now
                ],
            )?;
            Ok(c.last_insert_rowid())
        })?;
        let _ = self.reindex_organization(id);
        Ok(id)
    }

    pub fn update_organization(&self, id: i64, patch: &OrganizationPatch, now: i64) -> Result<()> {
        self.with(|c| {
            let exists: i64 =
                c.query_row("SELECT COUNT(*) FROM organizations WHERE id=?1", params![id], |r| {
                    r.get(0)
                })?;
            if exists == 0 {
                return Err(anyhow!("organization {id} not found"));
            }
            if let Some(v) = &patch.name {
                let v = v.trim();
                if v.is_empty() {
                    return Err(anyhow!("organization name cannot be empty"));
                }
                c.execute("UPDATE organizations SET name=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = &patch.kind {
                let v = normalize(v, crate::db::ORG_KINDS, "direct_customer");
                c.execute("UPDATE organizations SET kind=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = &patch.website {
                c.execute("UPDATE organizations SET website=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = &patch.domain {
                c.execute(
                    "UPDATE organizations SET domain=?2 WHERE id=?1",
                    params![id, v.trim().to_lowercase()],
                )?;
            }
            if let Some(v) = &patch.industry {
                c.execute("UPDATE organizations SET industry=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = &patch.size {
                c.execute("UPDATE organizations SET size=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = &patch.address {
                c.execute("UPDATE organizations SET address=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = &patch.logo_url {
                c.execute("UPDATE organizations SET logo_url=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = &patch.notes {
                c.execute("UPDATE organizations SET notes=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = &patch.tags {
                let tags = serde_json::to_string(v).unwrap_or_else(|_| "[]".into());
                c.execute("UPDATE organizations SET tags_json=?2 WHERE id=?1", params![id, tags])?;
            }
            c.execute("UPDATE organizations SET updated_at=?2 WHERE id=?1", params![id, now])?;
            Ok(())
        })?;
        let _ = self.reindex_organization(id);
        Ok(())
    }

    /// Deleting an account unlinks its people and deals rather than deleting
    /// them — the company going away doesn't mean you stop knowing the person.
    pub fn delete_organization(&self, id: i64) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM customer_organizations WHERE organization_id=?1", params![id])?;
            c.execute("UPDATE deals SET organization_id=0 WHERE organization_id=?1", params![id])?;
            c.execute(
                "DELETE FROM search_index WHERE entity_type='organization' AND entity_id=?1",
                params![id],
            )?;
            let n = c.execute("DELETE FROM organizations WHERE id=?1", params![id])?;
            if n == 0 {
                return Err(anyhow!("organization {id} not found"));
            }
            Ok(())
        })
    }

    // ---- person ↔ organization ----

    /// Link a contact to an org. Idempotent on (customer, org): re-linking just
    /// refreshes the title/primary flag instead of erroring.
    pub fn link_customer_org(
        &self,
        customer_id: i64,
        organization_id: i64,
        role_title: &str,
        is_primary: bool,
        now: i64,
    ) -> Result<()> {
        self.with(|c| {
            let ok: i64 = c.query_row(
                "SELECT COUNT(*) FROM customers WHERE id=?1",
                params![customer_id],
                |r| r.get(0),
            )?;
            if ok == 0 {
                return Err(anyhow!("customer {customer_id} not found"));
            }
            let ok: i64 = c.query_row(
                "SELECT COUNT(*) FROM organizations WHERE id=?1",
                params![organization_id],
                |r| r.get(0),
            )?;
            if ok == 0 {
                return Err(anyhow!("organization {organization_id} not found"));
            }
            if is_primary {
                c.execute(
                    "UPDATE customer_organizations SET is_primary=0 WHERE customer_id=?1",
                    params![customer_id],
                )?;
            }
            c.execute(
                "INSERT INTO customer_organizations(customer_id, organization_id, role_title, is_primary, created_at)
                 VALUES(?1,?2,?3,?4,?5)
                 ON CONFLICT(customer_id, organization_id)
                 DO UPDATE SET role_title=excluded.role_title, is_primary=excluded.is_primary",
                params![customer_id, organization_id, role_title.trim(), is_primary as i64, now],
            )?;
            // Keep the legacy free-text `company` column in step with the primary
            // org so old reads, the FTS body and CSV export stay truthful.
            if is_primary {
                c.execute(
                    "UPDATE customers SET company = (SELECT name FROM organizations WHERE id=?2), updated_at=?3
                     WHERE id=?1",
                    params![customer_id, organization_id, now],
                )?;
            }
            Ok(())
        })?;
        let _ = self.reindex_customer(customer_id);
        Ok(())
    }

    pub fn unlink_customer_org(&self, customer_id: i64, organization_id: i64, now: i64) -> Result<()> {
        self.with(|c| {
            let n = c.execute(
                "DELETE FROM customer_organizations WHERE customer_id=?1 AND organization_id=?2",
                params![customer_id, organization_id],
            )?;
            if n == 0 {
                return Err(anyhow!("link not found"));
            }
            // If we just removed the primary, promote whatever remains (if
            // anything) so `company` never points at a link that's gone.
            let remaining: Option<(i64, String)> = c
                .query_row(
                    "SELECT o.id, o.name FROM customer_organizations m
                     JOIN organizations o ON o.id = m.organization_id
                     WHERE m.customer_id=?1 ORDER BY m.is_primary DESC, m.created_at LIMIT 1",
                    params![customer_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            match remaining {
                Some((org_id, name)) => {
                    c.execute(
                        "UPDATE customer_organizations SET is_primary=1 WHERE customer_id=?1 AND organization_id=?2",
                        params![customer_id, org_id],
                    )?;
                    c.execute(
                        "UPDATE customers SET company=?2, updated_at=?3 WHERE id=?1",
                        params![customer_id, name, now],
                    )?;
                }
                None => {
                    c.execute(
                        "UPDATE customers SET company='', updated_at=?2 WHERE id=?1",
                        params![customer_id, now],
                    )?;
                }
            }
            Ok(())
        })?;
        let _ = self.reindex_customer(customer_id);
        Ok(())
    }

    pub fn orgs_of_customer(&self, customer_id: i64) -> Result<Vec<OrgMembership>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT o.id, o.name, o.kind, o.logo_url, m.role_title, m.is_primary
                 FROM customer_organizations m
                 JOIN organizations o ON o.id = m.organization_id
                 WHERE m.customer_id = ?1
                 ORDER BY m.is_primary DESC, o.name COLLATE NOCASE",
            )?;
            let rows = stmt
                .query_map(params![customer_id], |r| {
                    Ok(OrgMembership {
                        organization_id: r.get(0)?,
                        name: r.get(1)?,
                        kind: r.get(2)?,
                        logo_url: r.get(3)?,
                        role_title: r.get(4)?,
                        is_primary: r.get::<_, i64>(5)? != 0,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn contacts_of_org(&self, organization_id: i64) -> Result<Vec<OrgContact>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT c.id, c.name, c.email, c.avatar_url, c.role, m.role_title, m.is_primary
                 FROM customer_organizations m
                 JOIN customers c ON c.id = m.customer_id
                 WHERE m.organization_id = ?1
                 ORDER BY m.is_primary DESC, c.name COLLATE NOCASE",
            )?;
            let rows = stmt
                .query_map(params![organization_id], |r| {
                    Ok(OrgContact {
                        customer_id: r.get(0)?,
                        name: r.get(1)?,
                        email: r.get(2)?,
                        avatar_url: r.get(3)?,
                        role: r.get(4)?,
                        role_title: r.get(5)?,
                        is_primary: r.get::<_, i64>(6)? != 0,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// Deals booked at this organization. Shape matches `crate::db::Deal` so the
    /// UI reuses the same card component it renders on the pipeline board.
    pub fn deals_of_organization(&self, organization_id: i64) -> Result<Vec<crate::db::Deal>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT d.*, COALESCE(cu.name, '') AS customer_name,
                        COALESCE(o.name, '') AS organization_name
                 FROM deals d
                 LEFT JOIN customers cu ON cu.id = d.customer_id
                 LEFT JOIN organizations o ON o.id = d.organization_id
                 WHERE d.organization_id = ?1
                 ORDER BY d.updated_at DESC",
            )?;
            let rows = stmt
                .query_map(params![organization_id], Db::row_to_deal)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    // ---- services ----

    pub fn list_services(
        &self,
        q: Option<&str>,
        kind: Option<&str>,
        active_only: bool,
        limit: i64,
    ) -> Result<Vec<Service>> {
        self.with(|c| {
            let like = q
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| format!("%{}%", s.to_lowercase()));
            let kind = kind.map(|s| s.trim()).filter(|s| !s.is_empty());
            let mut stmt = c.prepare(
                "SELECT s.*,
                        (SELECT COUNT(*) FROM deal_services ds WHERE ds.service_id = s.id) AS deal_count
                 FROM services s
                 WHERE (?1 IS NULL OR LOWER(s.name) LIKE ?1 OR LOWER(s.sku) LIKE ?1
                        OR LOWER(s.description) LIKE ?1)
                   AND (?2 IS NULL OR s.kind = ?2)
                   AND (?3 = 0 OR s.active = 1)
                 ORDER BY s.name COLLATE NOCASE
                 LIMIT ?4",
            )?;
            let rows = stmt
                .query_map(params![like, kind, active_only as i64, limit], Self::row_to_service)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn get_service(&self, id: i64) -> Result<Option<Service>> {
        self.with(|c| {
            let row = c
                .query_row(
                    "SELECT s.*,
                            (SELECT COUNT(*) FROM deal_services ds WHERE ds.service_id = s.id) AS deal_count
                     FROM services s WHERE s.id = ?1",
                    params![id],
                    Self::row_to_service,
                )
                .optional()?;
            Ok(row)
        })
    }

    pub fn create_service(&self, input: &ServiceInput, now: i64) -> Result<i64> {
        let name = input.name.trim();
        if name.is_empty() {
            return Err(anyhow!("service name is required"));
        }
        let kind = normalize(&input.kind, crate::db::SERVICE_KINDS, "service");
        let pricing = normalize(&input.pricing_model, crate::db::PRICING_MODELS, "fixed");
        let currency = if input.currency.trim().is_empty() {
            "VND".to_string()
        } else {
            input.currency.trim().to_uppercase()
        };
        let id = self.with(|c| {
            c.execute(
                "INSERT INTO services(name, kind, amount, currency, pricing_model, unit, sku,
                                      description, active, created_at, updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,1,?9,?9)",
                params![
                    name,
                    kind,
                    input.amount,
                    currency,
                    pricing,
                    input.unit.trim(),
                    input.sku.trim(),
                    input.description,
                    now
                ],
            )?;
            Ok(c.last_insert_rowid())
        })?;
        let _ = self.reindex_service(id);
        Ok(id)
    }

    pub fn update_service(&self, id: i64, patch: &ServicePatch, now: i64) -> Result<()> {
        self.with(|c| {
            let exists: i64 =
                c.query_row("SELECT COUNT(*) FROM services WHERE id=?1", params![id], |r| r.get(0))?;
            if exists == 0 {
                return Err(anyhow!("service {id} not found"));
            }
            if let Some(v) = &patch.name {
                let v = v.trim();
                if v.is_empty() {
                    return Err(anyhow!("service name cannot be empty"));
                }
                c.execute("UPDATE services SET name=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = &patch.kind {
                let v = normalize(v, crate::db::SERVICE_KINDS, "service");
                c.execute("UPDATE services SET kind=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = patch.amount {
                c.execute("UPDATE services SET amount=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = &patch.currency {
                c.execute(
                    "UPDATE services SET currency=?2 WHERE id=?1",
                    params![id, v.trim().to_uppercase()],
                )?;
            }
            if let Some(v) = &patch.pricing_model {
                let v = normalize(v, crate::db::PRICING_MODELS, "fixed");
                c.execute("UPDATE services SET pricing_model=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = &patch.unit {
                c.execute("UPDATE services SET unit=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = &patch.sku {
                c.execute("UPDATE services SET sku=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = &patch.description {
                c.execute("UPDATE services SET description=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = patch.active {
                c.execute("UPDATE services SET active=?2 WHERE id=?1", params![id, v as i64])?;
            }
            c.execute("UPDATE services SET updated_at=?2 WHERE id=?1", params![id, now])?;
            Ok(())
        })?;
        let _ = self.reindex_service(id);
        Ok(())
    }

    /// Refuses to delete a catalogue entry that priced a real deal — that would
    /// silently rewrite history. Callers should deactivate instead.
    pub fn delete_service(&self, id: i64) -> Result<()> {
        self.with(|c| {
            let used: i64 = c.query_row(
                "SELECT COUNT(*) FROM deal_services WHERE service_id=?1",
                params![id],
                |r| r.get(0),
            )?;
            if used > 0 {
                return Err(anyhow!(
                    "service {id} is attached to {used} deal(s) — deactivate it instead of deleting"
                ));
            }
            c.execute(
                "DELETE FROM search_index WHERE entity_type='service' AND entity_id=?1",
                params![id],
            )?;
            let n = c.execute("DELETE FROM services WHERE id=?1", params![id])?;
            if n == 0 {
                return Err(anyhow!("service {id} not found"));
            }
            Ok(())
        })
    }

    // ---- deal line items ----

    /// Attach a catalogue entry to a deal. `unit_amount` defaults to the
    /// catalogue price at this moment and is then frozen on the line.
    pub fn attach_service(
        &self,
        deal_id: i64,
        service_id: i64,
        quantity: f64,
        unit_amount: Option<f64>,
        note: &str,
        now: i64,
    ) -> Result<i64> {
        let id = self.with(|c| {
            let ok: i64 =
                c.query_row("SELECT COUNT(*) FROM deals WHERE id=?1", params![deal_id], |r| {
                    r.get(0)
                })?;
            if ok == 0 {
                return Err(anyhow!("deal {deal_id} not found"));
            }
            let price: Option<f64> = c
                .query_row("SELECT amount FROM services WHERE id=?1", params![service_id], |r| {
                    r.get(0)
                })
                .optional()?;
            let price = price.ok_or_else(|| anyhow!("service {service_id} not found"))?;
            let unit = unit_amount.unwrap_or(price);
            let qty = if quantity <= 0.0 { 1.0 } else { quantity };
            c.execute(
                "INSERT INTO deal_services(deal_id, service_id, quantity, unit_amount, note, created_at)
                 VALUES(?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(deal_id, service_id)
                 DO UPDATE SET quantity=excluded.quantity, unit_amount=excluded.unit_amount, note=excluded.note",
                params![deal_id, service_id, qty, unit, note.trim(), now],
            )?;
            Ok(c.last_insert_rowid())
        })?;
        self.resync_deal_amount(deal_id, now)?;
        Ok(id)
    }

    pub fn detach_service(&self, deal_id: i64, service_id: i64, now: i64) -> Result<()> {
        self.with(|c| {
            let n = c.execute(
                "DELETE FROM deal_services WHERE deal_id=?1 AND service_id=?2",
                params![deal_id, service_id],
            )?;
            if n == 0 {
                return Err(anyhow!("line item not found"));
            }
            Ok(())
        })?;
        self.resync_deal_amount(deal_id, now)?;
        Ok(())
    }

    pub fn services_of_deal(&self, deal_id: i64) -> Result<Vec<DealService>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT ds.id, ds.deal_id, ds.service_id, s.name, s.kind, s.pricing_model, s.currency,
                        ds.quantity, ds.unit_amount, ds.note, ds.created_at
                 FROM deal_services ds
                 JOIN services s ON s.id = ds.service_id
                 WHERE ds.deal_id = ?1
                 ORDER BY ds.created_at",
            )?;
            let rows = stmt
                .query_map(params![deal_id], |r| {
                    let quantity: f64 = r.get(7)?;
                    let unit_amount: f64 = r.get(8)?;
                    Ok(DealService {
                        id: r.get(0)?,
                        deal_id: r.get(1)?,
                        service_id: r.get(2)?,
                        name: r.get(3)?,
                        kind: r.get(4)?,
                        pricing_model: r.get(5)?,
                        currency: r.get(6)?,
                        quantity,
                        unit_amount,
                        line_total: quantity * unit_amount,
                        note: r.get(9)?,
                        created_at: r.get(10)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// Recompute `deals.amount` from its line items. A deal with no lines keeps
    /// whatever amount it already had, so the flat-amount path still works.
    fn resync_deal_amount(&self, deal_id: i64, now: i64) -> Result<()> {
        self.with(|c| {
            let (n, total): (i64, f64) = c.query_row(
                "SELECT COUNT(*), COALESCE(SUM(quantity * unit_amount), 0) FROM deal_services WHERE deal_id=?1",
                params![deal_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if n > 0 {
                c.execute(
                    "UPDATE deals SET amount=?2, updated_at=?3 WHERE id=?1",
                    params![deal_id, total, now],
                )?;
            }
            Ok(())
        })
    }

    /// Total quantity of line items on a deal — the reference CRM surfaces this
    /// as "Service Quantity" on the deal card.
    pub fn deal_service_quantity(&self, deal_id: i64) -> Result<f64> {
        self.with(|c| {
            let q: f64 = c.query_row(
                "SELECT COALESCE(SUM(quantity), 0) FROM deal_services WHERE deal_id=?1",
                params![deal_id],
                |r| r.get(0),
            )?;
            Ok(q)
        })
    }

    // ---- reporting ----

    /// Deal value grouped by service kind (service vs hardware) — the
    /// "Deal Value By Type" chart. Only counts deals that aren't dead.
    pub fn value_by_service_kind(&self) -> Result<Vec<(String, f64)>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT s.kind, COALESCE(SUM(ds.quantity * ds.unit_amount), 0) AS total
                 FROM deal_services ds
                 JOIN services s ON s.id = ds.service_id
                 JOIN deals d ON d.id = ds.deal_id
                 WHERE d.stage <> 'abandoned'
                 GROUP BY s.kind ORDER BY total DESC",
            )?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// Deal value grouped by organization — the "Deal Value By Organizations"
    /// chart. Unlinked deals (organization_id=0) are excluded.
    pub fn value_by_organization(&self, limit: i64) -> Result<Vec<(String, f64)>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT o.name, COALESCE(SUM(d.amount), 0) AS total
                 FROM deals d JOIN organizations o ON o.id = d.organization_id
                 WHERE d.stage <> 'abandoned'
                 GROUP BY o.id ORDER BY total DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(params![limit], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn org_kind_counts(&self) -> Result<Vec<(String, i64)>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT kind, COUNT(*) FROM organizations GROUP BY kind ORDER BY COUNT(*) DESC",
            )?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    // ---- FTS ----

    /// Orgs and services aren't owned by a customer, so they're indexed with
    /// `customer_id = 0`. `reindex_customer` deletes by customer_id and ids
    /// start at 1, so it can never clobber these rows.
    pub(crate) fn reindex_organization(&self, id: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "DELETE FROM search_index WHERE entity_type='organization' AND entity_id=?1",
                params![id],
            )?;
            let row = c
                .query_row(
                    "SELECT name, kind, domain, industry, address, notes, tags_json
                     FROM organizations WHERE id=?1",
                    params![id],
                    |r| {
                        let name: String = r.get(0)?;
                        let body = format!(
                            "{} kind:{} {} {} {} {} {}",
                            name,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, String>(5)?,
                            r.get::<_, String>(6)?,
                        );
                        Ok((name, body))
                    },
                )
                .optional()?;
            if let Some((name, body)) = row {
                c.execute(
                    "INSERT INTO search_index(entity_type, entity_id, customer_id, title, body)
                     VALUES('organization', ?1, 0, ?2, ?3)",
                    params![id, name, body],
                )?;
            }
            Ok(())
        })
    }

    pub(crate) fn reindex_service(&self, id: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "DELETE FROM search_index WHERE entity_type='service' AND entity_id=?1",
                params![id],
            )?;
            let row = c
                .query_row(
                    "SELECT name, kind, sku, unit, pricing_model, description FROM services WHERE id=?1",
                    params![id],
                    |r| {
                        let name: String = r.get(0)?;
                        let body = format!(
                            "{} kind:{} {} {} {} {}",
                            name,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, String>(5)?,
                        );
                        Ok((name, body))
                    },
                )
                .optional()?;
            if let Some((name, body)) = row {
                c.execute(
                    "INSERT INTO search_index(entity_type, entity_id, customer_id, title, body)
                     VALUES('service', ?1, 0, ?2, ?3)",
                    params![id, name, body],
                )?;
            }
            Ok(())
        })
    }

    /// Re-index every org and service. Called from `reindex_all`.
    pub(crate) fn reindex_catalog(&self) -> Result<usize> {
        let (orgs, svcs): (Vec<i64>, Vec<i64>) = self.with(|c| {
            let mut a = c.prepare("SELECT id FROM organizations")?;
            let orgs: Vec<i64> = a.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
            let mut b = c.prepare("SELECT id FROM services")?;
            let svcs: Vec<i64> = b.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
            Ok((orgs, svcs))
        })?;
        let n = orgs.len() + svcs.len();
        for id in orgs {
            self.reindex_organization(id)?;
        }
        for id in svcs {
            self.reindex_service(id)?;
        }
        Ok(n)
    }

    // ---- row mappers ----

    fn row_to_org(r: &rusqlite::Row) -> rusqlite::Result<Organization> {
        let tags_json: String = r.get("tags_json")?;
        Ok(Organization {
            id: r.get("id")?,
            name: r.get("name")?,
            kind: r.get("kind")?,
            website: r.get("website")?,
            domain: r.get("domain")?,
            industry: r.get("industry")?,
            size: r.get("size")?,
            address: r.get("address")?,
            logo_url: r.get("logo_url")?,
            notes: r.get("notes")?,
            tags: serde_json::from_str(&tags_json).unwrap_or_default(),
            created_at: r.get("created_at")?,
            updated_at: r.get("updated_at")?,
            contact_count: r.get("contact_count")?,
            deal_count: r.get("deal_count")?,
            open_deal_value: r.get("open_deal_value")?,
        })
    }

    fn row_to_service(r: &rusqlite::Row) -> rusqlite::Result<Service> {
        Ok(Service {
            id: r.get("id")?,
            name: r.get("name")?,
            kind: r.get("kind")?,
            amount: r.get("amount")?,
            currency: r.get("currency")?,
            pricing_model: r.get("pricing_model")?,
            unit: r.get("unit")?,
            sku: r.get("sku")?,
            description: r.get("description")?,
            active: r.get::<_, i64>("active")? != 0,
            created_at: r.get("created_at")?,
            updated_at: r.get("updated_at")?,
            deal_count: r.get("deal_count")?,
        })
    }
}
