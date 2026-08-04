//! Stage 7: evidence → atomic claims, via one `llm.request`.
//!
//! Two constraints shape this, both learned the hard way elsewhere in the repo:
//!
//! * the bridge has **no `temperature`** ([[space-app-llm-bridge-no-temperature]]),
//!   so determinism has to come from the prompt and from validation, not a knob;
//! * the bridge has a practical **output ceiling**, and `finish == "length"` is
//!   an error rather than a short answer
//!   ([[space-app-llm-bridge-output-ceiling]]) — so the input must be capped and
//!   the output kept small.
//!
//! Evidence is presented as `[E1]`, `[E2]` … rather than raw ids. Asking a model
//! to copy `ev_18c40e2ab2215be80` back verbatim is asking for transcription
//! errors; a small integer is hard to get wrong, and an out-of-range one is
//! caught deterministically instead of becoming an uncheckable citation.

use crate::claims::{RawClaim, RawContradiction};
use crate::model::Evidence;
use crate::transport::Bridge;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::time::Duration;

/// Characters of each evidence item shown to the model.
const PER_ITEM_CHARS: usize = 700;
/// Total characters of evidence in one prompt — well under the observed ceiling.
const TOTAL_EVIDENCE_CHARS: usize = 24_000;

const SYSTEM: &str = "Bạn là trợ lý kiểm chứng thông tin. Bạn KHÔNG được dùng kiến thức có sẵn: \
chỉ được rút ra khẳng định từ các đoạn bằng chứng được cung cấp. \
Mỗi khẳng định phải là một mệnh đề NGUYÊN TỬ, kiểm chứng được, và phải dẫn ít nhất một bằng chứng. \
Nếu bằng chứng mâu thuẫn nhau, hãy nêu CẢ HAI phía và khai báo mâu thuẫn — tuyệt đối không tự chọn một bên. \
Chỉ trả về JSON, không giải thích, không rào đón, không khối mã.";

/// Numbered evidence + the JSON contract.
pub fn build_prompt(query: &str, evidence: &[Evidence]) -> String {
    let mut out = format!("Câu hỏi: {query}\n\nBằng chứng (chỉ dùng đúng những đoạn này):\n");
    let mut budget = TOTAL_EVIDENCE_CHARS;
    for (i, e) in evidence.iter().enumerate() {
        // Prefer fetched page text over a SERP snippet when we have it.
        let body = e.full_text.as_deref().unwrap_or(&e.snippet);
        let body = crate::util::truncate_chars(body, PER_ITEM_CHARS.min(budget));
        if body.trim().is_empty() && e.title.trim().is_empty() {
            continue;
        }
        let source = e.domain.clone().unwrap_or_else(|| {
            e.hits
                .first()
                .map(|h| h.source_id.clone())
                .unwrap_or_default()
        });
        let block = format!("[E{}] ({}) {}\n{}\n\n", i + 1, source, e.title, body);
        budget = budget.saturating_sub(block.chars().count());
        out.push_str(&block);
        if budget == 0 {
            break;
        }
    }

    out.push_str(
        "\nTrả về ĐÚNG JSON sau, không kèm gì khác:\n\
         {\"claims\":[{\"text\":\"...\",\"supports\":[1,2],\"refutes\":[]}],\
         \"contradictions\":[{\"claim_a\":0,\"claim_b\":1,\"summary\":\"...\"}]}\n\
         - supports/refutes: SỐ THỨ TỰ của [E…], ví dụ [E3] thì ghi 3.\n\
         - claim_a/claim_b: CHỈ SỐ trong mảng claims (bắt đầu từ 0).\n\
         - Tối đa 12 khẳng định. Không bịa số hiệu bằng chứng.",
    );
    out
}

#[derive(Debug, Deserialize)]
struct ExtractedRaw {
    #[serde(default)]
    claims: Vec<ClaimRaw>,
    #[serde(default)]
    contradictions: Vec<RawContradiction>,
}

