//! The checkpoint: does what we retrieved — and what we wrote — actually answer
//! the question that was asked?
//!
//! Every source in this app is a *nearest-neighbour* retriever. Ask an internal
//! knowledge base about world disasters and it will happily hand back its
//! closest notes about something else entirely; nothing upstream of here checks
//! topic, only rank. Claim extraction then dutifully mines those notes, and
//! synthesis writes a well-cited report about the wrong subject. That failure is
//! silent — it looks like a successful run.
//!
//! So there are two gates, and both are allowed to say "no":
//!
//! 1. [`screen_evidence`] — before claims are extracted, drop the items that are
//!    not about the question. Dropped items are never discarded from the run,
//!    only from the reasoning set: the caller still shows them, labelled.
//! 2. [`review_report`] — after the report is written, an independent pass judges
//!    it against the ORIGINAL question. A well-written report about another topic
//!    fails this gate.
//!
//! Both gates fail **open with a warning**, never closed-and-silent: an LLM
//! outage must not turn into "không tìm thấy gì". The lexical layer exists only
//! to *rescue* items — it can keep something the model wanted to drop, it can
//! never drop something on its own, because a cross-lingual question ("toàn bộ
//! ngôn ngữ khác nhau") legitimately retrieves evidence that shares no words
//! with the query.

use crate::model::Evidence;
use crate::transport::Bridge;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

/// Characters of each item shown to the screening model.
const SCREEN_ITEM_CHARS: usize = 240;
/// Total evidence characters in the screening prompt.
const SCREEN_TOTAL_CHARS: usize = 14_000;
/// Characters of the report handed to the reviewer.
const REVIEW_REPORT_CHARS: usize = 9_000;
/// `maxTokens` for the two gates.
///
/// Measured against the daemon bridge, not guessed: the reply actually produced
/// is a small fraction of `maxTokens` (400 → 11 output tokens, 4 000 → ~100), so
/// asking for the size you need gets a reply cut after a dozen tokens — the
/// verdict lost, the index list silently short. Ask big; the reply stays small.
const SCREEN_MAX_TOKENS: u32 = 6_000;
const REVIEW_MAX_TOKENS: u32 = 4_000;

/// Outcome of the evidence gate.
pub struct Screen {
    /// On-topic — the only evidence claim extraction and synthesis may see.
    pub kept: Vec<Evidence>,
    /// Off-topic. Kept for transparency, never reasoned over.
    pub dropped: Vec<Evidence>,
    /// A degradation worth telling the user about.
    pub note: Option<String>,
}

impl Screen {
    fn keep_all(evidence: Vec<Evidence>, note: Option<String>) -> Self {
        Self {
            kept: evidence,
            dropped: vec![],
            note,
        }
    }
}

/// Verdict of the report gate.
#[derive(Debug, Clone, Serialize)]
pub struct ReportReview {
    /// Does the report answer the question that was asked?
    pub answers: bool,
    /// 0–100, how well it answers it.
    pub score: u8,
    /// What is wrong, in the reviewer's words.
    pub issues: Vec<String>,
    /// Searches that would close the gap — used as follow-up queries.
    pub missing: Vec<String>,
    /// False when the reviewer could not run; `answers` is then a deterministic
    /// fallback, not a judgement.
    pub used_llm: bool,
}

impl ReportReview {
    fn passthrough(reason: &str) -> Self {
        Self {
            answers: true,
            score: 0,
            issues: vec![reason.to_string()],
            missing: vec![],
            used_llm: false,
        }
    }
}

const SCREEN_SYSTEM: &str = "Bạn là bộ lọc mức độ liên quan. Với mỗi mẩu tư liệu, \
hãy quyết định nó có GÓP ĐƯỢC GÌ cho việc trả lời CÂU HỎI được giao hay không. \
Nguyên tắc: chỉ LOẠI khi tư liệu rõ ràng nói về chuyện khác; còn nếu nó nhắc tới chủ đề, đối tượng hoặc \
số liệu mà câu hỏi đang hỏi — dù chỉ một phần — thì GIỮ. \
Tư liệu chỉ có tiêu đề ngắn, không có nội dung: hãy xét theo tiêu đề, tiêu đề chạm đúng chủ đề thì GIỮ. \
Tư liệu bằng ngôn ngữ khác vẫn tính là liên quan nếu nội dung đúng chủ đề. \
Nếu THỰC SỰ không mẩu nào dính tới chủ đề câu hỏi, hãy trả về danh sách rỗng — không cố vớt vát cho đủ. \
Chỉ trả về JSON, không giải thích ngoài JSON.";

