//! Text-to-speech subsystem.
//!
//! Pure-Rust TTS — no Python, no external runtimes. Multiple backends register
//! under a single [`TtsBackend`] interface; the HTTP layer in
//! [`crate::gateway::ui_server::tts`] calls [`synthesize`] which picks the
//! right backend by `model_id`.
//!
//! ## Backends today
//! - [`macos`] — native macOS `/usr/bin/say` (Vietnamese voice = Linh).
//! - [`zipvoice`] — pure-Rust MLX port of `k2-fsa/ZipVoice` (foundation built,
//!   synthesis path WIP — returns 501 until the encoder/flow-decoder/vocoder
//!   are wired). The first MLX backend; future MLX TTS models (Kokoro-MLX,
//!   etc.) live as siblings under this module.
//!
//! ## Adding a backend
//! 1. Add a module under `src/tts/<name>/` implementing [`TtsBackend`].
//! 2. Add a match arm in [`select_backend`] that recognises its `model_id`.
//! 3. Add an integration test in [`crate::gateway::ui_server::tts::synth_tests`].

use std::path::Path;

use axum::http::StatusCode;

pub mod macos;
pub mod mms_vits;
pub mod zipvoice;

/// Inputs to a single synthesis call.
#[derive(Debug, Clone, Copy)]
pub struct SynthesisRequest<'a> {
    /// Caller-supplied text to speak.
    pub text: &'a str,
    /// BCP-47-ish language hint (`"vi"`, `"en"`, …). Backends may ignore it.
    pub language: &'a str,
    /// Voice id (backend-specific; e.g. `"Linh"` for macOS, or `None`).
    pub voice: Option<&'a str>,
    /// Speed multiplier (1.0 = default, >1 faster, <1 slower).
    pub speed: f32,
    /// On-disk model directory (for backends that load weights).
    pub model_dir: Option<&'a Path>,
}

/// Errors a backend can surface. Maps to HTTP status codes via [`Self::status`].
#[derive(Debug)]
pub enum TtsError {
    /// Backend recognised the model but the implementation isn't wired yet.
    NotImplemented(String),
    /// Backend's host platform/runtime is absent (e.g. macOS-only on Linux).
    Unavailable(String),
    /// Caller-side problem — missing model dir, bad params, etc.
    BadInput(String),
    /// Synthesis attempted but failed (process error, empty output, …).
    Internal(String),
}

impl TtsError {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::NotImplemented(_) => StatusCode::NOT_IMPLEMENTED,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::BadInput(_) => StatusCode::BAD_REQUEST,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::NotImplemented(m)
            | Self::Unavailable(m)
            | Self::BadInput(m)
            | Self::Internal(m) => m,
        }
    }

    /// Convenience for the HTTP layer's `(StatusCode, String)` pair.
    pub fn into_http(self) -> (StatusCode, String) {
        (self.status(), self.message().to_string())
    }
}

impl std::fmt::Display for TtsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.status(), self.message())
    }
}

impl std::error::Error for TtsError {}

/// A TTS engine. Implementations are **blocking** — the HTTP handler calls
/// [`synthesize`] from inside `tokio::task::spawn_blocking`.
pub trait TtsBackend: Send + Sync {
    /// Stable id used by the public API (e.g. `"macos-speech"`,
    /// `"mlx-community/zipvoice-vietnamese"`). Returned by [`select_backend`].
    fn id(&self) -> &str;

    /// Short human label for diagnostics / UI.
    fn label(&self) -> &str;

