//! RelationshipEdge — typed, Hebbian-dynamic edge between two DataPoints.
//!
//! Port of shodh-memory `RelationshipEdge`. The struct is **storage-shaped**:
//! every field is also a column in `cog_edges`, so loading/saving is a 1:1
//! mapping. Hebbian / decay / LTP logic operates on this struct and lets
//! callers persist the result.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::ltp::{detect_ltp_status, LtpStatus};
use super::tiers::EdgeTier;

const HEBBIAN_LR: f32 = 0.1; // η — base learning rate
const STRENGTHEN_IMPORTANCE_FLOOR: f32 = 0.1;
const ACTIVATION_RING_CAP: usize = 32;
const LTP_PRUNE_FLOOR: f32 = 0.02;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipEdge {
    pub src: Uuid,
    pub dst: Uuid,
    pub predicate: String,
    pub props: Value,

    pub valid_from: i64,
    pub valid_to: Option<i64>,

    pub strength: f32,
    pub tier: EdgeTier,
    pub activation_count: u32,
    pub last_activated: i64,
    pub ltp_status: LtpStatus,
    pub ltp_detected_at: Option<i64>,
    pub entity_confidence: Option<f32>,
    pub endpoint_selectivity: Option<f32>,
    pub forman_curvature: Option<f32>,
    pub activation_timestamps: Vec<i64>,

    pub source_episode_id: Option<Uuid>,
    pub context: String,
    pub created_at: i64,
}

impl RelationshipEdge {
    pub fn new(src: Uuid, dst: Uuid, predicate: impl Into<String>, now: i64) -> Self {
        Self {
            src,
            dst,
            predicate: predicate.into(),
            props: Value::Object(Default::default()),
            valid_from: now,
            valid_to: None,
            strength: 0.1,
            tier: EdgeTier::L1Working,
            activation_count: 0,
            last_activated: now,
            ltp_status: LtpStatus::None,
            ltp_detected_at: None,
            entity_confidence: None,
            endpoint_selectivity: None,
            forman_curvature: None,
            activation_timestamps: Vec::new(),
            source_episode_id: None,
            context: String::new(),
            created_at: now,
        }
    }

    /// Read-only decay calculation — what the strength *would* be at `now`
    /// without mutating the edge. Used by retrievers to rank without
    /// triggering write traffic.
    pub fn effective_strength(&self, now: i64) -> f32 {
        let elapsed = (now - self.last_activated).max(0) as f32;
        let raw_decay = self.tier.decay_rate() * elapsed;
        let protection = self
            .ltp_status
            .effective_protection(self.endpoint_selectivity);
        let net = raw_decay / protection;
        (self.strength - net).max(0.0)
    }

    /// Hebbian strengthen: `w_new = w_old + η·(1 - w_old)·boost·importance_scale`.
    /// Returns `Some((from, to))` if the edge was promoted to a new tier.
    pub fn strengthen(&mut self, importance: f32, now: i64) -> Option<(EdgeTier, EdgeTier)> {
        let imp = importance.clamp(STRENGTHEN_IMPORTANCE_FLOOR, 1.0);
        let boost = self.tier.co_access_boost() * imp;
        let delta = HEBBIAN_LR * (1.0 - self.strength).max(0.0) * boost;
        self.strength = (self.strength + delta).min(1.5); // allow mild overshoot
        self.activation_count = self.activation_count.saturating_add(1);
        self.last_activated = now;

        // Ring buffer: drop oldest if at cap.
        if self.activation_timestamps.len() >= ACTIVATION_RING_CAP {
            self.activation_timestamps.remove(0);
        }
        self.activation_timestamps.push(now);

        // LTP detection
        let new_ltp = detect_ltp_status(&self.activation_timestamps, self.activation_count, now);
        if new_ltp as u8 > self.ltp_status as u8 {
            self.ltp_status = new_ltp;
            self.ltp_detected_at = Some(now);
        }

        // Promotion
        if self.strength >= self.tier.promotion_threshold() {
            if let Some(next) = self.tier.next() {
                let from = self.tier;
                self.tier = next;
                // After promotion, reset strength into the new tier's working range.
                self.strength = next.prune_threshold() + 0.05;
                return Some((from, next));
            }
        }
        None
    }

    /// Apply decay; return `true` if the edge should be pruned.
    pub fn decay(&mut self, now: i64) -> bool {
        let effective = self.effective_strength(now);
        self.strength = effective;

        // Zombie cleanup: LTP-protected but actually dead → strip protection.
        if effective <= LTP_PRUNE_FLOOR
            && matches!(
                self.ltp_status,
                LtpStatus::Full | LtpStatus::Weekly | LtpStatus::Burst
            )
        {
            self.ltp_status = LtpStatus::None;
            self.ltp_detected_at = None;
        }

        // Forced prune by max age (unless Full LTP protects it)
        if let Some(max_age) = self.tier.max_age_secs() {
            let age = now - self.created_at;
            if age > max_age && !matches!(self.ltp_status, LtpStatus::Full) {
                return true;
            }
        }

        // Strength prune (LTP::Full edges survive even when weak — Hebbian
        // permanence trumps simple threshold).
        if effective < self.tier.prune_threshold() && !matches!(self.ltp_status, LtpStatus::Full) {
            return true;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn fresh() -> RelationshipEdge {
        RelationshipEdge::new(Uuid::new_v4(), Uuid::new_v4(), "rel", 0)
    }

    #[test]
    fn strengthen_increases_strength_monotonically() {
        let mut e = fresh();
        let before = e.strength;
        e.strengthen(1.0, 1);
        assert!(e.strength > before);
        assert_eq!(e.activation_count, 1);
        assert_eq!(e.activation_timestamps.len(), 1);
    }

    #[test]
    fn effective_strength_decays_over_time() {
        let mut e = fresh();
        e.strength = 0.5;
        e.last_activated = 0;
        // 1 day later for L1 = 0.029/h * 24 ≈ 0.696 decay → clamped at 0
        let after = e.effective_strength(86_400);
        assert!(after < 0.5);
    }

    #[test]
    fn full_ltp_protects_from_prune() {
        let mut e = fresh();
        // Below the L1 prune threshold (0.05) but above the zombie floor (0.02)
        // so Full LTP genuinely protects rather than being stripped.
        e.strength = 0.03;
        e.ltp_status = LtpStatus::Full;
        e.last_activated = 0;
        let should_prune = e.decay(1_000);
        assert!(!should_prune);
    }

    #[test]
    fn promotion_advances_tier() {
        let mut e = fresh();
        // Strengthen repeatedly until we cross L1.promotion_threshold (0.6).
        let mut promoted = None;
        for t in 1..50 {
            if let Some(p) = e.strengthen(1.0, t) {
                promoted = Some(p);
                break;
            }
        }
        let (from, to) = promoted.expect("edge should promote within 50 strengthens");
        assert_eq!(from, EdgeTier::L1Working);
        assert_eq!(to, EdgeTier::L2Episodic);
    }
}
