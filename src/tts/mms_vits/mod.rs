//! Pure-Rust port of **Meta MMS-VITS** TTS.
//!
//! [`facebook/mms-tts-*`](https://huggingface.co/facebook/mms-tts-vie) ships
//! the Massively Multilingual Speech VITS models — one safetensors per
//! language (~145 MB for Vietnamese). VITS is a single end-to-end model
//! (text encoder + stochastic duration predictor + residual-coupling flow +
//! HiFi-GAN decoder) — no separate vocoder download, no reference-audio prompt.
//!
//! ## Status
//! - [x] `config.json` parsing ([`config`])
//! - [x] Char tokenizer: vocab.json + NFC + blank interspersing ([`tokenizer`])
//! - [x] Text encoder (relative-position attention) — mlx-rs
//! - [x] Stochastic duration predictor, reverse (RQ-spline flows)
//! - [x] Residual-coupling flow, reverse (weight-normed WaveNet)
//! - [x] HiFi-GAN decoder → 16 kHz waveform
//!
//! The synthesis path is compiled behind the `local-mlx-tts` feature (Apple
//! Silicon / MLX). Without the feature, [`MmsVitsBackend::synthesize`] returns
//! [`TtsError::NotImplemented`] and the dispatcher's auto-fallback
//! ([`super::synthesize_with_fallback`]) routes to the matching macOS preset.
//!
//! Long inputs are chunked at sentence boundaries (~[`MAX_CHUNK_CHARS`] chars)
//! and stitched with a short silence gap, since VITS quality degrades on very
//! long single sequences.

pub mod config;
pub mod tokenizer;

#[cfg(feature = "local-mlx-tts")]
pub mod model;

use super::{SynthesisRequest, TtsBackend, TtsError};

/// Status message for builds without the native synthesis path. Pinned via a
/// `const` so the dispatch test can match it.
pub const STUB_MESSAGE: &str =
    "MMS-VITS native synthesis requires the `local-mlx-tts` build feature (Apple \
     Silicon). This build lacks it, so the request transparently falls back to the \
     matching macOS native voice (see X-TTS-Fallback header).";

/// Max characters per synthesis chunk before splitting on sentence boundaries.
#[cfg(feature = "local-mlx-tts")]
const MAX_CHUNK_CHARS: usize = 300;

/// `TtsBackend` impl for an MMS-VITS model.
pub struct MmsVitsBackend {
    pub id: String,
    pub label: String,
    pub default_language: String,
}

impl MmsVitsBackend {
    /// Vietnamese preset (`facebook/mms-tts-vie`).
    pub const VIETNAMESE_ID: &'static str = "facebook/mms-tts-vie";

    pub fn vietnamese() -> Self {
        Self {
            id: Self::VIETNAMESE_ID.into(),
            label: "MMS-VITS Vietnamese (HF)".into(),
            default_language: "vi".into(),
        }
    }

    /// Any `facebook/mms-tts-<lang>` checkpoint — the weights define the
    /// language; the model itself is language-agnostic at inference.
    pub fn for_model_id(id: &str) -> Option<Self> {
        let lang = id.strip_prefix("facebook/mms-tts-")?;
        if lang.is_empty() {
            return None;
        }
        Some(Self {
            id: id.to_string(),
            label: format!("MMS-VITS {lang} (HF)"),
            default_language: lang.to_string(),
        })
    }

    /// Backend for any custom checkpoint whose `config.json` declares the HF
    /// `VitsModel` architecture (community MMS finetunes etc.).
    pub fn for_custom(id: &str) -> Self {
        Self {
            id: id.to_string(),
            label: format!("VITS ({id})"),
            default_language: "vi".to_string(),
        }
    }
}

/// True when `dir/config.json` declares the HF `VitsModel` architecture —
/// lets community finetunes route to this backend regardless of repo name.
/// Cheap (reads one small JSON) and safe: on any error it just returns false
/// and dispatch falls through to the default backend.
pub fn dir_is_vits_model(dir: &std::path::Path) -> bool {
    let Ok(s) = std::fs::read_to_string(dir.join("config.json")) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
        return false;
    };
    // Accept the inference class and the finetuning wrapper
    // ("VitsModelForPreTraining", used by ylacombe/finetune-hf-vits — same
    // inference tensors plus a discriminator our loader simply ignores).
    v["architectures"]
        .as_array()
        .is_some_and(|a| a.iter().any(|x| x.as_str().is_some_and(|s| s.starts_with("VitsModel"))))
}

