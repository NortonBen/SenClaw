//! Pure-Rust port of **Meta MMS-VITS** TTS (in progress, stub today).
//!
//! [`facebook/mms-tts-*`](https://huggingface.co/facebook/mms-tts-vie) ships
//! the Massively Multilingual Speech VITS models — one safetensors per
//! language (~145 MB for Vietnamese). VITS is a single end-to-end model
//! (text encoder + duration predictor + posterior flow + HiFi-GAN decoder),
//! genuinely simpler than ZipVoice's flow-matching stack: no separate vocoder
//! download, no reference-audio prompt, no zero-shot voice cloning.
//!
//! That makes it the most tractable HF Vietnamese TTS port for SenClaw — when
//! the pure-Rust path lands, this is likely the **first** HF backend that
//! actually synthesises audio. Until then, [`MmsVitsBackend::synthesize`]
//! returns [`TtsError::NotImplemented`] with an MMS-specific message; the
//! dispatcher's auto-fallback ([`super::synthesize_with_fallback`]) then
//! routes to the matching macOS preset so the UI still gets audio.
//!
//! ## Roadmap
//! - [ ] `config.json` parsing (VITS hyperparameters)
//! - [ ] Char-IPA tokenizer (`vocab.json` + uroman normalization)
//! - [ ] Text encoder (Transformer)
//! - [ ] Posterior flow + duration predictor
//! - [ ] HiFi-GAN decoder (the vocoder is baked in — no separate download)
//! - [ ] End-to-end synthesis on mlx-rs

use super::{SynthesisRequest, TtsBackend, TtsError};

/// Marker / status message. Pinned via a `const` so the dispatch test can match it.
pub const STUB_MESSAGE: &str =
    "Pure-Rust MMS-VITS is under construction: no Rust port of the text encoder + \
     posterior flow + HiFi-GAN decoder yet. Until it lands, the request transparently \
     falls back to the matching macOS native voice (see X-TTS-Fallback header).";

/// `TtsBackend` impl for an MMS-VITS model. Today every supported language
/// resolves to a single stub instance; once synthesis is wired, each language
/// will load its own checkpoint via `model_dir`.
pub struct MmsVitsBackend {
    pub id: &'static str,
    pub label: &'static str,
    pub default_language: &'static str,
}

impl MmsVitsBackend {
    /// Vietnamese preset (`facebook/mms-tts-vie`).
    pub const VIETNAMESE: Self = Self {
        id: "facebook/mms-tts-vie",
        label: "MMS-VITS Vietnamese (HF, WIP)",
        default_language: "vi",
    };
}

impl TtsBackend for MmsVitsBackend {
    fn id(&self) -> &str {
        self.id
    }

    fn label(&self) -> &str {
        self.label
    }

    fn synthesize(&self, _req: &SynthesisRequest<'_>) -> Result<Vec<u8>, TtsError> {
        Err(TtsError::NotImplemented(STUB_MESSAGE.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vietnamese_preset_has_stable_id() {
        let b = MmsVitsBackend::VIETNAMESE;
        assert_eq!(b.id(), "facebook/mms-tts-vie");
        assert!(b.label().contains("Vietnamese"));
        assert_eq!(b.default_language, "vi");
    }

    /// The stub returns NotImplemented so the dispatcher's auto-fallback
    /// triggers. The message must clearly identify MMS-VITS, not ZipVoice.
    #[test]
    fn stub_message_identifies_mms_vits_family() {
        let r = MmsVitsBackend::VIETNAMESE.synthesize(&SynthesisRequest {
            text: "Xin chào.",
            language: "vi",
            voice: None,
            speed: 1.0,
            model_dir: None,
        });
        match r {
            Err(TtsError::NotImplemented(msg)) => {
                assert!(
                    msg.to_lowercase().contains("mms-vits"),
                    "stub message must identify MMS-VITS, got: {msg}"
                );
            }
            Err(other) => panic!("expected NotImplemented, got {other:?}"),
            Ok(_) => panic!("MMS-VITS stub must error until implemented"),
        }
    }
}
