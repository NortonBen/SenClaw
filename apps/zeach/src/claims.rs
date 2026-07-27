//! Claims, corroboration and confidence tiers — the mechanical half of §5.4.
//!
//! An LLM proposes atomic claims and binds each to evidence ids. Everything
//! after that is arithmetic, and deliberately so: a confidence tier decided by
//! a model is a model's opinion, while a tier decided by counting independent
//! sources is a fact about what was retrieved.
//!
//! The single most important guard lives here: **a claim may only cite evidence
//! that actually exists in the run**. Models invent ids. An invented id that
//! survives turns into a citation the reader can't check, on a claim that looks
//! corroborated — the exact failure this whole app is built to avoid.

use crate::model::{Evidence, SourceKind};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    Verified,
    Supported,
    SingleSource,
    Disputed,
    Unverified,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Verified => "verified",
            Tier::Supported => "supported",
            Tier::SingleSource => "single-source",
            Tier::Disputed => "disputed",
            Tier::Unverified => "unverified",
        }
    }

    /// Vietnamese label for the UI, phrased as provenance rather than truth.
    pub fn label_vi(self) -> &'static str {
        match self {
            Tier::Verified => "nhiều nguồn độc lập",
            Tier::Supported => "có nguồn hậu thuẫn",
            Tier::SingleSource => "chỉ một nguồn",
            Tier::Disputed => "các nguồn mâu thuẫn",
            Tier::Unverified => "không có bằng chứng",
        }
    }
}

/// A claim as proposed by the extractor, before validation.
#[derive(Debug, Clone, Deserialize)]
pub struct RawClaim {
    pub text: String,
    #[serde(default)]
    pub supports: Vec<String>,
    #[serde(default)]
    pub refutes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Claim {
    pub id: String,
    pub text: String,
    pub tier: Tier,
    /// Human label for the tier, so the UI never re-implements this mapping
    /// and drift between the two is impossible.
    pub tier_label: String,
    /// Provenance strength in [0,1]. **Not** a probability that the claim is
    /// true — see `CONFIDENCE_IS_PROVENANCE`.
    pub confidence: f32,
    pub independent_count: usize,
    pub agreement: f32,
    pub high_stakes: bool,
    pub supports: Vec<String>,
    pub refutes: Vec<String>,
    /// Evidence ids the extractor cited that do not exist in this run.
    pub dropped_citations: Vec<String>,
}

pub const CONFIDENCE_IS_PROVENANCE: &str =
    "Điểm tin cậy đo ĐỘ CHỨNG THỰC của nguồn, không đo tính đúng sai. \
     Ba nguồn cùng chép lại một bản tin sai vẫn cho điểm cao mà nội dung vẫn sai.";

#[derive(Debug, Clone, Serialize)]
pub struct Contradiction {
    pub id: String,
    pub claim_a: String,
    pub claim_b: String,
    pub summary: String,
}

/// One independent unit of support: a source *family* plus a publisher.
///
/// Counting distinct `(kind, registrable_domain)` pairs — not distinct source
/// ids — is what stops three social platforms echoing one press release from
/// reading as three confirmations.
///
/// Evidence with no domain (wiki pages, graph nodes, corpus chunks) collapses
/// to one unit per kind. That is deliberate: your wiki and your cognitive graph
/// are largely built from each other, so the same note appearing in both is you
/// agreeing with yourself, not two witnesses. It under-counts rather than
/// over-claims.
fn independent_units<'a>(
    ids: &[String],
    evidence: &'a [Evidence],
) -> HashSet<(SourceKind, Option<&'a str>)> {
    let mut units = HashSet::new();
    for e in evidence.iter().filter(|e| ids.iter().any(|id| *id == e.id)) {
        let domain = e.domain.as_deref();
        for hit in &e.hits {
            units.insert((hit.kind, domain));
        }
    }
    units
}