const REVIEW_SYSTEM: &str = "Bạn là người kiểm định chất lượng báo cáo nghiên cứu. \
Nhiệm vụ DUY NHẤT: xác định báo cáo có trả lời đúng CÂU HỎI được giao hay không. \
Một báo cáo viết tốt, trích dẫn đầy đủ nhưng nói về chủ đề KHÁC là KHÔNG ĐẠT. \
Một báo cáo thừa nhận thẳng thắn là không đủ dữ liệu, đúng chủ đề, thì vẫn tính là ĐẠT nhưng điểm thấp. \
Chỉ trả về JSON, không giải thích ngoài JSON.";

/// Gate 1 — keep only the evidence that is about the question.
pub async fn screen_evidence(
    bridge: &Bridge,
    query: &str,
    evidence: Vec<Evidence>,
    timeout: Duration,
) -> Screen {
    if evidence.len() < 2 {
        return Screen::keep_all(evidence, None);
    }

    let prompt = build_screen_prompt(query, &evidence);
    let reply = match bridge.llm(SCREEN_SYSTEM, &prompt, SCREEN_MAX_TOKENS, timeout).await {
        Ok(r) => r,
        Err(e) => {
            return Screen::keep_all(
                evidence,
                Some(format!(
                    "Không lọc được mức độ liên quan của tư liệu ({e}) — giữ nguyên toàn bộ, \
                     hãy tự đối chiếu báo cáo với câu hỏi."
                )),
            )
        }
    };

    let picked = match parse_indices(&reply.text, "relevant") {
        Some(v) => v,
        None => {
            return Screen::keep_all(
                evidence,
                Some("Bộ lọc liên quan trả về dữ liệu không đọc được — giữ nguyên toàn bộ tư liệu.".into()),
            )
        }
    };

    let terms = topic_terms(query);
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for (i, e) in evidence.into_iter().enumerate() {
        // The lexical layer only ever rescues: a strong word overlap outvotes a
        // model that dropped the item, never the other way round.
        if picked.contains(&(i + 1)) || lexical_hit(&terms, &e) {
            kept.push(e);
        } else {
            dropped.push(e);
        }
    }

    let note = (!dropped.is_empty()).then(|| {
        format!(
            "Bỏ qua {} tư liệu không đúng chủ đề câu hỏi (vẫn liệt kê ở mục “Tư liệu đã loại”).",
            dropped.len()
        )
    });
    Screen {
        kept,
        dropped,
        note,
    }
}

fn build_screen_prompt(query: &str, evidence: &[Evidence]) -> String {
    let mut out = format!("CÂU HỎI: {query}\n\nTƯ LIỆU:\n");
    let mut budget = SCREEN_TOTAL_CHARS;
    for (i, e) in evidence.iter().enumerate() {
        let body = crate::util::truncate_chars(e.body(), SCREEN_ITEM_CHARS.min(budget));
        // Naming the source matters: a bare phrase from the knowledge graph is a
        // node label, not a thin article, and must be judged as such.
        let src = e
            .domain
            .clone()
            .or_else(|| e.hits.first().map(|h| h.source_id.clone()))
            .unwrap_or_else(|| "?".into());
        let block = format!("[{}] ({src}) {}\n{}\n\n", i + 1, e.title.trim(), body.trim());
        budget = budget.saturating_sub(block.chars().count());
        out.push_str(&block);
        if budget == 0 {
            break;
        }
    }
    out.push_str(
        "\nTrả về ĐÚNG JSON: {\"relevant\":[1,4,7]}\n\
         - Ghi SỐ của mọi tư liệu góp được gì đó cho câu trả lời (kể cả chỉ một phần).\n\
         - Bỏ qua những mẩu nói về chuyện khác hẳn.\n\
         - Không mẩu nào dính tới chủ đề thì trả {\"relevant\":[]}.",
    );
    out
}

