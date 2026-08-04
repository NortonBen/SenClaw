//! Stage 8: verified claims + evidence → a cited Markdown report.
//!
//! This is the deliverable `zeach_research` exists to produce. Two rules shape
//! it, both inherited from the rest of the app:
//!
//! * **Every sentence must be checkable.** The report cites evidence by `[n]`,
//!   where `n` is the 1-based position of an item in the SAME evidence list the
//!   caller receives — so a reader (or the UI) can resolve every citation. The
//!   numbering is computed here and returned, never invented by the model.
//! * **Provenance, not certainty.** Claims arrive already tiered by how many
//!   independent sources back them (`claims::assess`); the report surfaces that
//!   tier rather than restating a claim as settled fact, and disputed claims are
//!   shown as disputes.
//!
//! The LLM step is best-effort: it runs against the bridge, which has an output
//! ceiling (`finish == "length"` is an error, [[space-app-llm-bridge-output-ceiling]]).
//! A deterministic report is always built first, so a failed or truncated LLM
//! call degrades to a plain assembled report instead of returning nothing.

use crate::claims::{Claim, Contradiction, Tier, CONFIDENCE_IS_PROVENANCE};
use crate::model::Evidence;
use crate::transport::Bridge;
use std::collections::HashMap;
use std::time::Duration;

/// Characters of each evidence item shown to the model.
const PER_ITEM_CHARS: usize = 600;
/// Total evidence characters in one prompt — well under the bridge's ceiling.
const TOTAL_EVIDENCE_CHARS: usize = 22_000;
/// Claims are the backbone of the report; cap so the prompt stays bounded.
const MAX_CLAIMS: usize = 24;

const SYSTEM: &str = "Bạn là chuyên viên viết báo cáo nghiên cứu tổng hợp. \
Bạn CHỈ được dùng các KHẲNG ĐỊNH ĐÃ KIỂM CHỨNG và BẰNG CHỨNG được cung cấp — tuyệt đối không thêm kiến thức bên ngoài. \
Mỗi nhận định trong báo cáo phải dẫn nguồn bằng ký hiệu [n] trỏ tới bằng chứng được đánh số (ví dụ [1], [2][5]). \
Khi các nguồn mâu thuẫn, nêu CẢ HAI phía và nói rõ đang tranh cãi — không tự chọn một bên. \
Ưu tiên những khẳng định được nhiều nguồn độc lập xác nhận; với khẳng định chỉ một nguồn hoặc chưa được kiểm chứng, phải nói rõ mức độ chứng thực thay vì khẳng định chắc chắn. \
QUAN TRỌNG: báo cáo phải trả lời ĐÚNG câu hỏi được giao. Nếu bằng chứng không đủ để trả lời, hãy nói thẳng \
\"không đủ dữ liệu để trả lời câu hỏi này\" và nêu rõ còn thiếu gì — TUYỆT ĐỐI không chuyển sang tổng hợp một chủ đề khác \
chỉ vì bằng chứng có sẵn nói về chủ đề đó. Tiêu đề báo cáo phải là chủ đề của câu hỏi, không phải chủ đề của bằng chứng. \
Viết mạch lạc, có tiêu đề và các mục theo chủ đề. Trả về Markdown thuần, KHÔNG bọc trong khối mã.";

/// Map every evidence id to its 1-based citation number. The report and the UI
/// share this numbering so `[n]` always resolves.
pub fn number_evidence(evidence: &[Evidence]) -> HashMap<String, usize> {
    evidence
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id.clone(), i + 1))
        .collect()
}

/// Render a set of evidence ids as sorted, de-duplicated `[n]` markers.
fn cite(ids: &[String], map: &HashMap<String, usize>) -> String {
    let mut nums: Vec<usize> = ids.iter().filter_map(|id| map.get(id).copied()).collect();
    nums.sort_unstable();
    nums.dedup();
    nums.iter()
        .map(|n| format!("[{n}]"))
        .collect::<Vec<_>>()
        .join("")
}

/// Grouping order: strongest provenance first, disputes and gaps last.
fn tier_rank(t: Tier) -> u8 {
    match t {
        Tier::Verified => 0,
        Tier::Supported => 1,
        Tier::SingleSource => 2,
        Tier::Disputed => 3,
        Tier::Unverified => 4,
    }
}

