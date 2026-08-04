//! Bridge to the SenClaw daemon's shared LLM.
//!
//! This app never talks to a provider directly — every completion goes through
//! the daemon's space-app bridge. That replaces the Go backend's entire
//! `service/llm/provider` factory: no Gemini/DeepSeek/OpenRouter implementations,
//! no API-key pool, no per-user provider settings, no rate-limit bookkeeping.

use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};

use crate::config;

pub fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .expect("build http client")
    })
}

/// Optional SenClaw LLM-config profile (label or id) this app composes with.
/// Empty = follow the daemon's active model.
fn profile_cell() -> &'static std::sync::RwLock<String> {
    static P: OnceLock<std::sync::RwLock<String>> = OnceLock::new();
    P.get_or_init(|| std::sync::RwLock::new(String::new()))
}

pub fn set_profile(p: &str) {
    if let Ok(mut w) = profile_cell().write() {
        *w = p.trim().to_string();
    }
}

pub fn profile() -> String {
    profile_cell().read().map(|r| r.clone()).unwrap_or_default()
}

/// reqwest's `Display` hides the underlying cause; walk the chain.
fn describe(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        out.push_str(&format!(": {s}"));
        src = s.source();
    }
    out
}

/// One completion through the daemon bridge.
///
/// Returns `(text, model, finish)` where `finish == "length"` means the provider
/// truncated the output at the token cap — the caller must treat that as a
/// failure for a rewrite, since a silently truncated chunk would be persisted as
/// if it were complete.
pub async fn bridge_llm(
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<(String, String, String), String> {
    let url = format!(
        "{}/api/space/apps/{}/bridge",
        config::senclaw_base_url().trim_end_matches('/'),
        config::app_id()
    );
    let mut payload = json!({ "system": system, "prompt": user, "maxTokens": max_tokens });
    let p = profile();
    if !p.is_empty() {
        payload["profile"] = json!(p);
    }
    let body = json!({ "action": "llm.request", "payload": payload });

    let mut last_err = String::new();
    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(700 * attempt as u64)).await;
        }
        let resp = match http().post(&url).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("bridge llm.request failed ({url}): {}", describe(&e));
                continue;
            }
        };
        let v: Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                last_err = format!("invalid bridge response: {}", describe(&e));
                continue;
            }
        };
        return match v.get("status").and_then(|x| x.as_str()) {
            Some("ok") => Ok((
                v.get("text")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("model")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("finish")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
            )),
            Some("pending") => Err("bridge LLM chưa được bật trong daemon này".to_string()),
            _ => Err(v
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown LLM error")
                .to_string()),
        };
    }
    Err(last_err)
}

/// Parameters shaping one chunk rewrite.
pub struct RewriteParams<'a> {
    /// The rewrite plan / target style (`version_plan` in the Go original).
    pub target_style: &'a str,
    /// Free-form extra instructions from the user (`user_prompt`).
    pub additional_requirements: &'a str,
    /// Tail of the previously rewritten chunk, to keep the prose continuous.
    pub previous_chunk_paragraph: &'a str,
    pub target_language: &'a str,
    /// 0-100. Drives temperature.
    pub creativity_ratio: i64,
    /// Percentage tolerance around the source chunk's length.
    pub target_length_variance: i64,
}

/// Turns the 0-100 creativity ratio into an explicit instruction.
///
/// The Go original mapped this onto sampling temperature (0.2-1.0). The SenClaw
/// bridge exposes no temperature knob — `llm.request` takes only
/// system/prompt/maxTokens/profile — so mapping it there would silently make the
/// app's headline setting do nothing. Stating it in the prompt is also the more
/// faithful control: temperature governs token randomness, whereas what this
/// setting actually means is *how far the rewrite may drift from the source*.
pub fn creativity_instruction(creativity_ratio: i64) -> &'static str {
    match creativity_ratio {
        i64::MIN..=20 => "MỨC SÁNG TẠO: Rất thấp. Bám sát bản gốc, chủ yếu trau chuốt câu chữ; giữ nguyên tình tiết, lời thoại và trình tự sự kiện.",
        21..=45 => "MỨC SÁNG TẠO: Thấp. Giữ nguyên cốt truyện, tình tiết và trình tự sự kiện; làm mới cách diễn đạt và nhịp văn.",
        46..=70 => "MỨC SÁNG TẠO: Trung bình. Giữ cốt truyện chính và kết cục; được phép đổi chi tiết phụ, lời thoại và cách dẫn dắt.",
        _ => "MỨC SÁNG TẠO: Cao. Chỉ giữ khung sự kiện chính và tính cách nhân vật; tự do sáng tạo chi tiết, miêu tả, lời thoại và nhịp kể.",
    }
}

