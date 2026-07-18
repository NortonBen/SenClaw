//! Dynamic dashboard: user-defined charts over CRM data.
//!
//! Modelled on the reference CRM's chart builder. One chart is a small query
//! spec — `element` (what to count), `metric` (how to measure), `grouping` (what
//! to split by), `filters` (what to include) and `display` (how to draw it) —
//! which this module compiles into SQL.
//!
//! ## Why a registry rather than free-form SQL
//!
//! The spec arrives from a browser, so nothing in it may reach SQL as text.
//! Every element, metric, field and operator is looked up in the static
//! [`ELEMENTS`] registry by exact key; an unknown key is an error, not a
//! passthrough. The only SQL fragments ever concatenated are `&'static str`
//! literals written here. Every value the user supplies is bound as a parameter.
//! That is the whole injection story: user text picks *which* literal, never
//! *what* the literal says.
//!
//! ## Fan-out
//!
//! Metrics that reach across a join (a contact's deal value) would double-count
//! under `COUNT(*)`, so counts are always `COUNT(DISTINCT <pk>)`, and a join is
//! only added when some part of the spec actually needs it.

use anyhow::{anyhow, Result};
use rusqlite::types::ToSqlOutput;
use rusqlite::ToSql;
use serde::{Deserialize, Serialize};

use crate::db::Db;

// ---- registry ----

#[derive(Clone, Copy, PartialEq)]
pub enum FieldKind {
    /// Fixed vocabulary — groupable, filtered with in/notIn/isNull/isNotNull.
    Enum,
    /// Free text that still makes a sensible bucket (industry, source).
    Text,
    Number,
    Date,
    Bool,
    /// A related record's display name.
    Relation,
}

impl FieldKind {
    fn as_str(&self) -> &'static str {
        match self {
            FieldKind::Enum => "enum",
            FieldKind::Text => "text",
            FieldKind::Number => "number",
            FieldKind::Date => "date",
            FieldKind::Bool => "bool",
            FieldKind::Relation => "relation",
        }
    }

    /// Operators the UI may offer. Mirrors the reference's taxonomy.
    fn operators(&self) -> &'static [&'static str] {
        match self {
            FieldKind::Enum | FieldKind::Text | FieldKind::Relation => {
                &["in", "notIn", "isNull", "isNotNull"]
            }
            FieldKind::Bool => &["in"],
            FieldKind::Number | FieldKind::Date => {
                &["gt", "gte", "lt", "lte", "between", "inLastDays"]
            }
        }
    }
}

pub struct FieldDef {
    pub key: &'static str,
    pub kind: FieldKind,
    /// SQL expression. A `&'static str` written in this file — never user input.
    pub sql: &'static str,
    /// Join required to make `sql` resolvable, if any. Deduped across the query.
    pub join: Option<&'static str>,
    pub groupable: bool,
    /// Fixed vocabulary, for the UI's value picker. Empty = open set (the API
    /// serves distinct values from the data instead).
    pub values: &'static [&'static str],
}

pub struct MetricDef {
    pub key: &'static str,
    pub sql: &'static str,
    pub join: Option<&'static str>,
    /// Money aggregates render with a currency; counts don't.
    pub is_money: bool,
    /// Column holding the currency of each row this metric sums, when it sums
    /// money. `SUM(amount)` silently adds EUR to VND, so the engine reports
    /// which currencies actually contributed and the card can refuse to imply
    /// one. There is no FX table here, so detecting the mix is the honest
    /// ceiling — converting it would mean inventing a rate.
    pub currency_sql: Option<&'static str>,
}

pub struct ElementDef {
    pub key: &'static str,
    pub from: &'static str,
    pub metrics: &'static [MetricDef],
    pub fields: &'static [FieldDef],
}

const ROLES: &[&str] = &[
    "lead", "prospect", "customer", "vip", "contact", "partner", "referrer", "supplier",
    "investor", "employee", "former", "paused", "lost",
];
const DEAL_STAGES: &[&str] = &["qualifying", "proposal", "negotiation", "won", "lost"];
const TASK_STATUS: &[&str] = &["open", "done"];
const BOOL_VALUES: &[&str] = &["0", "1"];

/// Primary organization of a contact. Two hops, and `is_primary = 1` keeps it
/// 1:1 — without that predicate a contact in three orgs would triple its own
/// count.
const JOIN_PRIMARY_ORG: &str = "LEFT JOIN customer_organizations mo ON mo.customer_id = c.id AND mo.is_primary = 1 \
     LEFT JOIN organizations o ON o.id = mo.organization_id";