fn source_label(e: &Evidence) -> String {
    e.domain
        .clone()
        .or_else(|| e.hits.first().map(|h| h.source_id.clone()))
        .unwrap_or_else(|| "?".into())
}

/// The evidence appendix — the checkable half. Always appended to the report so
/// every `[n]` in the body has a resolvable target.
fn evidence_appendix(evidence: &[Evidence], map: &HashMap<String, usize>) -> String {
    let mut out = String::from("\n\n## Nguồn dẫn\n\n");
    let mut ordered: Vec<(&Evidence, usize)> = evidence
        .iter()
        .filter_map(|e| map.get(&e.id).map(|n| (e, *n)))
        .collect();
    ordered.sort_by_key(|(_, n)| *n);
    for (e, n) in ordered {
        let url = e.url.clone().unwrap_or_default();
        let title = if e.title.trim().is_empty() {
            "(không có tiêu đề)"
        } else {
            e.title.trim()
        };
        if url.is_empty() {
            out.push_str(&format!("{n}. **{title}** — {}\n", source_label(e)));
        } else {
            out.push_str(&format!("{n}. [{title}]({url}) — {}\n", source_label(e)));
        }
    }
    out
}

/// A report assembled purely from the tiered claims — no LLM. This is the floor:
/// `zeach_research` never returns "no report", only a plainer one.
pub fn deterministic_report(
    query: &str,
    claims: &[Claim],
    contradictions: &[Contradiction],
    evidence: &[Evidence],
    map: &HashMap<String, usize>,
) -> (String, String) {
    let title = format!("Báo cáo tổng hợp: {}", query.trim());
    let mut md = format!("# {title}\n\n");

    let corroborated = claims
        .iter()
        .filter(|c| matches!(c.tier, Tier::Verified | Tier::Supported))
        .count();
    md.push_str(&format!(
        "> Tổng hợp từ {} khẳng định trên {} bằng chứng ({} khẳng định được nhiều nguồn độc lập hậu thuẫn).\n\n",
        claims.len(),
        evidence.len(),
        corroborated
    ));

    if claims.is_empty() {
        md.push_str(
            "*Không rút được khẳng định nào từ bằng chứng thu thập được. \
             Xem phần Nguồn dẫn và nhật ký nguồn để biết nguồn nào không trả về kết quả.*\n",
        );
        md.push_str(&evidence_appendix(evidence, map));
        return (title, md);
    }

    let mut by_tier: Vec<&Claim> = claims.iter().collect();
    by_tier.sort_by_key(|c| tier_rank(c.tier));

    let mut current: Option<Tier> = None;
    for c in by_tier {
        if current != Some(c.tier) {
            md.push_str(&format!("\n## {}\n\n", c.tier_label));
            current = Some(c.tier);
        }
        let mut cites = cite(&c.supports, map);
        if !c.refutes.is_empty() {
            cites.push_str(&format!(" (nguồn phản bác: {})", cite(&c.refutes, map)));
        }
        md.push_str(&format!("- {} {}\n", c.text.trim(), cites));
    }

    if !contradictions.is_empty() {
        md.push_str("\n## Điểm còn tranh cãi\n\n");
        for ct in contradictions {
            md.push_str(&format!("- {}\n", ct.summary.trim()));
        }
    }

    md.push_str(&format!("\n---\n\n*{CONFIDENCE_IS_PROVENANCE}*\n"));
    md.push_str(&evidence_appendix(evidence, map));
    (title, md)
}

