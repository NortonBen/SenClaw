//! Human-cadence governor.
//!
//! Every write-ish action (post, dm, and the read scrapes that hammer a
//! platform) must pass through here before it is allowed to run. This is the
//! single choke point that enforces:
//!   * a minimum gap between two actions of the same class on the same account, and
//!   * a hard daily cap per action class.
//!
//! It is deliberately conservative and central so no individual MCP tool can
//! bypass the limits. It reduces — it does NOT eliminate — the risk of being
//! flagged; the platforms' ToS still forbid automation and detection is an
//! arms race.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Per action-class policy.
#[derive(Clone, Copy)]
pub struct Policy {
    pub min_gap: Duration,
    pub daily_cap: u32,
}

fn policy_for(action: &str) -> Policy {
    match action {
        // Posting is the highest-risk write; keep it slow and capped well under
        // TikTok's ~15–25/day official ceiling.
        "post" => Policy {
            min_gap: Duration::from_secs(90),
            daily_cap: 12,
        },
        // DM is reactive-only by product rule; still throttle hard.
        "dm" => Policy {
            min_gap: Duration::from_secs(30),
            daily_cap: 60,
        },
        // Reads still touch the platform; throttle but allow more.
        "search" | "feed" | "groups" | "inbox" => Policy {
            min_gap: Duration::from_secs(8),
            daily_cap: 400,
        },
        _ => Policy {
            min_gap: Duration::from_secs(15),
            daily_cap: 100,
        },
    }
}

struct Record {
    last: Option<Instant>,
    day: String,
    count: u32,
}

pub struct Cadence {
    inner: Mutex<HashMap<String, Record>>,
}

/// Outcome of a reservation attempt.
pub enum Decision {
    /// Cleared to run after waiting `delay` (jitter + remaining min-gap).
    Ok { delay: Duration },
    /// Daily cap reached; caller must not run.
    Blocked { reason: String },
}

impl Cadence {
    pub fn new() -> Self {
        Cadence {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn today() -> String {
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    }

    /// Cheap deterministic jitter in [0, span_ms) derived from the key + count,
    /// so we don't need a random source (which the surrounding harness forbids).
    fn jitter(key: &str, count: u32, span_ms: u64) -> Duration {
        let mut h: u64 = 1469598103934665603; // FNV-1a offset
        for b in key.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        h ^= count as u64;
        h = h.wrapping_mul(1099511628211);
        Duration::from_millis(h % span_ms.max(1))
    }

    /// Reserve a slot for `(platform, account, action)`. Returns how long the
    /// caller should sleep before acting, or a Blocked decision when the daily
    /// cap is hit. The reservation is recorded immediately (count incremented)
    /// so concurrent callers serialize correctly.
    pub fn reserve(&self, platform: &str, account: &str, action: &str) -> Decision {
        let policy = policy_for(action);
        let key = format!("{platform}:{account}:{action}");
        let today = Self::today();
        let now = Instant::now();

        let mut map = self.inner.lock().unwrap();
        let rec = map.entry(key.clone()).or_insert(Record {
            last: None,
            day: today.clone(),
            count: 0,
        });

        if rec.day != today {
            rec.day = today;
            rec.count = 0;
            rec.last = None;
        }

        if rec.count >= policy.daily_cap {
            return Decision::Blocked {
                reason: format!(
                    "đã chạm hạn mức {}/ngày cho '{action}' trên {platform} ({account}); thử lại ngày mai",
                    policy.daily_cap
                ),
            };
        }

        let mut delay = Duration::ZERO;
        if let Some(last) = rec.last {
            let elapsed = now.saturating_duration_since(last);
            if elapsed < policy.min_gap {
                delay = policy.min_gap - elapsed;
            }
        }
        delay += Self::jitter(&key, rec.count, 4000);

        rec.count += 1;
        rec.last = Some(now + delay);
        Decision::Ok { delay }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_action_has_only_jitter_delay() {
        let c = Cadence::new();
        match c.reserve("tiktok", "@a", "search") {
            Decision::Ok { delay } => assert!(delay < Duration::from_secs(5)),
            Decision::Blocked { .. } => panic!("first action must not block"),
        }
    }

    #[test]
    fn second_action_waits_at_least_the_min_gap_minus_jitter() {
        let c = Cadence::new();
        let _ = c.reserve("tiktok", "@a", "post");
        match c.reserve("tiktok", "@a", "post") {
            Decision::Ok { delay } => {
                // min_gap for post is 90s; even with jitter it stays large.
                assert!(delay >= Duration::from_secs(85));
            }
            Decision::Blocked { .. } => panic!(),
        }
    }

    #[test]
    fn daily_cap_eventually_blocks() {
        let c = Cadence::new();
        // post cap is 12.
        for _ in 0..12 {
            assert!(matches!(c.reserve("x", "@a", "post"), Decision::Ok { .. }));
        }
        assert!(matches!(
            c.reserve("x", "@a", "post"),
            Decision::Blocked { .. }
        ));
    }

    #[test]
    fn separate_accounts_do_not_share_a_bucket() {
        let c = Cadence::new();
        let _ = c.reserve("x", "@a", "post");
        assert!(matches!(c.reserve("x", "@b", "post"), Decision::Ok { .. }));
    }
}
