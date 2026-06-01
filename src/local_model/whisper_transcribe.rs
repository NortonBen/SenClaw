//! Whisper ASR driver — glues the pure-Rust audio front-end ([`super::audio`])
//! to the `mlx-rs` Whisper model ([`super::mlx_lm::models::whisper`]) and a
//! greedy decoder. No Python, no ffmpeg.
//!
//! v1 scope (documented simplifications):
//! - Greedy decoding (temperature 0); no beam search / temperature fallback.
//! - Non-overlapping 30 s windows concatenated — a word straddling a window
//!   boundary may be clipped. (Whisper's reference uses overlapped sliding
//!   windows with previous-text conditioning.)
//! - Special-token suppression keeps text + `<|endoftext|>` only; the full
//!   non-speech token blacklist is not applied.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{Context, Result};
use mlx_rs::{ops::indexing::IndexOp, Array, Dtype};

use super::audio::{self, N_SAMPLES};
use super::mlx_lm::models::whisper::{load_whisper_model, maybe_causal_mask, WhisperModel};
use super::mlx_lm_utils::whisper_tokenizer::WhisperTokenizer;

/// Default transcription language (this project prioritizes Vietnamese).
pub const DEFAULT_LANGUAGE: &str = "vi";

/// If the model's `<|nospeech|>` probability at the SOT position exceeds this,
/// the window is treated as silence and produces no text. Whisper's reference
/// default. Prevents the classic "hallucinate a YouTube outro over silence" bug.
const NO_SPEECH_THRESHOLD: f32 = 0.6;

/// Windows whose peak amplitude is below this are skipped without running the
/// model at all (digital silence / quiet mic noise). Cheap pre-filter.
const SILENCE_PEAK: f32 = 0.005;

/// If the mean per-token log-probability of a window's transcript is below this,
/// the output is discarded as a low-confidence hallucination (Whisper's default
/// `logprob_threshold`). This is what catches "silence → invented YouTube outro"
/// when the audio sneaks past the energy + no-speech gates.
const LOGPROB_THRESHOLD: f32 = -1.0;

/// Timing/throughput breakdown for one transcription (for benchmarks).
#[derive(Debug, Clone, Default)]
pub struct TranscribeStats {
    /// Audio length in seconds (post-decode, pre-padding).
    pub audio_secs: f32,
    /// Number of 30 s windows processed.
    pub n_chunks: usize,
    /// Total time spent in the audio encoder, milliseconds.
    pub encode_ms: f64,
    /// Total time spent in the greedy decoder, milliseconds.
    pub decode_ms: f64,
    /// Total time spent computing log-mel features, milliseconds.
    pub mel_ms: f64,
    /// Wall time for the whole transcription (excludes model load), milliseconds.
    pub total_ms: f64,
    /// Total text tokens generated across all windows.
    pub tokens: usize,
    /// Worst-case (max) `<|nospeech|>` probability across windows.
    pub no_speech_prob: f32,
    /// Worst-case (min) mean per-token log-probability across windows (closer
    /// to 0 = more confident; very negative = likely hallucination).
    pub avg_logprob: f32,
}

impl TranscribeStats {
    /// Real-time factor = processing time / audio duration. <1 is faster than realtime.
    pub fn rtf(&self) -> f64 {
        if self.audio_secs > 0.0 {
            self.total_ms / 1000.0 / self.audio_secs as f64
        } else {
            0.0
        }
    }
    /// Decode throughput in tokens/second.
    pub fn decode_tok_s(&self) -> f64 {
        if self.decode_ms > 0.0 {
            self.tokens as f64 / (self.decode_ms / 1000.0)
        } else {
            0.0
        }
    }
}

struct Loaded {
    model: WhisperModel,
    tokenizer: WhisperTokenizer,
    dtype: Dtype,
}

/// A lazily-loaded Whisper engine bound to one model directory.
pub struct WhisperEngine {
    model_dir: PathBuf,
    loaded: Mutex<Option<Loaded>>,
}

impl WhisperEngine {
    pub fn new(model_dir: impl Into<PathBuf>) -> Self {
        Self {
            model_dir: model_dir.into(),
            loaded: Mutex::new(None),
        }
    }