#[derive(Debug, Deserialize)]
struct ClaimRaw {
    #[serde(default)]
    text: String,
    // Indices are 1-based over the evidence list; accept numbers or numeric
    // strings, since models emit both.
    #[serde(default)]
    supports: Vec<serde_json::Value>,
    #[serde(default)]
    refutes: Vec<serde_json::Value>,
}

/// Strip prose and code fences and return the JSON body, if any.
///
/// Models wrap JSON in ```json fences, prepend "Đây là kết quả:", or trail a
/// closing remark, no matter what the prompt says.
pub(crate) fn isolate_json(text: &str) -> Option<&str> {
    let t = text.trim();
    let t = match t.find("```") {
        Some(start) => {
            let after = &t[start + 3..];
            let after = after.strip_prefix("json").unwrap_or(after);
            match after.find("```") {
                Some(end) => after[..end].trim(),
                None => after.trim(),
            }
        }
        None => t,
    };
    let start = t.find('{')?;
    Some(&t[start..])
}

/// Scan JSON, tracking the open-bracket stack and string state.
///
/// Returns `(byte offset just past the balanced object, stack, in_string)`.
pub(crate) fn scan(s: &str) -> (Option<usize>, Vec<char>, bool) {
    let mut stack: Vec<char> = Vec::new();
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' | '[' => stack.push(c),
            '}' | ']' => {
                stack.pop();
                if stack.is_empty() {
                    return (Some(i + c.len_utf8()), stack, in_str);
                }
            }
            _ => {}
        }
    }
    (None, stack, in_str)
}

/// Close whatever a truncated response left open.
///
/// The bridge's output ceiling cuts responses mid-structure, and a truncated
/// list of claims is still worth more than nothing — every claim is validated
/// downstream regardless. Brackets must be closed in reverse order of opening;
/// closing only `}` (as an earlier version did) leaves `[` open and the parse
/// still fails.
fn close_open(s: &str) -> String {
    let (_, stack, in_str) = scan(s);
    let mut out = s.to_string();
    if in_str {
        out.push('"');
    }
    while matches!(out.chars().last(), Some(c) if c == ',' || c.is_whitespace()) {
        out.pop();
    }
    for open in stack.iter().rev() {
        out.push(match open {
            '{' => '}',
            _ => ']',
        });
    }
    out
}

/// Candidate JSON strings to try, best first.
///
/// A cut that lands mid-key (`{"text":"A","sup`) cannot be closed into valid
/// JSON at all, so the fallbacks progressively discard the trailing partial
/// element and re-close at an earlier complete boundary.
pub(crate) fn candidates(body: &str) -> Vec<String> {
    if let (Some(end), _, _) = scan(body) {
        return vec![body[..end].to_string()];
    }
    let mut out = vec![close_open(body)];
    // Cut back to successively earlier `}` boundaries.
    let mut cut = body.len();
    for _ in 0..3 {
        match body[..cut].rfind('}') {
            Some(i) if i > 0 => {
                out.push(close_open(&body[..=i]));
                cut = i;
            }
            _ => break,
        }
    }
    out
}

/// Parse a JSON object out of a model reply, repairing a response the bridge
/// cut short. Shared with `review.rs`: the output ceiling truncates every
/// caller's reply, not just this one ([[space-app-llm-bridge-output-ceiling]]).
pub(crate) fn parse_lenient_object(text: &str) -> Option<serde_json::Value> {
    let body = isolate_json(text)?;
    candidates(body)
        .into_iter()
        .find_map(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .filter(|v| v.is_object())
}

fn as_index(v: &serde_json::Value) -> Option<usize> {
    v.as_u64().map(|n| n as usize).or_else(|| {
        v.as_str()
            .and_then(|s| s.trim().trim_start_matches('E').parse().ok())
    })
}

/// Map 1-based evidence indices onto real evidence ids.
///
/// Out-of-range indices are passed through as a sentinel so `claims::assess`
/// records them as dropped citations rather than silently ignoring them — a
/// model that cited [E99] of 12 should leave a trace.
fn to_ids(indices: &[serde_json::Value], evidence: &[Evidence]) -> Vec<String> {
    indices
        .iter()
        .filter_map(as_index)
        .map(|n| match n.checked_sub(1).and_then(|i| evidence.get(i)) {
            Some(e) => e.id.clone(),
            None => format!("E{n}(không tồn tại)"),
        })
        .collect()
}

pub fn parse_response(
    text: &str,
    evidence: &[Evidence],
) -> Result<(Vec<RawClaim>, Vec<RawContradiction>)> {
    let body = isolate_json(text).ok_or_else(|| anyhow!("phản hồi không chứa JSON nào"))?;
    let mut last_err = None;
    let raw: ExtractedRaw = 'parsed: {
        for candidate in candidates(body) {
            match serde_json::from_str::<ExtractedRaw>(&candidate) {
                Ok(v) => break 'parsed v,
                Err(e) => last_err = Some(e),
            }
        }
        return Err(anyhow!(
            "JSON không hợp lệ: {}",
            last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "không phân tích được".into())
        ));
    };

    let claims = raw
        .claims
        .into_iter()
        .filter(|c| !c.text.trim().is_empty())
        .map(|c| RawClaim {
            text: c.text,
            supports: to_ids(&c.supports, evidence),
            refutes: to_ids(&c.refutes, evidence),
        })
        .collect();
    Ok((claims, raw.contradictions))
}

