//! Dedupe and rank fusion.
//!
//! Two jobs, both mechanical (no LLM):
//!
//! 1. **Dedupe** — collapse the same item retrieved by several sources into one
//!    `Evidence` carrying every source's hit. Without this, corroboration
//!    counting is meaningless.
//! 2. **Fuse** — rank the merged set with Reciprocal Rank Fusion. Sources score
//!    on incompatible scales (BM25, cosine, SERP position, graph activation),
//!    so we fuse on RANK and never on raw score.
//!
//! See docs/search-app-design.md §5.2–5.3.

use crate::model::{Evidence, SourceKind};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

/// RRF smoothing constant. 60 is the value from the original Cormack et al.
/// paper and is what makes a rank-1 hit worth ~1.02× a rank-2 hit rather than
/// 2× — deliberately flat, so one source cannot dominate the fused order.
const RRF_K: f32 = 60.0;

/// Bonus per extra independent source KIND. `score * (1 + BETA * (kinds - 1))`.
const INDEPENDENCE_BETA: f32 = 0.25;

/// Hamming distance under which two snippets count as near-duplicates.
const SIMHASH_THRESHOLD: u32 = 3;

/// Tracking parameters stripped during URL canonicalization.
const TRACKING_PREFIXES: &[&str] = &["utm_", "ga_", "mc_", "pk_", "hsa_"];
const TRACKING_KEYS: &[&str] = &[
    "fbclid",
    "gclid",
    "dclid",
    "msclkid",
    "igshid",
    "mkt_tok",
    "ref",
    "ref_src",
    "ref_url",
    "source",
    "spm",
    "yclid",
    "_ga",
    "cmpid",
    "campaign_id",
];

/// Two-level public suffixes we care about, so `bbc.co.uk` and `vnexpress.net`
/// both resolve to the right registrable domain.
///
/// This is a pragmatic shortlist, NOT the Public Suffix List — an exotic
/// multi-level suffix will register one label too deep. That over-counts
/// independence in rare cases; it never under-counts.
const TWO_LEVEL_SUFFIXES: &[&str] = &[
    "co.uk", "org.uk", "ac.uk", "gov.uk", "co.jp", "or.jp", "ne.jp", "com.au", "net.au", "org.au",
    "com.br", "com.cn", "net.cn", "org.cn", "gov.cn", "com.vn", "net.vn", "org.vn", "edu.vn",
    "gov.vn", "com.sg", "com.hk", "com.tw", "co.kr", "co.in", "co.id", "co.th", "com.mx", "com.tr",
    "co.za", "com.ar",
];

/// Canonicalize a URL for identity comparison, returning `(canonical, domain)`.
///
/// Lowercases the host, drops `www.`, drops the fragment, drops tracking
/// parameters, sorts the remaining query, and collapses a trailing slash.
pub fn canonicalize_url(raw: &str) -> (Option<String>, Option<String>) {
    let parsed = match url::Url::parse(raw.trim()) {
        Ok(u) => u,
        Err(_) => return (None, None),
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return (None, None);
    }
    let host = match parsed.host_str() {
        Some(h) => h.to_lowercase(),
        None => return (None, None),
    };
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();

    let mut pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| {
            let k = k.to_ascii_lowercase();
            !TRACKING_KEYS.contains(&k.as_str())
                && !TRACKING_PREFIXES.iter().any(|p| k.starts_with(p))
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    pairs.sort();

    let mut path = parsed.path().to_string();
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }

    let mut canonical = format!("{}://{}{}", parsed.scheme(), host, path);
    if !pairs.is_empty() {
        let q: Vec<String> = pairs
            .iter()
            .map(|(k, v)| {
                if v.is_empty() {
                    k.clone()
                } else {
                    format!("{k}={v}")
                }
            })
            .collect();
        canonical.push('?');
        canonical.push_str(&q.join("&"));
    }

    (Some(canonical), Some(registrable_domain(&host)))
}

/// Registrable domain ("example.co.uk" from "news.example.co.uk").
pub fn registrable_domain(host: &str) -> String {
    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    if labels.len() <= 2 {
        return host.to_string();
    }
    let last_two = labels[labels.len() - 2..].join(".");
    if TWO_LEVEL_SUFFIXES.contains(&last_two.as_str()) && labels.len() >= 3 {
        labels[labels.len() - 3..].join(".")
    } else {
        last_two
    }
}