    fn ensure_loaded(&self) -> Result<()> {
        let mut guard = self.loaded.lock().unwrap();
        if guard.is_none() {
            let model = load_whisper_model(&self.model_dir)
                .with_context(|| format!("loading Whisper model from {}", self.model_dir.display()))?;
            let tokenizer = WhisperTokenizer::from_file(&self.model_dir)
                .context("loading Whisper tokenizer.json")?;
            let dtype = model.dtype();
            *guard = Some(Loaded {
                model,
                tokenizer,
                dtype,
            });
        }
        Ok(())
    }

    /// Transcribe an audio file. `language` defaults to Vietnamese; pass e.g.
    /// `Some("en")` to force another language.
    pub fn transcribe_file(&self, path: impl AsRef<Path>, language: Option<&str>) -> Result<String> {
        let pcm = audio::load_audio(path)?;
        self.transcribe_pcm(&pcm, language)
    }

    /// Transcribe already-decoded 16 kHz mono PCM.
    pub fn transcribe_pcm(&self, pcm: &[f32], language: Option<&str>) -> Result<String> {
        Ok(self.transcribe_pcm_timed(pcm, language)?.0)
    }

    /// Transcribe a file, returning text + timing/throughput stats (for benchmarks).
    pub fn transcribe_file_timed(
        &self,
        path: impl AsRef<Path>,
        language: Option<&str>,
    ) -> Result<(String, TranscribeStats)> {
        let pcm = audio::load_audio(path)?;
        self.transcribe_pcm_timed(&pcm, language)
    }

    /// Transcribe PCM, returning text + timing/throughput stats (for benchmarks).
    pub fn transcribe_pcm_timed(
        &self,
        pcm: &[f32],
        language: Option<&str>,
    ) -> Result<(String, TranscribeStats)> {
        self.ensure_loaded()?;
        let mut guard = self.loaded.lock().unwrap();
        let Loaded {
            model,
            tokenizer,
            dtype,
        } = guard.as_mut().unwrap();
        let dtype = *dtype;

        let lang = language.unwrap_or(DEFAULT_LANGUAGE);
        let lang_tok = tokenizer
            .lang_token(lang)
            .with_context(|| format!("unknown Whisper language code `{lang}`"))?;
        let sp = *tokenizer.specials();
        let initial: Vec<i32> = vec![
            sp.sot as i32,
            lang_tok as i32,
            sp.transcribe as i32,
            sp.no_timestamps as i32,
        ];

        let mut stats = TranscribeStats {
            audio_secs: pcm.len() as f32 / audio::SAMPLE_RATE as f32,
            avg_logprob: f32::INFINITY, // worst-case = min; lowered per window
            ..Default::default()
        };
        let t_total = Instant::now();

        // Split into 30 s windows; empty audio still yields one (silent) window.
        let mut out = String::new();
        let total = pcm.len().max(1);
        let mut start = 0usize;
        while start < total {
            let end = (start + N_SAMPLES).min(pcm.len());
            let content = &pcm[start..end.max(start)];
            stats.n_chunks += 1;

            // Energy pre-gate: a near-silent window can't contain speech — skip
            // it without running the model (avoids hallucinating text on silence).
            let peak = content.iter().fold(0f32, |m, &x| m.max(x.abs()));
            if peak < SILENCE_PEAK {
                start += N_SAMPLES;
                continue;
            }

            let mut window = content.to_vec();
            audio::pad_or_trim(&mut window, N_SAMPLES);

            let t_mel = Instant::now();
            let mel = audio::log_mel_spectrogram(&window, audio::N_MELS_LARGE_V3, 0)?;
            let mel_arr =
                Array::from_slice(&mel.data, &[1, mel.n_frames as i32, mel.n_mels as i32])
                    .as_dtype(dtype)?;
            stats.mel_ms += t_mel.elapsed().as_secs_f64() * 1000.0;

            let t_enc = Instant::now();
            let feats = model.encoder.forward(&mel_arr)?;
            feats.eval()?; // force evaluation so the timing is real, not lazy
            stats.encode_ms += t_enc.elapsed().as_secs_f64() * 1000.0;

            let t_dec = Instant::now();
            let (text, tokens, nsp, alp) =
                decode_window(model, tokenizer, &feats, &initial, dtype)?;
            stats.decode_ms += t_dec.elapsed().as_secs_f64() * 1000.0;
            stats.tokens += tokens;
            stats.no_speech_prob = stats.no_speech_prob.max(nsp);
            stats.avg_logprob = stats.avg_logprob.min(alp);

            if !text.is_empty() {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(text.trim());
            }

            start += N_SAMPLES;
        }
        if !stats.avg_logprob.is_finite() {
            stats.avg_logprob = 0.0; // no window decoded (all silence)
        }
        stats.total_ms = t_total.elapsed().as_secs_f64() * 1000.0;
        Ok((out, stats))
    }
}