/// Run the extraction. Returns claims + contradictions, both still unvalidated.
pub async fn extract_claims(
    bridge: &Bridge,
    query: &str,
    evidence: &[Evidence],
    timeout: Duration,
) -> Result<(Vec<RawClaim>, Vec<RawContradiction>)> {
    if evidence.is_empty() {
        return Ok((vec![], vec![]));
    }
    let reply = bridge
        .llm(SYSTEM, &build_prompt(query, evidence), 4_000, timeout)
        .await?;
    parse_response(&reply.text, evidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SourceKind;

    fn evidence(n: usize) -> Vec<Evidence> {
        (0..n)
            .map(|i| {
                let mut e = Evidence::new(
                    "web",
                    SourceKind::Web,
                    i as u32,
                    1.0,
                    format!("Tiêu đề {i}"),
                    format!("Nội dung {i}"),
                    Some(format!("https://site{i}.vn/a")),
                );
                e.id = format!("ev{i}");
                e
            })
            .collect()
    }

    #[test]
    fn plain_json_parses() {
        let evs = evidence(3);
        let (claims, _) = parse_response(
            r#"{"claims":[{"text":"Lãi suất giữ nguyên.","supports":[1,3],"refutes":[]}]}"#,
            &evs,
        )
        .unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(
            claims[0].supports,
            vec!["ev0".to_string(), "ev2".to_string()]
        );
    }

    #[test]
    fn a_fenced_response_with_prose_around_it_still_parses() {
        let evs = evidence(2);
        let text = "Đây là kết quả:\n```json\n{\"claims\":[{\"text\":\"A\",\"supports\":[1]}]}\n```\nHy vọng giúp được bạn.";
        let (claims, _) = parse_response(text, &evs).unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].supports, vec!["ev0".to_string()]);
    }

    #[test]
    fn a_truncated_response_is_repaired_rather_than_discarded() {
        // The bridge's output ceiling cuts mid-object; the claims that did
        // arrive are still usable, and every one is validated downstream.
        let evs = evidence(3);
        let text = r#"{"claims":[{"text":"Khẳng định một","supports":[1]},{"text":"Khẳng định hai","supports":[2"#;
        let (claims, _) = parse_response(text, &evs).unwrap();
        assert!(!claims.is_empty(), "should salvage what arrived");
        assert_eq!(claims[0].text, "Khẳng định một");
    }

    #[test]
    fn a_cut_landing_mid_key_falls_back_to_the_last_complete_claim() {
        // Unclosable at the cut point: the fallback must discard the partial
        // element rather than lose the whole response.
        let evs = evidence(3);
        let text = r#"{"claims":[{"text":"Khẳng định một","supports":[1]},{"text":"Hai","sup"#;
        let (claims, _) = parse_response(text, &evs).unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].text, "Khẳng định một");
    }

    #[test]
    fn closing_only_braces_would_not_be_enough() {
        // Regression: an earlier version closed `}` but not `]`, so every
        // truncated response failed to parse.
        let repaired = close_open(r#"{"claims":[{"text":"A","supports":[1"#);
        assert!(repaired.ends_with("]}]}"), "got {repaired}");
        assert!(serde_json::from_str::<serde_json::Value>(&repaired).is_ok());
    }

    #[test]
    fn a_truncated_string_is_closed_before_repair() {
        let evs = evidence(2);
        let text = r#"{"claims":[{"text":"Câu bị cắt giữa chừ"#;
        // Must not panic, and must either parse or error cleanly.
        let _ = parse_response(text, &evs);
    }

    #[test]
    fn out_of_range_evidence_indices_leave_a_visible_trace() {
        // [E99] of 3 must not vanish silently — claims::assess reports it as a
        // dropped citation.
        let evs = evidence(3);
        let (claims, _) =
            parse_response(r#"{"claims":[{"text":"A","supports":[1,99]}]}"#, &evs).unwrap();
        assert_eq!(claims[0].supports.len(), 2);
        assert!(claims[0].supports[1].contains("không tồn tại"));

        let assessed = crate::claims::assess("c".into(), &claims[0], &evs);
        assert_eq!(assessed.supports, vec!["ev0".to_string()]);
        assert_eq!(assessed.dropped_citations.len(), 1);
    }

    #[test]
    fn numeric_strings_and_e_prefixes_are_accepted() {
        let evs = evidence(3);
        let (claims, _) =
            parse_response(r#"{"claims":[{"text":"A","supports":["2","E3"]}]}"#, &evs).unwrap();
        assert_eq!(
            claims[0].supports,
            vec!["ev1".to_string(), "ev2".to_string()]
        );
    }

    #[test]
    fn a_zero_index_is_treated_as_out_of_range_not_as_the_first_item() {
        // Indices are 1-based; silently mapping 0 → first item would attach a
        // citation the model never made.
        let evs = evidence(3);
        let (claims, _) =
            parse_response(r#"{"claims":[{"text":"A","supports":[0]}]}"#, &evs).unwrap();
        assert!(claims[0].supports[0].contains("không tồn tại"));
    }

    #[test]
    fn a_response_with_no_json_is_an_error_not_an_empty_result() {
        // Silently returning zero claims would read as "nothing to say" when
        // the truth is "the model did not answer".
        let evs = evidence(2);
        assert!(parse_response("Xin lỗi, tôi không thể giúp.", &evs).is_err());
    }

    #[test]
    fn contradictions_survive_parsing() {
        let evs = evidence(2);
        let (_, cts) = parse_response(
            r#"{"claims":[{"text":"A","supports":[1]},{"text":"B","supports":[2]}],
                "contradictions":[{"claim_a":0,"claim_b":1,"summary":"trái ngược"}]}"#,
            &evs,
        )
        .unwrap();
        assert_eq!(cts.len(), 1);
        assert_eq!(cts[0].summary, "trái ngược");
    }

    #[test]
    fn braces_inside_strings_do_not_end_the_object() {
        let evs = evidence(1);
        let (claims, _) = parse_response(
            r#"{"claims":[{"text":"Giá trị là {x} và \"trích dẫn\"","supports":[1]}]}"#,
            &evs,
        )
        .unwrap();
        assert!(claims[0].text.contains("{x}"));
    }

    #[test]
    fn the_prompt_numbers_evidence_and_stays_within_budget() {
        let evs = evidence(200);
        let p = build_prompt("câu hỏi", &evs);
        assert!(p.contains("[E1]"));
        assert!(
            p.chars().count() < TOTAL_EVIDENCE_CHARS + 2_000,
            "prompt must stay under the bridge's input budget: {}",
            p.chars().count()
        );
    }

    #[test]
    fn the_prompt_prefers_fetched_page_text_over_a_snippet() {
        let mut evs = evidence(1);
        evs[0].full_text = Some("Toàn văn đã tải về".into());
        let p = build_prompt("q", &evs);
        assert!(p.contains("Toàn văn đã tải về"));
        assert!(!p.contains("Nội dung 0"));
    }
}
