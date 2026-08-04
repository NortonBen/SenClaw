//! Bridge to the SenClaw daemon's TTS subsystem. Same principle as `llm.rs`:
//! the app owns no voice models and no provider keys — narration is synthesized
//! by whichever backend the user installed and selected in SenClaw Settings
//! (VieNeu-TTS v3 Turbo, MMS-VITS, macOS Speech, …).

use serde_json::{json, Value};
use std::time::Duration;

/// `POST /api/tts/synthesize` → WAV bytes.
///
/// Every parameter is optional: omitted fields fall back to the daemon's
/// persisted TTS settings (model, voice, language, speed), so a project that
/// says nothing about narration still speaks in the user's chosen voice.
pub async fn synthesize(
    text: &str,
    language: &str,
    voice: &str,
    speed: Option<f32>,
    model_id: &str,
) -> Result<Vec<u8>, String> {
    if text.trim().is_empty() {
        return Err("tts: empty text".to_string());
    }
    let url = format!(
        "{}/api/tts/synthesize",
        crate::llm::base_url().trim_end_matches('/')
    );
    let mut body = json!({ "text": text });
    if !language.trim().is_empty() {
        body["language"] = json!(language.trim());
    }
    if !voice.trim().is_empty() {
        body["voice"] = json!(voice.trim());
    }
    if let Some(s) = speed {
        body["speed"] = json!(s);
    }
    if !model_id.trim().is_empty() {
        body["model_id"] = json!(model_id.trim());
    }

    let resp = crate::llm::http()
        .post(&url)
        // Long-form narration on a CPU backend is slow; well past the daemon's
        // own synth time but bounded so a wedged model can't stall the DAG task.
        .timeout(Duration::from_secs(180))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("tts request failed ({url}): {e}"))?;

    let status = resp.status();
    // The daemon reports an honest backend swap (e.g. ZipVoice → macOS Speech)
    // in this header rather than silently substituting a different voice.
    let fallback = resp
        .headers()
        .get("x-tts-fallback")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !status.is_success() {
        let msg = resp.text().await.unwrap_or_default();
        return Err(format!(
            "tts {}: {}",
            status.as_u16(),
            crate::llm::truncate(msg.trim(), 300)
        ));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("tts read failed: {e}"))?;
    if bytes.is_empty() {
        return Err("tts returned no audio".to_string());
    }
    if !fallback.is_empty() {
        println!("[tts] backend fell back to {fallback}");
    }
    Ok(bytes.to_vec())
}

/// `GET /api/tts/settings` — `{model_id, voice, speed, language}`. Used to show
/// what narration *would* sound like without synthesizing anything.
pub async fn settings() -> Result<Value, String> {
    let url = format!(
        "{}/api/tts/settings",
        crate::llm::base_url().trim_end_matches('/')
    );
    let v: Value = crate::llm::http()
        .get(&url)
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(v)
}

/// `GET /api/tts/models` — catalog with install status, so the app can tell the
/// user "no TTS model installed" instead of failing mid-pipeline.
pub async fn models() -> Result<Value, String> {
    let url = format!(
        "{}/api/tts/models",
        crate::llm::base_url().trim_end_matches('/')
    );
    let v: Value = crate::llm::http()
        .get(&url)
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(v)
}

/// True when at least one TTS model is installed and usable.
pub async fn is_available() -> bool {
    match models().await {
        Ok(v) => {
            let list = v
                .get("models")
                .and_then(|m| m.as_array())
                .cloned()
                .unwrap_or_default();
            list.iter().any(|m| {
                m.get("installed")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false)
                    || m.get("status").and_then(|s| s.as_str()).unwrap_or("") == "installed"
            })
        }
        Err(_) => false,
    }
}