/// Builds the chunk-rewrite prompt.
///
/// Ported from `service/llm/story_rewrite.go:34-42`, preserving the wording and
/// section order. One deliberate fix: the length requirement counts **characters**.
/// Go interpolated `len(chunkText)`, a byte count, into a sentence that says
/// "ký tự" (characters) — on Vietnamese text that overstates the target by ~33%
/// (measured on the project's own corpus), while the `±variance` tolerance next
/// to it is only a few percent.
pub fn build_rewrite_prompt(chunk_text: &str, p: &RewriteParams<'_>) -> String {
    let char_count = chunk_text.chars().count();
    let variance = p.target_length_variance.clamp(0, 100) as f64 / 100.0;
    let lo = ((char_count as f64) * (1.0 - variance)).round() as usize;
    let hi = ((char_count as f64) * (1.0 + variance)).round() as usize;
    // Spelled out as an explicit target with bounds, plus an anti-summarisation
    // clause. The Go phrasing ("xấp xỉ bằng ĐOẠN VĂN BẢN GỐC bên dưới (biến thiên
    // trong khoảng N ký tự ±M%)") left it ambiguous whether N was the target or
    // the tolerance, and models read it as licence to condense: measured output
    // ran ~18% of the source length across every chunk.
    let length_req = format!(
        "ĐỘ DÀI BẮT BUỘC: Đoạn viết lại phải dài khoảng {char_count} ký tự (tối thiểu {lo}, tối đa {hi}). \
         TUYỆT ĐỐI KHÔNG tóm tắt, rút gọn hay lược bỏ tình tiết — phải viết lại ĐẦY ĐỦ toàn bộ nội dung của đoạn gốc, \
         giữ nguyên mọi lời thoại và diễn biến, chỉ thay đổi cách hành văn."
    );

    let previous_chunk_info = if p.previous_chunk_paragraph.is_empty() {
        String::new()
    } else {
        format!(
            "\nTHÔNG TIN LIÊN TỤC NGỮ CẢNH (CHUNK TRƯỚC ĐÃ VIẾT LẠI):\n\"\"\"\n{}\n\"\"\"\n",
            p.previous_chunk_paragraph
        )
    };

    format!(
        "Viết lại đoạn văn sau theo yêu cầu:\n\
         PHONG CÁCH: {style}\n\
         YÊU CẦU THÊM: {extra}\n\
         {creativity}\n\
         {length_req}{previous_chunk_info}\n\
         VĂN BẢN GỐC (Phần cần viết lại):\n\
         \"\"\"\n{chunk_text}\n\"\"\"\n\
         YÊU CẦU TRẢ VỀ: Chỉ trả về nội dung văn bản đã được viết lại, không có lời dẫn hay giải thích. Ngôn ngữ: {lang}.\n\
         NHẮC LẠI: đoạn trả về phải dài khoảng {char_count} ký tự — viết lại đầy đủ, không tóm tắt.",
        style = p.target_style,
        extra = p.additional_requirements,
        creativity = creativity_instruction(p.creativity_ratio),
        lang = p.target_language,
    )
}