/// Build the LLM prompt: numbered evidence + tiered claims + the contract.
pub fn build_prompt(
    query: &str,
    claims: &[Claim],
    contradictions: &[Contradiction],
    evidence: &[Evidence],
    map: &HashMap<String, usize>,
) -> String {
    let mut out = format!("Câu hỏi nghiên cứu: {query}\n\n");

    out.push_str(
        "KHẲNG ĐỊNH ĐÃ KIỂM CHỨNG (đã đếm số nguồn độc lập, không phải mô hình tự chấm):\n",
    );
    let mut ordered: Vec<&Claim> = claims.iter().take(MAX_CLAIMS).collect();
    ordered.sort_by_key(|c| tier_rank(c.tier));
    for c in &ordered {
        let mut line = format!(
            "- [{}] {} — nguồn hậu thuẫn: {}",
            c.tier.as_str(),
            c.text.trim(),
            cite(&c.supports, map)
        );
        if !c.refutes.is_empty() {
            line.push_str(&format!("; nguồn phản bác: {}", cite(&c.refutes, map)));
        }
        out.push('\n');
        out.push_str(&line);
    }

    if !contradictions.is_empty() {
        out.push_str("\n\nMÂU THUẪN GIỮA CÁC NGUỒN (phải nêu cả hai phía):\n");
        for ct in contradictions {
            out.push_str(&format!("- {}\n", ct.summary.trim()));
        }
    }

    out.push_str("\n\nBẰNG CHỨNG (đánh số — dùng đúng số này khi trích dẫn [n]):\n");
    let mut budget = TOTAL_EVIDENCE_CHARS;
    let mut ev_ordered: Vec<(&Evidence, usize)> = evidence
        .iter()
        .filter_map(|e| map.get(&e.id).map(|n| (e, *n)))
        .collect();
    ev_ordered.sort_by_key(|(_, n)| *n);
    for (e, n) in ev_ordered {
        let body = e.full_text.as_deref().unwrap_or(&e.snippet);
        let body = crate::util::truncate_chars(body, PER_ITEM_CHARS.min(budget));
        let block = format!(
            "[{n}] ({}) {}\n{}\n\n",
            source_label(e),
            e.title.trim(),
            body
        );
        budget = budget.saturating_sub(block.chars().count());
        out.push_str(&block);
        if budget == 0 {
            break;
        }
    }

    out.push_str(
        "\nHãy viết một BÁO CÁO Markdown mạch lạc trả lời câu hỏi trên:\n\
         - Mở đầu bằng tiêu đề `# ...` (đúng chủ đề CÂU HỎI) và một đoạn tóm tắt.\n\
         - Các mục `## ...` theo chủ đề, mỗi nhận định dẫn [n].\n\
         - Một mục cho các điểm còn tranh cãi hoặc chỉ có một nguồn.\n\
         - Nếu bằng chứng không nói về chủ đề câu hỏi: viết đúng một mục ngắn nói rõ \
         KHÔNG ĐỦ DỮ LIỆU để trả lời và còn thiếu gì. Không được thay bằng chủ đề khác.\n\
         - KHÔNG bịa số bằng chứng, KHÔNG thêm thông tin ngoài danh sách trên.\n\
         - KHÔNG cần tự liệt kê lại danh sách nguồn ở cuối (hệ thống tự thêm).",
    );
    out
}

fn strip_outer_fence(text: &str) -> String {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest
            .strip_prefix("markdown")
            .or_else(|| rest.strip_prefix("md"))
            .unwrap_or(rest);
        if let Some(end) = rest.rfind("```") {
            return rest[..end].trim().to_string();
        }
    }
    t.to_string()
}

fn first_heading(md: &str) -> Option<String> {
    md.lines()
        .find_map(|l| l.trim().strip_prefix("# ").map(|h| h.trim().to_string()))
        .filter(|h| !h.is_empty())
}

/// Outcome of the synthesis stage.
pub struct Synthesis {
    pub title: String,
    pub markdown: String,
    /// True if the LLM produced the prose; false if we fell back to the
    /// deterministic assembly.
    pub used_llm: bool,
    pub warning: Option<String>,
}

