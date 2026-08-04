//! Parser khoan dung cho JSON member/manager trả về — model hay bọc ```json,
//! thêm lời dẫn, hoặc trả JSON đứt đuôi (finish=="length").

use serde::Deserialize;
use serde_json::Value;

/// Rút khối JSON đầu tiên: ưu tiên fence ```json ... ```, sau đó quét ngoặc
/// cân bằng có nhận biết chuỗi/escape.
pub fn extract_json(text: &str) -> Option<Value> {
    if let Some(start) = text.find("```json") {
        let rest = &text[start + 7..];
        if let Some(end) = rest.find("```") {
            if let Ok(v) = serde_json::from_str::<Value>(rest[..end].trim()) {
                return Some(v);
            }
        }
    }
    if let Some(start) = text.find("```") {
        let rest = &text[start + 3..];
        let rest = rest.trim_start_matches(|c: char| c.is_ascii_alphabetic());
        if let Some(end) = rest.find("```") {
            if let Ok(v) = serde_json::from_str::<Value>(rest[..end].trim()) {
                return Some(v);
            }
        }
    }
    // Quét { ... } cân bằng
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = balanced_end(text, i) {
                if let Ok(v) = serde_json::from_str::<Value>(&text[i..=end]) {
                    return Some(v);
                }
            }
        }
        i += 1;
    }
    None
}

fn balanced_end(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i64;
    let mut in_str = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

// ---------------- Cấu trúc lượt member ----------------

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CitationOut {
    #[serde(default)]
    pub kind: String, // doc|url|tool
    #[serde(default, alias = "ref")]
    pub r#ref: String,
    #[serde(default)]
    pub quote: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReactionOut {
    #[serde(default)]
    pub reply_to: i64,
    #[serde(default)]
    pub stance: String, // agree|disagree
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub supplement: String,
    #[serde(default)]
    pub citations: Vec<CitationOut>,
    #[serde(default)]
    pub hat: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClaimOut {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub claim_type: String, // evidence|inference|creative
    #[serde(default)]
    pub provability: String, // practical|theoretical
    #[serde(default)]
    pub hat: String,
    #[serde(default)]
    pub citations: Vec<CitationOut>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TurnOut {
    #[serde(default)]
    pub reactions: Vec<ReactionOut>,
    #[serde(default)]
    pub claims: Vec<ClaimOut>,
    #[serde(default)]
    pub memory_notes: Vec<String>,
    #[serde(default)]
    pub thinking: String,
}

pub fn parse_turn(text: &str) -> Option<TurnOut> {
    let v = extract_json(text)?;
    serde_json::from_value(v).ok()
}

// ---------------- Cấu trúc đánh giá Manager ----------------

#[derive(Debug, Clone, Deserialize, Default)]
pub struct NudgeOut {
    #[serde(default)]
    pub member: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ManagerOut {
    #[serde(default)]
    pub score: i64,
    #[serde(default)]
    pub met: bool,
    #[serde(default)]
    pub missing: Vec<String>,
    #[serde(default)]
    pub nudges: Vec<NudgeOut>,
    #[serde(default)]
    pub note: String,
}

pub fn parse_manager(text: &str) -> Option<ManagerOut> {
    let v = extract_json(text)?;
    serde_json::from_value(v).ok()
}

pub fn valid_claim_type(s: &str) -> Option<&'static str> {
    match s.trim().to_lowercase().as_str() {
        "evidence" | "dẫn chứng" | "dan chung" => Some("evidence"),
        "inference" | "suy diễn" | "suy dien" | "suy luận" | "suy luan" => Some("inference"),
        "creative" | "sáng tạo" | "sang tao" => Some("creative"),
        _ => None,
    }
}

pub fn valid_provability(s: &str) -> Option<&'static str> {
    match s.trim().to_lowercase().as_str() {
        "practical" | "thực tiễn" | "thuc tien" => Some("practical"),
        "theoretical" | "lý thuyết" | "ly thuyet" => Some("theoretical"),
        _ => None,
    }
}

/// Chuẩn hoá mũ thiên hướng NHIỀU giá trị: nhận `"black, red"` hoặc
/// `["black","red"]` → chuỗi phẩy đã validate + khử trùng lặp.
/// `None` khi không có mũ hợp lệ nào (caller quyết định giữ nguyên hay xoá).
pub fn normalize_hats(v: &serde_json::Value) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    match v {
        Value::String(s) => parts.extend(s.split(',').map(str::to_string)),
        Value::Array(a) => parts.extend(a.iter().filter_map(|x| x.as_str()).map(str::to_string)),
        _ => {}
    }
    let mut out: Vec<&'static str> = Vec::new();
    for p in parts {
        if let Some(h) = valid_hat(&p) {
            if !out.contains(&h) {
                out.push(h);
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join(","))
    }
}

pub fn valid_hat(s: &str) -> Option<&'static str> {
    match s.trim().to_lowercase().as_str() {
        "white" | "trắng" | "trang" => Some("white"),
        "red" | "đỏ" | "do" => Some("red"),
        "black" | "đen" | "den" => Some("black"),
        "yellow" | "vàng" | "vang" => Some("yellow"),
        "green" | "xanh lá" | "xanh la" => Some("green"),
        "blue" | "xanh dương" | "xanh duong" => Some("blue"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_fenced_json() {
        let t = "Đây là kết quả:\n```json\n{\"claims\":[{\"content\":\"A\",\"claim_type\":\"evidence\"}]}\n```\nhết.";
        let turn = parse_turn(t).unwrap();
        assert_eq!(turn.claims.len(), 1);
        assert_eq!(turn.claims[0].claim_type, "evidence");
    }

    #[test]
    fn extracts_bare_json_with_noise() {
        let t = "Tôi nghĩ vậy. {\"reactions\":[{\"reply_to\":5,\"stance\":\"disagree\",\"content\":\"không ổn\",\"citations\":[{\"kind\":\"url\",\"ref\":\"https://a.vn\",\"quote\":\"x { } y\"}]}],\"thinking\":\"vì...\"} xong";
        let turn = parse_turn(t).unwrap();
        assert_eq!(turn.reactions.len(), 1);
        assert_eq!(turn.reactions[0].reply_to, 5);
        assert_eq!(turn.reactions[0].citations[0].r#ref, "https://a.vn");
    }

    #[test]
    fn handles_braces_inside_strings() {
        let t = r#"{"note":"chuỗi có { và } bên trong","score":80,"met":false,"missing":["còn thiếu A"]}"#;
        let m = parse_manager(t).unwrap();
        assert_eq!(m.score, 80);
        assert_eq!(m.missing.len(), 1);
    }

    #[test]
    fn truncated_json_returns_none() {
        let t = r#"{"claims":[{"content":"bị cắt giữa chừ"#;
        assert!(parse_turn(t).is_none());
    }

    #[test]
    fn normalizes_vietnamese_labels() {
        assert_eq!(valid_claim_type("Sáng tạo"), Some("creative"));
        assert_eq!(valid_provability("Thực tiễn"), Some("practical"));
        assert_eq!(valid_hat("xanh lá"), Some("green"));
        assert_eq!(valid_claim_type("xyz"), None);
    }

    #[test]
    fn normalize_hats_string_array_dedup() {
        use serde_json::json;
        assert_eq!(normalize_hats(&json!("black, red")), Some("black,red".into()));
        assert_eq!(normalize_hats(&json!(["đen", "red", "black"])), Some("black,red".into()));
        assert_eq!(normalize_hats(&json!("xyz")), None);
        assert_eq!(normalize_hats(&json!([])), None);
    }
}
