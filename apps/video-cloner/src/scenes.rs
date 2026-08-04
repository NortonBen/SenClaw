//! Scene parsing and bulk editing.
//!
//! The model is asked to emit one JSON object per line. That format is what
//! makes partial output usable: if it stops halfway through a long run, every
//! complete line before the cut is still a valid scene.

use serde_json::{Map, Value};

/// Truncate on a character boundary.
///
/// `&s[..n]` panics on multi-byte text, and everything here is Vietnamese.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

/// Pull every scene object out of a raw model response.
///
/// Tolerates the model wrapping output in a ```json fence and adding prose
/// around the JSON, which it does often enough to matter.
pub fn parse_scenes(raw: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let trimmed = line
            .trim()
            .trim_start_matches("```json")
            .trim_matches('`')
            .trim();
        if trimmed.len() < 2 {
            continue;
        }
        if let Some(v) = parse_line(trimmed) {
            out.push(v);
        }
    }
    out
}

fn parse_line(line: &str) -> Option<Value> {
    let candidate = if line.starts_with('{') && line.ends_with('}') {
        line.to_string()
    } else {
        // Prose around the object: take the widest {...} span on the line.
        let start = line.find('{')?;
        let end = line.rfind('}')?;
        if end <= start {
            return None;
        }
        line[start..=end].to_string()
    };

    let v: Value = serde_json::from_str(&candidate).ok()?;
    // `scene_id` is the marker that this really is a scene and not some other
    // JSON the model decided to narrate with.
    if v.get("scene_id").is_some() {
        Some(v)
    } else {
        None
    }
}

/// The numeric part of a scene's `scene_id`.
///
/// The model is told to emit `"scene_id":"3"`, but it sometimes emits a bare
/// number instead, and both have to resolve to the same resume point.
pub fn scene_number(scene: &Value) -> Option<i64> {
    match scene.get("scene_id")? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Male,
    Female,
}

impl Voice {
    pub fn parse(s: &str) -> Option<Voice> {
        match s.trim().to_lowercase().as_str() {
            "male" | "nam" => Some(Voice::Male),
            "female" | "nữ" | "nu" => Some(Voice::Female),
            _ => None,
        }
    }

    fn gender(&self) -> &'static str {
        match self {
            Voice::Male => "Male",
            Voice::Female => "Female",
        }
    }

    fn voice_id(&self, char_id: &str) -> String {
        match self {
            Voice::Male => format!("MALE_{char_id}_VOICE"),
            Voice::Female => format!("FEMALE_{char_id}_VOICE"),
        }
    }

    fn personality(&self, style: &str) -> String {
        match self {
            Voice::Male => format!("Giọng Nam trầm ấm, phù hợp phong cách {style}"),
            Voice::Female => format!("Giọng Nữ truyền cảm, phù hợp phong cách {style}"),
        }
    }
}

/// A character discovered across the scene list.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DetectedCharacter {
    pub id: String,
    pub name: String,
    /// Whether this character actually speaks anywhere.
    pub has_dialogue: bool,
}

pub fn detect_characters(scenes: &[Value]) -> Vec<DetectedCharacter> {
    let mut order: Vec<String> = Vec::new();
    let mut names: std::collections::HashMap<String, String> = Default::default();
    let mut speakers: std::collections::HashSet<String> = Default::default();

    for scene in scenes {
        if let Some(lock) = scene.get("character_lock").and_then(|v| v.as_object()) {
            for ch in lock.values() {
                let (Some(id), Some(name)) = (
                    ch.get("id").and_then(|v| v.as_str()),
                    ch.get("name").and_then(|v| v.as_str()),
                ) else {
                    continue;
                };
                if !names.contains_key(id) {
                    order.push(id.to_string());
                }
                names.insert(id.to_string(), name.to_string());
            }
        }
        if let Some(lines) = scene.get("dialogue").and_then(|v| v.as_array()) {
            for line in lines {
                if let Some(sp) = line.get("speaker").and_then(|v| v.as_str()) {
                    speakers.insert(sp.to_string());
                }
            }
        }
    }

    order
        .into_iter()
        .map(|id| {
            let name = names.get(&id).cloned().unwrap_or_default();
            let has_dialogue = speakers.contains(&id) || speakers.contains(&name);
            DetectedCharacter {
                id,
                name,
                has_dialogue,
            }
        })
        .collect()
}