/// Gate 2 — judge the finished report against the original question.
pub async fn review_report(
    bridge: &Bridge,
    query: &str,
    report_md: &str,
    timeout: Duration,
) -> ReportReview {
    // Short output on purpose: `answers` and `score` come first so that even a
    // reply cut inside `issues` still carries the verdict.
    let prompt = format!(
        "CÂU HỎI ĐƯỢC GIAO: {query}\n\nBÁO CÁO CẦN KIỂM ĐỊNH:\n{}\n\n\
         Trả về ĐÚNG JSON trên MỘT dòng, theo thứ tự này:\n\
         {{\"answers\":true,\"score\":0,\"issues\":[\"...\"],\"missing\":[\"...\"]}}\n\
         - answers: true nếu báo cáo nói về ĐÚNG chủ đề câu hỏi; false nếu lạc đề.\n\
         - score: 0-100, mức độ trả lời được câu hỏi.\n\
         - issues: tối đa 2 ý, mỗi ý DƯỚI 20 từ. Đạt hoàn toàn thì để [].\n\
         - missing: tối đa 2 truy vấn ngắn nên tìm thêm.",
        crate::util::truncate_chars(report_md, REVIEW_REPORT_CHARS)
    );

    let reply = match bridge.llm(REVIEW_SYSTEM, &prompt, REVIEW_MAX_TOKENS, timeout).await {
        Ok(r) => r,
        Err(e) => return ReportReview::passthrough(&format!("không kiểm định được báo cáo ({e})")),
    };
    let Some(obj) = isolate_object(&reply.text) else {
        return ReportReview::passthrough("bộ kiểm định trả về dữ liệu không đọc được");
    };

    let score = obj
        .get("score")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.trim().parse().ok()))
        .unwrap_or(0)
        .min(100) as u8;
    // A missing `answers` must not read as a pass — an absent verdict on a
    // near-zero score is exactly the failure this gate exists to catch.
    let answers = obj
        .get("answers")
        .and_then(as_bool_loose)
        .unwrap_or(score >= 40);

    ReportReview {
        answers,
        score,
        issues: string_list(obj.get("issues"), 4),
        missing: string_list(obj.get("missing"), 3),
        used_llm: true,
    }
}

/// The report for a run that found nothing on topic. Written deterministically —
/// asking a model to "summarise" evidence that does not answer the question is
/// precisely how an off-topic report gets produced.
pub fn insufficient_report(
    query: &str,
    sub_queries: &[String],
    sources: &[crate::model::SourceOutcome],
    off_topic: &[Evidence],
) -> (String, String) {
    let title = format!("Không đủ dữ liệu để trả lời: {}", query.trim());
    let mut md = format!("# {title}\n\n");
    md.push_str(
        "> **Kiểm định trước khi trả kết quả:** không thu được tư liệu nào đúng chủ đề câu hỏi, \
         nên báo cáo này KHÔNG kết luận gì. Đây là trạng thái thật của lần tìm, không phải câu trả lời.\n\n",
    );

    md.push_str("## Đã tìm những gì\n\n");
    for q in sub_queries {
        md.push_str(&format!("- `{}`\n", q.trim()));
    }

    let failed: Vec<&crate::model::SourceOutcome> =
        sources.iter().filter(|s| s.status != "ok").collect();
    let empty: Vec<&crate::model::SourceOutcome> = sources
        .iter()
        .filter(|s| s.status == "ok" && s.item_count == 0)
        .collect();
    if !failed.is_empty() || !empty.is_empty() {
        md.push_str("\n## Vì sao có thể thiếu dữ liệu\n\n");
        for s in failed {
            md.push_str(&format!(
                "- **{}** — {} ({})\n",
                s.source_id,
                s.status,
                s.error.as_deref().unwrap_or("không rõ lý do")
            ));
        }
        for s in empty {
            md.push_str(&format!("- **{}** — chạy được nhưng không có kết quả\n", s.source_id));
        }
    }

    if !off_topic.is_empty() {
        md.push_str(&format!(
            "\n## Tư liệu đã loại ({} mục)\n\nNhững mục dưới đây có trong kết quả tìm kiếm nhưng \
             KHÔNG nói về chủ đề câu hỏi, nên không được dùng để rút ra khẳng định nào:\n\n",
            off_topic.len()
        ));
        for e in off_topic.iter().take(20) {
            let label = e
                .domain
                .clone()
                .or_else(|| e.hits.first().map(|h| h.source_id.clone()))
                .unwrap_or_else(|| "?".into());
            let title = if e.title.trim().is_empty() {
                "(không có tiêu đề)"
            } else {
                e.title.trim()
            };
            match e.url.as_deref() {
                Some(u) if !u.is_empty() => md.push_str(&format!("- [{title}]({u}) — {label}\n")),
                _ => md.push_str(&format!("- {title} — {label}\n")),
            }
        }
    }

    md.push_str(
        "\n## Nên làm gì tiếp\n\n\
         - Bật/kiểm tra các nguồn ngoài (Web, Tin tức) — nguồn nội bộ chỉ trả về tư liệu sẵn có của bạn.\n\
         - Thu hẹp câu hỏi (mốc thời gian, khu vực, loại sự kiện) rồi chạy lại.\n",
    );
    (title, md)
}

