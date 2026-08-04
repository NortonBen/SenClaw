//! Handoff to video-flow, the downstream video generator.
//!
//! video-flow has no bulk import, so a handoff is a sequence of plain CRUD
//! calls: project → entities → video → scenes. Two traps make the obvious
//! approaches wrong:
//!
//!   * **Never call `POST /api/pipeline/create` after writing scenes.** That
//!     runs its `script_parser` agent, which does `DELETE FROM scene WHERE
//!     video_id = ?` and rebuilds everything from an LLM re-read of the script
//!     — throwing away exactly the scenes we just handed over.
//!   * **Create entities over REST, not over video-flow's MCP.** Its
//!     `vf_character_create` upper-cases `entity_type`, but the column has a
//!     `CHECK(entity_type IN ('character', …))` on lowercase values, so the
//!     insert fails. `POST /api/projects/:id/characters` passes the value
//!     through untouched.
//!
//! video-flow also expects visual prompts in English (they feed Veo 3 directly)
//! while this app deliberately produces Vietnamese. `narrator_text` is the one
//! field that must stay in the original language — it drives narration audio.

use crate::db::{Project, Scene};
use crate::export;
use serde_json::{json, Value};
use std::time::Duration;

/// Appended to every video prompt: video-flow's own playbook requires it, and
/// without it Veo 3 likes to burn subtitles into the frame.
const NEGATIVE_SUFFIX: &str = "Negative: subtitles, watermark, text overlay.";

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("tạo HTTP client thất bại: {e}"))
}

/// Map free-text framing onto video-flow's `shot_type` enum.
///
/// The analysis model writes framing however it likes, in either language, so
/// this matches on both. Anything unrecognised falls back to video-flow's own
/// default rather than inventing a value the CHECK constraint would reject.
pub fn shot_type(framing: &str) -> &'static str {
    let f = framing.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| f.contains(n));

    if has(&["extreme close", "cực cận", "đặc tả cực"]) {
        "EXTREME_CLOSE_UP"
    } else if has(&["close", "cận cảnh", "cận "]) {
        "CLOSE_UP"
    } else if has(&["wide", "long shot", "toàn cảnh", "viễn cảnh", "rộng"]) {
        "WIDE"
    } else {
        "MEDIUM"
    }
}

/// Map free-text camera movement onto video-flow's `camera_movement` enum.
pub fn camera_movement(movement: &str) -> &'static str {
    let m = movement.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| m.contains(n));

    if has(&["dolly", "tracking", "đẩy máy", "lia tới", "ray"]) {
        "DOLLY"
    } else if has(&["zoom", "phóng to", "thu nhỏ"]) {
        "ZOOM"
    } else if has(&["crane", "cẩu", "bay lên", "drone"]) {
        "CRANE"
    } else if has(&["handheld", "cầm tay", "rung"]) {
        "HANDHELD"
    } else if has(&["tilt", "ngẩng", "cúi", "dọc"]) {
        "TILT"
    } else if has(&["pan", "lia", "quét ngang", "ngang"]) {
        "PAN"
    } else {
        "STATIC"
    }
}