pub static ELEMENTS: &[ElementDef] = &[
    ElementDef {
        key: "contact",
        from: "customers c",
        metrics: &[
            MetricDef { key: "count", sql: "COUNT(DISTINCT c.id)", join: None, is_money: false, currency_sql: None },
            MetricDef {
                key: "dealValue",
                sql: "COALESCE(SUM(dv.amount), 0)",
                join: Some("LEFT JOIN deals dv ON dv.customer_id = c.id"),
                is_money: true,
                currency_sql: Some("dv.currency"),
            },
        ],
        fields: &[
            FieldDef { key: "role", kind: FieldKind::Enum, sql: "c.role", join: None, groupable: true, values: ROLES },
            FieldDef { key: "sale_stage", kind: FieldKind::Enum, sql: "c.sale_stage", join: None, groupable: true, values: crate::db::SALE_STAGES },
            FieldDef { key: "temperature", kind: FieldKind::Enum, sql: "c.temperature", join: None, groupable: true, values: crate::db::TEMPERATURES },
            FieldDef { key: "source", kind: FieldKind::Text, sql: "c.source", join: None, groupable: true, values: &[] },
            FieldDef { key: "unsubscribed", kind: FieldKind::Bool, sql: "c.unsubscribed", join: None, groupable: true, values: BOOL_VALUES },
            FieldDef { key: "organization", kind: FieldKind::Relation, sql: "COALESCE(o.name, '')", join: Some(JOIN_PRIMARY_ORG), groupable: true, values: &[] },
            FieldDef { key: "lead_score", kind: FieldKind::Number, sql: "c.lead_score", join: None, groupable: false, values: &[] },
            FieldDef { key: "created_at", kind: FieldKind::Date, sql: "c.created_at", join: None, groupable: false, values: &[] },
            FieldDef { key: "updated_at", kind: FieldKind::Date, sql: "c.updated_at", join: None, groupable: false, values: &[] },
        ],
    },
    ElementDef {
        key: "organization",
        from: "organizations o",
        metrics: &[
            MetricDef { key: "count", sql: "COUNT(DISTINCT o.id)", join: None, is_money: false, currency_sql: None },
            MetricDef {
                key: "dealValue",
                sql: "COALESCE(SUM(dv.amount), 0)",
                join: Some("LEFT JOIN deals dv ON dv.organization_id = o.id"),
                is_money: true,
                currency_sql: Some("dv.currency"),
            },
        ],
        fields: &[
            // The account's own name — this is what makes "deal value by
            // organization" possible at all. Without it the only split is by
            // `kind`, which answers a different question than the title implies.
            FieldDef { key: "name", kind: FieldKind::Text, sql: "o.name", join: None, groupable: true, values: &[] },
            FieldDef { key: "kind", kind: FieldKind::Enum, sql: "o.kind", join: None, groupable: true, values: crate::db::ORG_KINDS },
            FieldDef { key: "industry", kind: FieldKind::Text, sql: "o.industry", join: None, groupable: true, values: &[] },
            FieldDef { key: "size", kind: FieldKind::Text, sql: "o.size", join: None, groupable: true, values: &[] },
            FieldDef { key: "created_at", kind: FieldKind::Date, sql: "o.created_at", join: None, groupable: false, values: &[] },
            FieldDef { key: "updated_at", kind: FieldKind::Date, sql: "o.updated_at", join: None, groupable: false, values: &[] },
        ],
    },
    ElementDef {
        key: "deal",
        from: "deals d",
        metrics: &[
            MetricDef { key: "count", sql: "COUNT(DISTINCT d.id)", join: None, is_money: false, currency_sql: None },
            MetricDef { key: "dealValue", sql: "COALESCE(SUM(d.amount), 0)", join: None, is_money: true, currency_sql: Some("d.currency") },
            MetricDef {
                key: "dealQuantity",
                sql: "COALESCE(SUM(dq.quantity), 0)",
                join: Some("LEFT JOIN deal_services dq ON dq.deal_id = d.id"),
                is_money: false,
                currency_sql: None,
            },
        ],
        fields: &[
            FieldDef { key: "stage", kind: FieldKind::Enum, sql: "d.stage", join: None, groupable: true, values: DEAL_STAGES },
            FieldDef { key: "currency", kind: FieldKind::Text, sql: "d.currency", join: None, groupable: true, values: &[] },
            FieldDef { key: "organization", kind: FieldKind::Relation, sql: "COALESCE(o.name, '')", join: Some("LEFT JOIN organizations o ON o.id = d.organization_id"), groupable: true, values: &[] },
            FieldDef { key: "contact", kind: FieldKind::Relation, sql: "COALESCE(c.name, '')", join: Some("LEFT JOIN customers c ON c.id = d.customer_id"), groupable: true, values: &[] },
            FieldDef { key: "amount", kind: FieldKind::Number, sql: "d.amount", join: None, groupable: false, values: &[] },
            FieldDef { key: "probability", kind: FieldKind::Number, sql: "d.probability", join: None, groupable: false, values: &[] },
            FieldDef { key: "expected_close_at", kind: FieldKind::Date, sql: "d.expected_close_at", join: None, groupable: false, values: &[] },
            FieldDef { key: "created_at", kind: FieldKind::Date, sql: "d.created_at", join: None, groupable: false, values: &[] },
            FieldDef { key: "updated_at", kind: FieldKind::Date, sql: "d.updated_at", join: None, groupable: false, values: &[] },
        ],
    },
    ElementDef {
        key: "service",
        from: "services s",
        metrics: &[
            MetricDef { key: "count", sql: "COUNT(DISTINCT s.id)", join: None, is_money: false, currency_sql: None },
            MetricDef {
                key: "dealValue",
                sql: "COALESCE(SUM(ds.quantity * ds.unit_amount), 0)",
                join: Some("LEFT JOIN deal_services ds ON ds.service_id = s.id"),
                is_money: true,
                currency_sql: Some("s.currency"),
            },
            MetricDef {
                key: "dealQuantity",
                sql: "COALESCE(SUM(ds.quantity), 0)",
                join: Some("LEFT JOIN deal_services ds ON ds.service_id = s.id"),
                is_money: false,
                currency_sql: None,
            },
        ],
        fields: &[
            // Revenue per catalogue entry — "which service earns most" is the
            // question the Services page invites and `kind` can't answer.
            FieldDef { key: "name", kind: FieldKind::Text, sql: "s.name", join: None, groupable: true, values: &[] },
            FieldDef { key: "kind", kind: FieldKind::Enum, sql: "s.kind", join: None, groupable: true, values: crate::db::SERVICE_KINDS },
            FieldDef { key: "pricing_model", kind: FieldKind::Enum, sql: "s.pricing_model", join: None, groupable: true, values: crate::db::PRICING_MODELS },
            FieldDef { key: "currency", kind: FieldKind::Text, sql: "s.currency", join: None, groupable: true, values: &[] },
            FieldDef { key: "active", kind: FieldKind::Bool, sql: "s.active", join: None, groupable: true, values: BOOL_VALUES },
            FieldDef { key: "amount", kind: FieldKind::Number, sql: "s.amount", join: None, groupable: false, values: &[] },
            FieldDef { key: "created_at", kind: FieldKind::Date, sql: "s.created_at", join: None, groupable: false, values: &[] },
            FieldDef { key: "updated_at", kind: FieldKind::Date, sql: "s.updated_at", join: None, groupable: false, values: &[] },
        ],
    },
    ElementDef {
        key: "task",
        from: "tasks t",
        metrics: &[MetricDef { key: "count", sql: "COUNT(DISTINCT t.id)", join: None, is_money: false, currency_sql: None }],
        fields: &[
            FieldDef { key: "status", kind: FieldKind::Enum, sql: "CASE WHEN t.done = 1 THEN 'done' ELSE 'open' END", join: None, groupable: true, values: TASK_STATUS },
            FieldDef { key: "contact", kind: FieldKind::Relation, sql: "COALESCE(c.name, '')", join: Some("LEFT JOIN customers c ON c.id = t.customer_id"), groupable: true, values: &[] },
            FieldDef { key: "due_at", kind: FieldKind::Date, sql: "t.due_at", join: None, groupable: false, values: &[] },
            FieldDef { key: "created_at", kind: FieldKind::Date, sql: "t.created_at", join: None, groupable: false, values: &[] },
        ],
    },
];