/// Numbers, money, dates, and legal/medical/financial vocabulary — the claims
/// where being wrong actually costs something, and which therefore earn the
/// adversarial pass in P3.
fn is_high_stakes(text: &str) -> bool {
    let lower = text.to_lowercase();

    // Any digit that is part of a quantity, percentage, money or date.
    let has_number = text.chars().any(|c| c.is_ascii_digit());
    if has_number
        && (lower.contains('%')
            || lower.contains("triệu")
            || lower.contains("tỷ")
            || lower.contains("nghìn")
            || lower.contains("đồng")
            || lower.contains("usd")
            || lower.contains("vnd")
            || lower.contains('$')
            || lower.contains("percent")
            || lower.contains("million")
            || lower.contains("billion"))
    {
        return true;
    }

    const RISKY: &[&str] = &[
        // legal
        "luật", "nghị định", "thông tư", "toà án", "tòa án", "phạt", "kiện", "hợp đồng",
        "bản quyền", "law", "court", "lawsuit", "illegal", "liability",
        // medical
        "thuốc", "liều", "bệnh", "điều trị", "chẩn đoán", "tác dụng phụ", "vắc xin",
        "dose", "treatment", "diagnosis", "symptom", "vaccine",
        // financial
        "lãi suất", "cổ phiếu", "đầu tư", "lợi nhuận", "thuế", "phá sản",
        "interest rate", "stock", "invest", "tax", "bankrupt",
    ];
    if RISKY.iter().any(|k| lower.contains(k)) {
        return true;
    }

    // Attribution: "theo X", "X cho biết", "X said" — a misattributed statement
    // is as damaging as a wrong number.
    lower.starts_with("theo ")
        || lower.contains(" cho biết")
        || lower.contains(" tuyên bố")
        || lower.contains(" khẳng định")
        || lower.contains(" said ")
        || lower.contains(" claimed ")
        || lower.contains(" announced ")
}

/// Validate a claim's citations and score it.
///
/// `evidence` is the run's fused evidence — the only ids a claim may cite.
pub fn assess(id: String, raw: &RawClaim, evidence: &[Evidence]) -> Claim {
    let known: HashSet<&str> = evidence.iter().map(|e| e.id.as_str()).collect();

    let mut dropped = Vec::new();
    let mut keep = |ids: &[String]| -> Vec<String> {
        let mut out = Vec::new();
        for id in ids {
            if known.contains(id.as_str()) {
                if !out.contains(id) {
                    out.push(id.clone());
                }
            } else if !dropped.contains(id) {
                dropped.push(id.clone());
            }
        }
        out
    };
    let supports = keep(&raw.supports);
    let refutes = keep(&raw.refutes);

    // An id cannot both support and refute the same claim; that is extractor
    // noise, and counting it twice would inflate `independent_count`.
    let refutes: Vec<String> = refutes
        .into_iter()
        .filter(|r| !supports.contains(r))
        .collect();

    let sup_units = independent_units(&supports, evidence);
    let ref_units = independent_units(&refutes, evidence);
    let independent_count = sup_units.len();

    // Agreement is measured in INDEPENDENT UNITS, not raw evidence rows: one
    // chatty source emitting five refuting snippets must not outvote two
    // genuinely separate publishers. (The design sketch said raw counts.)
    let total = sup_units.len() + ref_units.len();
    let agreement = if total == 0 {
        0.0
    } else {
        sup_units.len() as f32 / total as f32
    };

    let tier = if supports.is_empty() && refutes.is_empty() {
        Tier::Unverified
    } else if total > 1 && agreement < 0.7 {
        Tier::Disputed
    } else if independent_count >= 3 && agreement >= 0.8 {
        Tier::Verified
    } else if independent_count >= 2 && agreement >= 0.7 {
        Tier::Supported
    } else if independent_count == 1 {
        Tier::SingleSource
    } else {
        // Supported only by refuted/zero-unit evidence.
        Tier::Unverified
    };

    // Provenance score: saturating in independence, scaled by agreement.
    let breadth = 1.0 - (-(independent_count as f32) / 2.0).exp();
    let confidence = (breadth * agreement).clamp(0.0, 1.0);

    Claim {
        id,
        text: raw.text.trim().to_string(),
        tier,
        tier_label: tier.label_vi().to_string(),
        confidence,
        independent_count,
        agreement,
        high_stakes: is_high_stakes(&raw.text),
        supports,
        refutes,
        dropped_citations: dropped,
    }
}