impl TtsBackend for MmsVitsBackend {
    fn id(&self) -> &str {
        &self.id
    }

    fn label(&self) -> &str {
        &self.label
    }

    #[cfg(not(feature = "local-mlx-tts"))]
    fn synthesize(&self, _req: &SynthesisRequest<'_>) -> Result<Vec<u8>, TtsError> {
        Err(TtsError::NotImplemented(STUB_MESSAGE.into()))
    }

    #[cfg(feature = "local-mlx-tts")]
    fn synthesize(&self, req: &SynthesisRequest<'_>) -> Result<Vec<u8>, TtsError> {
        native::synthesize(self, req)
    }
}

/// Split text into synthesis chunks at sentence boundaries, keeping each chunk
/// under `max_chars`. Never splits mid-word: overlong sentences fall back to
/// clause separators, then whitespace.
pub fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut sentences: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?' | '…' | '\n' | ';') {
            if !cur.trim().is_empty() {
                sentences.push(cur.trim().to_string());
            }
            cur.clear();
        }
    }
    if !cur.trim().is_empty() {
        sentences.push(cur.trim().to_string());
    }

    // Merge short sentences up to max_chars; split overlong ones on , then space.
    let mut chunks: Vec<String> = Vec::new();
    for s in sentences {
        let pieces: Vec<String> = if s.chars().count() <= max_chars {
            vec![s]
        } else {
            split_long(&s, max_chars)
        };
        for p in pieces {
            match chunks.last_mut() {
                Some(last) if last.chars().count() + 1 + p.chars().count() <= max_chars => {
                    last.push(' ');
                    last.push_str(&p);
                }
                _ => chunks.push(p),
            }
        }
    }
    chunks
}

fn split_long(s: &str, max_chars: usize) -> Vec<String> {
    for sep in [',', ' '] {
        let parts: Vec<&str> = s.split(sep).collect();
        if parts.len() < 2 {
            continue;
        }
        let mut out: Vec<String> = Vec::new();
        let mut cur = String::new();
        for p in parts {
            if !cur.is_empty() && cur.chars().count() + 1 + p.chars().count() > max_chars {
                out.push(cur.trim().to_string());
                cur = String::new();
            }
            if !cur.is_empty() {
                cur.push(sep);
            }
            cur.push_str(p);
        }
        if !cur.trim().is_empty() {
            out.push(cur.trim().to_string());
        }
        if out.iter().all(|c| c.chars().count() <= max_chars) {
            return out;
        }
    }
    // Last resort: hard character split.
    s.chars()
        .collect::<Vec<_>>()
        .chunks(max_chars)
        .map(|c| c.iter().collect())
        .collect()
}