/// Default output-token budget per chunk.
///
/// Measured, not guessed. On the daemon's bridge the returned length tracks
/// `maxTokens` almost linearly and well below one character per token, so a
/// conservative value silently truncates the rewrite without ever setting
/// `finish = "length"`: asking 8192 for a 4000-character chunk produced 1118
/// characters (0.28x), while 32000 produced 2180 (0.55x) and 100000 added
/// nothing. Overriding is a setting because this is provider-specific.
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 32_000;

/// Largest source chunk, in characters.
///
/// This is the single most important number in the app, and it is empirical.
/// The model answers with roughly the same amount of prose no matter how much
/// you give it — measured on the daemon's bridge, a 4143-character chunk came
/// back as 2277 characters and a 5531-character chunk as 2246, while a
/// 2261-character chunk came back as 2337 (a complete rewrite). Raising
/// `max_output_tokens` past 32000 does not move that ceiling.
///
/// So a chunk larger than what the model will write back does not produce a
/// longer rewrite — it produces a *summary*, silently, with `finish = "stop"`.
/// Keeping chunks under the ceiling is what makes the output a rewrite at all.
pub const MAX_CHUNK_CHARS: usize = 2000;

/// Below this fraction of the requested minimum, a rewrite is treated as the
/// model having summarised rather than rewritten, and is retried once.
const TOO_SHORT_FRACTION: f64 = 0.7;

/// The default system instruction, used when the process doesn't carry one.
pub const DEFAULT_SYSTEM_INSTRUCTION: &str =
    "Bạn là một biên tập viên chuyên nghiệp, chuyên viết lại truyện.";

/// Rewrite one chunk.
///
/// The system instruction is sent as a real system prompt. In the Go original it
/// reached the model only by way of Gemini's context-cache API, so whenever cache
/// creation failed — a non-fatal, logged-and-continue path — the entire system
/// instruction was silently dropped and the rewrite ran unguided.
pub async fn rewrite_chunk(
    system_instruction: &str,
    chunk_text: &str,
    p: &RewriteParams<'_>,
    max_output_tokens: u32,
) -> Result<String, String> {
    let source_chars = chunk_text.chars().count();
    let prompt = build_rewrite_prompt(chunk_text, p);
    let max_tokens = max_output_tokens.max(2048);

    let mut text = one_pass(system_instruction, &prompt, max_tokens).await?;

    // Models on this bridge routinely answer a full-length rewrite request with a
    // condensed retelling — and report `finish = "stop"` while doing it, so
    // nothing downstream would notice. A short result is caught here and pushed
    // back once with the shortfall stated explicitly.
    let variance = p.target_length_variance.clamp(0, 100) as f64 / 100.0;
    let floor = ((source_chars as f64) * (1.0 - variance) * TOO_SHORT_FRACTION) as usize;
    if text.chars().count() < floor {
        let retry_prompt = format!(
            "{prompt}\n\nCẢNH BÁO: Bản nháp trước của bạn chỉ dài {} ký tự, quá ngắn so với yêu cầu {} ký tự — \
             bạn đã TÓM TẮT thay vì viết lại. Hãy viết lại lần nữa, bám sát từng tình tiết và từng lời thoại \
             của đoạn gốc theo đúng thứ tự, diễn đạt đầy đủ chứ không lược bỏ.",
            text.chars().count(),
            source_chars
        );
        match one_pass(system_instruction, &retry_prompt, max_tokens).await {
            // Keep whichever attempt came closer to the target.
            Ok(second) if second.chars().count() > text.chars().count() => text = second,
            Ok(_) => {}
            Err(e) => eprintln!("[llm] length retry failed, keeping first draft: {e}"),
        }
    }

    Ok(text)
}