/// Produce the report. Always returns a report — the LLM only ever upgrades the
/// deterministic floor, never blocks it.
pub async fn synthesize(
    bridge: &Bridge,
    query: &str,
    claims: &[Claim],
    contradictions: &[Contradiction],
    evidence: &[Evidence],
    timeout: Duration,
) -> Synthesis {
    let map = number_evidence(evidence);
    let (fb_title, fb_md) = deterministic_report(query, claims, contradictions, evidence, &map);

    // Nothing to synthesize from — the floor IS the report.
    if claims.is_empty() {
        return Synthesis {
            title: fb_title,
            markdown: fb_md,
            used_llm: false,
            warning: Some(
                "Không có khẳng định nào để tổng hợp — báo cáo chỉ liệt kê nguồn thu được.".into(),
            ),
        };
    }

    let prompt = build_prompt(query, claims, contradictions, evidence, &map);
    match bridge.llm(SYSTEM, &prompt, 6_000, timeout).await {
        Ok(reply) if !reply.text.trim().is_empty() => {
            let body = strip_outer_fence(&reply.text);
            let title = first_heading(&body).unwrap_or_else(|| fb_title.clone());
            // The LLM is told not to repeat the source list; we always append
            // the checkable appendix so every [n] resolves.
            let markdown = format!("{body}{}", evidence_appendix(evidence, &map));
            Synthesis {
                title,
                markdown,
                used_llm: true,
                warning: None,
            }
        }
        Ok(_) => Synthesis {
            title: fb_title,
            markdown: fb_md,
            used_llm: false,
            warning: Some("LLM trả về rỗng — dùng báo cáo tự dựng từ khẳng định.".into()),
        },
        Err(e) => Synthesis {
            title: fb_title,
            markdown: fb_md,
            used_llm: false,
            warning: Some(format!(
                "Tổng hợp bằng LLM thất bại ({e}) — dùng báo cáo tự dựng từ các khẳng định đã kiểm chứng."
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claims::{assess_all, RawClaim};
    use crate::model::{Evidence, SourceKind};

    fn ev(id: &str, kind: SourceKind, url: Option<&str>) -> Evidence {
        let mut e = Evidence::new(
            "web",
            kind,
            0,
            1.0,
            format!("Tiêu đề {id}"),
            "đoạn",
            url.map(String::from),
        );
        e.id = id.to_string();
        e
    }

    fn raw(text: &str, supports: &[&str]) -> RawClaim {
        RawClaim {
            text: text.into(),
            supports: supports.iter().map(|s| s.to_string()).collect(),
            refutes: vec![],
        }
    }

    #[test]
    fn citation_numbers_match_the_evidence_list_positions() {
        let evs = vec![
            ev("e1", SourceKind::Web, Some("https://a.vn/1")),
            ev("e2", SourceKind::Web, Some("https://b.vn/1")),
        ];
        let map = number_evidence(&evs);
        assert_eq!(cite(&["e2".into(), "e1".into()], &map), "[1][2]");
    }

    #[test]
    fn a_hallucinated_id_produces_no_citation_marker() {
        let evs = vec![ev("e1", SourceKind::Web, Some("https://a.vn/1"))];
        let map = number_evidence(&evs);
        // "ghost" is not in the run — it must simply not render, never as [0].
        assert_eq!(cite(&["ghost".into()], &map), "");
    }

    #[test]
    fn the_deterministic_report_always_has_a_body_and_source_appendix() {
        let evs = vec![
            ev("e1", SourceKind::Web, Some("https://vnexpress.net/a")),
            ev("e2", SourceKind::Web, Some("https://tuoitre.vn/b")),
        ];
        let claims = assess_all(&[raw("Lãi suất giữ nguyên.", &["e1", "e2"])], &evs);
        let map = number_evidence(&evs);
        let (title, md) = deterministic_report("lãi suất", &claims, &[], &evs, &map);
        assert!(title.contains("lãi suất"));
        assert!(md.contains("[1][2]"), "claim must cite both publishers");
        assert!(md.contains("## Nguồn dẫn"));
        assert!(md.contains("vnexpress.net"));
    }

    #[test]
    fn an_empty_claim_set_still_yields_a_report_naming_the_gap() {
        let evs = vec![ev("e1", SourceKind::Web, Some("https://a.vn/1"))];
        let map = number_evidence(&evs);
        let (_, md) = deterministic_report("q", &[], &[], &evs, &map);
        assert!(md.contains("Không rút được khẳng định"));
        assert!(md.contains("## Nguồn dẫn"));
    }

    #[test]
    fn an_outer_markdown_fence_is_stripped_but_inner_content_kept() {
        let s = "```markdown\n# Tiêu đề\n\nNội dung [1].\n```";
        let out = strip_outer_fence(s);
        assert!(out.starts_with("# Tiêu đề"));
        assert!(!out.contains("```"));
    }

    #[test]
    fn the_first_heading_becomes_the_title() {
        assert_eq!(
            first_heading("# Báo cáo A\n\nnội dung"),
            Some("Báo cáo A".into())
        );
        assert_eq!(first_heading("không có tiêu đề"), None);
    }
}
