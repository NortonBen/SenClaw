//! Reading study material aloud.
//!
//! Synthesis itself belongs to the daemon (`POST /api/tts/synthesize`, which
//! returns raw WAV bytes and picks the model from the user's TTS settings).
//! This module owns the two things the daemon does not:
//!
//! * **A cache.** Local TTS takes seconds per sentence. Hands-free flashcard
//!   review re-reads the same card at every interval, so re-synthesising is not
//!   a missed optimisation — it is the difference between usable and not.
//! * **Sentence splitting.** Podcast mode plays a section one sentence at a
//!   time so playback starts almost immediately instead of after the whole
//!   chapter has been rendered.
//!
//! When no TTS model is installed the daemon answers 400. That error is
//! surfaced verbatim, never swallowed: a silent play button is indistinguishable
//! from a broken one.

use std::path::PathBuf;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::config;
use crate::db::Db;

/// Longest text accepted in one synthesis call.
pub const MAX_SPEAK_CHARS: usize = 1_200;

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .expect("build http client")
}

fn cache_key(text: &str, voice: Option<&str>, speed: f64, model: Option<&str>) -> String {
    let mut h = Sha256::new();
    h.update(text.trim().as_bytes());
    h.update([0u8]);
    h.update(voice.unwrap_or("").as_bytes());
    h.update([0u8]);
    h.update(format!("{speed:.2}").as_bytes());
    h.update([0u8]);
    h.update(model.unwrap_or("").as_bytes());
    hex::encode(h.finalize())
}

/// Synthesize (or reuse) speech for `text`, returning the cached file name.
///
/// The name — not a path — is what the API hands the browser, which fetches it
/// back from `/api/audio/<name>`.
pub async fn speak(
    db: &Db,
    text: &str,
    voice: Option<&str>,
    speed: f64,
    model_id: Option<&str>,
) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("không có nội dung để đọc".into());
    }
    if text.chars().count() > MAX_SPEAK_CHARS {
        return Err(format!(
            "đoạn quá dài ({} ký tự) — cắt thành câu rồi đọc lần lượt (tối đa {MAX_SPEAK_CHARS})",
            text.chars().count()
        ));
    }
    let speed = if speed.is_finite() {
        speed.clamp(0.5, 2.0)
    } else {
        1.0
    };
    let hash = cache_key(text, voice, speed, model_id);
    let name = format!("{hash}.wav");
    let path = config::audio_dir().join(&name);

    // A cache row whose file vanished (cleared cache, moved home dir) must
    // re-synthesise rather than hand out a 404.
    if db.tts_cached(&hash).map_err(|e| e.to_string())?.is_some() && path.exists() {
        return Ok(name);
    }

    let url = format!(
        "{}/api/tts/synthesize",
        config::senclaw_base_url().trim_end_matches('/')
    );
    let mut body = serde_json::json!({ "text": text, "speed": speed });
    if let Some(v) = voice.filter(|v| !v.trim().is_empty()) {
        body["voice"] = serde_json::json!(v);
    }
    if let Some(m) = model_id.filter(|m| !m.trim().is_empty()) {
        body["model_id"] = serde_json::json!(m);
    }

    let resp = http()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("gọi TTS lỗi: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let msg = resp.text().await.unwrap_or_default();
        // 400 here almost always means "no TTS model installed" — say so in
        // the user's terms instead of echoing a bare status code.
        if status.as_u16() == 400 {
            return Err(format!(
                "chưa đọc được: {msg} — vào Cài đặt → TTS để cài giọng đọc"
            ));
        }
        return Err(format!("TTS trả {status}: {msg}"));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("đọc audio lỗi: {e}"))?;
    if bytes.is_empty() {
        return Err("TTS trả về audio rỗng".into());
    }

    std::fs::create_dir_all(config::audio_dir()).ok();
    std::fs::write(&path, &bytes).map_err(|e| format!("ghi audio lỗi: {e}"))?;
    db.tts_put(
        &hash,
        voice,
        speed,
        &path.to_string_lossy(),
        bytes.len() as i64,
    )
    .map_err(|e| e.to_string())?;
    Ok(name)
}

