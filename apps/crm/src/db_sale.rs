//! Proactive-selling state, absorbed from the standalone AI Sale app.
//!
//! The original kept a `leads` table as a 1:1 overlay on a CRM customer, because
//! it ran in its own process and could only name a customer by an id fetched over
//! HTTP. In-process that indirection buys nothing and costs correctness (it had a
//! dedupe hole: when the CRM was unreachable, `crm_customer_id` fell back to 0
//! and every capture of the same person made a fresh unlinked lead). So sales
//! state now lives in columns on `customers`, and everything here keys on
//! `customer_id`.
//!
//! Division of labour, unchanged from the original and deliberate: the
//! deterministic parts — the guardrail, rate limiting, scheduling, stage
//! transitions — are Rust. Only the wording of a message is the LLM's job.

use anyhow::{anyhow, Result};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use crate::db::Db;

#[derive(Serialize, Clone)]
pub struct SaleState {
    pub customer_id: i64,
    pub name: String,
    pub sale_stage: String,
    pub temperature: String,
    pub lead_score: i64,
    pub intent_signals: Vec<String>,
    pub unsubscribed: bool,
    pub unsubscribed_at: Option<i64>,
    pub last_inbound_at: Option<i64>,
    pub last_interaction_at: Option<i64>,
    pub checkin_count: i64,
    pub last_checkin_at: Option<i64>,
    pub owner: String,
    pub source: String,
}

#[derive(Serialize, Clone)]
pub struct Review {
    pub id: i64,
    pub customer_id: i64,
    pub customer_name: String,
    pub draft: String,
    pub channel: String,
    pub subject: String,
    pub risk_reason: String,
    pub status: String,
    pub edited: String,
    pub approved_by: String,
    pub approved_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Serialize, Clone)]
pub struct Escalation {
    pub id: i64,
    pub customer_id: i64,
    pub customer_name: String,
    pub reason: String,
    pub context: String,
    pub draft: String,
    pub status: String,
    pub resolved_by: String,
    pub resolved_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Serialize, Clone)]
pub struct SaleAction {
    pub id: i64,
    pub customer_id: Option<i64>,
    pub action_type: String,
    pub reasoning: String,
    pub tool_calls: String,
    pub tokens: i64,
    pub cost: f64,
    pub needs_review: bool,
    pub created_at: i64,
}

#[derive(Serialize, Clone)]
pub struct Sequence {
    pub key: String,
    pub name: String,
    pub description: String,
    pub steps: serde_json::Value,
    pub enabled: bool,
    pub created_at: i64,
}

#[derive(Serialize, Clone)]
pub struct SequenceRun {
    pub id: i64,
    pub customer_id: i64,
    pub sequence_key: String,
    pub current_step: i64,
    pub status: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub last_sent_at: Option<i64>,
}

#[derive(Serialize, Clone)]
pub struct Job {
    pub id: i64,
    pub customer_id: i64,
    pub job_type: String,
    pub run_at: i64,
    pub payload: String,
    pub status: String,
    pub executed_at: Option<i64>,
    pub error: String,
    pub created_at: i64,
}

impl Db {
    // ---- sales state on the customer row ----

    pub fn sale_state(&self, customer_id: i64) -> Result<Option<SaleState>> {
        self.with(|c| {
            let row = c
                .query_row(
                    "SELECT c.id, c.name, c.sale_stage, c.temperature, c.lead_score, c.intent_signals,
                            c.unsubscribed, c.unsubscribed_at, c.last_inbound_at, c.checkin_count,
                            c.last_checkin_at, c.owner, c.source,
                            (SELECT MAX(occurred_at) FROM interactions i WHERE i.customer_id = c.id)
                     FROM customers c WHERE c.id = ?1",
                    params![customer_id],
                    |r| {
                        let signals: String = r.get(5)?;
                        Ok(SaleState {
                            customer_id: r.get(0)?,
                            name: r.get(1)?,
                            sale_stage: r.get(2)?,
                            temperature: r.get(3)?,
                            lead_score: r.get(4)?,
                            intent_signals: serde_json::from_str(&signals).unwrap_or_default(),
                            unsubscribed: r.get::<_, i64>(6)? != 0,
                            unsubscribed_at: r.get(7)?,
                            last_inbound_at: r.get(8)?,
                            checkin_count: r.get(9)?,
                            last_checkin_at: r.get(10)?,
                            owner: r.get(11)?,
                            source: r.get(12)?,
                            last_interaction_at: r.get(13)?,
                        })
                    },
                )
                .optional()?;
            Ok(row)
        })
    }

