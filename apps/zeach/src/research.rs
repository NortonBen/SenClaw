//! `zeach_research` — the deep, multi-round, verified, report-producing pipeline.
//!
//! `zeach_search` (in `pipeline.rs`) is the fast, LLM-free retrieval channel
//! other components share. `research` is the opposite trade: it spends LLM calls
//! and several rounds of retrieval to answer a question *thoroughly enough to be
//! trustworthy*, and hands back a cited report rather than a list of links.
//!
//! Shape of one run:
//!   1. **Plan** — expand the question into several sub-queries (LLM), so the
//!      fan-out covers definitions, figures, disputes and recent updates, not
//!      one phrasing.
//!   2. **Gather** — run every sub-query through the shared fan-out
//!      (`pipeline::run`), then merge all their evidence into one fused,
//!      diversity-capped set and deepen the top web hits.
//!   3. **Screen** — the relevance checkpoint (`review::screen_evidence`). Every
//!      source is a nearest-neighbour retriever: it answers "closest to the
//!      query", never "about the query". Anything off topic is set aside HERE,
//!      before it can be mined for claims — otherwise a run about world
//!      disasters comes back as a well-cited report about something else.
//!   4. **Verify** — extract atomic claims and score each by COUNTING
//!      independent sources (`claims::assess`). This is the cross-source check:
//!      a claim only reaches `verified`/`supported` when ≥2 independent
//!      source-families back it; conflicts become `disputed`, never a silent
//!      pick.
//!   5. **Follow-up** (deep only) — chase the weak and high-stakes claims with a
//!      second gather round, then re-verify over the enlarged evidence.
//!   6. **Synthesize** — write a cited Markdown report (`synthesize.rs`), always
//!      with a deterministic floor so a failed LLM never yields "no report".
//!   7. **Review** — the second checkpoint (`review::review_report`): an
//!      independent pass judges the finished report against the ORIGINAL
//!      question. A report that fails is still returned — hiding it would hide
//!      its evidence — but it is labelled, and `status` says so.
//!
//! Persistence and optional export to wiki/knowledge are done by the caller
//! (`mcp.rs`), mirroring how `zeach_search` saves its run there — this module
//! stays pure so it is testable without a database or a live daemon.

use crate::claims::{self, Claim, Contradiction};
use crate::extract;
use crate::fusion;
use crate::model::{Evidence, SourceOutcome};
use crate::pipeline::{self, SearchRequest};
use crate::review::{self, ReportReview};
use crate::sources::Registry;
use crate::synthesize;
use crate::transport::{Bridge, Transports};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Depth {
    /// One phrasing, no page-deepening, no follow-up. Fast.
    Quick,
    /// A few sub-queries, deepen the top web hits. The default.
    Standard,
    /// Wide sub-query fan-out, deepen more, plus a follow-up round that chases
    /// weak and high-stakes claims. Slowest, most thorough.
    Deep,
}

impl Depth {
    pub fn parse(s: &str) -> Depth {
        match s.trim().to_lowercase().as_str() {
            "quick" | "fast" | "nhanh" => Depth::Quick,
            "deep" | "sâu" | "thorough" => Depth::Deep,
            _ => Depth::Standard,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Depth::Quick => "quick",
            Depth::Standard => "standard",
            Depth::Deep => "deep",
        }
    }
    /// Total sub-queries (including the verbatim question).
    fn sub_query_cap(self) -> usize {
        match self {
            Depth::Quick => 1,
            Depth::Standard => 3,
            Depth::Deep => 5,
        }
    }
    /// Per-source result cap for each sub-query's fan-out.
    fn per_source_limit(self) -> usize {
        match self {
            Depth::Quick => 8,
            Depth::Standard => 10,
            Depth::Deep => 12,
        }
    }
    /// How many merged top web hits get their full page text fetched.
    fn deepen_top(self) -> usize {
        match self {
            Depth::Quick => 0,
            Depth::Standard => 4,
            Depth::Deep => 6,
        }
    }
    fn wants_follow_up(self) -> bool {
        matches!(self, Depth::Deep)
    }
}

