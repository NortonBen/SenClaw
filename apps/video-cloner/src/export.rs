//! Export and handoff.
//!
//! The scenes this app produces are Veo 3 prompts: deeply nested JSON tuned for
//! one specific generator. That shape is exactly wrong for handing work to
//! another app, which generally wants readable text prompts. So every export
//! carries three views of the same scenes:
//!
//!   * `veo` — the original one-JSON-per-line text, for pasting into Veo 3;
//!   * `image_prompt` / `video_prompt` — flattened prose per scene, for any
//!     generator that takes a text prompt;
//!   * a Markdown screenplay, for a human or an agent to read.
//!
//! Consumers should key off `format` + `version`, never off field order.

use crate::db::{Project, Scene};
use crate::scenes;
use serde_json::{json, Map, Value};

pub const BUNDLE_FORMAT: &str = "senclaw.video-cloner.bundle";
pub const BUNDLE_VERSION: u32 = 1;

/// Read a string field, tolerating the model's occasional non-string values.
fn s(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn nested(v: &Value, path: &[&str]) -> String {
    let mut cur = v;
    for (i, key) in path.iter().enumerate() {
        match cur.get(key) {
            Some(next) => {
                if i + 1 == path.len() {
                    return match next {
                        Value::String(s) => s.trim().to_string(),
                        Value::Null => String::new(),
                        other => other.to_string(),
                    };
                }
                cur = next;
            }
            None => return String::new(),
        }
    }
    String::new()
}

/// Join non-empty fragments with ", ".
fn join(parts: Vec<String>) -> String {
    parts
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn characters_of(scene: &Value) -> Vec<(String, Map<String, Value>)> {
    scene
        .get("character_lock")
        .and_then(|v| v.as_object())
        .map(|lock| {
            lock.iter()
                .filter_map(|(k, v)| v.as_object().map(|o| (k.clone(), o.clone())))
                .collect()
        })
        .unwrap_or_default()
}

fn backgrounds_of(scene: &Value) -> Vec<Map<String, Value>> {
    scene
        .get("background_lock")
        .and_then(|v| v.as_object())
        .map(|lock| {
            lock.values()
                .filter_map(|v| v.as_object().cloned())
                .collect()
        })
        .unwrap_or_default()
}

/// One character rendered as an appearance phrase.
fn describe_character(ch: &Map<String, Value>) -> String {
    let v = Value::Object(ch.clone());
    let name = s(&v, "name");
    let looks = join(vec![
        s(&v, "species"),
        s(&v, "gender"),
        s(&v, "age"),
        s(&v, "body_build"),
        s(&v, "face_shape"),
        s(&v, "hair"),
        s(&v, "skin_or_fur_color"),
        s(&v, "signature_feature"),
        s(&v, "outfit_top"),
        s(&v, "outfit_bottom"),
        s(&v, "helmet_or_hat"),
        s(&v, "shoes_or_footwear"),
        s(&v, "props"),
    ]);
    match (name.is_empty(), looks.is_empty()) {
        (true, true) => String::new(),
        (false, true) => name,
        (true, false) => looks,
        (false, false) => format!("{name} ({looks})"),
    }
}

/// The still-frame description: who is in shot, where, how it is lit and framed.
///
/// Deliberately excludes motion and sound — a first-frame image generator that
/// is told about movement tends to render motion blur.
pub fn image_prompt(scene: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();

    let style = s(scene, "visual_style");
    if !style.is_empty() {
        parts.push(style);
    }

    for (_, ch) in characters_of(scene) {
        let v = Value::Object(ch.clone());
        let who = describe_character(&ch);
        let pose = join(vec![
            s(&v, "position"),
            s(&v, "orientation"),
            s(&v, "pose"),
            s(&v, "expression"),
            s(&v, "hand_detail"),
            s(&v, "foot_placement"),
        ]);
        if !who.is_empty() {
            parts.push(if pose.is_empty() {
                who
            } else {
                format!("{who} — {pose}")
            });
        }
    }

    for bg in backgrounds_of(scene) {
        let v = Value::Object(bg);
        let place = join(vec![
            s(&v, "name"),
            s(&v, "setting"),
            s(&v, "scenery"),
            s(&v, "props"),
            s(&v, "lighting"),
        ]);
        if !place.is_empty() {
            parts.push(format!("Bối cảnh: {place}"));
        }
    }

    let camera = join(vec![
        nested(scene, &["camera", "framing"]),
        nested(scene, &["camera", "angle"]),
        nested(scene, &["camera", "focus"]),
    ]);
    if !camera.is_empty() {
        parts.push(format!("Máy quay: {camera}"));
    }

    parts.join(". ")
}

/// The motion description: what happens over the 8 seconds, plus sound.
pub fn video_prompt(scene: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();

    for (_, ch) in characters_of(scene) {
        let v = Value::Object(ch.clone());
        let name = s(&v, "name");
        let flow = join(vec![
            nested(&v, &["action_flow", "pre_action"]),
            nested(&v, &["action_flow", "main_action"]),
            nested(&v, &["action_flow", "post_action"]),
        ]);
        if !flow.is_empty() {
            parts.push(if name.is_empty() {
                flow
            } else {
                format!("{name}: {flow}")
            });
        }
    }

    let movement = nested(scene, &["camera", "movement"]);
    if !movement.is_empty() {
        parts.push(format!("Chuyển động máy quay: {movement}"));
    }

    let ambience = string_list(scene, &["foley_and_ambience", "ambience"]);
    let fx = string_list(scene, &["foley_and_ambience", "fx"]);
    let music = nested(scene, &["foley_and_ambience", "music"]);
    let audio = join(vec![ambience.join(", "), fx.join(", "), music]);
    if !audio.is_empty() {
        parts.push(format!("Âm thanh: {audio}"));
    }

    for line in dialogue_of(scene) {
        let speaker = s(&line, "speaker");
        let text = s(&line, "line");
        if !text.is_empty() {
            parts.push(if speaker.is_empty() {
                format!("Thoại: \"{text}\"")
            } else {
                format!("{speaker} nói: \"{text}\"")
            });
        }
    }

    parts.join(". ")
}

fn string_list(scene: &Value, path: &[&str]) -> Vec<String> {
    let mut cur = scene;
    for key in path {
        match cur.get(key) {
            Some(next) => cur = next,
            None => return Vec::new(),
        }
    }
    cur.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn dialogue_of(scene: &Value) -> Vec<Value> {
    scene
        .get("dialogue")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Seconds a scene runs for; the pipeline is built on 8-second segments but the
/// model occasionally writes something else, and a consumer needs the truth.
pub fn duration_of(scene: &Value) -> f64 {
    match scene.get("duration_sec") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(8.0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(8.0),
        _ => 8.0,
    }
}

/// Every distinct character across the whole project, with the locked voice id.
///
/// Downstream generators need this as a cast list: the same `voice_id` across
/// segments is what tells them it is one speaker, not several.
pub fn cast(scene_values: &[Value]) -> Vec<Value> {
    let mut order: Vec<String> = Vec::new();
    let mut seen: std::collections::HashMap<String, Value> = Default::default();

    for scene in scene_values {
        for (_, ch) in characters_of(scene) {
            let v = Value::Object(ch.clone());
            let id = s(&v, "id");
            if id.is_empty() {
                continue;
            }
            if !seen.contains_key(&id) {
                order.push(id.clone());
                seen.insert(
                    id.clone(),
                    json!({
                        "id": id,
                        "name": s(&v, "name"),
                        "voice_id": s(&v, "voice_id"),
                        "gender": s(&v, "gender"),
                        "voice_personality": s(&v, "voice_personality"),
                        "appearance": describe_character(&ch),
                    }),
                );
            }
        }
    }

    order.into_iter().filter_map(|id| seen.remove(&id)).collect()
}

/// The full export payload.
pub fn bundle(project: &Project, stored: &[Scene], exported_at: &str) -> Value {
    let scene_values: Vec<Value> = stored.iter().map(|s| s.json.clone()).collect();

    let scenes_out: Vec<Value> = stored
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let sc = &row.json;
            json!({
                "index": i,
                "scene_id": if row.scene_id.is_empty() { (i + 1).to_string() } else { row.scene_id.clone() },
                "duration_sec": duration_of(sc),
                "image_prompt": image_prompt(sc),
                "video_prompt": video_prompt(sc),
                "dialogue": dialogue_of(sc),
                "job_id": row.job_id,
                // The untouched Veo 3 object, so nothing is lost in translation.
                "veo": sc,
            })
        })
        .collect();

    let total: f64 = scene_values.iter().map(duration_of).sum();

    json!({
        "format": BUNDLE_FORMAT,
        "version": BUNDLE_VERSION,
        "exported_at": exported_at,
        "source": {
            "app": "video-cloner",
            "project_id": project.id,
            "project_name": project.name,
            "video_filename": project.video_filename,
        },
        "config": {
            "style": project.style,
            "model": project.model,
            "char_description": project.char_description,
            "custom_dialogue": project.custom_dialogue,
            "bg_description": project.bg_description,
            "auto_magic": project.auto_magic,
            "visual_similarity": project.visual_similarity,
        },
        "summary": {
            "scene_count": scenes_out.len(),
            "total_duration_sec": total,
        },
        "cast": cast(&scene_values),
        "scenes": scenes_out,
        // The paste-into-Veo-3 form, kept verbatim.
        "veo_jsonl": scenes::export_text(&scene_values),
    })
}

/// A readable screenplay. This is what goes to the wiki and what an agent reads
/// when it needs to understand the project rather than feed it to a generator.
pub fn markdown(project: &Project, stored: &[Scene], exported_at: &str) -> String {
    let scene_values: Vec<Value> = stored.iter().map(|s| s.json.clone()).collect();
    let total: f64 = scene_values.iter().map(duration_of).sum();
    let mut out = String::new();

    out.push_str(&format!("# {}\n\n", project.name));
    out.push_str(&format!(
        "> Kịch bản sinh video, xuất từ SenClaw Video Cloner lúc {exported_at}.\n\n"
    ));

    out.push_str("## Tổng quan\n\n");
    out.push_str(&format!("- **Video gốc**: {}\n", project.video_filename));
    out.push_str(&format!("- **Phong cách**: {}\n", project.style));
    out.push_str(&format!(
        "- **Số đoạn**: {} (~{:.0} giây)\n",
        stored.len(),
        total
    ));
    out.push_str(&format!(
        "- **Độ tương đồng hình ảnh**: {}%{}\n",
        project.visual_similarity,
        if project.auto_magic {
            " (chế độ AI tự do sáng tạo — bỏ qua mô tả nhân vật/bối cảnh thủ công)"
        } else {
            ""
        }
    ));
    if !project.bg_description.trim().is_empty() {
        out.push_str(&format!("- **Bối cảnh**: {}\n", project.bg_description));
    }
    out.push('\n');

    let cast_list = cast(&scene_values);
    if !cast_list.is_empty() {
        out.push_str("## Nhân vật\n\n");
        out.push_str("| ID | Tên | Giọng | Ngoại hình |\n|---|---|---|---|\n");
        for c in &cast_list {
            out.push_str(&format!(
                "| `{}` | {} | `{}` | {} |\n",
                s(c, "id"),
                s(c, "name"),
                s(c, "voice_id"),
                s(c, "appearance").replace('|', "\\|"),
            ));
        }
        out.push_str(
            "\n> `voice_id` phải giữ nguyên ở mọi đoạn. Đổi giữa chừng sẽ bị hiểu thành nhân vật khác.\n\n",
        );
    }

    out.push_str("## Các đoạn\n\n");
    for (i, row) in stored.iter().enumerate() {
        let sc = &row.json;
        let id = if row.scene_id.is_empty() {
            (i + 1).to_string()
        } else {
            row.scene_id.clone()
        };
        out.push_str(&format!(
            "### Đoạn {} ({:.0}s)\n\n",
            id,
            duration_of(sc)
        ));

        let img = image_prompt(sc);
        if !img.is_empty() {
            out.push_str(&format!("**Khung hình**: {img}\n\n"));
        }
        let vid = video_prompt(sc);
        if !vid.is_empty() {
            out.push_str(&format!("**Diễn biến**: {vid}\n\n"));
        }
        for line in dialogue_of(sc) {
            let speaker = s(&line, "speaker");
            let text = s(&line, "line");
            if !text.is_empty() {
                out.push_str(&format!("> **{speaker}**: {text}\n\n"));
            }
        }
    }

    out.push_str("## Prompt Veo 3 (mỗi đoạn một dòng JSON)\n\n```json\n");
    out.push_str(&scenes::export_text(&scene_values));
    out.push_str("\n```\n");
    out
}

/// Fold a Vietnamese letter to its ASCII base.
///
/// Without this, "Cô gái phố đêm" slugs to "c-g-i-ph-m" — every accented vowel
/// is dropped and the filename becomes unreadable.
fn fold_vietnamese(c: char) -> Option<char> {
    const TABLE: [(&str, char); 12] = [
        ("àáảãạăằắẳẵặâầấẩẫậ", 'a'),
        ("èéẻẽẹêềếểễệ", 'e'),
        ("ìíỉĩị", 'i'),
        ("òóỏõọôồốổỗộơờớởỡợ", 'o'),
        ("ùúủũụưừứửữự", 'u'),
        ("ỳýỷỹỵ", 'y'),
        ("đ", 'd'),
        ("ÀÁẢÃẠĂẰẮẲẴẶÂẦẤẨẪẬ", 'a'),
        ("ÈÉẺẼẸÊỀẾỂỄỆ", 'e'),
        ("ÌÍỈĨỊ", 'i'),
        ("ÒÓỎÕỌÔỒỐỔỖỘƠỜỚỞỠỢ", 'o'),
        ("ÙÚỦŨỤƯỪỨỬỮỰ", 'u'),
    ];
    for (set, base) in TABLE {
        if set.contains(c) {
            return Some(base);
        }
    }
    match c {
        'Ỳ' | 'Ý' | 'Ỷ' | 'Ỹ' | 'Ỵ' => Some('y'),
        'Đ' => Some('d'),
        _ => None,
    }
}

/// Filesystem-safe slug for a project name, used for file and wiki page names.
pub fn slug(name: &str, project_id: i64) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for raw in name.chars() {
        let c = fold_vietnamese(raw).unwrap_or(raw);
        for c in c.to_lowercase() {
            if c.is_ascii_alphanumeric() {
                out.push(c);
                last_dash = false;
            } else if !last_dash {
                out.push('-');
                last_dash = true;
            }
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    // Vietnamese names can reduce to nothing once non-ASCII is dropped, and two
    // projects can share a name — the id keeps every slug unique and non-empty.
    if trimmed.is_empty() {
        format!("du-an-{project_id}")
    } else {
        format!("{trimmed}-{project_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::CloneConfig;

    fn scene() -> Value {
        json!({
            "scene_id": "1",
            "duration_sec": "8",
            "visual_style": "Synthwave neon",
            "character_lock": {
                "CHAR_1": {
                    "id": "CHAR_1", "name": "Lan", "gender": "Female", "age": "25",
                    "hair": "tóc dài đen", "outfit_top": "áo khoác đỏ",
                    "voice_id": "VOICE_CHAR_1", "voice_personality": "trầm ấm",
                    "position": "giữa khung", "pose": "đứng thẳng", "expression": "mỉm cười",
                    "action_flow": {
                        "pre_action": "quay đầu lại",
                        "main_action": "bước tới trước",
                        "post_action": "dừng lại nhìn xa"
                    }
                }
            },
            "background_lock": {
                "BACKGROUND_1": {
                    "id": "BACKGROUND_1", "name": "Phố đêm", "setting": "Outdoor",
                    "scenery": "biển hiệu neon", "lighting": "ánh tím lạnh"
                }
            },
            "camera": {
                "framing": "medium shot", "angle": "ngang tầm mắt",
                "movement": "dolly tới chậm", "focus": "khuôn mặt"
            },
            "foley_and_ambience": {
                "ambience": ["tiếng mưa"], "fx": ["tiếng bước chân"], "music": "synth trầm"
            },
            "dialogue": [
                { "speaker": "Lan", "voice_marker": "VOICE_CHAR_1", "line": "Tôi về rồi" }
            ]
        })
    }

    fn project() -> Project {
        Project {
            id: 3,
            name: "Test clip".into(),
            video_path: "/tmp/v.mp4".into(),
            video_mime: "video/mp4".into(),
            video_size: 100,
            video_filename: "v.mp4".into(),
            file_uri: String::new(),
            file_uri_at: String::new(),
            char_image_path: String::new(),
            char_image_mime: String::new(),
            has_char_image: false,
            style: "Synthwave".into(),
            model: "gemini-3-flash-preview".into(),
            char_description: String::new(),
            custom_dialogue: String::new(),
            bg_description: String::new(),
            auto_magic: false,
            visual_similarity: 100,
            created_at: "2026-07-20T00:00:00Z".into(),
            updated_at: "2026-07-20T00:00:00Z".into(),
        }
    }

    fn stored() -> Vec<Scene> {
        vec![Scene {
            id: 1,
            project_id: 3,
            position: 0,
            scene_id: "1".into(),
            json: scene(),
            job_id: 9,
            created_at: "2026-07-20T00:00:00Z".into(),
        }]
    }

    #[test]
    fn image_prompt_describes_the_frame_without_motion() {
        let p = image_prompt(&scene());
        assert!(p.contains("Synthwave neon"));
        assert!(p.contains("Lan"));
        assert!(p.contains("áo khoác đỏ"));
        assert!(p.contains("Phố đêm"));
        assert!(p.contains("medium shot"));
        // Motion belongs to the video prompt only.
        assert!(!p.contains("dolly tới chậm"), "got: {p}");
        assert!(!p.contains("bước tới trước"), "got: {p}");
    }

    #[test]
    fn video_prompt_carries_action_camera_move_sound_and_dialogue() {
        let p = video_prompt(&scene());
        assert!(p.contains("bước tới trước"));
        assert!(p.contains("dolly tới chậm"));
        assert!(p.contains("tiếng mưa"));
        assert!(p.contains("Tôi về rồi"));
    }

    #[test]
    fn an_empty_scene_yields_empty_prompts_rather_than_junk() {
        let empty = json!({ "scene_id": "1" });
        assert_eq!(image_prompt(&empty), "");
        assert_eq!(video_prompt(&empty), "");
    }

    #[test]
    fn a_scene_missing_optional_blocks_still_renders() {
        let partial = json!({
            "scene_id": "2",
            "visual_style": "Dark fantasy",
            "camera": { "framing": "close up" }
        });
        let p = image_prompt(&partial);
        assert!(p.contains("Dark fantasy"));
        assert!(p.contains("close up"));
    }

    #[test]
    fn cast_dedupes_across_scenes_and_keeps_the_voice_id() {
        let scenes = vec![scene(), scene()];
        let c = cast(&scenes);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0]["id"], "CHAR_1");
        assert_eq!(c[0]["voice_id"], "VOICE_CHAR_1");
    }

    #[test]
    fn bundle_is_self_describing_and_keeps_the_original_json() {
        let b = bundle(&project(), &stored(), "2026-07-20T10:00:00Z");
        assert_eq!(b["format"], BUNDLE_FORMAT);
        assert_eq!(b["version"], BUNDLE_VERSION);
        assert_eq!(b["summary"]["scene_count"], 1);
        assert_eq!(b["summary"]["total_duration_sec"], 8.0);
        // Round-trip: the untouched Veo object survives.
        assert_eq!(b["scenes"][0]["veo"]["visual_style"], "Synthwave neon");
        assert!(b["veo_jsonl"].as_str().unwrap().contains("\"scene_id\":\"1\""));
    }

    #[test]
    fn duration_reads_both_string_and_numeric_forms() {
        assert_eq!(duration_of(&json!({"duration_sec": "8"})), 8.0);
        assert_eq!(duration_of(&json!({"duration_sec": 6})), 6.0);
        // Missing duration falls back to the pipeline's segment length.
        assert_eq!(duration_of(&json!({})), 8.0);
    }

    #[test]
    fn markdown_has_the_cast_table_and_one_heading_per_scene() {
        let md = markdown(&project(), &stored(), "2026-07-20T10:00:00Z");
        assert!(md.starts_with("# Test clip"));
        assert!(md.contains("| `CHAR_1` | Lan | `VOICE_CHAR_1` |"));
        assert!(md.contains("### Đoạn 1 (8s)"));
        assert!(md.contains("**Lan**: Tôi về rồi"));
        assert!(md.contains("## Prompt Veo 3"));
    }

    #[test]
    fn slug_is_safe_and_unique_even_for_vietnamese_names() {
        assert_eq!(slug("Test clip", 3), "test-clip-3");
        // Diacritics fold to their ASCII base instead of being dropped.
        assert_eq!(slug("Phố cổ Hội An!!", 7), "pho-co-hoi-an-7");
        assert_eq!(slug("Cô gái phố đêm", 1), "co-gai-pho-dem-1");
        assert_eq!(slug("ĐƯỜNG XƯA", 2), "duong-xua-2");
        // A script with no ASCII form would otherwise collapse to an empty name.
        assert_eq!(slug("日本語", 5), "du-an-5");
        assert_eq!(slug("", 9), "du-an-9");
    }

    #[test]
    fn markdown_escapes_pipes_so_the_cast_table_survives() {
        let mut rows = stored();
        rows[0].json["character_lock"]["CHAR_1"]["hair"] = json!("tóc | dài");
        let md = markdown(&project(), &rows, "t");
        assert!(md.contains("tóc \\| dài"), "pipe must be escaped in a table cell");
    }

    #[test]
    fn config_round_trips_into_the_bundle() {
        let mut p = project();
        p.auto_magic = true;
        p.visual_similarity = 30;
        let b = bundle(&p, &stored(), "t");
        assert_eq!(b["config"]["auto_magic"], true);
        assert_eq!(b["config"]["visual_similarity"], 30);
        let _ = CloneConfig::from(&p);
    }
}
