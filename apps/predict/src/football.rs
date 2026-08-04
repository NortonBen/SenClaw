//! Football prediction math: ClubElo win expectancy → 1X2 probabilities, and a
//! bivariate-independent Poisson score matrix for best score / Over 2.5 / BTTS.
//! Pure functions — the LLM layer only *narrates* these numbers, never invents
//! its own. Team-strength inputs come from the ClubElo snapshot; until a
//! football-data.org key is configured, goal expectations are derived from the
//! Elo difference (documented simplification, see docs/sieu-du-doan-app-design.md).

use serde_json::{json, Value};

/// Home advantage in Elo points (ClubElo's own home-field estimate is ~65).
pub const HOME_ADV: f64 = 65.0;
/// League-average goals used to anchor Poisson expectations.
const AVG_HOME_GOALS: f64 = 1.5;
const AVG_AWAY_GOALS: f64 = 1.2;
/// Max goals modelled per side in the score matrix (0..=MAX_GOALS).
const MAX_GOALS: usize = 6;

/// 1X2 probabilities from two Elo ratings. Draw share is highest for evenly
/// matched sides (~26%) and shrinks as the mismatch grows.
pub fn elo_probs(elo_home: f64, elo_away: f64) -> (f64, f64, f64) {
    let diff = elo_home + HOME_ADV - elo_away;
    // Win expectancy for home counting a draw as half a win.
    let e = 1.0 / (1.0 + 10f64.powf(-diff / 400.0));
    let p_draw = (0.26 * (1.0 - (e - 0.5).abs() * 1.6)).clamp(0.05, 0.30);
    let p_home = (e - p_draw / 2.0).clamp(0.02, 0.95);
    let p_away = (1.0 - e - p_draw / 2.0).clamp(0.02, 0.95);
    normalize3(p_home, p_draw, p_away)
}

fn normalize3(a: f64, b: f64, c: f64) -> (f64, f64, f64) {
    let s = a + b + c;
    (a / s, b / s, c / s)
}

/// Expected goals per side derived from the Elo difference. A 400-point edge
/// roughly x2.5s the favourite's expectation; clamped to keep λ sane.
pub fn lambdas_from_elo(elo_home: f64, elo_away: f64) -> (f64, f64) {
    let diff = elo_home + HOME_ADV - elo_away;
    let lh = (AVG_HOME_GOALS * 10f64.powf(diff / 1000.0)).clamp(0.2, 4.5);
    let la = (AVG_AWAY_GOALS * 10f64.powf(-diff / 1000.0)).clamp(0.2, 4.5);
    (lh, la)
}

fn poisson_pmf(lambda: f64, k: usize) -> f64 {
    let mut p = (-lambda).exp();
    for i in 1..=k {
        p *= lambda / i as f64;
    }
    p
}

/// Derived quantities from the (MAX_GOALS+1)² independent-Poisson score matrix.
pub struct ScoreModel {
    pub p_home: f64,
    pub p_draw: f64,
    pub p_away: f64,
    pub best_score: (usize, usize),
    pub best_score_p: f64,
    pub p_over25: f64,
    pub p_btts: f64,
}

pub fn poisson_model(lambda_home: f64, lambda_away: f64) -> ScoreModel {
    let ph: Vec<f64> = (0..=MAX_GOALS)
        .map(|k| poisson_pmf(lambda_home, k))
        .collect();
    let pa: Vec<f64> = (0..=MAX_GOALS)
        .map(|k| poisson_pmf(lambda_away, k))
        .collect();
    let (mut p_h, mut p_d, mut p_a) = (0.0, 0.0, 0.0);
    let (mut best, mut best_p) = ((0usize, 0usize), 0.0f64);
    let (mut over25, mut btts) = (0.0, 0.0);
    for (i, phi) in ph.iter().enumerate() {
        for (j, paj) in pa.iter().enumerate() {
            let p = phi * paj;
            match i.cmp(&j) {
                std::cmp::Ordering::Greater => p_h += p,
                std::cmp::Ordering::Equal => p_d += p,
                std::cmp::Ordering::Less => p_a += p,
            }
            if p > best_p {
                best_p = p;
                best = (i, j);
            }
            if i + j >= 3 {
                over25 += p;
            }
            if i >= 1 && j >= 1 {
                btts += p;
            }
        }
    }
    // Renormalize over the truncated matrix so 1X2 sums to 1.
    let s = p_h + p_d + p_a;
    ScoreModel {
        p_home: p_h / s,
        p_draw: p_d / s,
        p_away: p_a / s,
        best_score: best,
        best_score_p: best_p,
        p_over25: over25,
        p_btts: btts,
    }
}