/// Greedy-decode one audio window.
/// Returns `(text, n_tokens, no_speech_prob, avg_logprob)`.
fn decode_window(
    model: &mut WhisperModel,
    tokenizer: &WhisperTokenizer,
    feats: &Array,
    initial: &[i32],
    dtype: Dtype,
) -> Result<(String, usize, f32, f32)> {
    let (mut self_caches, mut cross_caches) = model.new_caches();
    let eot = tokenizer.specials().eot;
    let n_text_ctx = model.dims.n_text_ctx;
    let max_new = (n_text_ctx / 2) as usize; // Whisper sample_len

    // Prefill the initial prompt tokens.
    let prompt = Array::from_slice(initial, &[1, initial.len() as i32]);
    let mask = maybe_causal_mask(initial.len() as i32, dtype)?;
    let logits = model.decoder.forward(
        &prompt,
        feats,
        0,
        mask.as_ref(),
        &mut self_caches,
        &mut cross_caches,
    )?;

    // No-speech gate: if the model strongly flags silence at the SOT position,
    // emit nothing rather than hallucinating text over silence/noise.
    let ns = tokenizer.specials().no_speech;
    let mut no_speech_prob = 0.0f32;
    if ns != 0 {
        let sot_row = logits.index((0, 0, ..)).as_dtype(Dtype::Float32)?;
        sot_row.eval()?;
        no_speech_prob = softmax_prob(sot_row.as_slice::<f32>(), ns as usize);
        if no_speech_prob > NO_SPEECH_THRESHOLD {
            return Ok((String::new(), 0, no_speech_prob, 0.0));
        }
    }
    let debug = std::env::var("SENCLAW_WHISPER_DEBUG").is_ok();

    let mut row = last_row_f32(&logits)?;
    // First sampled position: forbid <|endoftext|> (suppress_blank-ish) so an
    // empty transcript isn't produced immediately.
    let (mut next, mut next_lp) = pick(&row, tokenizer, false);

    let mut text_ids: Vec<u32> = Vec::new();
    let mut logprob_sum = 0.0f32;
    let mut offset = initial.len() as i32;

    loop {
        if next == eot || text_ids.len() >= max_new || offset >= n_text_ctx {
            break;
        }
        text_ids.push(next);
        logprob_sum += next_lp;

        let inp = Array::from_slice(&[next as i32], &[1, 1]);
        let logits = model.decoder.forward(
            &inp,
            feats,
            offset,
            None,
            &mut self_caches,
            &mut cross_caches,
        )?;
        offset += 1;
        row = last_row_f32(&logits)?;
        let (n, lp) = pick(&row, tokenizer, true);
        next = n;
        next_lp = lp;
    }

    let n = text_ids.len();
    let avg_logprob = if n > 0 { logprob_sum / n as f32 } else { 0.0 };
    if debug {
        let txt = tokenizer.decode(&text_ids).unwrap_or_default();
        eprintln!(
            "[whisper-debug] no_speech_prob={no_speech_prob:.3} avg_logprob={avg_logprob:.3} tokens={n} text={txt:?}"
        );
    }
    // Discard degenerate, very-low-confidence output (repetition loops / garbage
    // run well below this). NOTE: confident common-phrase hallucinations on loud
    // broadband noise (e.g. "thanks for watching") score like real speech and are
    // NOT caught here — that needs a real VAD; the energy gate covers the common
    // case (silence / quiet room tone).
    if n > 0 && avg_logprob < LOGPROB_THRESHOLD {
        return Ok((String::new(), n, no_speech_prob, avg_logprob));
    }
    Ok((
        tokenizer.decode(&text_ids).map_err(anyhow::Error::from)?,
        n,
        no_speech_prob,
        avg_logprob,
    ))
}