    /// Synthesize speech, returning a complete WAV blob (RIFF header + PCM).
    fn synthesize(&self, req: &SynthesisRequest<'_>) -> Result<Vec<u8>, TtsError>;
}

/// Pick the backend that owns the given `model_id`.
///
/// Returns `None` if no backend recognises the id. Backend instances are
/// stateless and constructed on demand — there's no shared registry.
pub fn select_backend(model_id: &str) -> Option<Box<dyn TtsBackend>> {
    match model_id {
        "macos-speech" => Some(Box::new(macos::MacosSpeech::VIETNAMESE)),
        "macos-speech-en" => Some(Box::new(macos::MacosSpeech::ENGLISH)),
        "facebook/mms-tts-vie" => Some(Box::new(mms_vits::MmsVitsBackend::VIETNAMESE)),
        // Any other id is assumed to be a HuggingFace ZipVoice-family model.
        // When more TTS families land their explicit ids go above this arm.
        _ => Some(Box::new(zipvoice::ZipVoiceBackend)),
    }
}

/// True when `model_id` resolves to a macOS-speech preset (any language).
fn is_macos_speech(model_id: &str) -> bool {
    matches!(model_id, "macos-speech" | "macos-speech-en")
}

/// Synthesize via the backend matching `model_id`.
///
/// Returns `(StatusCode, message)` on failure so the HTTP handler can wrap it
/// in `AppError` without juggling enum variants.
pub fn synthesize(
    model_id: &str,
    model_dir: Option<&Path>,
    text: &str,
    language: &str,
    voice: Option<&str>,
    speed: f32,
) -> Result<Vec<u8>, (StatusCode, String)> {
    let backend = select_backend(model_id).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("no TTS backend registered for `{model_id}`"),
        )
    })?;
    let req = SynthesisRequest {
        text,
        language,
        voice,
        speed,
        model_dir,
    };
    backend.synthesize(&req).map_err(TtsError::into_http)
}

/// Outcome of a synthesis attempt — including transparent fallback metadata.
#[derive(Debug)]
pub struct SynthesisOutcome {
    pub wav: Vec<u8>,
    /// Backend id that actually produced the WAV.
    pub used_backend: String,
    /// Set if a fallback occurred — explains why the originally-requested
    /// backend was skipped (e.g. "ZipVoice synthesis is not yet implemented").
    pub fallback_reason: Option<String>,
}