/// Elo(60%) + Poisson(40%) blended 1X2 plus the Poisson-only score outputs,
/// as the JSON payload every consumer (REST/MCP/LLM narration) uses.
pub fn predict(home: &str, away: &str, elo_home: f64, elo_away: f64) -> Value {
    let (eh, ed, ea) = elo_probs(elo_home, elo_away);
    let (lh, la) = lambdas_from_elo(elo_home, elo_away);
    let m = poisson_model(lh, la);
    let (p_home, p_draw, p_away) = normalize3(
        0.6 * eh + 0.4 * m.p_home,
        0.6 * ed + 0.4 * m.p_draw,
        0.6 * ea + 0.4 * m.p_away,
    );
    json!({
        "home": home,
        "away": away,
        "elo_home": (elo_home * 10.0).round() / 10.0,
        "elo_away": (elo_away * 10.0).round() / 10.0,
        "p_home": round3(p_home),
        "p_draw": round3(p_draw),
        "p_away": round3(p_away),
        "lambda_home": round3(lh),
        "lambda_away": round3(la),
        "best_score": format!("{}-{}", m.best_score.0, m.best_score.1),
        "best_score_p": round3(m.best_score_p),
        "p_over25": round3(m.p_over25),
        "p_btts": round3(m.p_btts),
    })
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

// ---- team-name matching against the ClubElo table ----

/// Lowercase, strip diacritics-free punctuation and noise words (FC, AFC, CF…).
pub fn normalize_team(name: &str) -> String {
    let lowered = name.to_lowercase();
    let cleaned: String = lowered
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    cleaned
        .split_whitespace()
        .filter(|w| {
            !matches!(
                *w,
                "fc" | "afc" | "cf" | "ac" | "as" | "ssc" | "cd" | "sc" | "club"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Hand alias map: TheSportsDB full names → ClubElo short names.
fn alias(norm: &str) -> Option<&'static str> {
    Some(match norm {
        "manchester city" => "man city",
        "manchester united" => "man united",
        "newcastle united" => "newcastle",
        "tottenham hotspur" => "tottenham",
        "wolverhampton wanderers" => "wolves",
        "west ham united" => "west ham",
        "brighton and hove albion" | "brighton hove albion" => "brighton",
        "nottingham forest" => "forest",
        "sheffield united" => "sheffield united",
        "leeds united" => "leeds",
        "leicester city" => "leicester",
        "paris saint germain" | "paris sg" => "paris sg",
        "bayern munich" | "bayern münchen" | "bayern munchen" => "bayern",
        "borussia dortmund" => "dortmund",
        "bayer leverkusen" => "leverkusen",
        "borussia monchengladbach" | "borussia mönchengladbach" => "gladbach",
        "atletico madrid" | "atlético madrid" => "atletico",
        "athletic bilbao" | "athletic club" => "bilbao",
        "real sociedad" => "sociedad",
        "real betis" => "betis",
        "internazionale" | "inter milan" => "inter",
        "milan" => "milan",
        "juventus" => "juventus",
        "napoli" => "napoli",
        "roma" => "roma",
        "sporting cp" | "sporting lisbon" => "sporting",
        _ => return None,
    })
}

/// Find a team's Elo in the snapshot. Returns `(elo, matched_name)`;
/// `None` when nothing matches (caller falls back to a league-average rating).
pub fn find_elo(table: &[(String, String, f64)], name: &str) -> Option<(f64, String)> {
    let norm = normalize_team(name);
    let target = alias(&norm)
        .map(str::to_string)
        .unwrap_or_else(|| norm.clone());
    // Exact normalized match first, then substring either way.
    for (club, _, elo) in table {
        if normalize_team(club) == target {
            return Some((*elo, club.clone()));
        }
    }
    for (club, _, elo) in table {
        let cn = normalize_team(club);
        if !cn.is_empty() && (target.contains(&cn) || cn.contains(&target)) {
            return Some((*elo, club.clone()));
        }
    }
    None
}

/// Rating used when a club is missing from the ClubElo snapshot.
pub const FALLBACK_ELO: f64 = 1600.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elo_probs_sum_to_one_and_order() {
        let (h, d, a) = elo_probs(2000.0, 1700.0);
        assert!((h + d + a - 1.0).abs() < 1e-9);
        assert!(h > a, "stronger home side must be favourite");
        assert!(d < h);
        // Even matchup: draw share near its max, home edge from HOME_ADV.
        let (h2, d2, a2) = elo_probs(1800.0, 1800.0);
        assert!((h2 + d2 + a2 - 1.0).abs() < 1e-9);
        assert!(d2 > 0.2 && h2 > a2);
    }

    #[test]
    fn poisson_model_sane() {
        let m = poisson_model(1.8, 1.0);
        assert!((m.p_home + m.p_draw + m.p_away - 1.0).abs() < 1e-9);
        assert!(m.p_home > m.p_away);
        assert!(m.p_over25 > 0.0 && m.p_over25 < 1.0);
        assert!(m.p_btts > 0.0 && m.p_btts < 1.0);
        // Equal small lambdas → 1-1 or 0-0 as modal score.
        let even = poisson_model(1.1, 1.1);
        assert!(matches!(even.best_score, (0, 0) | (1, 1)));
    }

    #[test]
    fn predict_payload_shape() {
        let v = predict("Arsenal", "Chelsea", 2050.0, 1900.0);
        let sum = v["p_home"].as_f64().unwrap()
            + v["p_draw"].as_f64().unwrap()
            + v["p_away"].as_f64().unwrap();
        assert!((sum - 1.0).abs() < 0.01);
        assert!(v["best_score"].as_str().unwrap().contains('-'));
    }

    #[test]
    fn team_matching() {
        let table = vec![
            ("Man City".to_string(), "ENG".to_string(), 1970.0),
            ("Arsenal".to_string(), "ENG".to_string(), 2063.0),
            ("Paris SG".to_string(), "FRA".to_string(), 1940.0),
        ];
        assert_eq!(find_elo(&table, "Manchester City").unwrap().1, "Man City");
        assert_eq!(find_elo(&table, "Arsenal FC").unwrap().1, "Arsenal");
        assert_eq!(
            find_elo(&table, "Paris Saint-Germain").unwrap().1,
            "Paris SG"
        );
        assert!(find_elo(&table, "Hà Nội FC").is_none());
    }
}