/// `narrator_text` in the shape video-flow's audio stage parses: `NAME: line`.
fn narrator_text(scene: &Value) -> String {
    scene
        .get("dialogue")
        .and_then(|v| v.as_array())
        .map(|lines| {
            lines
                .iter()
                .filter_map(|l| {
                    let text = l.get("line").and_then(|v| v.as_str()).unwrap_or("").trim();
                    if text.is_empty() {
                        return None;
                    }
                    let speaker = l
                        .get("speaker")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim();
                    Some(if speaker.is_empty() {
                        text.to_string()
                    } else {
                        format!("{speaker}: {text}")
                    })
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn character_names(scene: &Value) -> Vec<String> {
    scene
        .get("character_lock")
        .and_then(|v| v.as_object())
        .map(|lock| {
            lock.values()
                .filter_map(|c| c.get("name").and_then(|v| v.as_str()))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// One scene, in video-flow's row shape.
pub fn scene_row(index: usize, row: &Scene) -> Value {
    let sc = &row.json;
    let image = export::image_prompt(sc);
    let motion = export::video_prompt(sc);

    let video_prompt = if motion.is_empty() {
        NEGATIVE_SUFFIX.to_string()
    } else {
        format!("{motion}. {NEGATIVE_SUFFIX}")
    };

    json!({
        "display_order": index as i64 + 1,
        "prompt": image,
        "image_prompt": image,
        "video_prompt": video_prompt,
        "action_sequence": motion,
        "narrator_text": narrator_text(sc),
        // Sent as a real array: video-flow's REST layer serializes it into the
        // TEXT column itself. (Its MCP tool does not — another reason to use REST.)
        "character_names": character_names(sc),
        "duration": export::duration_of(sc),
        "shot_type": shot_type(&nested_str(sc, "camera", "framing")),
        "camera_movement": camera_movement(&nested_str(sc, "camera", "movement")),
        // Only the first scene starts a chain; the rest continue it, which is
        // what tells video-flow to carry visual continuity forward.
        "chain_type": if index == 0 { "ROOT" } else { "CONTINUATION" },
    })
}

fn nested_str(v: &Value, a: &str, b: &str) -> String {
    v.get(a)
        .and_then(|x| x.get(b))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

/// The cast, as video-flow entity rows.
pub fn entity_rows(scene_values: &[Value]) -> Vec<Value> {
    export::cast(scene_values)
        .iter()
        .map(|c| {
            let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let appearance = c
                .get("appearance")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            json!({
                "name": name,
                // Lowercase on purpose — the column's CHECK constraint rejects
                // anything else.
                "entity_type": "character",
                "description": appearance,
                "image_prompt": format!(
                    "Character reference portrait: {appearance}, neutral grey background, studio lighting, reference sheet style"
                ),
                "voice_description": c
                    .get("voice_personality")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            })
        })
        .filter(|e| !e["name"].as_str().unwrap_or("").is_empty())
        .collect()
}

/// The whole payload, before it is pushed anywhere.
///
/// Exposed on its own so a caller can preview exactly what would be created —
/// and so the tests can check the mapping without a live video-flow.
pub fn plan(project: &Project, stored: &[Scene], orientation: &str) -> Value {
    let scene_values: Vec<Value> = stored.iter().map(|s| s.json.clone()).collect();
    json!({
        "project": {
            "name": project.name,
            "description": format!(
                "Sao chép từ video \"{}\" bằng SenClaw Video Cloner (phong cách: {}).",
                project.video_filename, project.style
            ),
            "language": "vi",
            "material": "realistic",
        },
        "video": {
            "title": project.name,
            "orientation": normalize_orientation(orientation),
            "display_order": 0,
        },
        "entities": entity_rows(&scene_values),
        "scenes": stored
            .iter()
            .enumerate()
            .map(|(i, r)| scene_row(i, r))
            .collect::<Vec<_>>(),
    })
}

pub fn normalize_orientation(o: &str) -> &'static str {
    match o.trim().to_uppercase().as_str() {
        "VERTICAL" | "PORTRAIT" | "DOC" | "DỌC" => "VERTICAL",
        _ => "HORIZONTAL",
    }
}

/// Translate the visual prompts of a plan into English, in place.
///
/// video-flow feeds `prompt` / `image_prompt` / `video_prompt` straight to Veo 3
/// and its playbook requires English; this app produces Vietnamese by design.
/// `narrator_text` is deliberately left alone — it drives narration audio and
/// must stay in the original language.
pub async fn translate_plan(plan: &mut Value) -> Result<usize, String> {
    let Some(scenes) = plan.get_mut("scenes").and_then(|v| v.as_array_mut()) else {
        return Ok(0);
    };

    let mut translated = 0usize;
    for scene in scenes.iter_mut() {
        let image = scene["image_prompt"].as_str().unwrap_or("").to_string();
        let motion = scene["action_sequence"].as_str().unwrap_or("").to_string();
        if image.trim().is_empty() && motion.trim().is_empty() {
            continue;
        }

        let user = format!(
            "FRAME:\n{image}\n\nMOTION:\n{motion}\n\n\
             Dịch sang tiếng Anh. Trả về đúng hai dòng, không thêm gì khác:\n\
             FRAME: <bản dịch>\nMOTION: <bản dịch>"
        );
        let out = crate::llm::bridge_llm(TRANSLATE_SYSTEM, &user, 2000).await?;
        let (en_image, en_motion) = parse_translation(&out);

        if !en_image.is_empty() {
            scene["prompt"] = json!(en_image);
            scene["image_prompt"] = json!(en_image);
        }
        if !en_motion.is_empty() {
            scene["action_sequence"] = json!(en_motion);
            scene["video_prompt"] = json!(format!("{en_motion}. {NEGATIVE_SUFFIX}"));
        }
        translated += 1;
    }
    Ok(translated)
}

const TRANSLATE_SYSTEM: &str = "You translate Vietnamese film-scene descriptions into English prompts for an AI video generator. \
Keep every concrete visual detail: subject, wardrobe, setting, lighting, camera framing and movement. \
Do not add details that are not present. Do not add commentary. Keep proper names unchanged. \
Answer with exactly two lines, prefixed 'FRAME:' and 'MOTION:'.";

/// Pull the two labelled lines out of a translation response.
///
/// A model that ignores the format and answers with a bare paragraph would
/// otherwise silently overwrite both prompts with the same text.
pub fn parse_translation(out: &str) -> (String, String) {
    let mut frame = String::new();
    let mut motion = String::new();
    for line in out.lines() {
        let t = line.trim().trim_start_matches(['*', '-', '#', ' ']);
        if let Some(rest) = strip_label(t, "FRAME:") {
            frame = rest;
        } else if let Some(rest) = strip_label(t, "MOTION:") {
            motion = rest;
        }
    }
    (frame, motion)
}

fn strip_label(line: &str, label: &str) -> Option<String> {
    if !line.to_uppercase().starts_with(label) {
        return None;
    }
    // `**FRAME:** text` leaves a trailing `**` once the label itself is cut.
    Some(
        line[label.len()..]
            .trim_matches(|c: char| c == '*' || c == '_' || c.is_whitespace())
            .to_string(),
    )
}

/// Result of a successful push.
pub struct Pushed {
    pub project_id: String,
    pub video_id: String,
    pub entity_count: usize,
    pub scene_count: usize,
}

/// Create the project, entities, video and scenes inside a running video-flow.
pub async fn push(base_url: &str, plan: &Value) -> Result<Pushed, String> {
    let client = http()?;
    let base = base_url.trim_end_matches('/');

    let project = post_json(&client, &format!("{base}/api/projects"), &plan["project"])
        .await
        .map_err(|e| format!("tạo project trong video-flow thất bại: {e}"))?;
    let project_id =
        id_of(&project).ok_or_else(|| "video-flow không trả về id của project".to_string())?;

    let mut entity_count = 0usize;
    if let Some(entities) = plan["entities"].as_array() {
        for e in entities {
            // A failed entity is not fatal: the scenes still import, and the
            // user can add the missing reference by hand in video-flow.
            if post_json(
                &client,
                &format!("{base}/api/projects/{project_id}/characters"),
                e,
            )
            .await
            .is_ok()
            {
                entity_count += 1;
            }
        }
    }

    let mut video_body = plan["video"].clone();
    video_body["project_id"] = json!(project_id);
    let video = post_json(&client, &format!("{base}/api/videos"), &video_body)
        .await
        .map_err(|e| format!("tạo video trong video-flow thất bại: {e}"))?;
    let video_id =
        id_of(&video).ok_or_else(|| "video-flow không trả về id của video".to_string())?;

    let mut scene_count = 0usize;
    if let Some(scenes) = plan["scenes"].as_array() {
        for sc in scenes {
            let mut body = sc.clone();
            body["video_id"] = json!(video_id);
            post_json(&client, &format!("{base}/api/scenes"), &body)
                .await
                .map_err(|e| format!("tạo scene thất bại: {e}"))?;
            scene_count += 1;
        }
    }

    Ok(Pushed {
        project_id,
        video_id,
        entity_count,
        scene_count,
    })
}

fn id_of(v: &Value) -> Option<String> {
    v.get("id")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            v.get("data")
                .and_then(|d| d.get("id"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
}

async fn post_json(client: &reqwest::Client, url: &str, body: &Value) -> Result<Value, String> {
    let resp = client
        .post(url)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("không gọi được {url}: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "{url} trả {status}: {}",
            crate::scenes::truncate_chars(text.trim(), 300)
        ));
    }
    serde_json::from_str(&text).map_err(|e| format!("{url} trả về không phải JSON: {e}"))
}

/// Whether a video-flow is reachable at `base_url`.
pub async fn probe(base_url: &str) -> Result<Value, String> {
    let client = http()?;
    let url = format!("{}/api/status", base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("không kết nối được video-flow tại {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("video-flow trả {} tại {url}", resp.status()));
    }
    // A daemon that answers the SPA fallback for an unknown /api path returns
    // HTML with a 200, so "reachable" only counts if the body parses as JSON.
    let text = resp.text().await.unwrap_or_default();
    serde_json::from_str(&text)
        .map_err(|_| format!("{url} không trả JSON — có thể không phải video-flow"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Scene;

    fn scene_json() -> Value {
        json!({
            "scene_id": "1",
            "duration_sec": "8",
            "visual_style": "Synthwave neon",
            "character_lock": {
                "CHAR_1": {
                    "id": "CHAR_1", "name": "Lan", "hair": "tóc dài",
                    "voice_id": "VOICE_CHAR_1", "voice_personality": "trầm ấm",
                    "action_flow": { "main_action": "bước tới trước" }
                }
            },
            "camera": { "framing": "close up", "movement": "dolly tới chậm" },
            "dialogue": [{ "speaker": "Lan", "line": "Tôi về rồi" }]
        })
    }

    fn row(index: i64, json: Value) -> Scene {
        Scene {
            id: index + 1,
            project_id: 1,
            position: index,
            scene_id: (index + 1).to_string(),
            json,
            job_id: 1,
            created_at: "t".into(),
        }
    }

    #[test]
    fn shot_type_maps_english_and_vietnamese_framing() {
        assert_eq!(shot_type("close up"), "CLOSE_UP");
        assert_eq!(shot_type("cận cảnh khuôn mặt"), "CLOSE_UP");
        assert_eq!(shot_type("extreme close-up"), "EXTREME_CLOSE_UP");
        assert_eq!(shot_type("wide shot"), "WIDE");
        assert_eq!(shot_type("toàn cảnh"), "WIDE");
    }

    #[test]
    fn an_unknown_framing_falls_back_to_the_safe_default() {
        assert_eq!(shot_type(""), "MEDIUM");
        assert_eq!(shot_type("một cái gì đó lạ"), "MEDIUM");
    }

    #[test]
    fn camera_movement_maps_both_languages() {
        assert_eq!(camera_movement("dolly in slowly"), "DOLLY");
        assert_eq!(camera_movement("đẩy máy chậm"), "DOLLY");
        assert_eq!(camera_movement("lia ngang"), "PAN");
        assert_eq!(camera_movement("zoom out"), "ZOOM");
        assert_eq!(camera_movement("drone bay lên"), "CRANE");
        assert_eq!(camera_movement("cầm tay rung"), "HANDHELD");
        assert_eq!(camera_movement(""), "STATIC");
    }

    #[test]
    fn every_mapped_enum_is_one_video_flow_accepts() {
        // The columns carry CHECK constraints; an invented value fails the insert.
        const SHOTS: [&str; 4] = ["WIDE", "MEDIUM", "CLOSE_UP", "EXTREME_CLOSE_UP"];
        const MOVES: [&str; 7] = [
            "STATIC", "PAN", "TILT", "DOLLY", "ZOOM", "HANDHELD", "CRANE",
        ];
        for probe in ["", "wide", "cận", "lia", "zoom", "gì đó", "handheld"] {
            assert!(SHOTS.contains(&shot_type(probe)), "bad shot for {probe}");
            assert!(
                MOVES.contains(&camera_movement(probe)),
                "bad move for {probe}"
            );
        }
    }

    #[test]
    fn the_first_scene_roots_the_chain_and_the_rest_continue_it() {
        assert_eq!(scene_row(0, &row(0, scene_json()))["chain_type"], "ROOT");
        assert_eq!(
            scene_row(1, &row(1, scene_json()))["chain_type"],
            "CONTINUATION"
        );
    }

    #[test]
    fn display_order_is_one_based() {
        assert_eq!(scene_row(0, &row(0, scene_json()))["display_order"], 1);
        assert_eq!(scene_row(4, &row(4, scene_json()))["display_order"], 5);
    }

    #[test]
    fn every_video_prompt_carries_the_negative_clause() {
        let r = scene_row(0, &row(0, scene_json()));
        assert!(r["video_prompt"]
            .as_str()
            .unwrap()
            .contains("Negative: subtitles"));
    }

    #[test]
    fn an_empty_scene_still_gets_a_usable_video_prompt() {
        let r = scene_row(0, &row(0, json!({ "scene_id": "1" })));
        assert_eq!(r["video_prompt"], NEGATIVE_SUFFIX);
    }

    #[test]
    fn narrator_text_keeps_the_original_language_in_name_colon_form() {
        let r = scene_row(0, &row(0, scene_json()));
        assert_eq!(r["narrator_text"], "Lan: Tôi về rồi");
    }

    #[test]
    fn character_names_is_a_real_array_not_a_string() {
        let r = scene_row(0, &row(0, scene_json()));
        assert!(r["character_names"].is_array());
        assert_eq!(r["character_names"][0], "Lan");
    }

    #[test]
    fn entities_use_the_lowercase_type_the_check_constraint_demands() {
        let e = entity_rows(&[scene_json()]);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0]["entity_type"], "character");
        assert!(e[0]["image_prompt"]
            .as_str()
            .unwrap()
            .starts_with("Character reference portrait:"));
    }

    #[test]
    fn a_nameless_character_is_dropped_rather_than_creating_a_blank_entity() {
        let sc = json!({
            "scene_id": "1",
            "character_lock": { "CHAR_1": { "id": "CHAR_1", "name": "" } }
        });
        assert!(entity_rows(&[sc]).is_empty());
    }

    #[test]
    fn orientation_normalizes_to_the_two_accepted_values() {
        assert_eq!(normalize_orientation("vertical"), "VERTICAL");
        assert_eq!(normalize_orientation("dọc"), "VERTICAL");
        assert_eq!(normalize_orientation(""), "HORIZONTAL");
        assert_eq!(normalize_orientation("nonsense"), "HORIZONTAL");
    }

    #[test]
    fn parse_translation_reads_the_two_labelled_lines() {
        let (f, m) = parse_translation("FRAME: A woman stands\nMOTION: She steps forward");
        assert_eq!(f, "A woman stands");
        assert_eq!(m, "She steps forward");
    }

    #[test]
    fn parse_translation_survives_markdown_decoration() {
        let (f, m) = parse_translation("**FRAME:** A woman\n- MOTION: She walks");
        assert_eq!(f, "A woman");
        assert_eq!(m, "She walks");
    }

    #[test]
    fn an_unlabelled_reply_translates_nothing_instead_of_corrupting_both_fields() {
        let (f, m) = parse_translation("A woman stands and then walks forward.");
        assert!(f.is_empty());
        assert!(m.is_empty());
    }
}