fn element(key: &str) -> Result<&'static ElementDef> {
    ELEMENTS.iter().find(|e| e.key == key).ok_or_else(|| anyhow!("unknown element '{key}'"))
}

fn metric<'a>(e: &'a ElementDef, key: &str) -> Result<&'a MetricDef> {
    e.metrics
        .iter()
        .find(|m| m.key == key)
        .ok_or_else(|| anyhow!("element '{}' has no metric '{key}'", e.key))
}

fn field<'a>(e: &'a ElementDef, key: &str) -> Result<&'a FieldDef> {
    e.fields
        .iter()
        .find(|f| f.key == key)
        .ok_or_else(|| anyhow!("element '{}' has no field '{key}'", e.key))
}

/// The registry as JSON, so the chart builder can render the right options for
/// each element without hardcoding a second copy of this knowledge.
pub fn schema_json() -> serde_json::Value {
    let elements: Vec<serde_json::Value> = ELEMENTS
        .iter()
        .map(|e| {
            serde_json::json!({
                "key": e.key,
                "metrics": e.metrics.iter().map(|m| serde_json::json!({
                    "key": m.key, "isMoney": m.is_money
                })).collect::<Vec<_>>(),
                "fields": e.fields.iter().map(|f| serde_json::json!({
                    "key": f.key,
                    "kind": f.kind.as_str(),
                    "groupable": f.groupable,
                    "operators": f.kind.operators(),
                    "values": f.values,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    serde_json::json!({
        "elements": elements,
        "displayTypes": [
            "verticalBarChart", "horizontalBarChart",
            "verticalBarChartWithLabels", "horizontalBarChartWithLabels",
            "doughnutChart", "radarChart"
        ],
        "sizes": ["small", "medium", "large"],
    })
}

// ---- chart spec ----

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Filter {
    pub field: String,
    pub op: String,
    /// Operand list. `in`/`notIn` take many; comparisons take one; `between`
    /// takes two; `isNull`/`isNotNull` take none.
    #[serde(default)]
    pub values: Vec<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Chart {
    pub id: i64,
    pub name: String,
    pub element: String,
    pub metric: String,
    /// Empty string = no grouping (a single total).
    pub grouping: String,
    pub filters: Vec<Filter>,
    pub display: serde_json::Value,
    pub size: String,
    pub sort: i64,
    pub is_template: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Deserialize)]
pub struct ChartInput {
    pub name: String,
    pub element: String,
    #[serde(default = "default_metric")]
    pub metric: String,
    #[serde(default)]
    pub grouping: String,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub display: serde_json::Value,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub is_template: bool,
}

fn default_metric() -> String {
    "count".into()
}

#[derive(Deserialize, Default)]
pub struct ChartPatch {
    pub name: Option<String>,
    pub element: Option<String>,
    pub metric: Option<String>,
    pub grouping: Option<String>,
    pub filters: Option<Vec<Filter>>,
    pub display: Option<serde_json::Value>,
    pub size: Option<String>,
    pub sort: Option<i64>,
    pub is_template: Option<bool>,
}

// ---- query builder ----

/// A bound parameter. Kept as an owned enum so the builder can hand rusqlite a
/// `&dyn ToSql` slice without borrowing from the caller's JSON.
enum Bind {
    Num(f64),
    Text(String),
}

impl ToSql for Bind {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        match self {
            Bind::Num(n) => n.to_sql(),
            Bind::Text(s) => s.to_sql(),
        }
    }
}

fn bind_of(v: &serde_json::Value) -> Bind {
    match v {
        serde_json::Value::Number(n) => Bind::Num(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::Bool(b) => Bind::Num(if *b { 1.0 } else { 0.0 }),
        serde_json::Value::String(s) => Bind::Text(s.clone()),
        other => Bind::Text(other.to_string()),
    }
}

/// Render one filter to a SQL fragment plus its bindings.
///
/// `f.sql` is a registry literal; every operand becomes a `?`. The operator is
/// matched against a closed set, so an unknown one is rejected rather than
/// concatenated.
fn filter_sql(def: &FieldDef, filter: &Filter, binds: &mut Vec<Bind>) -> Result<String> {
    if !def.kind.operators().contains(&filter.op.as_str()) {
        return Err(anyhow!("operator '{}' is not valid for field '{}'", filter.op, def.key));
    }
    let sql = def.sql;
    match filter.op.as_str() {
        "in" | "notIn" => {
            if filter.values.is_empty() {
                // An empty `in` matches nothing and an empty `notIn` matches
                // everything; treating both as "no filter" is the least
                // surprising reading of a half-built filter row.
                return Ok("1=1".into());
            }
            let placeholders = vec!["?"; filter.values.len()].join(",");
            for v in &filter.values {
                binds.push(bind_of(v));
            }
            if filter.op == "in" {
                Ok(format!("{sql} IN ({placeholders})"))
            } else {
                // NULL is not "not in X" under SQL's three-valued logic, but a
                // human reading "status not in Abandoned" expects the blanks
                // included — so spell it out.
                Ok(format!("({sql} IS NULL OR {sql} NOT IN ({placeholders}))"))
            }
        }
        "isNull" => Ok(format!("({sql} IS NULL OR {sql} = '')")),
        "isNotNull" => Ok(format!("({sql} IS NOT NULL AND {sql} <> '')")),
        "gt" | "gte" | "lt" | "lte" => {
            let v = filter.values.first().ok_or_else(|| anyhow!("'{}' needs a value", filter.op))?;
            binds.push(bind_of(v));
            let cmp = match filter.op.as_str() {
                "gt" => ">",
                "gte" => ">=",
                "lt" => "<",
                _ => "<=",
            };
            Ok(format!("{sql} {cmp} ?"))
        }
        "between" => {
            if filter.values.len() < 2 {
                return Err(anyhow!("'between' needs two values"));
            }
            binds.push(bind_of(&filter.values[0]));
            binds.push(bind_of(&filter.values[1]));
            Ok(format!("{sql} BETWEEN ? AND ?"))
        }
        "inLastDays" => {
            let v = filter.values.first().ok_or_else(|| anyhow!("'inLastDays' needs a value"))?;
            let days = v.as_f64().unwrap_or(0.0);
            binds.push(Bind::Num(days * 86_400.0));
            // Relative to now, evaluated by SQLite at query time so a saved
            // chart keeps meaning "the last 30 days" rather than freezing a date.
            Ok(format!("{sql} >= (strftime('%s','now') - ?)"))
        }
        other => Err(anyhow!("unknown operator '{other}'")),
    }
}

/// Most buckets a grouped chart will return. Grouping by a high-cardinality
/// field (organization, contact) on a real CRM could otherwise produce thousands
/// of slices — unreadable as a chart and pointless to ship.
const MAX_BUCKETS: usize = 200;

#[derive(Serialize)]
pub struct ChartResult {
    pub rows: Vec<ChartRow>,
    /// Sum over the returned buckets. When `truncated`, that is the top
    /// [`MAX_BUCKETS`] only — not the grand total.
    pub total: f64,
    pub groups: usize,
    pub is_money: bool,
    /// The currencies that actually contributed to a money metric.
    ///
    /// One entry = safe to render `total` in that currency. More than one means
    /// `SUM` added different currencies together and the number is **not** a
    /// real amount — the UI must say so rather than stamp a symbol on it. Empty
    /// for non-money metrics, or when nothing matched.
    pub currencies: Vec<String>,
    /// True when there were more buckets than [`MAX_BUCKETS`] and the tail was
    /// dropped. Surfaced so the UI can say "top 200" rather than quietly
    /// presenting a partial chart as the whole picture.
    pub truncated: bool,
    /// Human-readable filter summary — what the reference prints under the
    /// title when "Show filters" is on ("Status not in Abandoned").
    pub filter_summary: Vec<String>,
}

#[derive(Serialize)]
pub struct ChartRow {
    pub bucket: String,
    pub value: f64,
}

impl Db {
    /// Run one chart spec and return its buckets.
    pub fn run_chart(
        &self,
        element_key: &str,
        metric_key: &str,
        grouping: &str,
        filters: &[Filter],
    ) -> Result<ChartResult> {
        let e = element(element_key)?;
        let m = metric(e, metric_key)?;

        // Collect joins, deduped — a metric and a filter may want the same one.
        let mut joins: Vec<&'static str> = Vec::new();
        let add_join = |j: Option<&'static str>, joins: &mut Vec<&'static str>| {
            if let Some(j) = j {
                if !joins.contains(&j) {
                    joins.push(j);
                }
            }
        };
        add_join(m.join, &mut joins);

        let group_def = if grouping.is_empty() {
            None
        } else {
            let d = field(e, grouping)?;
            if !d.groupable {
                return Err(anyhow!("field '{}' cannot be used as a grouping", d.key));
            }
            add_join(d.join, &mut joins);
            Some(d)
        };

        let mut binds: Vec<Bind> = Vec::new();
        let mut wheres: Vec<String> = Vec::new();
        for f in filters {
            let d = field(e, &f.field)?;
            add_join(d.join, &mut joins);
            wheres.push(filter_sql(d, f, &mut binds)?);
        }

        let join_sql = joins.join(" ");
        let where_sql =
            if wheres.is_empty() { String::new() } else { format!("WHERE {}", wheres.join(" AND ")) };

        // Fetch one past the cap so a full page is distinguishable from an
        // exactly-full one.
        let sql = match group_def {
            Some(d) => format!(
                "SELECT {} AS bucket, {} AS value FROM {} {} {} GROUP BY bucket ORDER BY value DESC, bucket LIMIT {}",
                d.sql,
                m.sql,
                e.from,
                join_sql,
                where_sql,
                MAX_BUCKETS + 1
            ),
            None => format!(
                "SELECT '' AS bucket, {} AS value FROM {} {} {}",
                m.sql, e.from, join_sql, where_sql
            ),
        };

        let mut rows: Vec<ChartRow> = self.with(|c| {
            let mut stmt = c.prepare(&sql)?;
            let params: Vec<&dyn ToSql> = binds.iter().map(|b| b as &dyn ToSql).collect();
            let rows = stmt
                .query_map(params.as_slice(), |r| {
                    Ok(ChartRow {
                        bucket: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                        value: r.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })?;

        // Which currencies this sum actually mixed. Same FROM/joins/filters as
        // the aggregate, so it describes exactly the rows that were summed.
        let currencies: Vec<String> = match m.currency_sql {
            Some(cur) if m.is_money => {
                let cur_sql = format!(
                    "SELECT DISTINCT {cur} AS c FROM {} {} {} {} ORDER BY c LIMIT 10",
                    e.from,
                    join_sql,
                    where_sql,
                    if wheres.is_empty() { "WHERE c IS NOT NULL AND c <> ''" } else { "AND c IS NOT NULL AND c <> ''" },
                );
                self.with(|conn| {
                    let mut stmt = conn.prepare(&cur_sql)?;
                    let params: Vec<&dyn ToSql> = binds.iter().map(|b| b as &dyn ToSql).collect();
                    let rows = stmt
                        .query_map(params.as_slice(), |r| r.get::<_, String>(0))?
                        .filter_map(|r| r.ok())
                        .collect();
                    Ok(rows)
                })
                .unwrap_or_default()
            }
            _ => Vec::new(),
        };

        let truncated = rows.len() > MAX_BUCKETS;
        rows.truncate(MAX_BUCKETS);
        let total = rows.iter().map(|r| r.value).sum();
        Ok(ChartResult {
            groups: rows.len(),
            total,
            is_money: m.is_money,
            currencies,
            truncated,
            filter_summary: filters.iter().map(summarize_filter).collect(),
            rows,
        })
    }

    // ---- CRUD ----

    pub fn list_charts(&self) -> Result<Vec<Chart>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM dashboard_charts ORDER BY sort, id")?;
            let rows = stmt.query_map([], row_to_chart)?.filter_map(|r| r.ok()).collect();
            Ok(rows)
        })
    }

    pub fn get_chart(&self, id: i64) -> Result<Option<Chart>> {
        self.with(|c| {
            let row = c
                .query_row("SELECT * FROM dashboard_charts WHERE id=?1", rusqlite::params![id], row_to_chart)
                .ok();
            Ok(row)
        })
    }

    /// Validate the spec by compiling it before it is stored — a chart that
    /// cannot run should fail at save time, where the author can see why, not
    /// silently render an error card forever after.
    fn validate(&self, element: &str, metric: &str, grouping: &str, filters: &[Filter]) -> Result<()> {
        self.run_chart(element, metric, grouping, filters).map(|_| ())
    }

    pub fn create_chart(&self, input: &ChartInput, now: i64) -> Result<i64> {
        let name = input.name.trim();
        if name.is_empty() {
            return Err(anyhow!("chart name is required"));
        }
        self.validate(&input.element, &input.metric, &input.grouping, &input.filters)?;
        let size = normalize_size(&input.size);
        let filters = serde_json::to_string(&input.filters)?;
        let display = if input.display.is_null() {
            serde_json::json!({ "type": "verticalBarChart" })
        } else {
            input.display.clone()
        };
        self.with(|c| {
            let next: i64 = c.query_row(
                "SELECT COALESCE(MAX(sort), 0) + 1 FROM dashboard_charts",
                [],
                |r| r.get(0),
            )?;
            c.execute(
                "INSERT INTO dashboard_charts(name, element, metric, grouping, filters_json,
                        display_json, size, sort, is_template, created_at, updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
                rusqlite::params![
                    name,
                    input.element,
                    input.metric,
                    input.grouping,
                    filters,
                    display.to_string(),
                    size,
                    next,
                    input.is_template as i64,
                    now
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn update_chart(&self, id: i64, patch: &ChartPatch, now: i64) -> Result<()> {
        let cur = self
            .get_chart(id)?
            .ok_or_else(|| anyhow!("chart {id} not found"))?;
        // Validate the POST-patch spec, so a partial edit can't be saved into a
        // combination that doesn't compile.
        let element = patch.element.clone().unwrap_or(cur.element);
        let metric = patch.metric.clone().unwrap_or(cur.metric);
        let grouping = patch.grouping.clone().unwrap_or(cur.grouping);
        let filters = patch.filters.clone().unwrap_or(cur.filters);
        self.validate(&element, &metric, &grouping, &filters)?;
        let filters_json = serde_json::to_string(&filters)?;

        self.with(|c| {
            if let Some(v) = &patch.name {
                let v = v.trim();
                if v.is_empty() {
                    return Err(anyhow!("chart name cannot be empty"));
                }
                c.execute("UPDATE dashboard_charts SET name=?2 WHERE id=?1", rusqlite::params![id, v])?;
            }
            c.execute(
                "UPDATE dashboard_charts SET element=?2, metric=?3, grouping=?4, filters_json=?5,
                        updated_at=?6 WHERE id=?1",
                rusqlite::params![id, element, metric, grouping, filters_json, now],
            )?;
            if let Some(v) = &patch.display {
                c.execute(
                    "UPDATE dashboard_charts SET display_json=?2 WHERE id=?1",
                    rusqlite::params![id, v.to_string()],
                )?;
            }
            if let Some(v) = &patch.size {
                c.execute(
                    "UPDATE dashboard_charts SET size=?2 WHERE id=?1",
                    rusqlite::params![id, normalize_size(v)],
                )?;
            }
            if let Some(v) = patch.sort {
                c.execute("UPDATE dashboard_charts SET sort=?2 WHERE id=?1", rusqlite::params![id, v])?;
            }
            if let Some(v) = patch.is_template {
                c.execute(
                    "UPDATE dashboard_charts SET is_template=?2 WHERE id=?1",
                    rusqlite::params![id, v as i64],
                )?;
            }
            Ok(())
        })
    }

    pub fn delete_chart(&self, id: i64) -> Result<()> {
        self.with(|c| {
            let n = c.execute("DELETE FROM dashboard_charts WHERE id=?1", rusqlite::params![id])?;
            if n == 0 {
                return Err(anyhow!("chart {id} not found"));
            }
            Ok(())
        })
    }

    /// Apply a drag-reorder: `ids` in their new order.
    pub fn reorder_charts(&self, ids: &[i64], now: i64) -> Result<()> {
        self.with(|c| {
            for (i, id) in ids.iter().enumerate() {
                c.execute(
                    "UPDATE dashboard_charts SET sort=?2, updated_at=?3 WHERE id=?1",
                    rusqlite::params![id, i as i64 + 1, now],
                )?;
            }
            Ok(())
        })
    }

    /// Distinct values actually present for an open-set field, for the filter
    /// value picker (industry, source, currency — anything with no fixed list).
    pub fn chart_field_values(&self, element_key: &str, field_key: &str) -> Result<Vec<String>> {
        let e = element(element_key)?;
        let d = field(e, field_key)?;
        if !d.values.is_empty() {
            return Ok(d.values.iter().map(|s| s.to_string()).collect());
        }
        let join = d.join.unwrap_or("");
        let sql = format!(
            "SELECT DISTINCT {} AS v FROM {} {} WHERE v IS NOT NULL AND v <> '' ORDER BY v LIMIT 200",
            d.sql, e.from, join
        );
        self.with(|c| {
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// Seed the starter dashboard exactly once, ever.
    ///
    /// The guard is a persisted flag, not `COUNT(*) = 0`. An empty table is
    /// ambiguous — it means both "fresh install" and "the operator deleted every
    /// chart on purpose" — and reseeding would resurrect all six on the next
    /// restart, which reads as the app refusing to forget.
    pub(crate) fn seed_charts(&self, now: i64) -> Result<()> {
        if self.get_setting("dashboard_seeded")?.is_some() {
            return Ok(());
        }
        self.set_setting("dashboard_seeded", "1")?;
        let defaults = [
            ("Tổ chức theo loại", "organization", "count", "kind", "doughnutChart", "small"),
            // Group by `name`, not `kind`: the title promises revenue per
            // account, and `kind` silently answers "per account TYPE" instead.
            ("Giá trị deal theo tổ chức", "organization", "dealValue", "name", "horizontalBarChartWithLabels", "medium"),
            ("Giá trị deal theo loại dịch vụ", "service", "dealValue", "kind", "doughnutChart", "small"),
            ("Phễu bán hàng", "contact", "count", "sale_stage", "verticalBarChart", "medium"),
            ("Deal theo giai đoạn", "deal", "dealValue", "stage", "verticalBarChartWithLabels", "medium"),
            ("Khách theo vai trò", "contact", "count", "role", "doughnutChart", "small"),
        ];
        for (i, (name, element, metric, grouping, display, size)) in defaults.iter().enumerate() {
            let input = ChartInput {
                name: (*name).into(),
                element: (*element).into(),
                metric: (*metric).into(),
                grouping: (*grouping).into(),
                filters: vec![],
                display: serde_json::json!({ "type": display, "showFilters": true }),
                size: (*size).into(),
                is_template: false,
            };
            // A seed that no longer compiles (someone renamed a field) must not
            // take startup down with it.
            if let Ok(id) = self.create_chart(&input, now) {
                let _ = self.with(|c| {
                    c.execute(
                        "UPDATE dashboard_charts SET sort=?2 WHERE id=?1",
                        rusqlite::params![id, i as i64 + 1],
                    )?;
                    Ok(())
                });
            }
        }
        Ok(())
    }
}

fn normalize_size(s: &str) -> String {
    match s.trim() {
        "small" | "large" => s.trim().to_string(),
        _ => "medium".to_string(),
    }
}

/// One filter as a phrase, e.g. `stage not in won, lost` — the line the
/// reference prints under a chart title when "Show filters" is on. The UI
/// localizes the field name; this fixes the shape.
fn summarize_filter(f: &Filter) -> String {
    let vals: Vec<String> = f
        .values
        .iter()
        .map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect();
    match f.op.as_str() {
        // Operand goes in the middle of the phrase, not appended to it.
        "inLastDays" => format!("{} in last {} days", f.field, vals.first().cloned().unwrap_or_default()),
        "between" if vals.len() >= 2 => format!("{} between {} and {}", f.field, vals[0], vals[1]),
        "isNull" => format!("{} is empty", f.field),
        "isNotNull" => format!("{} is not empty", f.field),
        op => {
            let word = match op {
                "in" => "in",
                "notIn" => "not in",
                "gt" => ">",
                "gte" => "≥",
                "lt" => "<",
                "lte" => "≤",
                other => other,
            };
            if vals.is_empty() {
                format!("{} {}", f.field, word)
            } else {
                format!("{} {} {}", f.field, word, vals.join(", "))
            }
        }
    }
}

fn row_to_chart(r: &rusqlite::Row) -> rusqlite::Result<Chart> {
    let filters: String = r.get("filters_json")?;
    let display: String = r.get("display_json")?;
    Ok(Chart {
        id: r.get("id")?,
        name: r.get("name")?,
        element: r.get("element")?,
        metric: r.get("metric")?,
        grouping: r.get("grouping")?,
        filters: serde_json::from_str(&filters).unwrap_or_default(),
        display: serde_json::from_str(&display).unwrap_or(serde_json::json!({})),
        size: r.get("size")?,
        sort: r.get("sort")?,
        is_template: r.get::<_, i64>("is_template")? != 0,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn db() -> Db {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).unwrap();
        std::mem::forget(dir);
        db
    }

    fn seed(db: &Db) -> (i64, i64) {
        let org = db
            .create_organization(
                &serde_json::from_value(serde_json::json!({"name":"Bayer","kind":"direct_customer"}))
                    .unwrap(),
                100,
            )
            .unwrap();
        let c1 = db
            .create_customer(
                &serde_json::from_value(serde_json::json!({"name":"A","role":"customer"})).unwrap(),
                100,
            )
            .unwrap();
        let c2 = db
            .create_customer(
                &serde_json::from_value(serde_json::json!({"name":"B","role":"lead"})).unwrap(),
                100,
            )
            .unwrap();
        db.link_customer_org(c1, org, "", true, 100).unwrap();
        db.create_deal(
            &serde_json::from_value(serde_json::json!({
                "customer_id": c1, "title": "D1", "amount": 100.0, "stage": "won"
            }))
            .unwrap(),
            100,
        )
        .unwrap();
        db.create_deal(
            &serde_json::from_value(serde_json::json!({
                "customer_id": c1, "title": "D2", "amount": 40.0, "stage": "lost"
            }))
            .unwrap(),
            100,
        )
        .unwrap();
        (org, c2)
    }

    #[test]
    fn count_with_no_grouping_is_a_single_total() {
        let db = db();
        seed(&db);
        let r = db.run_chart("contact", "count", "", &[]).unwrap();
        assert_eq!(r.rows.len(), 1);
        assert_eq!(r.rows[0].value, 2.0);
        assert!(!r.is_money);
    }

    #[test]
    fn grouping_splits_into_buckets() {
        let db = db();
        seed(&db);
        let r = db.run_chart("contact", "count", "role", &[]).unwrap();
        let mut got: Vec<(String, f64)> = r.rows.iter().map(|x| (x.bucket.clone(), x.value)).collect();
        got.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(got, vec![("customer".into(), 1.0), ("lead".into(), 1.0)]);
        assert_eq!(r.groups, 2);
    }

    /// A contact with two deals must still count once. `COUNT(*)` over the join
    /// would say 2 — this is the fan-out the DISTINCT exists to stop.
    #[test]
    fn joined_metric_does_not_inflate_counts() {
        let db = db();
        seed(&db);
        let r = db.run_chart("contact", "count", "", &[]).unwrap();
        assert_eq!(r.rows[0].value, 2.0, "two contacts, regardless of deal count");
    }

    #[test]
    fn money_metric_sums_across_the_join() {
        let db = db();
        seed(&db);
        let r = db.run_chart("contact", "dealValue", "", &[]).unwrap();
        assert_eq!(r.rows[0].value, 140.0);
        assert!(r.is_money);
    }

    /// The subtitle is user-facing prose, so the operand has to land in the
    /// right place: "in last 7 days", not "in last days 7".
    #[test]
    fn filter_summaries_read_as_sentences() {
        let f = |field: &str, op: &str, vals: Vec<serde_json::Value>| Filter {
            field: field.into(),
            op: op.into(),
            values: vals,
        };
        assert_eq!(summarize_filter(&f("stage", "notIn", vec![json!("won"), json!("lost")])), "stage not in won, lost");
        assert_eq!(summarize_filter(&f("created_at", "inLastDays", vec![json!(7)])), "created_at in last 7 days");
        assert_eq!(summarize_filter(&f("amount", "between", vec![json!(1), json!(9)])), "amount between 1 and 9");
        assert_eq!(summarize_filter(&f("source", "isNull", vec![])), "source is empty");
        assert_eq!(summarize_filter(&f("amount", "gte", vec![json!(5)])), "amount ≥ 5");
    }

    #[test]
    fn not_in_filter_includes_rows_the_reference_would_expect() {
        let db = db();
        seed(&db);
        let f = Filter {
            field: "stage".into(),
            op: "notIn".into(),
            values: vec![serde_json::json!("lost")],
        };
        let r = db.run_chart("deal", "dealValue", "", std::slice::from_ref(&f)).unwrap();
        assert_eq!(r.rows[0].value, 100.0, "the lost deal must be excluded");
        assert_eq!(r.filter_summary, vec!["stage not in lost"]);
    }

    #[test]
    fn in_filter_binds_values_rather_than_interpolating() {
        let db = db();
        seed(&db);
        // A value engineered to break out of a quoted literal. If it were
        // interpolated this would be a syntax error or worse; bound, it is just
        // a string that matches nothing.
        let f = Filter {
            field: "role".into(),
            op: "in".into(),
            values: vec![serde_json::json!("lead'); DROP TABLE customers;--")],
        };
        let r = db.run_chart("contact", "count", "", std::slice::from_ref(&f)).unwrap();
        assert_eq!(r.rows[0].value, 0.0);
        // The table is still there.
        assert_eq!(db.run_chart("contact", "count", "", &[]).unwrap().rows[0].value, 2.0);
    }

    #[test]
    fn unknown_keys_are_rejected_not_concatenated() {
        let db = db();
        assert!(db.run_chart("customers; DROP TABLE customers", "count", "", &[]).is_err());
        assert!(db.run_chart("contact", "count; --", "", &[]).is_err());
        assert!(db.run_chart("contact", "count", "role; --", &[]).is_err());
        let f = Filter { field: "role; --".into(), op: "in".into(), values: vec![] };
        assert!(db.run_chart("contact", "count", "", &[f]).is_err());
        let bad_op = Filter { field: "role".into(), op: "; DROP".into(), values: vec![] };
        assert!(db.run_chart("contact", "count", "", &[bad_op]).is_err());
    }

    /// A chart grouped by a high-cardinality field must cap its buckets AND say
    /// that it did — a silently-truncated chart reads as the whole picture.
    #[test]
    fn high_cardinality_grouping_is_capped_and_declares_it() {
        let db = db();
        for i in 0..(MAX_BUCKETS + 25) {
            db.create_customer(
                &serde_json::from_value(json!({ "name": format!("C{i}"), "source": format!("src{i}") }))
                    .unwrap(),
                100,
            )
            .unwrap();
        }
        let r = db.run_chart("contact", "count", "source", &[]).unwrap();
        assert_eq!(r.rows.len(), MAX_BUCKETS, "must cap at MAX_BUCKETS");
        assert_eq!(r.groups, MAX_BUCKETS);
        assert!(r.truncated, "must admit the tail was dropped");

        // Under the cap, nothing is hidden and the flag stays false.
        let r2 = db.run_chart("contact", "count", "role", &[]).unwrap();
        assert!(!r2.truncated);
    }

    /// The reference's flagship chart is "Deal Value By Organizations" showing
    /// account NAMES. Without a groupable `name` the only split is `kind`, and a
    /// card titled "by organization" would silently answer "by organization
    /// type" — a different question with plausible-looking numbers.
    #[test]
    fn organizations_can_be_grouped_by_name() {
        let db = db();
        seed(&db);
        let r = db.run_chart("organization", "dealValue", "name", &[]).unwrap();
        assert!(r.rows.iter().any(|x| x.bucket == "Bayer"), "buckets must be account names, got {:?}",
                r.rows.iter().map(|x| &x.bucket).collect::<Vec<_>>());
    }

    /// SUM over mixed currencies is not an amount. The engine can't convert
    /// (no FX table), so it must at least report what it added together.
    #[test]
    fn money_metric_reports_the_currencies_it_summed() {
        let db = db();
        let c = db
            .create_customer(&serde_json::from_value(json!({"name":"A"})).unwrap(), 100)
            .unwrap();
        db.create_deal(
            &serde_json::from_value(json!({"customer_id": c, "title":"vnd", "amount": 100.0, "currency":"VND"})).unwrap(),
            100,
        )
        .unwrap();
        // One currency: the card may safely render a symbol.
        let r = db.run_chart("deal", "dealValue", "", &[]).unwrap();
        assert_eq!(r.currencies, vec!["VND"]);

        db.create_deal(
            &serde_json::from_value(json!({"customer_id": c, "title":"eur", "amount": 5.0, "currency":"EUR"})).unwrap(),
            100,
        )
        .unwrap();
        let r = db.run_chart("deal", "dealValue", "", &[]).unwrap();
        assert_eq!(r.currencies, vec!["EUR", "VND"], "must admit it mixed two currencies");
        assert_eq!(r.rows[0].value, 105.0, "and the number really is a meaningless 100+5");

        // Filtering back down to one currency makes the total honest again.
        let f = Filter { field: "currency".into(), op: "in".into(), values: vec![json!("VND")] };
        let r = db.run_chart("deal", "dealValue", "", &[f]).unwrap();
        assert_eq!(r.currencies, vec!["VND"]);
        assert_eq!(r.rows[0].value, 100.0);
    }

    /// Counts aren't money, so they carry no currency claim at all.
    #[test]
    fn count_metric_claims_no_currency() {
        let db = db();
        seed(&db);
        assert!(db.run_chart("contact", "count", "", &[]).unwrap().currencies.is_empty());
        assert!(db.run_chart("deal", "dealQuantity", "", &[]).unwrap().currencies.is_empty());
    }

    #[test]
    fn non_groupable_field_is_refused() {
        let db = db();
        // Dates would explode into one bucket per second.
        assert!(db.run_chart("contact", "count", "created_at", &[]).is_err());
    }

    #[test]
    fn operator_must_match_the_field_kind() {
        let db = db();
        seed(&db);
        let f = Filter { field: "role".into(), op: "between".into(), values: vec![] };
        assert!(db.run_chart("contact", "count", "", &[f]).is_err());
    }

    #[test]
    fn grouping_by_relation_uses_the_related_name() {
        let db = db();
        seed(&db);
        let r = db.run_chart("contact", "dealValue", "organization", &[]).unwrap();
        let bayer = r.rows.iter().find(|x| x.bucket == "Bayer").unwrap();
        assert_eq!(bayer.value, 140.0);
    }

    #[test]
    fn saving_an_invalid_chart_fails_at_save_time() {
        let db = db();
        let bad = ChartInput {
            name: "x".into(),
            element: "contact".into(),
            metric: "dealQuantity".into(), // contact has no such metric
            grouping: String::new(),
            filters: vec![],
            display: serde_json::Value::Null,
            size: String::new(),
            is_template: false,
        };
        assert!(db.create_chart(&bad, 1).is_err());
    }

    #[test]
    fn crud_and_reorder_roundtrip() {
        let db = db();
        seed(&db);
        // `Db::open` seeds the starter dashboard; clear it so this test asserts
        // on its own charts only.
        for c in db.list_charts().unwrap() {
            db.delete_chart(c.id).unwrap();
        }
        let mk = |n: &str| ChartInput {
            name: n.into(),
            element: "contact".into(),
            metric: "count".into(),
            grouping: "role".into(),
            filters: vec![],
            display: serde_json::Value::Null,
            size: "small".into(),
            is_template: false,
        };
        let a = db.create_chart(&mk("A"), 1).unwrap();
        let b = db.create_chart(&mk("B"), 1).unwrap();
        assert_eq!(db.list_charts().unwrap().iter().map(|c| c.name.clone()).collect::<Vec<_>>(), vec!["A", "B"]);
        db.reorder_charts(&[b, a], 2).unwrap();
        assert_eq!(db.list_charts().unwrap().iter().map(|c| c.name.clone()).collect::<Vec<_>>(), vec!["B", "A"]);
        db.delete_chart(a).unwrap();
        assert_eq!(db.list_charts().unwrap().len(), 1);
        assert!(db.delete_chart(a).is_err());
    }

    #[test]
    fn seed_runs_once_and_respects_a_deliberate_wipe() {
        let db = db();
        db.seed_charts(1).unwrap();
        let n = db.list_charts().unwrap().len();
        assert_eq!(n, 6);
        db.seed_charts(2).unwrap();
        assert_eq!(db.list_charts().unwrap().len(), n, "seeding twice must not duplicate");

        // Deleting every chart is a decision, not a fresh install: a later
        // startup must leave the dashboard empty rather than restore the seeds.
        for c in db.list_charts().unwrap() {
            db.delete_chart(c.id).unwrap();
        }
        db.seed_charts(3).unwrap();
        assert_eq!(db.list_charts().unwrap().len(), 0, "a deliberate wipe must stick");
    }

    #[test]
    fn seeded_charts_all_compile_and_return_buckets() {
        let db = db();
        seed(&db);
        db.seed_charts(1).unwrap();
        let charts = db.list_charts().unwrap();
        assert_eq!(charts.len(), 6);
        for ch in charts {
            db.run_chart(&ch.element, &ch.metric, &ch.grouping, &ch.filters)
                .unwrap_or_else(|e| panic!("seeded chart {:?} does not run: {e}", ch.name));
        }
    }
}
