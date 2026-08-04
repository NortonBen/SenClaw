//! Scoring for the prediction ledger. Multi-class Brier: sum over outcomes of
//! (p_i − o_i)² — 0 is perfect, 2 is maximally wrong. `correct` = the argmax
//! outcome actually happened (ties break by first key order).

use serde_json::Value;

/// Score a probability map against the realized `outcome` key.
/// Unknown outcome keys count as "everything assigned elsewhere was wrong".
pub fn brier(probs: &Value, outcome: &str) -> f64 {
    let Some(map) = probs.as_object() else {
        return 2.0;
    };
    let mut score = 0.0;
    let mut outcome_seen = false;
    for (k, v) in map {
        let p = v.as_f64().unwrap_or(0.0);
        let o = if k == outcome { 1.0 } else { 0.0 };
        if k == outcome {
            outcome_seen = true;
        }
        score += (p - o) * (p - o);
    }
    if !outcome_seen {
        // The realized outcome had implicit probability 0.
        score += 1.0;
    }
    score
}

/// The outcome key the forecast committed to (highest probability).
pub fn argmax(probs: &Value) -> Option<String> {
    let map = probs.as_object()?;
    map.iter()
        .max_by(|a, b| {
            let pa = a.1.as_f64().unwrap_or(0.0);
            let pb = b.1.as_f64().unwrap_or(0.0);
            pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(k, _)| k.clone())
}

/// Convenience: score + correctness in one call.
pub fn score(probs: &Value, outcome: &str) -> (f64, bool) {
    let b = brier(probs, outcome);
    let correct = argmax(probs).as_deref() == Some(outcome);
    (b, correct)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn brier_perfect_and_worst() {
        let p = json!({ "H": 1.0, "D": 0.0, "A": 0.0 });
        assert!(brier(&p, "H").abs() < 1e-12);
        assert!((brier(&p, "A") - 2.0).abs() < 1e-12);
    }

    #[test]
    fn brier_typical() {
        let p = json!({ "H": 0.6, "D": 0.25, "A": 0.15 });
        // (0.6-1)² + 0.25² + 0.15² = 0.16 + 0.0625 + 0.0225 = 0.245
        assert!((brier(&p, "H") - 0.245).abs() < 1e-9);
        let (b, correct) = score(&p, "H");
        assert!((b - 0.245).abs() < 1e-9);
        assert!(correct);
        let (_, wrong) = score(&p, "D");
        assert!(!wrong);
    }

    #[test]
    fn brier_binary_hit_miss() {
        // Entertainment lottery pick ledgered at the honest baseline p≈0.238.
        let p = json!({ "hit": 0.238, "miss": 0.762 });
        let (b_miss, c_miss) = score(&p, "miss");
        assert!(b_miss < 0.2 && c_miss);
        let (b_hit, c_hit) = score(&p, "hit");
        assert!(b_hit > 1.0 && !c_hit);
    }

    #[test]
    fn unknown_outcome_penalized() {
        let p = json!({ "H": 0.7, "A": 0.3 });
        assert!((brier(&p, "D") - (0.49 + 0.09 + 1.0)).abs() < 1e-9);
        assert_eq!(argmax(&p).as_deref(), Some("H"));
    }
}