async fn one_pass(system: &str, prompt: &str, max_tokens: u32) -> Result<String, String> {
    let (text, _model, finish) = bridge_llm(system, prompt, max_tokens).await?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("model trả về nội dung rỗng".to_string());
    }
    if finish == "length" {
        // Persisting a truncated chunk would corrupt the assembled story and,
        // worse, the resume logic would treat it as done.
        return Err(format!(
            "model cắt output giữa chừng (finish=length, max_tokens={max_tokens}) — \
             hãy giảm hybrid_split_max_size hoặc tăng max_output_tokens"
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params<'a>() -> RewriteParams<'a> {
        RewriteParams {
            target_style: "Cổ trang",
            additional_requirements: "Giữ tên nhân vật",
            previous_chunk_paragraph: "",
            target_language: "Vietnamese",
            creativity_ratio: 40,
            target_length_variance: 5,
        }
    }

    #[test]
    fn creativity_ratio_actually_reaches_the_model() {
        // The setting is worthless if it never lands in the prompt — which is
        // exactly what a temperature mapping would have done, since the bridge
        // has no temperature parameter.
        let mut low = params();
        low.creativity_ratio = 10;
        let mut high = params();
        high.creativity_ratio = 90;

        let low_prompt = build_rewrite_prompt("x", &low);
        let high_prompt = build_rewrite_prompt("x", &high);

        assert!(low_prompt.contains("MỨC SÁNG TẠO: Rất thấp"));
        assert!(high_prompt.contains("MỨC SÁNG TẠO: Cao"));
        assert_ne!(low_prompt, high_prompt);
    }

    #[test]
    fn creativity_bands_cover_the_whole_range() {
        let bands: Vec<&str> = [0, 20, 21, 45, 46, 70, 71, 100, 1000, -5]
            .iter()
            .map(|r| creativity_instruction(*r))
            .collect();
        assert!(bands.iter().all(|b| b.starts_with("MỨC SÁNG TẠO:")));
        assert_eq!(creativity_instruction(-5), creativity_instruction(0));
        assert_eq!(creativity_instruction(1000), creativity_instruction(100));
    }

    #[test]
    fn length_requirement_counts_characters_not_bytes() {
        // 20 Vietnamese chars, noticeably more bytes.
        let chunk = "đằng ẵ ộ ế ứ ườ ạ ẫ";
        let prompt = build_rewrite_prompt(chunk, &params());

        assert!(
            prompt.contains(&format!("{} ký tự", chunk.chars().count())),
            "prompt should state the character count: {prompt}"
        );
        assert!(
            !prompt.contains(&format!("{} ký tự", chunk.len())),
            "prompt must not leak the byte count"
        );
    }

    /// The length target must be an unambiguous number with explicit bounds, and
    /// must forbid summarising. Without this models condensed to ~18% of source.
    #[test]
    fn length_requirement_states_bounds_and_forbids_summarising() {
        let chunk = "x".repeat(1000);
        let mut p = params();
        p.target_length_variance = 10;
        let prompt = build_rewrite_prompt(&chunk, &p);

        assert!(
            prompt.contains("tối thiểu 900"),
            "missing lower bound: {prompt}"
        );
        assert!(prompt.contains("tối đa 1100"), "missing upper bound");
        assert!(prompt.contains("KHÔNG tóm tắt"));
        // Restated at the end, where models weight instructions most heavily.
        assert!(prompt.trim_end().ends_with("không tóm tắt."));
    }

    #[test]
    fn continuity_section_appears_only_when_there_is_a_previous_chunk() {
        let without = build_rewrite_prompt("x", &params());
        assert!(!without.contains("THÔNG TIN LIÊN TỤC NGỮ CẢNH"));

        let mut p = params();
        p.previous_chunk_paragraph = "Đoạn trước.";
        let with = build_rewrite_prompt("x", &p);
        assert!(with.contains("THÔNG TIN LIÊN TỤC NGỮ CẢNH"));
        assert!(with.contains("Đoạn trước."));
    }

    #[test]
    fn prompt_carries_style_extra_and_source() {
        let prompt = build_rewrite_prompt("NỘI DUNG GỐC", &params());
        assert!(prompt.contains("PHONG CÁCH: Cổ trang"));
        assert!(prompt.contains("YÊU CẦU THÊM: Giữ tên nhân vật"));
        assert!(prompt.contains("NỘI DUNG GỐC"));
        assert!(prompt.contains("Ngôn ngữ: Vietnamese"));
    }
}