#[cfg(feature = "local-mlx-tts")]
mod native {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Mutex, Once};
    use std::time::{Duration, Instant};

    use once_cell::sync::Lazy;

    use super::model::{encode_wav_pcm16, MmsVits};
    use super::tokenizer::VitsTokenizer;
    use super::{MmsVitsBackend, SynthesisRequest, TtsBackend, TtsError, MAX_CHUNK_CHARS};

    /// Drop a loaded model after this long without a request. Reloading takes
    /// only a few hundred ms, so idle RAM wins over warm-start latency.
    const IDLE_TTL: Duration = Duration::from_secs(180);
    /// Reaper wake-up interval.
    const REAP_EVERY: Duration = Duration::from_secs(60);

    /// Loaded models keyed by directory. `mlx_rs::Array` is `Send` but not
    /// `Sync`, so the cache holds models directly and synthesis runs under the
    /// lock — TTS requests are serialized, which also keeps MLX memory bounded.
    ///
    /// Memory discipline (the daemon was observed at 6 GB RSS from TTS alone):
    /// - only ONE model stays resident — switching models evicts the rest;
    /// - `mlx_clear_cache` runs after every request: sentence-pipelined TTS
    ///   allocates different-sized activations per request, which MLX's
    ///   size-bucketed buffer cache can almost never reuse, so without the
    ///   clear it grows without bound;
    /// - a background reaper drops the model after [`IDLE_TTL`] of no use.
    struct CacheEntry {
        model: MmsVits,
        tokenizer: VitsTokenizer,
        last_used: Instant,
    }

    static CACHE: Lazy<Mutex<HashMap<PathBuf, CacheEntry>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));
    static REAPER: Once = Once::new();

    /// Free MLX's buffer cache back to the OS. Callers must hold (or be
    /// certain no one else holds) the process-wide MLX serial lock.
    fn clear_mlx_cache_locked() {
        unsafe { mlx_sys::mlx_clear_cache() };
    }

    fn spawn_reaper() {
        REAPER.call_once(|| {
            std::thread::Builder::new()
                .name("tts-model-reaper".into())
                .spawn(|| loop {
                    std::thread::sleep(REAP_EVERY);
                    // Dropping model Arrays frees Metal buffers — like
                    // mlx_native's unload, that must not run concurrently with
                    // another thread's MLX work. try-lock: skip when busy, the
                    // next pass retries.
                    let Some(_g) = crate::local_model::mlx_native::mlx_serial_try_lock()
                    else {
                        continue;
                    };
                    let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
                    let before = cache.len();
                    cache.retain(|_, e| e.last_used.elapsed() < IDLE_TTL);
                    let evicted = before - cache.len();
                    drop(cache);
                    if evicted > 0 {
                        // Freeing weights only returns buffers to MLX's cache;
                        // clear it so the memory actually goes back to the OS.
                        clear_mlx_cache_locked();
                    }
                })
                .ok();
        });
    }

    pub(super) fn synthesize(
        backend: &MmsVitsBackend,
        req: &SynthesisRequest<'_>,
    ) -> Result<Vec<u8>, TtsError> {
        let dir = req.model_dir.ok_or_else(|| {
            TtsError::BadInput(format!(
                "model `{}` requires a downloaded model directory",
                backend.id()
            ))
        })?;
        if !dir.join("model.safetensors").exists() {
            return Err(TtsError::BadInput(format!(
                "model.safetensors not found in {} — download the model first",
                dir.display()
            )));
        }
        spawn_reaper();

        // Serialize against every other MLX user in the process (local LLM,
        // whisper) — concurrent MLX work on separate threads corrupts Metal
        // state. Held for the whole synthesis; we're already on a blocking thread.
        let _mlx = crate::local_model::mlx_native::mlx_serial_lock();
        let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
        // Keep at most one model resident: switching voices evicts the others
        // (each is 150–350 MB of f32 weights).
        let had_other = cache.len() > usize::from(cache.contains_key(dir));
        cache.retain(|k, _| k == dir);
        if had_other {
            clear_mlx_cache_locked();
        }
        if !cache.contains_key(dir) {
            let model = MmsVits::load(dir)
                .map_err(|e| TtsError::Internal(format!("loading MMS-VITS: {e:#}")))?;
            let tokenizer = VitsTokenizer::load(dir)
                .map_err(|e| TtsError::Internal(format!("loading tokenizer: {e:#}")))?;
            cache.insert(dir.to_path_buf(), CacheEntry { model, tokenizer, last_used: Instant::now() });
        }
        let entry = cache.get_mut(dir).expect("just inserted");
        entry.last_used = Instant::now();

        let sample_rate = entry.model.cfg.sampling_rate;
        let gap = vec![0.0f32; (sample_rate as usize) * 15 / 100]; // 150 ms
        let mut samples: Vec<f32> = Vec::new();
        let mut spoke_any = false;
        let mut result: Result<(), TtsError> = Ok(());
        for chunk in super::chunk_text(req.text, MAX_CHUNK_CHARS) {
            let ids = match entry.tokenizer.encode(&chunk) {
                Ok(ids) => ids,
                Err(_) => continue, // chunk had no speakable characters
            };
            match entry.model.infer(&ids, req.speed) {
                Ok(wav) => {
                    if spoke_any {
                        samples.extend_from_slice(&gap);
                    }
                    samples.extend(wav);
                    spoke_any = true;
                }
                Err(e) => {
                    result = Err(TtsError::Internal(format!("MMS-VITS synthesis: {e:#}")));
                    break;
                }
            }
        }
        // Synthesis activations vary in size per request and are effectively
        // never reused by MLX's buffer cache — release them (success or error)
        // so RSS returns to baseline instead of compounding request over request.
        clear_mlx_cache_locked();
        result?;
        if !spoke_any {
            return Err(TtsError::BadInput(
                "text contains no speakable characters for this voice".into(),
            ));
        }
        Ok(encode_wav_pcm16(&samples, sample_rate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vietnamese_preset_has_stable_id() {
        let b = MmsVitsBackend::vietnamese();
        assert_eq!(b.id(), "facebook/mms-tts-vie");
        assert!(b.label().contains("Vietnamese"));
        assert_eq!(b.default_language, "vi");
    }

    /// Without the native feature the stub returns NotImplemented so the
    /// dispatcher's auto-fallback triggers, and the message identifies MMS-VITS.
    #[cfg(not(feature = "local-mlx-tts"))]
    #[test]
    fn stub_message_identifies_mms_vits_family() {
        let r = MmsVitsBackend::vietnamese().synthesize(&SynthesisRequest {
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
            Ok(_) => panic!("MMS-VITS stub must error without local-mlx-tts"),
        }
    }

    /// With the native feature, a missing model dir is a caller error (400),
    /// NOT NotImplemented — it must not silently fall back to macOS.
    #[cfg(feature = "local-mlx-tts")]
    #[test]
    fn native_requires_model_dir() {
        let r = MmsVitsBackend::vietnamese().synthesize(&SynthesisRequest {
            text: "Xin chào.",
            language: "vi",
            voice: None,
            speed: 1.0,
            model_dir: None,
        });
        match r {
            Err(TtsError::BadInput(_)) => {}
            other => panic!("expected BadInput for missing model dir, got {other:?}"),
        }
    }

    /// Full native synthesis on real weights. Run with:
    /// `cargo test --features local-mlx-tts -- --ignored mms_native --test-threads=1`
    #[cfg(feature = "local-mlx-tts")]
    #[test]
    #[ignore = "needs ~/.senclaw/tts-models/facebook__mms-tts-vie downloaded"]
    fn mms_native_synthesizes_vietnamese_wav() {
        let dir = dirs::home_dir()
            .unwrap()
            .join(".senclaw/tts-models/facebook__mms-tts-vie");
        if !dir.join("model.safetensors").exists() {
            eprintln!("model not downloaded; skipping");
            return;
        }
        let wav = MmsVitsBackend::vietnamese()
            .synthesize(&SynthesisRequest {
                text: "Xin chào, hôm nay trời rất đẹp.",
                language: "vi",
                voice: None,
                speed: 1.0,
                model_dir: Some(&dir),
            })
            .expect("native synthesis");
        assert!(&wav[0..4] == b"RIFF" && &wav[8..12] == b"WAVE");
        // ≥ 0.5 s of 16 kHz PCM16 audio.
        assert!(wav.len() > 16_000, "wav suspiciously small: {} bytes", wav.len());
    }

    #[test]
    fn chunker_respects_sentences_and_limits() {
        let text = "Câu một. Câu hai khá dài hơn một chút! Câu ba?";
        let chunks = chunk_text(text, 30);
        assert!(chunks.iter().all(|c| c.chars().count() <= 30), "{chunks:?}");
        assert!(chunks.len() >= 2);
        // No characters lost (modulo separators/spaces).
        let joined: String = chunks.concat();
        assert!(joined.contains("Câu một") && joined.contains("Câu ba"));
    }

    #[test]
    fn chunker_splits_overlong_sentence_without_midword_cuts() {
        let long = "một hai ba bốn năm sáu bảy tám chín mười ".repeat(5);
        let chunks = chunk_text(&long, 40);
        assert!(chunks.len() > 1);
        for c in &chunks {
            assert!(c.chars().count() <= 40, "chunk too long: {c}");
            assert!(!c.starts_with(' ') && !c.ends_with(' '));
        }
    }

    #[test]
    fn chunker_single_short_text_is_one_chunk() {
        assert_eq!(chunk_text("Xin chào.", 300), vec!["Xin chào.".to_string()]);
    }
}