pub struct ReplaceRequest {
    pub find: String,
    pub replace: String,
    /// Only act if the target character actually has spoken lines.
    pub only_with_dialogue: bool,
    /// char_id → voice to force.
    pub voice_overrides: std::collections::HashMap<String, Voice>,
    /// Used to phrase the generated `voice_personality` text.
    pub style: String,
}

pub struct ReplaceOutcome {
    pub scenes: Vec<Value>,
    pub replaced_text: bool,
    pub voices_applied: usize,
}

/// Apply a bulk rename and/or voice change across every scene.
///
/// Two passes, in this order:
///   1. plain text substitution over the serialized scene, so a renamed
///      character is also renamed inside prose fields that mention it;
///   2. structural repair, which re-derives `voice_id`, `gender` and every
///      `audio_markers` / `dialogue` reference from the character lock.
///
/// The second pass is what keeps the output usable: Veo 3 treats a voice id
/// that drifts between segments as a different speaker, so a rename that only
/// touched some references would silently split one character into two.
pub fn apply_replace(scenes: &[Value], req: &ReplaceRequest) -> Result<ReplaceOutcome, String> {
    let characters = detect_characters(scenes);
    let find = req.find.trim();

    let target = characters
        .iter()
        .find(|c| c.name == find || c.id == find)
        .cloned();

    if !find.is_empty() && req.only_with_dialogue {
        match &target {
            Some(c) if !c.has_dialogue => {
                return Err(format!("Nhân vật \"{find}\" không có lời thoại nào."));
            }
            None => {
                return Err(format!(
                    "Không tìm thấy nhân vật \"{find}\" để kiểm tra lời thoại."
                ));
            }
            _ => {}
        }
    }

    let mut out: Vec<Value> = Vec::with_capacity(scenes.len());
    let mut replaced_text = false;

    for scene in scenes {
        let mut scene = scene.clone();

        if !find.is_empty() {
            let serialized = scene.to_string();
            if serialized.contains(find) {
                // Escape the replacement for the JSON string context it lands in.
                let escaped = json_escape_fragment(&req.replace);
                let patched = serialized.replace(find, &escaped);
                match serde_json::from_str::<Value>(&patched) {
                    Ok(v) => {
                        scene = v;
                        replaced_text = true;
                    }
                    // A replacement that breaks the JSON leaves the scene alone
                    // rather than corrupting it.
                    Err(_) => {}
                }
            }
        }

        out.push(scene);
    }

    let mut voices_applied = 0usize;
    let target_id = target.as_ref().map(|c| c.id.clone());

    for scene in out.iter_mut() {
        let Some(lock) = scene
            .get("character_lock")
            .and_then(|v| v.as_object())
            .cloned()
        else {
            continue;
        };

        let mut new_lock = Map::new();
        // char_id → (name, voice_id) for the reference-sync pass below.
        let mut resolved: Vec<(String, String, String)> = Vec::new();

        for (key, ch) in lock {
            let mut ch = ch;
            let char_id = ch
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or(key.as_str())
                .to_string();

            if let Some(voice) = req.voice_overrides.get(&char_id) {
                if let Some(obj) = ch.as_object_mut() {
                    obj.insert("gender".into(), Value::String(voice.gender().into()));
                    obj.insert("voice_id".into(), Value::String(voice.voice_id(&char_id)));
                    obj.insert(
                        "voice_personality".into(),
                        Value::String(voice.personality(&req.style)),
                    );
                }
                voices_applied += 1;
            }

            if Some(&char_id) == target_id.as_ref() && !req.replace.trim().is_empty() {
                if let Some(obj) = ch.as_object_mut() {
                    obj.insert("name".into(), Value::String(req.replace.clone()));
                }
            }

            let name = ch
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let voice_id = ch
                .get("voice_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            resolved.push((char_id, name, voice_id));
            new_lock.insert(key, ch);
        }

        if let Some(obj) = scene.as_object_mut() {
            obj.insert("character_lock".into(), Value::Object(new_lock));
        }

        for (char_id, name, voice_id) in &resolved {
            if voice_id.is_empty() {
                continue;
            }

            if let Some(samples) = scene
                .pointer_mut("/audio_markers/voice_samples")
                .and_then(|v| v.as_object_mut())
            {
                if samples.contains_key(char_id) {
                    samples.insert(char_id.clone(), Value::String(voice_id.clone()));
                }
            }

            if let Some(lines) = scene.get_mut("dialogue").and_then(|v| v.as_array_mut()) {
                for line in lines.iter_mut() {
                    let speaker = line
                        .get("speaker")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let matches = speaker == *char_id
                        || speaker == *name
                        || (!find.is_empty() && speaker == find);
                    if !matches {
                        continue;
                    }
                    if let Some(obj) = line.as_object_mut() {
                        obj.insert("voice_marker".into(), Value::String(voice_id.clone()));
                        if Some(char_id) == target_id.as_ref() && !req.replace.trim().is_empty() {
                            obj.insert("speaker".into(), Value::String(req.replace.clone()));
                        }
                    }
                }
            }
        }
    }

    Ok(ReplaceOutcome {
        scenes: out,
        replaced_text,
        voices_applied,
    })
}

/// Escape a user string so it survives being spliced into serialized JSON.
fn json_escape_fragment(s: &str) -> String {
    let quoted = Value::String(s.to_string()).to_string();
    quoted[1..quoted.len() - 1].to_string()
}

/// The export format: one compact JSON per line, blank line between scenes —
/// the shape the Veo 3 workflow expects to paste from.
pub fn export_text(scenes: &[Value]) -> String {
    scenes
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_one_object_per_line_ignoring_blanks() {
        let raw = "{\"scene_id\":\"1\"}\n\n{\"scene_id\":\"2\"}\n";
        let scenes = parse_scenes(raw);
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[1]["scene_id"], "2");
    }

    #[test]
    fn ignores_json_without_a_scene_id() {
        let raw = "{\"hello\":\"world\"}\n{\"scene_id\":\"1\"}";
        assert_eq!(parse_scenes(raw).len(), 1);
    }

    #[test]
    fn survives_a_code_fence_and_surrounding_prose() {
        let raw = "```json\nĐây là scene: {\"scene_id\":\"3\",\"duration_sec\":\"8\"}\n```";
        let scenes = parse_scenes(raw);
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0]["scene_id"], "3");
    }

    #[test]
    fn a_truncated_final_line_does_not_lose_the_earlier_scenes() {
        let raw = "{\"scene_id\":\"1\"}\n{\"scene_id\":\"2\",\"visual_sty";
        assert_eq!(parse_scenes(raw).len(), 1);
    }

    #[test]
    fn scene_number_reads_both_string_and_numeric_ids() {
        assert_eq!(scene_number(&json!({"scene_id":"1"})), Some(1));
        assert_eq!(scene_number(&json!({"scene_id": 7})), Some(7));
        assert_eq!(scene_number(&json!({"scene_id":"x"})), None);
        assert_eq!(scene_number(&json!({})), None);
    }

    fn sample() -> Vec<Value> {
        vec![json!({
            "scene_id": "1",
            "character_lock": {
                "CHAR_1": { "id": "CHAR_1", "name": "Lan", "voice_id": "VOICE_CHAR_1", "gender": "Female" }
            },
            "audio_markers": { "voice_samples": { "CHAR_1": "VOICE_CHAR_1" } },
            "dialogue": [ { "speaker": "Lan", "voice_marker": "VOICE_CHAR_1", "line": "Lan chào bạn" } ]
        })]
    }

    fn req(find: &str, replace: &str) -> ReplaceRequest {
        ReplaceRequest {
            find: find.into(),
            replace: replace.into(),
            only_with_dialogue: false,
            voice_overrides: Default::default(),
            style: "Synthwave".into(),
        }
    }

    #[test]
    fn rename_updates_the_lock_the_speaker_and_the_prose() {
        let out = apply_replace(&sample(), &req("Lan", "Mai")).unwrap();
        let s = &out.scenes[0];
        assert_eq!(s["character_lock"]["CHAR_1"]["name"], "Mai");
        assert_eq!(s["dialogue"][0]["speaker"], "Mai");
        assert_eq!(s["dialogue"][0]["line"], "Mai chào bạn");
        assert!(out.replaced_text);
    }

    #[test]
    fn voice_override_rewrites_id_gender_and_every_reference() {
        let mut r = req("", "");
        r.voice_overrides.insert("CHAR_1".into(), Voice::Male);
        let out = apply_replace(&sample(), &r).unwrap();
        let s = &out.scenes[0];

        assert_eq!(
            s["character_lock"]["CHAR_1"]["voice_id"],
            "MALE_CHAR_1_VOICE"
        );
        assert_eq!(s["character_lock"]["CHAR_1"]["gender"], "Male");
        assert_eq!(
            s["audio_markers"]["voice_samples"]["CHAR_1"],
            "MALE_CHAR_1_VOICE"
        );
        assert_eq!(s["dialogue"][0]["voice_marker"], "MALE_CHAR_1_VOICE");
        assert_eq!(out.voices_applied, 1);
    }

    #[test]
    fn voice_stays_identical_across_every_scene() {
        let mut scenes = sample();
        let mut second = scenes[0].clone();
        second["scene_id"] = json!("2");
        scenes.push(second);

        let mut r = req("", "");
        r.voice_overrides.insert("CHAR_1".into(), Voice::Female);
        let out = apply_replace(&scenes, &r).unwrap();

        let a = &out.scenes[0]["character_lock"]["CHAR_1"]["voice_id"];
        let b = &out.scenes[1]["character_lock"]["CHAR_1"]["voice_id"];
        assert_eq!(a, b, "voice id must not drift between segments");
    }

    #[test]
    fn only_with_dialogue_rejects_a_silent_character() {
        let scenes = vec![json!({
            "scene_id": "1",
            "character_lock": { "CHAR_1": { "id": "CHAR_1", "name": "Lan", "voice_id": "V" } },
            "dialogue": []
        })];
        let mut r = req("Lan", "Mai");
        r.only_with_dialogue = true;
        assert!(apply_replace(&scenes, &r).is_err());
    }

    #[test]
    fn a_replacement_containing_quotes_does_not_corrupt_the_scene() {
        let out = apply_replace(&sample(), &req("Lan", "A \"B\" C")).unwrap();
        assert_eq!(
            out.scenes[0]["character_lock"]["CHAR_1"]["name"],
            "A \"B\" C"
        );
    }

    #[test]
    fn detect_characters_reports_who_actually_speaks() {
        let chars = detect_characters(&sample());
        assert_eq!(chars.len(), 1);
        assert_eq!(chars[0].id, "CHAR_1");
        assert!(chars[0].has_dialogue);
    }

    #[test]
    fn export_separates_scenes_with_a_blank_line() {
        let text = export_text(&[json!({"scene_id":"1"}), json!({"scene_id":"2"})]);
        assert_eq!(text, "{\"scene_id\":\"1\"}\n\n{\"scene_id\":\"2\"}");
    }

    #[test]
    fn truncate_never_splits_a_vietnamese_character() {
        let s = "Điện Biên Phủ trên không";
        assert_eq!(truncate_chars(s, 5), "Điện …");
        assert_eq!(truncate_chars(s, 100), s);
    }
}