/// Stable evidence id. URL-bearing items key on the canonical URL so the same
/// page from different sources hashes identically; URL-less items fall back to
/// source + title + snippet.
pub fn evidence_id(
    canonical_url: Option<&str>,
    source_id: &str,
    title: &str,
    snippet: &str,
) -> String {
    let mut h = Sha256::new();
    match canonical_url {
        Some(u) => {
            h.update(b"u:");
            h.update(u.as_bytes());
        }
        None => {
            h.update(b"s:");
            h.update(source_id.as_bytes());
            h.update(b"\x1f");
            h.update(title.as_bytes());
            h.update(b"\x1f");
            h.update(snippet.as_bytes());
        }
    }
    hex::encode(&h.finalize()[..16])
}

// ---------------------------------------------------------------------------
// Near-duplicate detection
// ---------------------------------------------------------------------------

/// 64-bit SimHash over character 3-grams of the normalized text.
///
/// Character grams (not words) because the corpus is mixed Vietnamese/English
/// and word tokenization would need per-language rules to be fair.
pub fn simhash(text: &str) -> u64 {
    let norm: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect();
    let norm = norm.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = norm.chars().collect();
    if chars.len() < 3 {
        return 0;
    }

    let mut v = [0i32; 64];
    for w in chars.windows(3) {
        let gram: String = w.iter().collect();
        let mut h = Sha256::new();
        h.update(gram.as_bytes());
        let d = h.finalize();
        let bits = u64::from_be_bytes(d[..8].try_into().unwrap_or([0; 8]));
        for (i, slot) in v.iter_mut().enumerate() {
            if bits >> i & 1 == 1 {
                *slot += 1;
            } else {
                *slot -= 1;
            }
        }
    }
    let mut out = 0u64;
    for (i, slot) in v.iter().enumerate() {
        if *slot > 0 {
            out |= 1 << i;
        }
    }
    out
}

fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

// ---------------------------------------------------------------------------
// Dedupe + fuse
// ---------------------------------------------------------------------------

/// Fold `other`'s provenance into `keep`, preferring the richer text.
fn merge_into(keep: &mut Evidence, other: Evidence) {
    for hit in other.hits {
        // Same source twice (e.g. two sub-queries) — keep the better rank.
        if let Some(existing) = keep.hits.iter_mut().find(|h| h.source_id == hit.source_id) {
            if hit.rank < existing.rank {
                existing.rank = hit.rank;
                existing.raw_score = hit.raw_score;
            }
        } else {
            keep.hits.push(hit);
        }
    }
    if keep.full_text.is_none() {
        keep.full_text = other.full_text;
    }
    if other.snippet.len() > keep.snippet.len() {
        keep.snippet = other.snippet;
    }
    if keep.url.is_none() {
        keep.url = other.url;
    }
    if keep.published_at.is_none() {
        keep.published_at = other.published_at;
    }
    if keep.author.is_none() {
        keep.author = other.author;
    }
    if keep.title.trim().is_empty() {
        keep.title = other.title;
    }
}

/// Collapse duplicates: exact canonical-URL match first, then near-duplicate
/// text. Order of the survivors is not meaningful — `fuse` re-ranks.
pub fn dedupe(items: Vec<Evidence>) -> Vec<Evidence> {
    // Pass 1 — canonical URL.
    let mut by_url: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Evidence> = Vec::with_capacity(items.len());
    for ev in items {
        match ev.canonical_url.clone() {
            Some(u) => match by_url.get(&u) {
                Some(&idx) => merge_into(&mut out[idx], ev),
                None => {
                    by_url.insert(u, out.len());
                    out.push(ev);
                }
            },
            None => out.push(ev),
        }
    }

    // Pass 2 — near-duplicate body text. O(n²) on the survivors; fan-out result
    // sets are in the hundreds, so this stays well under a millisecond.
    let hashes: Vec<u64> = out.iter().map(|e| simhash(e.body())).collect();
    let mut absorbed: HashSet<usize> = HashSet::new();
    let mut merges: Vec<(usize, usize)> = Vec::new();
    for i in 0..out.len() {
        if absorbed.contains(&i) || hashes[i] == 0 {
            continue;
        }
        for j in (i + 1)..out.len() {
            if absorbed.contains(&j) || hashes[j] == 0 {
                continue;
            }
            // Different known URLs are different documents even if the
            // snippets look alike (syndicated copies must stay separate, or
            // independence counting silently collapses).
            let distinct_urls = matches!(
                (&out[i].canonical_url, &out[j].canonical_url),
                (Some(a), Some(b)) if a != b
            );
            if distinct_urls {
                continue;
            }
            if hamming(hashes[i], hashes[j]) <= SIMHASH_THRESHOLD {
                absorbed.insert(j);
                merges.push((i, j));
            }
        }
    }
    for (keep, gone) in merges {
        let taken = out[gone].clone();
        merge_into(&mut out[keep], taken);
    }
    let mut idx = 0;
    out.retain(|_| {
        let keep = !absorbed.contains(&idx);
        idx += 1;
        keep
    });
    out
}