/// The banner prepended to a report that failed gate 2. The report is still
/// shown — hiding it would hide the evidence too — but it can never again be
/// mistaken for an answer.
pub fn off_topic_banner(query: &str, review: &ReportReview) -> String {
    let mut s = format!(
        "> ⚠️ **Kiểm định trước khi trả kết quả: báo cáo này KHÔNG trả lời được câu hỏi «{}».**\n",
        query.trim()
    );
    if review.used_llm {
        s.push_str(&format!("> Điểm trả lời câu hỏi: {}/100.\n", review.score));
    }
    for i in review.issues.iter().take(4) {
        s.push_str(&format!("> - {}\n", i.trim()));
    }
    s.push_str(
        "> Nội dung bên dưới là những gì tìm được, giữ lại để bạn tự đối chiếu — \
         KHÔNG phải câu trả lời cho câu hỏi trên.\n\n",
    );
    s
}

// ---------------------------------------------------------------- parsing ---

/// Pull a JSON object out of a possibly-fenced, possibly-truncated response.
/// The bridge cuts replies mid-structure, so repair is the norm, not the
/// exception ([[space-app-llm-bridge-output-ceiling]]).
fn isolate_object(text: &str) -> Option<Value> {
    crate::extract::parse_lenient_object(text)
}

/// `{"relevant":[1,"3","E4"]}` → `[1, 3, 4]`. Models emit all three forms.
///
/// Truncation is NOT repaired here, unlike everywhere else: a cut list of
/// indices parses cleanly into *fewer* relevant items, which would silently
/// discard evidence. An unbalanced reply returns `None` so the caller fails
/// open and keeps everything.
fn parse_indices(text: &str, key: &str) -> Option<Vec<usize>> {
    let body = crate::extract::isolate_json(text)?;
    let (end, _, _) = crate::extract::scan(body);
    let obj: Value = serde_json::from_str(&body[..end?]).ok()?;
    let arr = obj.get(key)?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|v| {
                v.as_u64().map(|n| n as usize).or_else(|| {
                    v.as_str()
                        .and_then(|s| s.trim().trim_start_matches(['E', 'e', '[']).parse().ok())
                })
            })
            .filter(|n| *n > 0)
            .collect(),
    )
}

fn as_bool_loose(v: &Value) -> Option<bool> {
    v.as_bool().or_else(|| match v.as_str()?.trim() {
        "true" | "True" | "TRUE" | "có" | "yes" => Some(true),
        "false" | "False" | "FALSE" | "không" | "no" => Some(false),
        _ => None,
    })
}

fn string_list(v: Option<&Value>, max: usize) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .take(max)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

// --------------------------------------------------------------- lexical ----

