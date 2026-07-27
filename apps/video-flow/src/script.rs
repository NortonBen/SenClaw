//! Screenplay parsing — port of `internal/script` (parser.go, types.go,
//! timing.go). The LLM turns screenplay markdown into structured scenes +
//! entities. `parse` handles a full screenplay (capped), `parse_blocks` parses
//! per-scene blocks and merges (avoids output truncation).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// ---- types (port of types.go) ----

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ParsedScript {
    #[serde(default)]
    pub scenes: Vec<ParsedScene>,
    #[serde(default)]
    pub characters: Vec<ParsedCharacter>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ParsedScene {
    #[serde(default)]
    pub display_order: i64,
    /// image generation prompt (static opening frame)
    #[serde(default)]
    pub prompt: String,
    /// camera + motion for Veo3
    #[serde(default)]
    pub video_prompt: String,
    /// ordered character actions in English for Veo3
    #[serde(default)]
    pub action_sequence: String,
    /// reference-entity names appearing in this scene
    #[serde(default)]
    pub character_names: Vec<String>,
    /// estimated seconds (1/8 page ≈ 8s)
    #[serde(default)]
    pub duration: f64,
    /// WIDE|MEDIUM|CLOSE_UP|EXTREME_CLOSE_UP
    #[serde(default)]
    pub shot_type: String,
    /// STATIC|PAN|TILT|DOLLY|ZOOM|HANDHELD
    #[serde(default)]
    pub camera_movement: String,
    /// voiceover / dialogue (original language)
    #[serde(default)]
    pub narrator_text: String,
    #[serde(default)]
    pub transition_note: String,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ParsedCharacter {
    #[serde(default)]
    pub name: String,
    /// character|location|creature|visual_asset|generic_troop|faction
    #[serde(default)]
    pub entity_type: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub image_prompt: String,
}

// ---- system prompts (ported verbatim from parser.go) ----

pub const PARSER_SYSTEM_PROMPT: &str = r#"Bạn là chuyên gia phân tích kịch bản phim chuyên nghiệp.
Nhiệm vụ: Phân tích kịch bản và trả về JSON CHÍNH XÁC (không có text thừa, không có markdown fence).

QUAN TRỌNG VỀ ĐỘ DÀI OUTPUT:
- prompt: BẰNG TIẾNG ANH — mô tả TRẠNG THÁI ĐẦU TIÊN của cảnh: nhân vật đứng/ngồi ở đâu, biểu cảm ban đầu, bối cảnh xung quanh. KHÔNG mô tả hành động, KHÔNG reference cảnh trước.
- video_prompt: chuyển động camera và loại shot. PHẢI bằng tiếng Anh.
- action_sequence: chuỗi hành động nhân vật theo thứ tự. PHẢI bằng tiếng Anh.
- narrator_text: giữ nguyên ngôn ngữ gốc — toàn bộ lời thoại và narration.
  QUAN TRỌNG: trong narrator_text, nếu cần trích dẫn thì dùng nháy đơn '...' thay vì nháy kép "...".
  Tuyệt đối không chèn dấu nháy kép " vào nội dung narrator_text để tránh làm hỏng JSON.
- image_prompt (characters): mô tả đầy đủ nhân vật/entity.
Giữ JSON nhỏ gọn để tránh bị cắt giữa chừng. TẤT CẢ string value phải trên MỘT DÒNG DUY NHẤT — không được xuống dòng bên trong chuỗi JSON.

QUY TẮC SCENES:
- prompt: PHẢI BẰNG TIẾNG ANH. Mô tả FRAME ĐẦU TIÊN của cảnh: nhân vật ở đâu, đang làm gì ngay lúc cảnh bắt đầu (chưa có hành động), bối cảnh/địa điểm cụ thể. KHÔNG kế thừa trạng thái từ cảnh trước. Ví dụ đúng: "NAM sits alone at corner table, untouched rice dish in front of him, busy restaurant background". Ví dụ SAI: "NAM continues talking on phone" (đây là hành động, không phải trạng thái đầu).
- video_prompt: chỉ mô tả loại shot và chuyển động camera (PHẢI bằng tiếng Anh cho Veo3).
- action_sequence: MÔ TẢ ĐẦY ĐỦ chuỗi hành động vật lý của nhân vật trong cảnh, theo thứ tự thời gian.
  PHẢI bằng tiếng Anh. Ví dụ: "NAM sits alone at corner table with untouched food. Phone rings — he answers, tense expression. He hangs up, sighs. Picks up spoon, glances at food, checks wallet — only small bills. Stands, calls waiter."
  Bao gồm: ai làm gì, khi nào, biểu cảm/trạng thái ra sao. Khi nhân vật nói, ghi rõ: tên speaks into phone / turns to face / whispers, v.v.
- character_names: danh sách entity xuất hiện trong cảnh (bao gồm character, visual_asset, creature, faction, generic_troop, location nếu cần reference).
- duration: 1/8 trang ≈ 8 giây. Cảnh ngắn = 4-6s, cảnh dài = 10-16s.
- shot_type: WIDE|MEDIUM|CLOSE_UP|EXTREME_CLOSE_UP
- camera_movement: STATIC|PAN|TILT|DOLLY|ZOOM|HANDHELD|CRANE
- narrator_text: toàn bộ lời thoại và narration, giữ nguyên ngôn ngữ gốc. Format: "TÊN: lời thoại".
  Nếu có từ/cụm cần quote (ví dụ Treo), phải ghi dạng 'Treo', không dùng "Treo" bên trong narrator_text.

QUY TẮC ENTITIES (mảng "characters"):
Extract TẤT CẢ entities quan trọng với đúng entity_type:
- "character"     : nhân vật người/humanoid có vai trò trong câu chuyện
- "location"      : địa điểm, môi trường, bối cảnh cảnh quay
- "creature"      : sinh vật, quái vật, thú, không phải người
- "visual_asset"  : đạo cụ, vật thể đặc trưng, trang phục, biểu tượng quan trọng
- "generic_troop" : nhóm quân/đám đông đồng nhất (không có tên riêng)
- "faction"       : phe phái, tổ chức, băng nhóm (cần logo/emblem/uniform)

image_prompt theo entity_type:
- character/creature : "Character reference portrait: <full physical description>, neutral grey background, studio lighting, reference sheet style"
- location           : "Location establishing shot: <architecture, lighting, atmosphere, spatial layout>, photorealistic"
- visual_asset       : "Product/prop reference: <detailed object description>, neutral background, studio lighting"
- generic_troop      : "Troop reference group shot: <uniform, armor, weapons, formation>, neutral background"
- faction            : "Faction emblem/insignia: <symbol, colors, style>, clean background, graphic design style"

OUTPUT FORMAT (JSON):
{
  "scenes": [
    {
      "display_order": 1,
      "prompt": "...",
      "video_prompt": "...",
      "action_sequence": "...",
      "character_names": ["Name1"],
      "duration": 8.0,
      "shot_type": "WIDE",
      "camera_movement": "STATIC",
      "narrator_text": "...",
      "transition_note": "..."
    }
  ],
  "characters": [
    {
      "name": "...",
      "entity_type": "character",
      "description": "...",
      "image_prompt": "Character reference portrait: ..."
    }
  ]
}"#;

pub const PARSER_JSON_REPAIR_PROMPT: &str = r#"Bạn là bộ chuẩn hóa JSON nghiêm ngặt.
Nhiệm vụ: nhận input có thể là JSON lỗi và trả về DUY NHẤT một JSON object hợp lệ.

Yêu cầu bắt buộc:
- Không markdown fence.
- Không text giải thích.
- Chỉ JSON.
- Đảm bảo key top-level gồm:
  {
    "scenes": [...],
    "characters": [...]
  }
- Nếu thiếu trường thì điền giá trị mặc định hợp lý thay vì để JSON hỏng.
- Escape đúng mọi ký tự trong string (đặc biệt dấu ngoặc kép)."#;

/// Character cap for the screenplay sent to the parser LLM (maxScreenplayRunes).
const MAX_SCREENPLAY_CHARS: usize = 12000;

fn sys_prompt(system_override: &str) -> String {
    // Go's Parser.SetSystemPrompt equivalent: non-empty override (the soul)
    // replaces the built-in Vietnamese prompt.
    let s = system_override.trim();
    if s.is_empty() {
        PARSER_SYSTEM_PROMPT.to_string()
    } else {
        s.to_string()
    }
}

fn user_prompt(screenplay: &str) -> String {
    format!("Phân tích kịch bản sau:\n\n---\n{screenplay}\n---\n\nTrả về JSON.")
}

/// Parse a full screenplay markdown string (capped at 12000 chars). Retries once
/// with half the input when the JSON came back truncated/broken.
pub async fn parse(system_override: &str, screenplay: &str) -> Result<ParsedScript, String> {
    if screenplay.trim().is_empty() {
        return Err("screenplay is empty".to_string());
    }
    let sys = sys_prompt(system_override);

    let chars: Vec<char> = screenplay.chars().collect();
    let screenplay = if chars.len() > MAX_SCREENPLAY_CHARS {
        chars[..MAX_SCREENPLAY_CHARS].iter().collect::<String>()
            + "\n\n[... screenplay truncated for parsing ...]"
    } else {
        screenplay.to_string()
    };

    let (raw, _) = crate::llm::complete(&sys, &user_prompt(&screenplay), 8000)
        .await
        .map_err(|e| format!("llm parse: {e}"))?;

    let mut result = match extract_parsed(&raw) {
        Ok(r) => r,
        Err(err) => {
            // Retry once with a shorter screenplay (half) to avoid token overflow.
            let rchars: Vec<char> = screenplay.chars().collect();
            let half = rchars.len() / 2;
            if half < 500 {
                return Err(format!(
                    "parse json: {err}\nraw response: {}",
                    crate::llm::truncate(&raw, 500)
                ));
            }
            let shortened =
                rchars[..half].iter().collect::<String>() + "\n\n[... screenplay truncated ...]";
            let (raw2, _) = crate::llm::complete(&sys, &user_prompt(&shortened), 8000)
                .await
                .map_err(|e| format!("llm parse retry: {e}"))?;
            extract_parsed(&raw2).map_err(|e| {
                format!(
                    "parse json (after retry): {e}\nraw response: {}",
                    crate::llm::truncate(&raw2, 500)
                )
            })?
        }
    };

    normalize_result(&mut result);
    Ok(result)
}

/// Parse per-scene screenwriter blocks (`{scene_id, heading, content}`) one at a
/// time and merge. Blocks are not truncated — each is a single scene.
pub async fn parse_blocks(
    system_override: &str,
    blocks: &[Map<String, Value>],
) -> Result<ParsedScript, String> {
    if blocks.is_empty() {
        return Err("no scene blocks provided".to_string());
    }

    // Blocks are independent — each call sees only its own scene — so they are
    // parsed concurrently instead of one 9-call chain. Results are re-ordered
    // by block index afterwards, because `display_order` must follow the
    // screenplay, not completion time.
    let jobs: Vec<(usize, String)> = blocks
        .iter()
        .enumerate()
        .filter_map(|(i, block)| {
            let mut content =
                block.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if content.is_empty() {
                content = block.get("heading").and_then(|v| v.as_str()).unwrap_or("").to_string();
            }
            if content.trim().is_empty() {
                None
            } else {
                Some((i, content))
            }
        })
        .collect();

    let mut results: Vec<(usize, ParsedScript)> = {
        use futures_util::stream::{self, StreamExt};
        stream::iter(jobs.into_iter().map(|(i, content)| async move {
            parse_block(system_override, &content)
                .await
                .map(|r| (i, r))
                .map_err(|e| format!("block {}: {e}", i + 1))
        }))
        .buffer_unordered(crate::config::llm_concurrency())
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, String>>()?
    };
    results.sort_by_key(|(i, _)| *i);

    let mut merged = ParsedScript::default();
    let mut char_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, result) in results {
        for mut sc in result.scenes {
            sc.display_order = merged.scenes.len() as i64 + 1;
            merged.scenes.push(sc);
        }
        for ch in result.characters {
            let key = ch.name.trim().to_lowercase();
            if key.is_empty() || char_seen.contains(&key) {
                continue;
            }
            char_seen.insert(key);
            merged.characters.push(ch);
        }
    }

    if merged.scenes.is_empty() {
        return Err("no scenes parsed from blocks".to_string());
    }
    Ok(merged)
}

/// Parse a single scene block (no length cap). On malformed JSON, asks the LLM
/// to repair its own output into strict JSON once before failing.
async fn parse_block(system_override: &str, content: &str) -> Result<ParsedScript, String> {
    let sys = sys_prompt(system_override);
    let (raw, _) = crate::llm::complete(&sys, &user_prompt(content), 8000)
        .await
        .map_err(|e| format!("llm parse: {e}"))?;

    let mut result = match extract_parsed(&raw) {
        Ok(r) => r,
        Err(err) => {
            let repair_user = format!(
                "Sửa JSON sau thành JSON hợp lệ theo schema ParsedScript (scenes[], characters[]).\n\
                 Giữ nguyên nội dung nghĩa gốc tối đa, không thêm text ngoài JSON.\n\n---\n{raw}\n---"
            );
            let (repair_raw, _) = crate::llm::complete(PARSER_JSON_REPAIR_PROMPT, &repair_user, 8000)
                .await
                .map_err(|_| format!("parse json: {err}\nraw: {}", crate::llm::truncate(&raw, 500)))?;
            extract_parsed(&repair_raw).map_err(|e| {
                format!(
                    "parse json (after repair): {e}\nraw: {}",
                    crate::llm::truncate(&repair_raw, 500)
                )
            })?
        }
    };

    normalize_result(&mut result);
    Ok(result)
}

fn normalize_result(result: &mut ParsedScript) {
    for (i, sc) in result.scenes.iter_mut().enumerate() {
        if sc.display_order == 0 {
            sc.display_order = i as i64 + 1;
        }
        normalize_scene(sc);
    }
    for ch in result.characters.iter_mut() {
        if ch.entity_type.is_empty() {
            ch.entity_type = infer_entity_type(&ch.name, &ch.description);
        }
    }
}

/// Extract the JSON payload from an LLM reply: strip fences, take the outermost
/// object, escape bare control chars inside strings, then parse. Falls back to
/// the shared truncation-repairing parser.
fn extract_parsed(raw: &str) -> Result<ParsedScript, String> {
    let mut s = raw.trim().to_string();
    if let Some(idx) = s.find("```json") {
        s = s[idx + 7..].to_string();
    } else if let Some(idx) = s.find("```") {
        s = s[idx + 3..].to_string();
    }
    if let Some(idx) = s.rfind("```") {
        s = s[..idx].to_string();
    }
    let s = s.trim();

    let start = s.find('{');
    let end = s.rfind('}');
    if let (Some(start), Some(end)) = (start, end) {
        if end > start {
            let cand = sanitize_json_strings(&s[start..=end]);
            if let Ok(r) = serde_json::from_str::<ParsedScript>(&cand) {
                return Ok(r);
            }
        }
    }
    // Last resort: fence-stripping + truncation repair from the llm module.
    crate::llm::parse_json::<ParsedScript>(&sanitize_json_strings(raw))
}

/// Escape bare newlines / carriage returns / tabs inside JSON string literals
/// (LLMs frequently emit these in multi-sentence fields).
pub fn sanitize_json_strings(s: &str) -> String {
    let mut b = String::with_capacity(s.len());
    let mut in_string = false;
    let mut escaped = false;
    for r in s.chars() {
        if escaped {
            b.push(r);
            escaped = false;
            continue;
        }
        if r == '\\' && in_string {
            b.push(r);
            escaped = true;
            continue;
        }
        if r == '"' {
            in_string = !in_string;
            b.push(r);
            continue;
        }
        if in_string {
            match r {
                '\n' => b.push_str("\\n"),
                '\r' => b.push_str("\\r"),
                '\t' => b.push_str("\\t"),
                _ => b.push(r),
            }
            continue;
        }
        b.push(r);
    }
    b
}

fn normalize_scene(s: &mut ParsedScene) {
    if s.shot_type.is_empty() {
        s.shot_type = "MEDIUM".to_string();
    }
    if s.camera_movement.is_empty() {
        s.camera_movement = "STATIC".to_string();
    }
    if s.duration <= 0.0 {
        s.duration = 8.0;
    }
}

pub fn infer_entity_type(name: &str, desc: &str) -> String {
    let lower = format!("{name} {desc}").to_lowercase();
    let has = |subs: &[&str]| subs.iter().any(|s| lower.contains(s));
    if has(&[
        "địa điểm", "location", "căn phòng", "room", "rừng", "forest", "biển", "sea",
        "thành phố", "city", "palace", "cung điện", "dungeon", "hầm ngục",
    ]) {
        "location".to_string()
    } else if has(&["quái vật", "monster", "creature", "sinh vật", "dragon", "rồng", "beast", "dã thú"]) {
        "creature".to_string()
    } else if has(&[
        "đạo cụ", "prop", "artifact", "bảo vật", "sword", "kiếm", "armor", "giáp", "relic",
        "trang phục", "costume",
    ]) {
        "visual_asset".to_string()
    } else if has(&[
        "quân đội", "troop", "lính", "soldier", "army", "đội quân", "legion", "battalion",
        "đám đông", "crowd",
    ]) {
        "generic_troop".to_string()
    } else if has(&[
        "phe", "faction", "guild", "hội", "clan", "tổ chức", "organization", "order", "empire",
        "đế chế", "kingdom", "vương quốc",
    ]) {
        "faction".to_string()
    } else {
        "character".to_string()
    }
}

// ---- timing (port of timing.go) ----

/// Estimate scene duration from screenplay text: 1 page ≈ 55 lines ≈ 60s,
/// clamped to [4, 16] seconds.
pub fn estimate_duration(text: &str) -> f64 {
    if text.trim().is_empty() {
        return 8.0;
    }
    let non_empty = text
        .trim()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();
    let seconds = (non_empty as f64 / 55.0) * 60.0;
    seconds.clamp(4.0, 16.0)
}

/// Recommended Veo3 timing hint for a given duration.
pub fn shot_duration_hint(dur: f64) -> &'static str {
    if dur <= 5.0 {
        "0-3s: establish, 3-5s: action"
    } else if dur <= 8.0 {
        "0-3s: establish, 3-6s: action, 6-8s: reaction"
    } else {
        "0-3s: wide establish, 3-6s: medium action, 6-8s: close detail, 8s+: hold/resolution"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The block loop must preserve screenplay order even though the calls now
    /// complete out of order — `display_order` follows the script, not latency.
    #[test]
    fn merge_preserves_block_order() {
        // Simulate what the concurrent stage produces: results arriving shuffled.
        let mut results: Vec<(usize, ParsedScript)> = vec![
            (2, ParsedScript { scenes: vec![ParsedScene { prompt: "C".into(), ..Default::default() }], characters: vec![] }),
            (0, ParsedScript { scenes: vec![ParsedScene { prompt: "A".into(), ..Default::default() }], characters: vec![] }),
            (1, ParsedScript { scenes: vec![ParsedScene { prompt: "B".into(), ..Default::default() }], characters: vec![] }),
        ];
        results.sort_by_key(|(i, _)| *i);
        let mut merged = ParsedScript::default();
        for (_, r) in results {
            for mut sc in r.scenes {
                sc.display_order = merged.scenes.len() as i64 + 1;
                merged.scenes.push(sc);
            }
        }
        let order: Vec<String> = merged.scenes.iter().map(|s| s.prompt.clone()).collect();
        assert_eq!(order, vec!["A", "B", "C"]);
        assert_eq!(merged.scenes[2].display_order, 3);
    }

    #[test]
    fn estimate_duration_rules() {
        assert_eq!(estimate_duration(""), 8.0);
        assert_eq!(estimate_duration("   \n  \n"), 8.0);
        // 1 line → 60/55 ≈ 1.09 → clamped to min 4.0
        assert_eq!(estimate_duration("one line"), 4.0);
        // 7 lines ≈ 1/8 page → ~7.64s (inside [4, 16])
        let seven = (0..7).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let d = estimate_duration(&seven);
        assert!((d - 7.636).abs() < 0.01, "got {d}");
        // 55+ lines → clamped to max 16.0
        let many = (0..80).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        assert_eq!(estimate_duration(&many), 16.0);
    }

    #[test]
    fn shot_hints() {
        assert!(shot_duration_hint(4.0).starts_with("0-3s: establish, 3-5s"));
        assert!(shot_duration_hint(8.0).contains("6-8s: reaction"));
        assert!(shot_duration_hint(12.0).contains("hold/resolution"));
    }

    #[test]
    fn sanitize_escapes_bare_newlines_in_strings() {
        let bad = "{\"a\": \"line1\nline2\"}";
        let fixed = sanitize_json_strings(bad);
        let v: Value = serde_json::from_str(&fixed).unwrap();
        assert_eq!(v["a"], "line1\nline2");
    }

    #[test]
    fn extract_parsed_handles_fences_and_defaults() {
        let raw = "```json\n{\"scenes\":[{\"prompt\":\"NAM stands\"}],\"characters\":[{\"name\":\"NAM\"}]}\n```";
        let mut r = extract_parsed(raw).unwrap();
        normalize_result(&mut r);
        assert_eq!(r.scenes.len(), 1);
        assert_eq!(r.scenes[0].display_order, 1);
        assert_eq!(r.scenes[0].shot_type, "MEDIUM");
        assert_eq!(r.scenes[0].camera_movement, "STATIC");
        assert_eq!(r.scenes[0].duration, 8.0);
        assert_eq!(r.characters[0].entity_type, "character");
    }

    #[test]
    fn infer_entity_types() {
        assert_eq!(infer_entity_type("Khu rừng", "rừng già âm u"), "location");
        assert_eq!(infer_entity_type("Rồng lửa", "quái vật khổng lồ"), "creature");
        assert_eq!(infer_entity_type("Thanh kiếm", "bảo vật gia truyền"), "visual_asset");
        assert_eq!(infer_entity_type("NAM", "chàng trai 25 tuổi"), "character");
    }
}