/// Reciprocal Rank Fusion with an independence bonus.
///
/// ```text
/// rrf(e)   = Σ_s  w_s / (K + rank_s(e))
/// score(e) = rrf(e) · (1 + β · (independent_kinds(e) − 1))
/// ```
///
/// `weights` maps source id → prior trust weight (default 1.0).
pub fn fuse(items: &mut [Evidence], weights: &HashMap<String, f32>) {
    for ev in items.iter_mut() {
        let mut rrf = 0.0f32;
        let mut kinds: HashSet<SourceKind> = HashSet::new();
        for hit in &ev.hits {
            let w = weights.get(&hit.source_id).copied().unwrap_or(1.0);
            rrf += w / (RRF_K + hit.rank as f32);
            kinds.insert(hit.kind);
        }
        ev.independent_kinds = kinds.len();
        // A single document is one domain; the bonus comes from KIND diversity.
        ev.independent_domains = ev.domain.iter().count();
        let bonus = 1.0 + INDEPENDENCE_BETA * (kinds.len().saturating_sub(1)) as f32;
        ev.fused_score = rrf * bonus;
    }
    items.sort_by(|a, b| {
        b.fused_score
            .partial_cmp(&a.fused_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// The source that contributed an item's best (lowest) rank.
fn primary_source(ev: &Evidence) -> &str {
    ev.hits
        .iter()
        .min_by_key(|h| h.rank)
        .map(|h| h.source_id.as_str())
        .unwrap_or("")
}

/// Truncate to `limit` while guaranteeing every contributing source a fair
/// share of the slots.
///
/// Weighted RRF has a failure mode that only shows up on real data: when
/// sources carry systematically different weights, `w_s / (K + rank)` orders
/// by *source weight first, rank second*. A source weighted 1.3 sweeps every
/// slot ahead of a source weighted 1.0, no matter how good the latter's hits
/// are. Observed live: wiki (w=1.3) took all 8 slots and the web source — 7
/// real results — contributed nothing. A federated search that returns one
/// source has not aggregated anything.
///
/// So: cap each source at `ceil(limit / sources_present)`, walking in fused
/// order and deferring (never dropping) the overflow. Leftover slots are then
/// filled from the deferred pool, still in fused order — if only one source
/// actually returned anything, it still gets the whole list.
pub fn select_diverse(items: &mut Vec<Evidence>, limit: usize) {
    let limit = limit.max(1);
    if items.len() <= limit {
        return;
    }

    let present: HashSet<&str> = items.iter().map(primary_source).collect();
    let cap = limit.div_ceil(present.len().max(1)).max(1);

    let mut taken: HashMap<String, usize> = HashMap::new();
    let mut accepted: Vec<Evidence> = Vec::with_capacity(limit);
    let mut deferred: Vec<Evidence> = Vec::new();

    for ev in items.drain(..) {
        if accepted.len() >= limit {
            break;
        }
        let src = primary_source(&ev).to_string();
        let n = taken.entry(src).or_insert(0);
        if *n < cap {
            *n += 1;
            accepted.push(ev);
        } else {
            deferred.push(ev);
        }
    }

    for ev in deferred {
        if accepted.len() >= limit {
            break;
        }
        accepted.push(ev);
    }

    *items = accepted;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Evidence, SourceKind};

    #[test]
    fn canonicalize_strips_tracking_and_www() {
        let (c, d) =
            canonicalize_url("https://WWW.Example.com/a/b/?utm_source=x&q=1&fbclid=z#frag");
        assert_eq!(c.unwrap(), "https://example.com/a/b?q=1");
        assert_eq!(d.unwrap(), "example.com");
    }

    #[test]
    fn canonicalize_rejects_non_http() {
        assert_eq!(canonicalize_url("mailto:a@b.com").0, None);
        assert_eq!(canonicalize_url("not a url").0, None);
    }

    #[test]
    fn registrable_domain_handles_two_level_suffixes() {
        assert_eq!(registrable_domain("news.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(registrable_domain("vnexpress.net"), "vnexpress.net");
        assert_eq!(registrable_domain("a.b.vnexpress.net"), "vnexpress.net");
        assert_eq!(registrable_domain("shop.tiki.com.vn"), "tiki.com.vn");
    }

    #[test]
    fn same_url_from_two_sources_merges_into_one_evidence() {
        let a = Evidence::new(
            "web",
            SourceKind::Web,
            0,
            1.0,
            "T",
            "snippet a",
            Some("https://example.com/x?utm_source=g".into()),
        );
        let b = Evidence::new(
            "social:threads",
            SourceKind::Social,
            2,
            0.5,
            "T",
            "snippet b longer",
            Some("https://www.example.com/x".into()),
        );
        let merged = dedupe(vec![a, b]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].hits.len(), 2);
        // The longer snippet survives.
        assert_eq!(merged[0].snippet, "snippet b longer");
    }

    #[test]
    fn distinct_urls_with_similar_text_stay_separate() {
        // Syndicated copies must NOT collapse — that would erase the very
        // independence signal corroboration depends on.
        let text = "the quick brown fox jumps over the lazy dog repeatedly today";
        let a = Evidence::new(
            "web",
            SourceKind::Web,
            0,
            1.0,
            "A",
            text,
            Some("https://one.com/x".into()),
        );
        let b = Evidence::new(
            "web",
            SourceKind::Web,
            1,
            0.9,
            "B",
            text,
            Some("https://two.com/y".into()),
        );
        assert_eq!(dedupe(vec![a, b]).len(), 2);
    }

    #[test]
    fn urlless_near_duplicates_collapse() {
        let text = "Ngân hàng nhà nước công bố lãi suất điều hành mới trong quý này";
        let a = Evidence::new("wiki", SourceKind::Internal, 0, 1.0, "A", text, None);
        let b = Evidence::new(
            "knowledge",
            SourceKind::Internal,
            1,
            0.8,
            "B",
            &format!("{text}."),
            None,
        );
        let merged = dedupe(vec![a, b]);
        assert_eq!(merged.len(), 1, "near-identical text should merge");
        assert_eq!(merged[0].hits.len(), 2);
    }

    #[test]
    fn cross_kind_corroboration_outranks_a_better_single_rank() {
        // rank-1 from one kind vs rank-3 seen by two kinds: the corroborated
        // item wins. This is the whole point of the independence bonus.
        let mut solo = Evidence::new(
            "web",
            SourceKind::Web,
            0,
            1.0,
            "solo",
            "s",
            Some("https://a.com/1".into()),
        );
        solo.hits[0].rank = 1;

        let mut duo = Evidence::new(
            "web",
            SourceKind::Web,
            3,
            1.0,
            "duo",
            "d",
            Some("https://b.com/2".into()),
        );
        duo.hits.push(crate::model::SourceHit {
            source_id: "knowledge".into(),
            kind: SourceKind::Internal,
            rank: 3,
            raw_score: 1.0,
        });

        let mut items = vec![solo, duo];
        fuse(&mut items, &HashMap::new());
        assert_eq!(items[0].title, "duo");
        assert_eq!(items[0].independent_kinds, 2);
    }

    #[test]
    fn same_source_twice_keeps_the_better_rank_and_does_not_double_count() {
        // Two sub-queries hitting the same page from the same source must not
        // read as corroboration.
        let a = Evidence::new(
            "web",
            SourceKind::Web,
            7,
            1.0,
            "T",
            "x",
            Some("https://a.com/1".into()),
        );
        let b = Evidence::new(
            "web",
            SourceKind::Web,
            2,
            1.0,
            "T",
            "x",
            Some("https://a.com/1".into()),
        );
        let mut merged = dedupe(vec![a, b]);
        assert_eq!(merged[0].hits.len(), 1);
        assert_eq!(merged[0].hits[0].rank, 2);
        fuse(&mut merged, &HashMap::new());
        assert_eq!(merged[0].independent_kinds, 1);
    }

    #[test]
    fn weights_scale_contribution() {
        // Same rank from both sources — only the configured weight separates
        // them, so a distrusted source cannot outrank a trusted one.
        let mut items = vec![
            Evidence::new(
                "cheap",
                SourceKind::Web,
                0,
                1.0,
                "low",
                "x",
                Some("https://a.com/1".into()),
            ),
            Evidence::new(
                "trusted",
                SourceKind::Web,
                0,
                1.0,
                "high",
                "y",
                Some("https://b.com/2".into()),
            ),
        ];
        let w = HashMap::from([("cheap".to_string(), 0.2), ("trusted".to_string(), 2.0)]);
        fuse(&mut items, &w);
        assert_eq!(items[0].title, "high");
    }

    fn from_source(source: &str, kind: SourceKind, n: usize) -> Vec<Evidence> {
        (0..n)
            .map(|i| {
                Evidence::new(
                    source,
                    kind,
                    i as u32,
                    1.0,
                    format!("{source}-{i}"),
                    "body",
                    Some(format!("https://{source}.example/{i}")),
                )
            })
            .collect()
    }

    #[test]
    fn a_heavily_weighted_source_cannot_monopolize_the_result_list() {
        // The live regression: wiki (w=1.3) swept all 8 slots and the web
        // source's 7 real results never appeared.
        let mut items = from_source("wiki", SourceKind::Internal, 5);
        items.extend(from_source("knowledge", SourceKind::Internal, 8));
        items.extend(from_source("web", SourceKind::Web, 7));
        let w = HashMap::from([
            ("wiki".to_string(), 1.3),
            ("knowledge".to_string(), 1.2),
            ("web".to_string(), 1.0),
        ]);
        fuse(&mut items, &w);
        assert_eq!(
            primary_source(&items[0]),
            "wiki",
            "fusion order itself is unchanged"
        );

        select_diverse(&mut items, 8);
        assert_eq!(items.len(), 8);
        let sources: HashSet<&str> = items.iter().map(primary_source).collect();
        assert!(
            sources.contains("web"),
            "every contributing source must reach the list, got {sources:?}"
        );
        assert_eq!(sources.len(), 3);
    }

    #[test]
    fn a_single_source_still_gets_the_whole_list() {
        // Fair-share must not starve the result set when only one source
        // actually returned anything.
        let mut items = from_source("web", SourceKind::Web, 20);
        fuse(&mut items, &HashMap::new());
        select_diverse(&mut items, 8);
        assert_eq!(items.len(), 8);
    }

    #[test]
    fn a_source_with_few_hits_does_not_shrink_the_list() {
        // wiki has 1 hit and a cap of 4; its 3 unused slots must be refilled
        // from the deferred pool, not left empty.
        let mut items = from_source("wiki", SourceKind::Internal, 1);
        items.extend(from_source("web", SourceKind::Web, 20));
        fuse(&mut items, &HashMap::new());
        select_diverse(&mut items, 8);
        assert_eq!(items.len(), 8);
    }

    #[test]
    fn selection_below_the_limit_is_left_untouched() {
        let mut items = from_source("web", SourceKind::Web, 3);
        fuse(&mut items, &HashMap::new());
        select_diverse(&mut items, 8);
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn diverse_selection_preserves_fused_order_within_a_source() {
        let mut items = from_source("web", SourceKind::Web, 6);
        items.extend(from_source("wiki", SourceKind::Internal, 6));
        fuse(&mut items, &HashMap::new());
        select_diverse(&mut items, 6);
        let web: Vec<&str> = items
            .iter()
            .filter(|e| primary_source(e) == "web")
            .map(|e| e.title.as_str())
            .collect();
        let mut sorted = web.clone();
        sorted.sort();
        assert_eq!(web, sorted, "within a source, rank order must survive");
    }
}