/// Assess a batch, dropping claims with no usable text.
pub fn assess_all(raws: &[RawClaim], evidence: &[Evidence]) -> Vec<Claim> {
    raws.iter()
        .filter(|r| !r.text.trim().is_empty())
        .enumerate()
        .map(|(i, r)| assess(crate::db::new_id(&format!("cl{i}")), r, evidence))
        .collect()
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawContradiction {
    #[serde(default)]
    pub claim_a: usize,
    #[serde(default)]
    pub claim_b: usize,
    #[serde(default)]
    pub summary: String,
}

/// Keep only contradictions whose both sides exist and differ.
///
/// A contradiction pointing at a claim index that was dropped would render as
/// a disagreement between a claim and nothing.
pub fn validate_contradictions(raws: &[RawContradiction], claims: &[Claim]) -> Vec<Contradiction> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (i, r) in raws.iter().enumerate() {
        let (Some(a), Some(b)) = (claims.get(r.claim_a), claims.get(r.claim_b)) else {
            continue;
        };
        if a.id == b.id {
            continue;
        }
        // Order-independent dedupe: (a,b) and (b,a) are one disagreement.
        let key = if a.id < b.id {
            (a.id.clone(), b.id.clone())
        } else {
            (b.id.clone(), a.id.clone())
        };
        if !seen.insert(key) {
            continue;
        }
        out.push(Contradiction {
            id: crate::db::new_id(&format!("ct{i}")),
            claim_a: a.id.clone(),
            claim_b: b.id.clone(),
            summary: r.summary.trim().to_string(),
        });
    }
    out
}