/// Absolute path of a cached clip, refusing anything that is not a plain
/// `<hex>.wav` name — the audio route takes this straight from the URL.
pub fn cached_path(name: &str) -> Option<PathBuf> {
    let ok = name.ends_with(".wav")
        && name.len() > 4
        && name[..name.len() - 4].chars().all(|c| c.is_ascii_hexdigit());
    if !ok {
        return None;
    }
    let p = config::audio_dir().join(name);
    p.exists().then_some(p)
}

/// Split prose into speakable sentences.
///
/// Splits on `. ! ? …` and newlines, then merges fragments so no piece is
/// pointlessly short (a heading alone) or longer than the synthesis limit.
pub fn sentences(text: &str, max_chars: usize) -> Vec<String> {
    let max = max_chars.clamp(80, MAX_SPEAK_CHARS);
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();

    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let t = cur.trim();
        if !t.is_empty() {
            out.push(t.to_string());
        }
        cur.clear();
    };

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            flush(&mut cur, &mut out);
            continue;
        }
        for ch in line.chars() {
            cur.push(ch);
            let ends = matches!(ch, '.' | '!' | '?' | '…' | ';');
            if (ends && cur.trim().chars().count() >= 40) || cur.chars().count() >= max {
                flush(&mut cur, &mut out);
            }
        }
        cur.push(' ');
    }
    flush(&mut cur, &mut out);

    // Merge tiny fragments forward so playback isn't a stutter of two-word clips.
    let mut merged: Vec<String> = Vec::new();
    for s in out {
        match merged.last_mut() {
            Some(prev)
                if prev.chars().count() < 40
                    && prev.chars().count() + s.chars().count() + 1 <= max =>
            {
                prev.push(' ');
                prev.push_str(&s);
            }
            _ => merged.push(s),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_key_changes_with_voice_speed_and_model() {
        let a = cache_key("xin chào", None, 1.0, None);
        assert_eq!(a, cache_key("xin chào", None, 1.0, None));
        assert_ne!(a, cache_key("xin chào", Some("v2"), 1.0, None));
        assert_ne!(a, cache_key("xin chào", None, 1.5, None));
        assert_ne!(a, cache_key("xin chào", None, 1.0, Some("vieneu")));
        assert_ne!(a, cache_key("chào bạn", None, 1.0, None));
    }

    #[test]
    fn only_a_plain_hex_wav_name_can_be_served() {
        assert!(cached_path("../../etc/passwd").is_none());
        assert!(cached_path("abc.wav").is_none(), "non-hex must be refused");
        assert!(cached_path("deadbeef.mp3").is_none());
    }

    #[test]
    fn sentences_split_on_punctuation_and_stay_within_the_limit() {
        let text = "Lãi suất điều hành là công cụ chính sách tiền tệ quan trọng nhất. \
                    Nó tác động tới lãi suất huy động và cho vay của ngân hàng thương mại! \
                    Vậy khi nào nó thay đổi?";
        let s = sentences(text, 200);
        assert_eq!(s.len(), 3);
        assert!(s[0].ends_with('.'));
        assert!(s.iter().all(|x| x.chars().count() <= 200));
    }

    #[test]
    fn a_heading_line_is_merged_into_the_sentence_after_it() {
        let s = sentences("Chương 1\nLãi suất điều hành do NHNN công bố định kỳ hằng quý.", 300);
        assert_eq!(s.len(), 1, "a two-word heading alone is a stutter");
        assert!(s[0].starts_with("Chương 1"));
    }

    #[test]
    fn a_wall_of_text_with_no_punctuation_is_still_chopped() {
        let wall = "từ ".repeat(2_000);
        let s = sentences(&wall, 300);
        assert!(s.len() > 1);
        assert!(s.iter().all(|x| x.chars().count() <= 300));
    }

    #[test]
    fn empty_input_yields_no_clips() {
        assert!(sentences("   \n\n ", 300).is_empty());
    }
}