pub struct ResearchRequest {
    pub query: String,
    pub sources: Option<Vec<String>>,
    pub lang: Option<String>,
    pub depth: Depth,
    /// Upper bound on the merged evidence set carried into synthesis.
    pub max_evidence: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResearchOutcome {
    pub query: String,
    pub depth: Depth,
    pub sub_queries: Vec<String>,
    pub evidence: Vec<Evidence>,
    /// Retrieved but judged off topic, so never reasoned over. Returned so a
    /// filtered run is legible instead of mysteriously thin.
    pub off_topic: Vec<Evidence>,
    pub sources: Vec<SourceOutcome>,
    pub unknown_sources: Vec<String>,
    pub claims: Vec<Claim>,
    pub contradictions: Vec<Contradiction>,
    /// `ok` | `off_topic` (a report was written but does not answer the
    /// question) | `insufficient` (nothing on topic was found at all).
    pub status: String,
    /// Verdict of the post-synthesis checkpoint, when one ran.
    pub review: Option<ReportReview>,
    pub report_title: String,
    pub report_markdown: String,
    /// True when the LLM wrote the prose; false when we fell back to the
    /// deterministic assembly.
    pub report_llm: bool,
    pub confidence_note: String,
    pub rounds: usize,
    pub total_before_dedupe: usize,
    pub deepened: usize,
    /// Every degradation along the way — a thin report must be legible.
    pub warnings: Vec<String>,
    pub ms: u64,
}

impl ResearchOutcome {
    /// Adapt to the shape `db::save_run` and `db::save_claims` persist.
    pub fn as_search_outcome(&self) -> pipeline::SearchOutcome {
        pipeline::SearchOutcome {
            query: self.query.clone(),
            evidence: self.evidence.clone(),
            sources: self.sources.clone(),
            unknown_sources: self.unknown_sources.clone(),
            total_before_dedupe: self.total_before_dedupe,
            deepened: self.deepened,
            ms: self.ms,
        }
    }
}

pub async fn run(
    registry: &Registry,
    transports: &Arc<Transports>,
    req: &ResearchRequest,
) -> ResearchOutcome {
    let started = Instant::now();
    let bridge = &transports.bridge;
    let mut warnings = Vec::new();

    let unknown_sources = req
        .sources
        .as_deref()
        .map(|w| registry.unknown(w))
        .unwrap_or_default();

    // 1. Plan.
    let mut sub_queries = plan(
        bridge,
        &req.query,
        req.lang.as_deref(),
        req.depth,
        &mut warnings,
    )
    .await;

    // 2–3. Gather + relevance checkpoint (round 1).
    let mut off_topic = Vec::new();
    let (mut evidence, mut sources, mut total_before) = gather_screened(
        registry,
        transports,
        &sub_queries,
        req,
        &mut off_topic,
        &mut warnings,
    )
    .await;
    let mut deepened = deepen(transports, &mut evidence, req.depth).await;
    let mut rounds = 1;

    // 3.5. Nothing survived the gate. Retrieval, not synthesis, is what failed —
    // so retry retrieval with keyword-shaped queries instead of writing a report
    // out of material we just judged off topic.
    if evidence.is_empty() && !off_topic.is_empty() && req.depth != Depth::Quick {
        let retry = replan(bridge, &req.query, req.lang.as_deref()).await;
        if !retry.is_empty() {
            warnings.push(format!(
                "Vòng 1 không có tư liệu nào đúng chủ đề — tìm lại với: {}.",
                retry.join(" · ")
            ));
            rounds += 1;
            let (ev2, src2, tb2) = gather_screened(
                registry,
                transports,
                &retry,
                req,
                &mut off_topic,
                &mut warnings,
            )
            .await;
            evidence = ev2;
            sources.extend(src2);
            total_before += tb2;
            deepened += deepen(transports, &mut evidence, req.depth).await;
            for q in retry {
                push_unique(&mut sub_queries, q);
            }
        }
    }

    // 4. Verify.
    let (mut claims, mut contradictions) =
        verify(bridge, &req.query, &evidence, &mut warnings).await;

    // 5. Follow-up round for the weak and high-stakes claims (deep only).
    if req.depth.wants_follow_up() && !claims.is_empty() {
        let follow = follow_up_queries(&claims);
        if !follow.is_empty() {
            rounds += 1;
            let (ev2, src2, tb2) = gather_screened(
                registry,
                transports,
                &follow,
                req,
                &mut off_topic,
                &mut warnings,
            )
            .await;
            sources.extend(src2);
            total_before += tb2;
            let mut all = std::mem::take(&mut evidence);
            all.extend(ev2);
            evidence = merge(all, &registry.weights(), req.max_evidence);
            deepened += deepen(transports, &mut evidence, req.depth).await;
            let (c2, ct2) = verify(bridge, &req.query, &evidence, &mut warnings).await;
            claims = c2;
            contradictions = ct2;
        }
    }

    // 6–7. Synthesize, then check the result before returning it.
    let (report_title, report_markdown, report_llm, status, reviewed) = if evidence.is_empty() {
        // Refusing is the honest answer. A model handed nothing on topic writes
        // a report about whatever it was handed instead.
        let (t, md) = review::insufficient_report(&req.query, &sub_queries, &sources, &off_topic);
        warnings.push(
            "Không có tư liệu nào đúng chủ đề câu hỏi — không tổng hợp báo cáo để tránh trả lời lạc đề."
                .into(),
        );
        (t, md, false, "insufficient".to_string(), None)
    } else {
        let syn = synthesize::synthesize(
            bridge,
            &req.query,
            &claims,
            &contradictions,
            &evidence,
            Duration::from_secs(240),
        )
        .await;
        if let Some(w) = syn.warning.clone() {
            warnings.push(w);
        }
        let verdict =
            review::review_report(bridge, &req.query, &syn.markdown, Duration::from_secs(90)).await;
        if verdict.answers {
            (
                syn.title,
                syn.markdown,
                syn.used_llm,
                "ok".to_string(),
                Some(verdict),
            )
        } else {
            warnings.push(format!(
                "Kiểm định: báo cáo không trả lời được câu hỏi ({}/100).{}",
                verdict.score,
                if verdict.issues.is_empty() {
                    String::new()
                } else {
                    format!(" {}", verdict.issues.join("; "))
                }
            ));
            let banner = review::off_topic_banner(&req.query, &verdict);
            let title = format!("[Chưa trả lời được] {}", req.query.trim());
            (
                title,
                format!("{banner}{}", syn.markdown),
                syn.used_llm,
                "off_topic".to_string(),
                Some(verdict),
            )
        }
    };

    ResearchOutcome {
        query: req.query.clone(),
        depth: req.depth,
        sub_queries,
        evidence,
        off_topic,
        sources,
        unknown_sources,
        claims,
        contradictions,
        status,
        review: reviewed,
        report_title,
        report_markdown,
        report_llm,
        confidence_note: claims::CONFIDENCE_IS_PROVENANCE.to_string(),
        rounds,
        total_before_dedupe: total_before,
        deepened,
        warnings,
        ms: started.elapsed().as_millis() as u64,
    }
}

/// Expand the question into sub-queries. The verbatim question is always first;
/// the LLM only adds angles. Failure degrades to the single question.
async fn plan(
    bridge: &Bridge,
    query: &str,
    lang: Option<&str>,
    depth: Depth,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let mut subs = vec![query.trim().to_string()];
    let cap = depth.sub_query_cap();
    if cap <= 1 {
        return subs;
    }

    let sys = "Bạn là người lập kế hoạch tìm kiếm. Từ một câu hỏi nghiên cứu, hãy sinh ra các truy vấn tìm kiếm con \
        bao phủ nhiều khía cạnh khác nhau (định nghĩa/bối cảnh, số liệu/dữ kiện, các bên tranh cãi, cập nhật mới nhất). \
        Mỗi truy vấn ngắn gọn, khác nhau rõ rệt, KHÔNG lặp lại câu gốc. Chỉ trả về JSON, không giải thích.";
    let prompt = format!(
        "Câu hỏi gốc: {query}\nNgôn ngữ ưu tiên: {}\n\nTrả về ĐÚNG JSON: {{\"queries\":[\"...\"]}} — tối đa {} truy vấn con.",
        lang.unwrap_or("vi"),
        cap - 1
    );

    // 700 came back cut after a couple of queries — the bridge yields a small
    // fraction of `maxTokens`, so the budget has to be asked for generously.
    match bridge.llm(sys, &prompt, 3_000, Duration::from_secs(60)).await {
        Ok(reply) => {
            for q in parse_queries(&reply.text) {
                push_unique(&mut subs, q);
                if subs.len() >= cap {
                    break;
                }
            }
        }
        Err(e) => warnings.push(format!(
            "Không lập được kế hoạch truy vấn con ({e}) — chỉ tìm với câu hỏi gốc."
        )),
    }
    subs.truncate(cap);
    subs
}

/// Pull the `queries` array out of a possibly-fenced, possibly-noisy response.
fn parse_queries(text: &str) -> Vec<String> {
    let start = match text.find('{') {
        Some(i) => i,
        None => return vec![],
    };
    // Trim any trailing prose after the last closing brace.
    let end = text.rfind('}').map(|i| i + 1).unwrap_or(text.len());
    let body = &text[start..end.max(start + 1)];
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    parsed
        .get("queries")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn push_unique(subs: &mut Vec<String>, q: String) {
    let ql = q.to_lowercase();
    if !subs.iter().any(|s| s.to_lowercase() == ql) {
        subs.push(q);
    }
}

/// Fan every sub-query out through the shared pipeline and merge the results
/// into one fused, diversity-capped evidence set. Sub-queries run concurrently;
/// each already has its own per-source error boundary in `pipeline::run`.
async fn gather(
    registry: &Registry,
    transports: &Arc<Transports>,
    sub_queries: &[String],
    req: &ResearchRequest,
) -> (Vec<Evidence>, Vec<SourceOutcome>, usize) {
    let per = req.depth.per_source_limit();
    let futs = sub_queries.iter().map(|sq| {
        let sr = SearchRequest {
            query: sq.clone(),
            sources: req.sources.clone(),
            limit: per,
            lang: req.lang.clone(),
            depth: 1, // deepen once, on the merged set — not per sub-query.
        };
        async move { pipeline::run(registry, transports, &sr).await }
    });
    let outcomes = futures_util::future::join_all(futs).await;

    let mut all = Vec::new();
    let mut sources = Vec::new();
    let mut total_before = 0;
    for o in outcomes {
        total_before += o.total_before_dedupe;
        all.extend(o.evidence);
        sources.extend(o.sources);
    }
    let merged = merge(all, &registry.weights(), req.max_evidence);
    (merged, sources, total_before)
}

/// `gather` + the relevance checkpoint. Off-topic items are moved to `off_topic`
/// rather than dropped, so the caller can still show what was found.
async fn gather_screened(
    registry: &Registry,
    transports: &Arc<Transports>,
    sub_queries: &[String],
    req: &ResearchRequest,
    off_topic: &mut Vec<Evidence>,
    warnings: &mut Vec<String>,
) -> (Vec<Evidence>, Vec<SourceOutcome>, usize) {
    let (evidence, sources, total_before) = gather(registry, transports, sub_queries, req).await;
    let screen = review::screen_evidence(
        &transports.bridge,
        &req.query,
        evidence,
        Duration::from_secs(90),
    )
    .await;
    if let Some(note) = screen.note {
        if !warnings.contains(&note) {
            warnings.push(note);
        }
    }
    off_topic.extend(screen.dropped);
    (screen.kept, sources, total_before)
}

/// Re-plan after the gate emptied round 1. The first plan produced queries that
/// retrieved the wrong topic, so this asks for keyword-shaped queries built from
/// the concrete nouns of the question instead of a rephrasing of it.
async fn replan(bridge: &Bridge, query: &str, lang: Option<&str>) -> Vec<String> {
    let sys = "Bạn là người sửa truy vấn tìm kiếm. Lần tìm trước KHÔNG ra tư liệu đúng chủ đề. \
        Hãy viết lại thành các truy vấn NGẮN, chỉ gồm từ khoá cụ thể (danh từ riêng, sự kiện, mốc thời gian, địa danh) — \
        không viết thành câu, không diễn giải. Chỉ trả về JSON.";
    let prompt = format!(
        "Câu hỏi gốc: {query}\nNgôn ngữ ưu tiên: {}\n\n\
         Trả về ĐÚNG JSON: {{\"queries\":[\"...\"]}} — tối đa 3 truy vấn từ khoá, khác nhau rõ rệt. \
         Nếu chủ đề mang tính quốc tế, có thể thêm một truy vấn bằng tiếng Anh.",
        lang.unwrap_or("vi")
    );
    // Ask big: the bridge yields only a small fraction of `maxTokens`, so a
    // tight budget comes back cut after a few tokens (see `review.rs`).
    match bridge.llm(sys, &prompt, 3_000, Duration::from_secs(60)).await {
        Ok(reply) => {
            let mut out: Vec<String> = Vec::new();
            for q in parse_queries(&reply.text) {
                push_unique(&mut out, q);
                if out.len() >= 3 {
                    break;
                }
            }
            out
        }
        Err(_) => vec![],
    }
}

/// Re-run dedupe → fuse → diversity-cap over a union of evidence sets.
fn merge(
    all: Vec<Evidence>,
    weights: &std::collections::HashMap<String, f32>,
    limit: usize,
) -> Vec<Evidence> {
    let mut merged = fusion::dedupe(all);
    fusion::fuse(&mut merged, weights);
    fusion::select_diverse(&mut merged, limit);
    merged
}

async fn deepen(transports: &Arc<Transports>, evidence: &mut [Evidence], depth: Depth) -> usize {
    let top = depth.deepen_top();
    if top == 0 {
        return 0;
    }
    pipeline::deepen(transports, evidence, top).await
}

/// Extract atomic claims and score each by independent-source count. This is the
/// verification step: the tier is arithmetic over provenance, not the model's
/// opinion.
async fn verify(
    bridge: &Bridge,
    query: &str,
    evidence: &[Evidence],
    warnings: &mut Vec<String>,
) -> (Vec<Claim>, Vec<Contradiction>) {
    if evidence.is_empty() {
        return (vec![], vec![]);
    }
    match extract::extract_claims(bridge, query, evidence, Duration::from_secs(180)).await {
        Ok((raw, raw_ct)) => {
            let mut claims = claims::assess_all(&raw, evidence);
            let cts = claims::validate_contradictions(&raw_ct, &claims);
            claims::mark_disputed(&mut claims, &cts);
            (claims, cts)
        }
        Err(e) => {
            warnings.push(format!(
                "Không rút được khẳng định để kiểm chứng ({e}) — báo cáo sẽ dựa trực tiếp trên bằng chứng."
            ));
            (vec![], vec![])
        }
    }
}

/// Turn the shakiest claims into follow-up queries. Deterministic — no LLM: a
/// single-source or unverified claim, or a high-stakes claim with fewer than two
/// independent sources, is exactly what a second gather round should try to
/// corroborate or refute.
fn follow_up_queries(claims: &[Claim]) -> Vec<String> {
    use crate::claims::Tier;
    let mut out: Vec<String> = Vec::new();
    for c in claims {
        let weak = matches!(c.tier, Tier::SingleSource | Tier::Unverified)
            || (c.high_stakes && c.independent_count < 2);
        if weak {
            let q = crate::util::truncate_chars(c.text.trim(), 120);
            if !q.is_empty() && !out.iter().any(|e| e.eq_ignore_ascii_case(&q)) {
                out.push(q);
            }
        }
        if out.len() >= 3 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claims::{assess_all, RawClaim, Tier};
    use crate::model::{Evidence, SourceKind};

    #[test]
    fn depth_parses_forgivingly_and_scales_effort() {
        assert_eq!(Depth::parse("SÂU"), Depth::Deep);
        assert_eq!(Depth::parse("nhanh"), Depth::Quick);
        assert_eq!(Depth::parse("gì đó"), Depth::Standard);
        assert!(Depth::Deep.sub_query_cap() > Depth::Quick.sub_query_cap());
        assert!(Depth::Deep.wants_follow_up());
        assert!(!Depth::Quick.wants_follow_up());
        assert_eq!(Depth::Quick.deepen_top(), 0);
    }

    #[test]
    fn planner_output_is_parsed_out_of_fenced_noise() {
        let text = "Đây là kế hoạch:\n```json\n{\"queries\":[\"lãi suất 2026\",\" \",\"tác động lạm phát\"]}\n```\nxong.";
        let qs = parse_queries(text);
        assert_eq!(
            qs,
            vec!["lãi suất 2026".to_string(), "tác động lạm phát".to_string()]
        );
    }

    #[test]
    fn a_response_with_no_json_yields_no_sub_queries() {
        assert!(parse_queries("xin lỗi tôi không giúp được").is_empty());
    }

    #[test]
    fn sub_queries_never_duplicate_the_original() {
        let mut subs = vec!["câu gốc".to_string()];
        push_unique(&mut subs, "CÂU GỐC".into());
        push_unique(&mut subs, "góc nhìn mới".into());
        assert_eq!(
            subs,
            vec!["câu gốc".to_string(), "góc nhìn mới".to_string()]
        );
    }

    fn ev(id: &str, url: &str) -> Evidence {
        let mut e = Evidence::new("web", SourceKind::Web, 0, 1.0, "t", "s", Some(url.into()));
        e.id = id.to_string();
        e
    }

    #[test]
    fn follow_up_targets_weak_and_high_stakes_claims_only() {
        let evs = vec![
            ev("e1", "https://a.vn/1"),
            ev("e2", "https://b.vn/1"),
            ev("e3", "https://c.vn/1"),
        ];
        let claims = assess_all(
            &[
                // verified (3 publishers) — must NOT be chased
                RawClaim {
                    text: "Trời xanh.".into(),
                    supports: vec!["e1".into(), "e2".into(), "e3".into()],
                    refutes: vec![],
                },
                // single-source — chased
                RawClaim {
                    text: "Chỉ một nơi nói điều này.".into(),
                    supports: vec!["e1".into()],
                    refutes: vec![],
                },
                // high-stakes single-source — chased
                RawClaim {
                    text: "Lãi suất giảm còn 4,5%.".into(),
                    supports: vec!["e2".into()],
                    refutes: vec![],
                },
            ],
            &evs,
        );
        assert_eq!(claims[0].tier, Tier::Verified);
        let follow = follow_up_queries(&claims);
        assert_eq!(
            follow.len(),
            2,
            "only the two weak claims become follow-ups"
        );
        assert!(follow.iter().any(|q| q.contains("4,5%")));
        assert!(!follow.iter().any(|q| q.contains("Trời xanh")));
    }
}