/// Claims that a contradiction touches must not read as settled.
pub fn mark_disputed(claims: &mut [Claim], contradictions: &[Contradiction]) {
    let touched: HashSet<&str> = contradictions
        .iter()
        .flat_map(|c| [c.claim_a.as_str(), c.claim_b.as_str()])
        .collect();
    for c in claims.iter_mut() {
        if touched.contains(c.id.as_str()) && matches!(c.tier, Tier::Verified | Tier::Supported) {
            c.tier = Tier::Disputed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Evidence;

    fn ev(id: &str, source: &str, kind: SourceKind, url: Option<&str>) -> Evidence {
        let mut e = Evidence::new(source, kind, 0, 1.0, "t", "s", url.map(String::from));
        e.id = id.to_string();
        e
    }

    fn raw(text: &str, supports: &[&str], refutes: &[&str]) -> RawClaim {
        RawClaim {
            text: text.into(),
            supports: supports.iter().map(|s| s.to_string()).collect(),
            refutes: refutes.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Three genuinely separate web publishers.
    fn three_publishers() -> Vec<Evidence> {
        vec![
            ev("e1", "web", SourceKind::Web, Some("https://vnexpress.net/a")),
            ev("e2", "web", SourceKind::Web, Some("https://tuoitre.vn/b")),
            ev("e3", "web", SourceKind::Web, Some("https://thanhnien.vn/c")),
        ]
    }

    #[test]
    fn three_independent_publishers_reach_verified() {
        let evs = three_publishers();
        let c = assess("c1".into(), &raw("Lãi suất giữ nguyên.", &["e1", "e2", "e3"], &[]), &evs);
        assert_eq!(c.independent_count, 3);
        assert_eq!(c.agreement, 1.0);
        assert_eq!(c.tier, Tier::Verified);
    }

    #[test]
    fn three_pages_on_one_domain_are_one_independent_source() {
        // The core anti-echo rule: same publisher, three URLs, one voice.
        let evs = vec![
            ev("e1", "web", SourceKind::Web, Some("https://vnexpress.net/a")),
            ev("e2", "web", SourceKind::Web, Some("https://vnexpress.net/b")),
            ev("e3", "web", SourceKind::Web, Some("https://m.vnexpress.net/c")),
        ];
        let c = assess("c1".into(), &raw("X.", &["e1", "e2", "e3"], &[]), &evs);
        assert_eq!(c.independent_count, 1, "one publisher is one source");
        assert_eq!(c.tier, Tier::SingleSource);
    }

    #[test]
    fn a_hallucinated_evidence_id_is_dropped_and_reported() {
        // A model citing an id that does not exist must not produce a claim
        // that looks corroborated by an uncheckable citation.
        let evs = three_publishers();
        let c = assess("c1".into(), &raw("X.", &["e1", "e999", "nope"], &[]), &evs);
        assert_eq!(c.supports, vec!["e1".to_string()]);
        assert_eq!(c.dropped_citations, vec!["e999".to_string(), "nope".to_string()]);
        assert_eq!(c.independent_count, 1);
    }

    #[test]
    fn a_claim_citing_only_invented_evidence_is_unverified_not_supported() {
        let evs = three_publishers();
        let c = assess("c1".into(), &raw("X.", &["ghost1", "ghost2"], &[]), &evs);
        assert_eq!(c.tier, Tier::Unverified);
        assert_eq!(c.confidence, 0.0);
        assert_eq!(c.dropped_citations.len(), 2);
    }

    #[test]
    fn genuine_conflict_becomes_disputed_not_a_confident_pick() {
        let evs = three_publishers();
        let c = assess("c1".into(), &raw("X.", &["e1"], &["e2", "e3"]), &evs);
        assert_eq!(c.tier, Tier::Disputed);
        assert!(c.agreement < 0.7);
    }

    #[test]
    fn one_chatty_source_cannot_outvote_two_publishers() {
        // Agreement is counted in independent units, not raw rows: five
        // refuting snippets from one site must not beat two separate ones.
        let mut evs = vec![
            ev("s1", "web", SourceKind::Web, Some("https://a.vn/1")),
            ev("s2", "web", SourceKind::Web, Some("https://b.vn/1")),
        ];
        for i in 0..5 {
            evs.push(ev(
                &format!("r{i}"),
                "web",
                SourceKind::Web,
                Some(&format!("https://spam.vn/{i}")),
            ));
        }
        let refs: Vec<&str> = ["r0", "r1", "r2", "r3", "r4"].into();
        let c = assess("c1".into(), &raw("X.", &["s1", "s2"], &refs), &evs);
        assert_eq!(c.independent_count, 2);
        // 2 supporting units vs 1 refuting unit → 0.667. Still disputed, but
        // by an honest 2-vs-1, not a misleading 2-vs-5.
        assert!((c.agreement - 2.0 / 3.0).abs() < 0.01, "got {}", c.agreement);
    }

    #[test]
    fn the_same_id_cannot_both_support_and_refute() {
        let evs = three_publishers();
        let c = assess("c1".into(), &raw("X.", &["e1", "e2"], &["e2"]), &evs);
        assert!(c.refutes.is_empty(), "contradictory binding must not double-count");
        assert_eq!(c.independent_count, 2);
    }

    #[test]
    fn duplicate_citations_do_not_inflate_the_count() {
        let evs = three_publishers();
        let c = assess("c1".into(), &raw("X.", &["e1", "e1", "e1"], &[]), &evs);
        assert_eq!(c.supports.len(), 1);
        assert_eq!(c.independent_count, 1);
    }

    #[test]
    fn different_kinds_on_one_domain_still_count_separately() {
        // A YouTube video and a web page on youtube.com are different families
        // of evidence even though the domain matches.
        let evs = vec![
            ev("e1", "web", SourceKind::Web, Some("https://youtube.com/watch?v=1")),
            ev("e2", "youtube", SourceKind::Social, Some("https://youtube.com/watch?v=2")),
        ];
        let c = assess("c1".into(), &raw("X.", &["e1", "e2"], &[]), &evs);
        assert_eq!(c.independent_count, 2);
    }

    #[test]
    fn internal_sources_without_domains_collapse_to_one_unit_per_kind() {
        // Your wiki agreeing with your knowledge graph is you agreeing with
        // yourself — under-count rather than over-claim.
        let evs = vec![
            ev("e1", "wiki", SourceKind::Internal, None),
            ev("e2", "knowledge", SourceKind::Internal, None),
        ];
        let c = assess("c1".into(), &raw("X.", &["e1", "e2"], &[]), &evs);
        assert_eq!(c.independent_count, 1);
        assert_eq!(c.tier, Tier::SingleSource);
    }

    #[test]
    fn high_stakes_detection_catches_money_law_medicine_and_attribution() {
        for t in [
            "Lãi suất giảm còn 4,5%.",
            "Giá vàng đạt 14,8 triệu đồng mỗi chỉ.",
            "Nghị định mới quy định mức phạt.",
            "Liều thuốc khuyến cáo là 500mg.",
            "Theo Ngân hàng Nhà nước, tỷ giá ổn định.",
            "The CEO announced a $2 billion investment.",
        ] {
            assert!(is_high_stakes(t), "should be high stakes: {t}");
        }
        for t in ["Trời hôm nay đẹp.", "Con mèo đang ngủ."] {
            assert!(!is_high_stakes(t), "should not be high stakes: {t}");
        }
    }

    #[test]
    fn a_bare_number_without_a_unit_is_not_automatically_high_stakes() {
        assert!(!is_high_stakes("Có 3 con mèo trong sân."));
    }

    #[test]
    fn confidence_rises_with_independence_but_never_reaches_certainty() {
        let evs = three_publishers();
        let one = assess("a".into(), &raw("X.", &["e1"], &[]), &evs).confidence;
        let three = assess("b".into(), &raw("X.", &["e1", "e2", "e3"], &[]), &evs).confidence;
        assert!(three > one);
        assert!(three < 1.0, "provenance is never certainty: {three}");
    }

    #[test]
    fn contradictions_referencing_missing_claims_are_dropped() {
        let evs = three_publishers();
        let claims = assess_all(&[raw("A.", &["e1"], &[]), raw("B.", &["e2"], &[])], &evs);
        let raws = vec![
            RawContradiction { claim_a: 0, claim_b: 1, summary: "trái ngược".into() },
            RawContradiction { claim_a: 0, claim_b: 99, summary: "ghost".into() },
            RawContradiction { claim_a: 1, claim_b: 1, summary: "self".into() },
        ];
        let out = validate_contradictions(&raws, &claims);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].summary, "trái ngược");
    }

    #[test]
    fn the_same_disagreement_reported_twice_is_stored_once() {
        let evs = three_publishers();
        let claims = assess_all(&[raw("A.", &["e1"], &[]), raw("B.", &["e2"], &[])], &evs);
        let raws = vec![
            RawContradiction { claim_a: 0, claim_b: 1, summary: "x".into() },
            RawContradiction { claim_a: 1, claim_b: 0, summary: "x đảo chiều".into() },
        ];
        assert_eq!(validate_contradictions(&raws, &claims).len(), 1);
    }

    #[test]
    fn a_contradicted_claim_cannot_stay_verified() {
        let evs = three_publishers();
        let mut claims = assess_all(
            &[raw("A.", &["e1", "e2", "e3"], &[]), raw("B.", &["e1"], &[])],
            &evs,
        );
        assert_eq!(claims[0].tier, Tier::Verified);
        let cts = validate_contradictions(
            &[RawContradiction { claim_a: 0, claim_b: 1, summary: "ngược nhau".into() }],
            &claims,
        );
        mark_disputed(&mut claims, &cts);
        assert_eq!(claims[0].tier, Tier::Disputed, "a disagreement must not read as settled");
    }

    #[test]
    fn empty_claims_are_discarded() {
        let evs = three_publishers();
        assert!(assess_all(&[raw("   ", &["e1"], &[])], &evs).is_empty());
    }
}