/// Vietnamese function words carry no topic; keeping them would make every
/// document look like a partial match.
const STOPWORDS: &[&str] = &[
    "là", "và", "của", "các", "những", "một", "cho", "với", "trên", "trong", "từ", "về", "được",
    "có", "không", "khi", "này", "đó", "như", "để", "thì", "mà", "ở", "ra", "vào", "nào", "gì",
    "hay", "hoặc", "bị", "sẽ", "đã", "đang", "rất", "toàn", "bộ", "tất", "cả", "nhau", "khác",
    "theo", "tôi", "bạn", "phân", "tích", "tìm", "kiếm", "hãy", "nêu", "the", "and", "for", "with",
    "from", "that", "this", "what", "how", "all",
];

/// Content terms of the query, lowercased and diacritics preserved.
fn topic_terms(query: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
    {
        if raw.chars().count() < 2 || STOPWORDS.contains(&raw) {
            continue;
        }
        let t = raw.to_string();
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// True when the item shares enough distinctive words with the query that
/// dropping it would be obviously wrong. Deliberately strict — this is a rescue
/// valve, not a filter.
fn lexical_hit(terms: &[String], e: &Evidence) -> bool {
    if terms.len() < 3 {
        return false;
    }
    let hay = format!(
        "{} {}",
        e.title.to_lowercase(),
        crate::util::truncate_chars(e.body(), 4_000).to_lowercase()
    );
    let matched = terms.iter().filter(|t| hay.contains(t.as_str())).count();
    matched * 2 >= terms.len() && matched >= 3
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SourceKind;

    fn ev(title: &str, body: &str) -> Evidence {
        Evidence::new(
            "web",
            SourceKind::Web,
            0,
            1.0,
            title,
            body,
            Some(format!("https://x.vn/{}", title.len())),
        )
    }

    #[test]
    fn the_screener_reads_indices_in_every_shape_a_model_emits() {
        assert_eq!(
            parse_indices(r#"```json {"relevant":[1,"3","E4"]} ```"#, "relevant"),
            Some(vec![1, 3, 4])
        );
        assert_eq!(parse_indices(r#"{"relevant":[]}"#, "relevant"), Some(vec![]));
        assert_eq!(parse_indices("không phải JSON", "relevant"), None);
    }

    #[test]
    fn a_zero_index_is_discarded_rather_than_wrapping_to_the_last_item() {
        assert_eq!(parse_indices(r#"{"relevant":[0,2]}"#, "relevant"), Some(vec![2]));
    }

    #[test]
    fn topic_terms_drop_function_words_but_keep_the_subject() {
        let t = topic_terms("phân tích tìm kiếm các thiên tai trên thế giới, toàn bộ ngôn ngữ khác nhau, từ đầu năm");
        assert!(t.contains(&"thiên".to_string()));
        assert!(t.contains(&"tai".to_string()));
        assert!(!t.contains(&"các".to_string()));
        assert!(!t.contains(&"toàn".to_string()));
    }

    #[test]
    fn the_lexical_valve_rescues_an_on_topic_item_but_not_a_neighbouring_topic() {
        // The bug this app shipped with: a question about world disasters
        // answered with the user's notes about languages and AI adoption.
        let terms = topic_terms("thiên tai lũ lụt động đất thế giới");
        let on = ev("Động đất và lũ lụt: thiên tai lớn nhất thế giới", "thống kê");
        let off = ev("Có gần 7.000 ngôn ngữ trên thế giới", "ngôn ngữ, AI doanh nghiệp");
        assert!(lexical_hit(&terms, &on));
        assert!(!lexical_hit(&terms, &off));
    }

    #[test]
    fn a_short_query_never_triggers_the_lexical_valve() {
        // Two words can co-occur by chance; rescuing on that would defeat the
        // model's judgement for exactly the queries it is best at.
        assert!(!lexical_hit(&topic_terms("bão lụt"), &ev("bão lụt", "bão lụt")));
    }

    #[test]
    fn a_reviewer_reply_cut_mid_issue_still_yields_its_verdict() {
        // The bridge truncates every caller's reply; `answers`/`score` come
        // first precisely so the verdict survives the cut.
        let cut = r#"{"answers":false,"score":10,"issues":["Báo cáo hoàn"#;
        let obj = isolate_object(cut).expect("a truncated verdict must be repaired");
        assert_eq!(as_bool_loose(obj.get("answers").unwrap()), Some(false));
        assert_eq!(obj.get("score").and_then(Value::as_u64), Some(10));
    }

    #[test]
    fn a_truncated_relevance_list_fails_open_instead_of_dropping_evidence() {
        // Repairing this would parse into FEWER relevant items — evidence
        // discarded because a reply was cut, not because it was off topic.
        assert_eq!(parse_indices(r#"{"relevant":[1,2,3"#, "relevant"), None);
    }

    #[test]
    fn a_reviewer_verdict_without_answers_falls_back_to_the_score() {
        let low = isolate_object(r#"{"score":10,"issues":["lạc đề"]}"#).unwrap();
        let score = low.get("score").and_then(Value::as_u64).unwrap() as u8;
        assert!(score < 40, "a score this low must not pass by default");
        assert_eq!(string_list(low.get("issues"), 4), vec!["lạc đề".to_string()]);
    }

    #[test]
    fn loose_booleans_are_understood() {
        assert_eq!(as_bool_loose(&Value::from("false")), Some(false));
        assert_eq!(as_bool_loose(&Value::from("có")), Some(true));
        assert_eq!(as_bool_loose(&Value::from("ừ thì")), None);
    }

    #[test]
    fn the_insufficient_report_names_the_gap_and_never_concludes() {
        let sources = vec![crate::model::SourceOutcome {
            source_id: "web".into(),
            sub_query: "q".into(),
            status: "skipped".into(),
            item_count: 0,
            dropped_count: 0,
            ms: 3,
            error: Some("extension chưa kết nối".into()),
        }];
        let off = vec![ev("Có gần 7.000 ngôn ngữ", "…")];
        let (title, md) = insufficient_report(
            "thiên tai trên thế giới từ đầu năm",
            &["thiên tai trên thế giới từ đầu năm".to_string()],
            &sources,
            &off,
        );
        assert!(title.starts_with("Không đủ dữ liệu"));
        assert!(md.contains("extension chưa kết nối"), "must say WHY it is thin");
        assert!(md.contains("Tư liệu đã loại"));
        assert!(md.contains("Có gần 7.000 ngôn ngữ"));
    }

    #[test]
    fn the_off_topic_banner_states_the_question_it_failed() {
        let r = ReportReview {
            answers: false,
            score: 12,
            issues: vec!["báo cáo nói về ngôn ngữ và AI".into()],
            missing: vec![],
            used_llm: true,
        };
        let b = off_topic_banner("thiên tai thế giới", &r);
        assert!(b.contains("KHÔNG trả lời được câu hỏi «thiên tai thế giới»"));
        assert!(b.contains("12/100"));
        assert!(b.contains("ngôn ngữ và AI"));
    }

    #[tokio::test]
    async fn a_single_item_set_skips_the_screen_entirely() {
        let s = screen_evidence(
            &Bridge::from_config(),
            "q",
            vec![ev("a", "b")],
            Duration::from_millis(1),
        )
        .await;
        assert_eq!(s.kept.len(), 1);
        assert!(s.dropped.is_empty());
        assert!(s.note.is_none());
    }

    #[tokio::test]
    async fn an_unreachable_bridge_keeps_everything_and_says_so() {
        // Fail OPEN: an LLM outage must never masquerade as "nothing on topic".
        let s = screen_evidence(
            &Bridge::new("http://127.0.0.1:1", "zeach"),
            "q",
            vec![ev("a", "b"), ev("c", "d")],
            Duration::from_millis(300),
        )
        .await;
        assert_eq!(s.kept.len(), 2);
        assert!(s.dropped.is_empty());
        assert!(s.note.unwrap().contains("giữ nguyên toàn bộ"));
    }

    #[tokio::test]
    async fn an_unreachable_reviewer_does_not_block_the_report() {
        let r = review_report(
            &Bridge::new("http://127.0.0.1:1", "zeach"),
            "q",
            "# báo cáo",
            Duration::from_millis(300),
        )
        .await;
        assert!(r.answers, "a broken reviewer must not fail a good report");
        assert!(!r.used_llm);
    }
}