/// Softmax probability of index `idx` over a logit row (numerically stable).
fn softmax_prob(row: &[f32], idx: usize) -> f32 {
    let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let sum: f32 = row.iter().map(|&x| (x - max).exp()).sum();
    if sum > 0.0 {
        (row[idx] - max).exp() / sum
    } else {
        0.0
    }
}

/// Pull the last position's logits to CPU as f32.
fn last_row_f32(logits: &Array) -> Result<Vec<f32>> {
    let l = logits.shape()[1];
    let row = logits.index((0, l - 1, ..)).as_dtype(Dtype::Float32)?;
    row.eval()?;
    Ok(row.as_slice::<f32>().to_vec())
}

/// Argmax over `row`, suppressing control tokens. Text tokens (< sot) and, when
/// `allow_eot`, `<|endoftext|>` are eligible; all other specials are skipped.
/// Returns `(token_id, log_probability)` — the logprob (over the full
/// distribution) is accumulated to gate low-confidence hallucinations.
fn pick(row: &[f32], tokenizer: &WhisperTokenizer, allow_eot: bool) -> (u32, f32) {
    let eot = tokenizer.specials().eot;
    let mut best = 0u32;
    let mut best_v = f32::NEG_INFINITY;
    for (i, &v) in row.iter().enumerate() {
        let id = i as u32;
        let suppressed =
            (tokenizer.is_special(id) && id != eot) || (!allow_eot && id == eot);
        if suppressed {
            continue;
        }
        if v > best_v {
            best_v = v;
            best = id;
        }
    }
    // log P(best) = logit[best] - logsumexp(logits), over the full row.
    let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let lse = max + row.iter().map(|&x| (x - max).exp()).sum::<f32>().ln();
    (best, best_v - lse)
}

/// Lowercase, strip punctuation, collapse whitespace → token list. Used by the
/// accuracy test to compare ASR output against the reference text tolerantly.
pub fn normalize_words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(|w| w.to_string())
        .collect()
}