/// Synthesize with **honest auto-fallback to `macos-speech`** when the
/// requested backend can't produce audio yet.
///
/// Behaviour:
///   - Try the requested backend first.
///   - If it returns [`TtsError::NotImplemented`] (the only "I'm not built yet"
///     signal), fall back to `macos-speech` and tag the outcome.
///   - All other errors propagate unchanged — they're real failures, not
///     "feature still under construction".
///
/// The caller (HTTP handler) is expected to surface `fallback_reason` to the
/// user via a response header (`X-TTS-Fallback`) so the UI never sees a
/// **silent** model swap.
pub fn synthesize_with_fallback(
    model_id: &str,
    model_dir: Option<&Path>,
    text: &str,
    language: &str,
    voice: Option<&str>,
    speed: f32,
) -> Result<SynthesisOutcome, (StatusCode, String)> {
    let backend = select_backend(model_id).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("no TTS backend registered for `{model_id}`"),
        )
    })?;
    let req = SynthesisRequest {
        text,
        language,
        voice,
        speed,
        model_dir,
    };
    match backend.synthesize(&req) {
        Ok(wav) => Ok(SynthesisOutcome {
            wav,
            used_backend: backend.id().to_string(),
            fallback_reason: None,
        }),
        Err(TtsError::NotImplemented(msg)) if !is_macos_speech(model_id) => {
            // The requested backend is honest-stubbed. Pick the macOS preset
            // that best matches the request language so an English request
            // falling back from a stub doesn't surprise the user with a
            // Vietnamese voice.
            let fallback = macos::MacosSpeech::for_language(language);
            let fallback_req = SynthesisRequest {
                model_dir: None, // macos-speech ignores model_dir
                ..req
            };
            match fallback.synthesize(&fallback_req) {
                Ok(wav) => {
                    let used = fallback.id().to_string();
                    Ok(SynthesisOutcome {
                        wav,
                        used_backend: used.clone(),
                        fallback_reason: Some(format!(
                            "{backend_id} not yet implemented; auto-fell-back to {used} ({msg})",
                            backend_id = backend.id(),
                        )),
                    })
                }
                Err(e) => Err(e.into_http()),
            }
        }
        Err(e) => Err(e.into_http()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_id_routes_to_zipvoice_then_501() {
        // No backend bails — we route HF-ish ids to ZipVoice. Until it lands,
        // any non-macOS id surfaces as 501 from the ZipVoice stub.
        let backend = select_backend("totally/unknown-tts").expect("dispatch");
        assert_eq!(backend.id(), zipvoice::ZipVoiceBackend.id());
    }

    #[test]
    fn macos_speech_id_dispatches_to_macos_backend() {
        let backend = select_backend("macos-speech").expect("dispatch");
        assert_eq!(backend.id(), "macos-speech");
    }

    #[test]
    fn macos_speech_en_id_dispatches_to_english_preset() {
        let backend = select_backend("macos-speech-en").expect("dispatch");
        assert_eq!(backend.id(), "macos-speech-en");
        assert!(backend.label().contains("English"));
    }

    #[test]
    fn mms_vits_id_dispatches_to_dedicated_backend() {
        let backend = select_backend("facebook/mms-tts-vie").expect("dispatch");
        assert_eq!(backend.id(), "facebook/mms-tts-vie");
        assert!(backend.label().contains("MMS-VITS"));
    }

    /// MMS-VITS fallback should produce audio via Vietnamese macOS preset
    /// (since the request language is `vi`) AND the reason must name MMS-VITS
    /// — not ZipVoice — so logs/headers are honest about which stub triggered.
    #[cfg(target_os = "macos")]
    #[test]
    fn mms_vits_fallback_is_distinct_from_zipvoice_reason() {
        let outcome = synthesize_with_fallback(
            "facebook/mms-tts-vie",
            None,
            "Xin chào.",
            "vi",
            None,
            1.0,
        )
        .expect("fallback should succeed via macos-speech");
        assert_eq!(outcome.used_backend, "macos-speech");
        let reason = outcome.fallback_reason.expect("must explain fallback");
        let lower = reason.to_lowercase();
        assert!(
            lower.contains("mms-vits"),
            "fallback reason must name MMS-VITS, got: {reason}"
        );
        assert!(
            !lower.contains("zipvoice"),
            "fallback reason must NOT name ZipVoice for an MMS request, got: {reason}"
        );
    }

    /// Fallback must respect the request language — English text falling back
    /// from a stubbed backend should not end up on a Vietnamese voice.
    #[cfg(target_os = "macos")]
    #[test]
    fn fallback_picks_english_preset_for_en_requests() {
        let outcome = synthesize_with_fallback(
            "mlx-community/zipvoice-vietnamese",
            None,
            "Hello, this is a test.",
            "en",
            None,
            1.0,
        )
        .expect("english fallback should succeed");
        assert_eq!(outcome.used_backend, "macos-speech-en");
        assert!(outcome.fallback_reason.is_some());
    }

    /// When ZipVoice returns NotImplemented, fallback uses macos-speech and
    /// the outcome carries a non-empty `fallback_reason`. macOS-only because
    /// the fallback backend is macOS-native.
    #[cfg(target_os = "macos")]
    #[test]
    fn fallback_to_macos_on_not_implemented_is_transparent() {
        let outcome = synthesize_with_fallback(
            "mlx-community/zipvoice-vietnamese",
            None,
            "Xin chào.",
            "vi",
            None,
            1.0,
        )
        .expect("fallback should succeed via macos-speech");
        assert_eq!(outcome.used_backend, "macos-speech");
        let reason = outcome.fallback_reason.expect("reason must be set");
        assert!(
            reason.contains("not yet implemented"),
            "reason should explain the fallback, got: {reason}"
        );
        assert!(outcome.wav.len() > 1024, "fallback wav suspiciously small");
        assert_eq!(&outcome.wav[0..4], b"RIFF");
    }

    /// Direct call to macos-speech must NOT report a fallback — it served the
    /// request itself.
    #[cfg(target_os = "macos")]
    #[test]
    fn no_fallback_when_macos_handles_directly() {
        let outcome = synthesize_with_fallback("macos-speech", None, "Xin chào.", "vi", None, 1.0)
            .expect("macos-speech should succeed");
        assert_eq!(outcome.used_backend, "macos-speech");
        assert!(outcome.fallback_reason.is_none());
    }

    #[test]
    fn tts_error_maps_to_http_status() {
        assert_eq!(
            TtsError::NotImplemented("x".into()).status(),
            StatusCode::NOT_IMPLEMENTED
        );
        assert_eq!(
            TtsError::Unavailable("x".into()).status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            TtsError::BadInput("x".into()).status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            TtsError::Internal("x".into()).status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