    /// List customers as sales leads. `stage`/`temperature`/`q` all optional.
    pub fn list_leads(
        &self,
        stage: Option<&str>,
        temperature: Option<&str>,
        q: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SaleState>> {
        self.with(|c| {
            let like = q
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| format!("%{}%", s.to_lowercase()));
            let stage = stage.map(|s| s.trim()).filter(|s| !s.is_empty());
            let temp = temperature.map(|s| s.trim()).filter(|s| !s.is_empty());
            let mut stmt = c.prepare(
                "SELECT c.id, c.name, c.sale_stage, c.temperature, c.lead_score, c.intent_signals,
                        c.unsubscribed, c.unsubscribed_at, c.last_inbound_at, c.checkin_count,
                        c.last_checkin_at, c.owner, c.source,
                        (SELECT MAX(occurred_at) FROM interactions i WHERE i.customer_id = c.id)
                 FROM customers c
                 WHERE (?1 IS NULL OR c.sale_stage = ?1)
                   AND (?2 IS NULL OR c.temperature = ?2)
                   AND (?3 IS NULL OR LOWER(c.name) LIKE ?3 OR LOWER(c.email) LIKE ?3
                        OR LOWER(c.company) LIKE ?3)
                 ORDER BY c.lead_score DESC, c.updated_at DESC
                 LIMIT ?4",
            )?;
            let rows = stmt
                .query_map(params![stage, temp, like, limit], |r| {
                    let signals: String = r.get(5)?;
                    Ok(SaleState {
                        customer_id: r.get(0)?,
                        name: r.get(1)?,
                        sale_stage: r.get(2)?,
                        temperature: r.get(3)?,
                        lead_score: r.get(4)?,
                        intent_signals: serde_json::from_str(&signals).unwrap_or_default(),
                        unsubscribed: r.get::<_, i64>(6)? != 0,
                        unsubscribed_at: r.get(7)?,
                        last_inbound_at: r.get(8)?,
                        checkin_count: r.get(9)?,
                        last_checkin_at: r.get(10)?,
                        owner: r.get(11)?,
                        source: r.get(12)?,
                        last_interaction_at: r.get(13)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn update_sale_stage(
        &self,
        customer_id: i64,
        stage: Option<&str>,
        temperature: Option<&str>,
        lead_score: Option<i64>,
        now: i64,
    ) -> Result<()> {
        self.with(|c| {
            let exists: i64 = c.query_row(
                "SELECT COUNT(*) FROM customers WHERE id=?1",
                params![customer_id],
                |r| r.get(0),
            )?;
            if exists == 0 {
                return Err(anyhow!("customer {customer_id} not found"));
            }
            if let Some(s) = stage {
                if !crate::db::SALE_STAGES.contains(&s) {
                    return Err(anyhow!("unknown sale stage '{s}'"));
                }
                c.execute(
                    "UPDATE customers SET sale_stage=?2 WHERE id=?1",
                    params![customer_id, s],
                )?;
            }
            if let Some(t) = temperature {
                if !crate::db::TEMPERATURES.contains(&t) {
                    return Err(anyhow!("unknown temperature '{t}'"));
                }
                c.execute(
                    "UPDATE customers SET temperature=?2 WHERE id=?1",
                    params![customer_id, t],
                )?;
            }
            if let Some(s) = lead_score {
                c.execute(
                    "UPDATE customers SET lead_score=?2 WHERE id=?1",
                    params![customer_id, s.clamp(0, 100)],
                )?;
            }
            c.execute(
                "UPDATE customers SET updated_at=?2 WHERE id=?1",
                params![customer_id, now],
            )?;
            Ok(())
        })
    }

    /// Nudge the score and (optionally) the temperature. Clamped to 0..=100.
    pub fn bump_score(
        &self,
        customer_id: i64,
        delta: i64,
        temperature: Option<&str>,
        now: i64,
    ) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE customers SET lead_score = MAX(0, MIN(100, lead_score + ?2)), updated_at=?3
                 WHERE id=?1",
                params![customer_id, delta, now],
            )?;
            if let Some(t) = temperature {
                if crate::db::TEMPERATURES.contains(&t) {
                    c.execute(
                        "UPDATE customers SET temperature=?2 WHERE id=?1",
                        params![customer_id, t],
                    )?;
                }
            }
            Ok(())
        })
    }

    pub fn set_unsubscribed(&self, customer_id: i64, on: bool, now: i64) -> Result<()> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE customers SET unsubscribed=?2, unsubscribed_at=?3, updated_at=?4 WHERE id=?1",
                params![customer_id, on as i64, if on { Some(now) } else { None }, now],
            )?;
            if n == 0 {
                return Err(anyhow!("customer {customer_id} not found"));
            }
            Ok(())
        })
    }

    pub fn mark_inbound(&self, customer_id: i64, now: i64) -> Result<()> {
        self.with(|c| {
            // A reply resets the silence clock AND the check-in counter: the
            // customer is back, so the next quiet spell starts from scratch.
            c.execute(
                "UPDATE customers SET last_inbound_at=?2, checkin_count=0, updated_at=?2 WHERE id=?1",
                params![customer_id, now],
            )?;
            Ok(())
        })
    }

    /// Customers who have gone quiet long enough to deserve a check-in, and
    /// whose last check-in is outside the cooldown. Never returns unsubscribed
    /// or already-churned people.
    pub fn leads_for_checkin(
        &self,
        inactive_before: i64,
        cooldown_before: i64,
    ) -> Result<Vec<i64>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id FROM customers
                 WHERE unsubscribed = 0
                   AND sale_stage NOT IN ('churned','closed_won')
                   AND COALESCE(last_inbound_at, created_at) < ?1
                   AND COALESCE(last_checkin_at, 0) < ?2
                 ORDER BY lead_score DESC
                 LIMIT 50",
            )?;
            let rows = stmt
                .query_map(params![inactive_before, cooldown_before], |r| {
                    r.get::<_, i64>(0)
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn mark_checkin(&self, customer_id: i64, now: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE customers SET checkin_count = checkin_count + 1, last_checkin_at=?2,
                        updated_at=?2 WHERE id=?1",
                params![customer_id, now],
            )?;
            Ok(())
        })
    }

    // ---- reviews ----

    pub fn create_review(
        &self,
        customer_id: i64,
        draft: &str,
        channel: &str,
        risk_reason: &str,
        now: i64,
    ) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO sale_reviews(customer_id, draft, channel, risk_reason, status, created_at)
                 VALUES(?1,?2,?3,?4,'pending',?5)",
                params![customer_id, draft, channel, risk_reason, now],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn get_review(&self, id: i64) -> Result<Option<Review>> {
        self.with(|c| {
            let row = c
                .query_row(
                    "SELECT r.*, COALESCE(c.name,'') AS customer_name FROM sale_reviews r
                     LEFT JOIN customers c ON c.id = r.customer_id WHERE r.id=?1",
                    params![id],
                    Self::row_to_review,
                )
                .optional()?;
            Ok(row)
        })
    }

    pub fn list_reviews(&self, status: Option<&str>, limit: i64) -> Result<Vec<Review>> {
        self.with(|c| {
            let status = status.map(|s| s.trim()).filter(|s| !s.is_empty());
            let mut stmt = c.prepare(
                "SELECT r.*, COALESCE(c.name,'') AS customer_name FROM sale_reviews r
                 LEFT JOIN customers c ON c.id = r.customer_id
                 WHERE (?1 IS NULL OR r.status = ?1)
                 ORDER BY r.created_at DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![status, limit], Self::row_to_review)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn resolve_review(
        &self,
        id: i64,
        status: &str,
        edited: &str,
        by: &str,
        now: i64,
    ) -> Result<()> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE sale_reviews SET status=?2, edited=?3, approved_by=?4, approved_at=?5
                 WHERE id=?1",
                params![id, status, edited, by, now],
            )?;
            if n == 0 {
                return Err(anyhow!("review {id} not found"));
            }
            Ok(())
        })
    }

    // ---- escalations ----

    pub fn create_escalation(
        &self,
        customer_id: i64,
        reason: &str,
        context: &str,
        draft: &str,
        now: i64,
    ) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO sale_escalations(customer_id, reason, context, draft, status, created_at)
                 VALUES(?1,?2,?3,?4,'open',?5)",
                params![customer_id, reason, context, draft, now],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn list_escalations(&self, status: Option<&str>, limit: i64) -> Result<Vec<Escalation>> {
        self.with(|c| {
            let status = status.map(|s| s.trim()).filter(|s| !s.is_empty());
            let mut stmt = c.prepare(
                "SELECT e.*, COALESCE(c.name,'') AS customer_name FROM sale_escalations e
                 LEFT JOIN customers c ON c.id = e.customer_id
                 WHERE (?1 IS NULL OR e.status = ?1)
                 ORDER BY e.created_at DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![status, limit], |r| {
                    Ok(Escalation {
                        id: r.get("id")?,
                        customer_id: r.get("customer_id")?,
                        customer_name: r.get("customer_name")?,
                        reason: r.get("reason")?,
                        context: r.get("context")?,
                        draft: r.get("draft")?,
                        status: r.get("status")?,
                        resolved_by: r.get("resolved_by")?,
                        resolved_at: r.get("resolved_at")?,
                        created_at: r.get("created_at")?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn resolve_escalation(&self, id: i64, by: &str, now: i64) -> Result<()> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE sale_escalations SET status='resolved', resolved_by=?2, resolved_at=?3
                 WHERE id=?1 AND status='open'",
                params![id, by, now],
            )?;
            if n == 0 {
                return Err(anyhow!("escalation {id} not found or already resolved"));
            }
            Ok(())
        })
    }

    // ---- agent action log ----

    pub fn log_action(
        &self,
        customer_id: Option<i64>,
        action_type: &str,
        reasoning: &str,
        tool_calls: &str,
        tokens: i64,
        needs_review: bool,
        now: i64,
    ) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO sale_actions(customer_id, action_type, reasoning, tool_calls, tokens,
                                          needs_review, created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    customer_id,
                    action_type,
                    reasoning,
                    tool_calls,
                    tokens,
                    needs_review as i64,
                    now
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn list_actions(&self, customer_id: Option<i64>, limit: i64) -> Result<Vec<SaleAction>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM sale_actions
                 WHERE (?1 IS NULL OR customer_id = ?1)
                 ORDER BY created_at DESC, id DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![customer_id, limit], |r| {
                    Ok(SaleAction {
                        id: r.get("id")?,
                        customer_id: r.get("customer_id")?,
                        action_type: r.get("action_type")?,
                        reasoning: r.get("reasoning")?,
                        tool_calls: r.get("tool_calls")?,
                        tokens: r.get("tokens")?,
                        cost: r.get("cost")?,
                        needs_review: r.get::<_, i64>("needs_review")? != 0,
                        created_at: r.get("created_at")?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    // ---- sequences ----

    pub fn list_sequences(&self) -> Result<Vec<Sequence>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM sequences ORDER BY key")?;
            let rows = stmt
                .query_map([], |r| {
                    let steps: String = r.get("steps_json")?;
                    Ok(Sequence {
                        key: r.get("key")?,
                        name: r.get("name")?,
                        description: r.get("description")?,
                        steps: serde_json::from_str(&steps)
                            .unwrap_or(serde_json::Value::Array(vec![])),
                        enabled: r.get::<_, i64>("enabled")? != 0,
                        created_at: r.get("created_at")?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// The step list of one sequence, or an empty vec if it's unknown/disabled.
    pub fn sequence_steps(&self, key: &str) -> Result<Vec<serde_json::Value>> {
        self.with(|c| {
            let steps: Option<String> = c
                .query_row(
                    "SELECT steps_json FROM sequences WHERE key=?1 AND enabled=1",
                    params![key],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(steps
                .and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok())
                .unwrap_or_default())
        })
    }

    pub fn set_sequence_enabled(&self, key: &str, enabled: bool) -> Result<()> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE sequences SET enabled=?2 WHERE key=?1",
                params![key, enabled as i64],
            )?;
            if n == 0 {
                return Err(anyhow!("sequence '{key}' not found"));
            }
            Ok(())
        })
    }

    pub fn create_sequence_run(&self, customer_id: i64, key: &str, now: i64) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO sequence_runs(customer_id, sequence_key, current_step, status, started_at)
                 VALUES(?1,?2,0,'active',?3)",
                params![customer_id, key, now],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn advance_run(&self, run_id: i64, step: i64, status: &str, now: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE sequence_runs SET current_step=?2, status=?3, last_sent_at=?4,
                        completed_at = CASE WHEN ?3 IN ('completed','stopped') THEN ?4 ELSE completed_at END
                 WHERE id=?1",
                params![run_id, step, status, now],
            )?;
            Ok(())
        })
    }

    pub fn list_runs(&self, customer_id: Option<i64>) -> Result<Vec<SequenceRun>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM sequence_runs WHERE (?1 IS NULL OR customer_id = ?1)
                 ORDER BY started_at DESC LIMIT 100",
            )?;
            let rows = stmt
                .query_map(params![customer_id], |r| {
                    Ok(SequenceRun {
                        id: r.get("id")?,
                        customer_id: r.get("customer_id")?,
                        sequence_key: r.get("sequence_key")?,
                        current_step: r.get("current_step")?,
                        status: r.get("status")?,
                        started_at: r.get("started_at")?,
                        completed_at: r.get("completed_at")?,
                        last_sent_at: r.get("last_sent_at")?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// True if this customer already has an active run of this sequence — the
    /// guard against enrolling someone in `welcome` twice.
    pub fn has_active_run(&self, customer_id: i64, key: &str) -> Result<bool> {
        self.with(|c| {
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM sequence_runs
                 WHERE customer_id=?1 AND sequence_key=?2 AND status='active'",
                params![customer_id, key],
                |r| r.get(0),
            )?;
            Ok(n > 0)
        })
    }

    // ---- follow-up jobs ----

    pub fn enqueue_job(
        &self,
        customer_id: i64,
        job_type: &str,
        run_at: i64,
        payload: &str,
        now: i64,
    ) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO followup_jobs(customer_id, job_type, run_at, payload, status, created_at)
                 VALUES(?1,?2,?3,?4,'pending',?5)",
                params![customer_id, job_type, run_at, payload, now],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn due_jobs(&self, now: i64, limit: i64) -> Result<Vec<Job>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM followup_jobs WHERE status='pending' AND run_at <= ?1
                 ORDER BY run_at LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![now, limit], Self::row_to_job)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn mark_job(&self, id: i64, status: &str, error: &str, now: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE followup_jobs SET status=?2, error=?3, executed_at=?4 WHERE id=?1",
                params![id, status, error, now],
            )?;
            Ok(())
        })
    }

    /// Reclaim jobs left `running` by a crash. Called once at startup: without
    /// this they'd sit in limbo forever, since nothing else ever revisits them.
    pub fn requeue_stuck_jobs(&self) -> Result<usize> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE followup_jobs SET status='pending', error='' WHERE status='running'",
                [],
            )?;
            Ok(n)
        })
    }

    pub fn list_jobs(&self, customer_id: Option<i64>, limit: i64) -> Result<Vec<Job>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM followup_jobs WHERE (?1 IS NULL OR customer_id = ?1)
                 ORDER BY run_at DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![customer_id, limit], Self::row_to_job)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    // ---- settings ----

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.with(|c| {
            let v = c
                .query_row(
                    "SELECT value FROM settings WHERE key=?1",
                    params![key],
                    |r| r.get::<_, String>(0),
                )
                .optional()?;
            Ok(v)
        })
    }

    pub fn setting_or(&self, key: &str, default: &str) -> String {
        self.get_setting(key)
            .ok()
            .flatten()
            .unwrap_or_else(|| default.to_string())
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO settings(key, value) VALUES(?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
    }

    pub fn all_settings(&self) -> Result<serde_json::Map<String, serde_json::Value>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT key, value FROM settings")?;
            let mut map = serde_json::Map::new();
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .filter_map(|r| r.ok());
            for (k, v) in rows {
                map.insert(k, serde_json::Value::String(v));
            }
            Ok(map)
        })
    }

    // ---- reporting ----

    /// The sales funnel + win rate. `win_rate` is won / (won + churned), i.e.
    /// decided outcomes only — counting still-open leads as losses would make
    /// the number sag every time a new lead arrives.
    pub fn sale_stats(&self) -> Result<serde_json::Value> {
        self.with(|c| {
            let mut stmt =
                c.prepare("SELECT sale_stage, COUNT(*) FROM customers GROUP BY sale_stage")?;
            let mut funnel = serde_json::Map::new();
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                .filter_map(|r| r.ok());
            for (stage, n) in rows {
                funnel.insert(stage, serde_json::json!(n));
            }
            let won = funnel
                .get("closed_won")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let churned = funnel.get("churned").and_then(|v| v.as_i64()).unwrap_or(0);
            let decided = won + churned;
            let win_rate = if decided > 0 {
                (won as f64) * 100.0 / (decided as f64)
            } else {
                0.0
            };
            let hot: i64 = c.query_row(
                "SELECT COUNT(*) FROM customers WHERE temperature='hot'",
                [],
                |r| r.get(0),
            )?;
            let pending: i64 = c.query_row(
                "SELECT COUNT(*) FROM sale_reviews WHERE status='pending'",
                [],
                |r| r.get(0),
            )?;
            let open_esc: i64 = c.query_row(
                "SELECT COUNT(*) FROM sale_escalations WHERE status='open'",
                [],
                |r| r.get(0),
            )?;
            let unsub: i64 = c.query_row(
                "SELECT COUNT(*) FROM customers WHERE unsubscribed=1",
                [],
                |r| r.get(0),
            )?;
            let tokens: i64 = c.query_row(
                "SELECT COALESCE(SUM(tokens), 0) FROM sale_actions",
                [],
                |r| r.get(0),
            )?;
            Ok(serde_json::json!({
                "funnel": funnel,
                "won": won,
                "churned": churned,
                "winRate": (win_rate * 10.0).round() / 10.0,
                "hotLeads": hot,
                "pendingReviews": pending,
                "openEscalations": open_esc,
                "unsubscribed": unsub,
                "tokens": tokens,
            }))
        })
    }

    // ---- row mappers ----

    fn row_to_review(r: &rusqlite::Row) -> rusqlite::Result<Review> {
        Ok(Review {
            id: r.get("id")?,
            customer_id: r.get("customer_id")?,
            customer_name: r.get("customer_name")?,
            draft: r.get("draft")?,
            channel: r.get("channel")?,
            subject: r.get("subject")?,
            risk_reason: r.get("risk_reason")?,
            status: r.get("status")?,
            edited: r.get("edited")?,
            approved_by: r.get("approved_by")?,
            approved_at: r.get("approved_at")?,
            created_at: r.get("created_at")?,
        })
    }

    fn row_to_job(r: &rusqlite::Row) -> rusqlite::Result<Job> {
        Ok(Job {
            id: r.get("id")?,
            customer_id: r.get("customer_id")?,
            job_type: r.get("job_type")?,
            run_at: r.get("run_at")?,
            payload: r.get("payload")?,
            status: r.get("status")?,
            executed_at: r.get("executed_at")?,
            error: r.get("error")?,
            created_at: r.get("created_at")?,
        })
    }
}