/// Multiset word-recall of `got` against `expected` in [0,1] (1 = every
/// reference word present). Tolerant of word order and extra words.
pub fn word_accuracy(expected: &str, got: &str) -> f64 {
    let exp = normalize_words(expected);
    if exp.is_empty() {
        return 1.0;
    }
    let mut got_counts: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
    for w in normalize_words(got) {
        *got_counts.entry(w).or_default() += 1;
    }
    let mut hits = 0usize;
    for w in &exp {
        if let Some(c) = got_counts.get_mut(w) {
            if *c > 0 {
                *c -= 1;
                hits += 1;
            }
        }
    }
    hits as f64 / exp.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_accuracy_basics() {
        assert!((word_accuracy("Xin chào, thế giới!", "xin chào thế giới") - 1.0).abs() < 1e-9);
        assert!((word_accuracy("a b c d", "a b") - 0.5).abs() < 1e-9);
        assert_eq!(word_accuracy("", "anything"), 1.0);
    }

    /// Silence / quiet room tone must NOT hallucinate text — regression for the
    /// "audio is silence, transcript is an invented YouTube outro" bug. Covers
    /// the realistic recorded-silence cases the energy gate is responsible for:
    /// digital zeros and sub-floor mic noise (~ -50 dBFS). Requires
    /// SENCLAW_WHISPER_DIR.
    ///
    /// (Loud broadband white noise ≥ the energy floor is deliberately NOT tested:
    /// the model produces *confident* common-phrase hallucinations there that
    /// score identically to real speech, so no decoder-side gate can reject them
    /// without a real VAD. That is out of scope for v1.)
    #[test]
    #[ignore = "requires SENCLAW_WHISPER_DIR"]
    fn silence_produces_no_text() {
        let dir = std::env::var("SENCLAW_WHISPER_DIR").expect("SENCLAW_WHISPER_DIR");
        let engine = WhisperEngine::new(dir);

        // 5 s of digital silence.
        let silent = vec![0.0f32; audio::SAMPLE_RATE * 5];
        let t = engine.transcribe_pcm(&silent, Some("vi")).unwrap();
        assert!(t.trim().is_empty(), "digital silence hallucinated: {t:?}");

        // 5 s of sub-floor mic noise (±0.003, below SILENCE_PEAK=0.005).
        let mut seed = 0x1234_5678u32;
        let noise: Vec<f32> = (0..audio::SAMPLE_RATE * 5)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed as f64 / u32::MAX as f64 - 0.5) as f32 * 0.006 // ±0.003
            })
            .collect();
        let t = engine.transcribe_pcm(&noise, Some("vi")).unwrap();
        assert!(t.trim().is_empty(), "quiet room tone hallucinated: {t:?}");
    }

    /// Functional accuracy check: synthesize speech with macOS `say`, transcribe
    /// it, and assert the reference words are recovered. Requires the model dir
    /// + macOS `say`. Run with:
    /// `SENCLAW_WHISPER_DIR=… cargo test --features local-mlx-whisper -- --ignored --test-threads=1 accuracy_check`
    #[test]
    #[ignore = "requires SENCLAW_WHISPER_DIR + macOS `say`"]
    fn accuracy_check() {
        let dir = std::env::var("SENCLAW_WHISPER_DIR").expect("SENCLAW_WHISPER_DIR");
        let engine = WhisperEngine::new(dir);
        let tmp = tempfile::tempdir().unwrap();

        // (lang, say-voice, reference text, min accuracy)
        let cases: &[(&str, &str, &str, f64)] = &[
            (
                "vi",
                "Linh",
                "Xin chào, hôm nay trời rất đẹp và tôi đang thử nghiệm nhận diện giọng nói tiếng Việt.",
                0.8,
            ),
            (
                "en",
                "Samantha",
                "The quick brown fox jumps over the lazy dog.",
                0.8,
            ),
        ];

        for (lang, voice, text, min_acc) in cases {
            let wav = tmp.path().join(format!("{lang}.wav"));
            let ok = std::process::Command::new("say")
                .args(["-v", voice, "-o"])
                .arg(&wav)
                .args(["--data-format=LEI16@16000", text])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ok || !wav.exists() {
                eprintln!("skip {lang}: `say -v {voice}` unavailable");
                continue;
            }
            let got = engine.transcribe_file(&wav, Some(lang)).unwrap();
            let acc = word_accuracy(text, &got);
            println!("[{lang}] acc={acc:.2}\n  expected: {text}\n  got:      {got}");
            assert!(
                acc >= *min_acc,
                "[{lang}] accuracy {acc:.2} < {min_acc} — got: {got}"
            );
        }
    }

    /// End-to-end transcription on a real file. Run with:
    /// `SENCLAW_WHISPER_DIR=… SENCLAW_WHISPER_AUDIO=/tmp/wt_vi.wav SENCLAW_WHISPER_LANG=vi \
    ///   cargo test --features local-mlx-whisper -- --ignored --test-threads=1 e2e_transcribe`
    #[test]
    #[ignore = "requires SENCLAW_WHISPER_DIR + SENCLAW_WHISPER_AUDIO"]
    fn e2e_transcribe() {
        let dir = std::env::var("SENCLAW_WHISPER_DIR").expect("SENCLAW_WHISPER_DIR");
        let audio = std::env::var("SENCLAW_WHISPER_AUDIO").expect("SENCLAW_WHISPER_AUDIO");
        let lang = std::env::var("SENCLAW_WHISPER_LANG").ok();
        let engine = WhisperEngine::new(dir);
        let text = engine.transcribe_file(&audio, lang.as_deref()).unwrap();
        println!("\n=== TRANSCRIPT ===\n{text}\n==================");
        assert!(!text.trim().is_empty(), "empty transcript");
    }
}
