//! TTS round-trip test + benchmark — synthesize, then verify with Whisper STT.
//!
//! Drives the production `gateway::ui_server::tts::synthesize_blocking` (the
//! same function the HTTP `/api/tts/synthesize` route calls), pipes the WAV
//! back through the on-device Whisper engine, and reports word-recall vs. the
//! original prompt. Use it to confirm whether a TTS model (macOS native or a
//! HuggingFace mlx-audio model) actually produces intelligible speech.
//!
//! ```bash
//! # macOS native (always installed):
//! cargo run --release --features local-mlx-whisper --example tts_roundtrip -- \
//!   --tts macos-speech \
//!   --whisper ~/.senclaw/local-models/mlx-community__whisper-large-v3-turbo-4bit \
//!   --lang vi --iters 3
//!
//! # HuggingFace MLX-Audio model (must be installed under
//! # ~/.senclaw/tts-models/<org>__<repo>/ with config.json + weights):
//! cargo run --release --features local-mlx-whisper --example tts_roundtrip -- \
//!   --tts mlx-community/zipvoice-vietnamese \
//!   --whisper ~/.senclaw/local-models/mlx-community__whisper-large-v3-turbo-4bit \
//!   --lang vi --iters 3
//! ```
//!
//! Optional: `--phrase "..."` (repeatable) overrides the built-in Vietnamese
//! phrase set; `--save-wav <dir>` keeps the generated WAVs for inspection.

#[cfg(not(feature = "local-mlx-whisper"))]
fn main() {
    eprintln!("build with --features local-mlx-whisper");
}

#[cfg(feature = "local-mlx-whisper")]
fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("create tokio runtime");
    if let Err(e) = rt.block_on(run()) {
        eprintln!("tts_roundtrip failed: {e:#}");
        std::process::exit(1);
    }
}

#[cfg(feature = "local-mlx-whisper")]
async fn run() -> anyhow::Result<()> {
    use anyhow::{anyhow, Context};
    use senclaw::tts::synthesize_with_fallback;
    use senclaw::local_model::audio;
    use senclaw::local_model::whisper_transcribe::{word_accuracy, WhisperEngine};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Instant;

    // ── Args parsing (tiny ad-hoc; avoid pulling clap into examples) ─────────
    let argv: Vec<String> = std::env::args().collect();
    let mut tts_id: Option<String> = None;
    let mut whisper_dir: Option<PathBuf> = None;
    let mut lang = "vi".to_string();
    let mut iters: usize = 1;
    let mut voice: Option<String> = None;
    let mut speed: f32 = 1.0;
    let mut phrases: Vec<String> = Vec::new();
    let mut save_dir: Option<PathBuf> = None;
    let mut threshold: f64 = 0.6;

    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].as_str();
        let next = || -> anyhow::Result<String> {
            argv.get(i + 1)
                .cloned()
                .ok_or_else(|| anyhow!("missing value for {a}"))
        };
        match a {
            "--tts" => {
                tts_id = Some(next()?);
                i += 2;
            }
            "--whisper" => {
                whisper_dir = Some(PathBuf::from(next()?));
                i += 2;
            }
            "--lang" => {
                lang = next()?;
                i += 2;
            }
            "--iters" => {
                iters = next()?.parse().context("--iters")?;
                i += 2;
            }
            "--voice" => {
                voice = Some(next()?);
                i += 2;
            }
            "--speed" => {
                speed = next()?.parse().context("--speed")?;
                i += 2;
            }
            "--phrase" => {
                phrases.push(next()?);
                i += 2;
            }
            "--save-wav" => {
                save_dir = Some(PathBuf::from(next()?));
                i += 2;
            }
            "--threshold" => {
                threshold = next()?.parse().context("--threshold")?;
                i += 2;
            }
            "-h" | "--help" => {
                usage();
                return Ok(());
            }
            other => return Err(anyhow!("unknown arg: {other}")),
        }
    }

    let tts_id = tts_id.ok_or_else(|| anyhow!("--tts is required (e.g. macos-speech)"))?;
    let whisper_dir = whisper_dir
        .ok_or_else(|| anyhow!("--whisper <dir> is required (path to Whisper model)"))?;

    if phrases.is_empty() {
        phrases = default_phrases(&lang);
    }
    if let Some(dir) = &save_dir {
        std::fs::create_dir_all(dir).ok();
    }

    // Resolve TTS model directory (None for the macOS system voice).
    let tts_model_path: Option<PathBuf> = if tts_id == "macos-speech" {
        None
    } else {
        let p = PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".senclaw/tts-models")
            .join(tts_id.replace('/', "__"));
        Some(p)
    };

    println!("════════════════════════════════════════════════════════════════");
    println!(" TTS round-trip (synthesize → Whisper STT → word recall)");
    println!("════════════════════════════════════════════════════════════════");
    println!("  tts model : {tts_id}");
    if let Some(p) = &tts_model_path {
        println!("  tts dir   : {}", p.display());
    }
    println!("  whisper   : {}", whisper_dir.display());
    println!("  language  : {lang}");
    println!("  voice     : {}", voice.as_deref().unwrap_or("(default)"));
    println!("  speed     : {speed}");
    println!("  iters     : {iters} (per phrase)");
    println!("  threshold : {:.0}% word recall to pass", threshold * 100.0);
    println!("  phrases   : {}", phrases.len());
    println!("════════════════════════════════════════════════════════════════");
    println!();

    let engine = Arc::new(WhisperEngine::new(whisper_dir.clone()));

    struct PhraseResult {
        idx: usize,
        text: String,
        wav_bytes: usize,
        audio_secs: f32,
        synth_ms_med: f64,
        stt_ms_med: f64,
        rtf_med: f64,
        transcript: String,
        recall: f64,
        deterministic: bool,
    }
    let mut results: Vec<PhraseResult> = Vec::new();
    let mut synthesis_ok = true;

    for (idx, phrase) in phrases.iter().enumerate() {
        println!("┌─ phrase {} ────────────────────────────────────────────────", idx + 1);
        println!("│  expected: {phrase}");

        // ── 1. Synthesize (with retries to measure stability) ────────────────
        //
        // Uses the same `synthesize_with_fallback` the HTTP handler uses, so
        // the benchmark mirrors what the UI sees: if the requested backend
        // is still under construction, we transparently fall back to
        // macos-speech and report which backend produced the audio.
        let mut synth_times = Vec::new();
        let mut wav: Vec<u8> = Vec::new();
        let mut synth_err: Option<String> = None;
        let mut used_backend = String::new();
        let mut fallback_note: Option<String> = None;
        for _ in 0..iters.max(1) {
            let t0 = Instant::now();
            let r = synthesize_with_fallback(
                &tts_id,
                tts_model_path.as_deref(),
                phrase,
                &lang,
                voice.as_deref(),
                speed,
            );
            let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
            match r {
                Ok(outcome) => {
                    synth_times.push(elapsed);
                    wav = outcome.wav;
                    used_backend = outcome.used_backend;
                    fallback_note = outcome.fallback_reason;
                }
                Err((code, msg)) => {
                    synth_err = Some(format!("HTTP {code}: {msg}"));
                    break;
                }
            }
        }

        if let Some(err) = synth_err {
            println!("│  synth   : ✗ FAILED — {err}");
            println!("└──────────────────────────────────────────────────────────");
            println!();
            synthesis_ok = false;
            continue;
        }
        if !used_backend.is_empty() && used_backend != tts_id {
            println!("│  backend : {used_backend} (fallback)");
            if let Some(reason) = &fallback_note {
                println!("│  reason  : {reason}");
            }
        }
        if wav.is_empty() {
            println!("│  synth   : ✗ FAILED — empty WAV");
            println!("└──────────────────────────────────────────────────────────");
            println!();
            synthesis_ok = false;
            continue;
        }

        if let Some(dir) = &save_dir {
            let path = dir.join(format!("phrase_{:02}.wav", idx + 1));
            std::fs::write(&path, &wav).ok();
            println!("│  saved   : {}", path.display());
        }

        // ── 2. Decode the WAV through symphonia → 16 kHz mono PCM ────────────
        let tmp_path = std::env::temp_dir().join(format!(
            "senclaw-tts-roundtrip-{}-{}.wav",
            std::process::id(),
            idx
        ));
        std::fs::write(&tmp_path, &wav).context("writing temp wav")?;
        let pcm = match audio::load_audio(&tmp_path) {
            Ok(p) => p,
            Err(e) => {
                println!("│  decode  : ✗ FAILED — {e}");
                println!("└──────────────────────────────────────────────────────────");
                println!();
                let _ = std::fs::remove_file(&tmp_path);
                synthesis_ok = false;
                continue;
            }
        };
        let _ = std::fs::remove_file(&tmp_path);
        let audio_secs = pcm.len() as f32 / audio::SAMPLE_RATE as f32;

        // ── 3. Transcribe with Whisper (iters passes for stability) ─────────
        let mut stt_times = Vec::new();
        let mut transcripts: Vec<String> = Vec::new();
        for _ in 0..iters.max(1) {
            let engine = engine.clone();
            let pcm_clone = pcm.clone();
            let lang_clone = lang.clone();
            let (text, st) = tokio::task::spawn_blocking(move || {
                engine.transcribe_pcm_timed(&pcm_clone, Some(&lang_clone))
            })
            .await??;
            stt_times.push(st.total_ms);
            transcripts.push(text);
        }
        let deterministic = transcripts.windows(2).all(|w| w[0] == w[1]);
        let transcript = transcripts[0].clone();
        let recall = word_accuracy(phrase, &transcript);

        let synth_med = median(&mut synth_times);
        let stt_med = median(&mut stt_times);
        let rtf = if audio_secs > 0.0 {
            stt_med / 1000.0 / audio_secs as f64
        } else {
            0.0
        };

        println!("│  got     : {transcript}");
        println!(
            "│  recall  : {:.1}%   wav={} bytes   audio={:.2}s",
            recall * 100.0,
            wav.len(),
            audio_secs
        );
        println!(
            "│  timing  : synth median {synth_med:.0} ms   stt median {stt_med:.0} ms   stt RTF {rtf:.3}×"
        );
        println!(
            "│  verdict : {} {}",
            if recall >= threshold { "✓ PASS" } else { "✗ FAIL" },
            if deterministic {
                "(stt deterministic)"
            } else {
                "(stt VARIED — non-deterministic)"
            }
        );
        println!("└──────────────────────────────────────────────────────────");
        println!();

        results.push(PhraseResult {
            idx,
            text: phrase.clone(),
            wav_bytes: wav.len(),
            audio_secs,
            synth_ms_med: synth_med,
            stt_ms_med: stt_med,
            rtf_med: rtf,
            transcript,
            recall,
            deterministic,
        });
    }

    // ── Summary ──────────────────────────────────────────────────────────────
    println!("════════════════════════════════════════════════════════════════");
    println!(" Summary");
    println!("════════════════════════════════════════════════════════════════");
    if !synthesis_ok && results.is_empty() {
        println!("  ✗ All syntheses failed — TTS pipeline is non-functional for `{tts_id}`.");
        std::process::exit(2);
    }
    let n = results.len();
    let passed = results.iter().filter(|r| r.recall >= threshold).count();
    let mean_recall: f64 = results.iter().map(|r| r.recall).sum::<f64>() / n as f64;
    let mean_synth: f64 = results.iter().map(|r| r.synth_ms_med).sum::<f64>() / n as f64;
    let mean_stt: f64 = results.iter().map(|r| r.stt_ms_med).sum::<f64>() / n as f64;
    let mean_rtf: f64 = results.iter().map(|r| r.rtf_med).sum::<f64>() / n as f64;

    println!(
        "  {passed}/{n} phrases passed (≥ {:.0}% word recall)",
        threshold * 100.0
    );
    println!("  mean recall : {:.1}%", mean_recall * 100.0);
    println!("  mean synth  : {mean_synth:.0} ms");
    println!("  mean stt    : {mean_stt:.0} ms   (RTF {mean_rtf:.3}×)");
    println!();
    println!(
        "  {:<3} {:<8} {:>8} {:>9} {:>9} {:>7}  {}",
        "#", "recall", "wav(KB)", "audio(s)", "synth(ms)", "rtf", "transcript (first chars)"
    );
    for r in &results {
        let head: String = r.transcript.chars().take(50).collect();
        println!(
            "  {:<3} {:<7.1}% {:>8} {:>9.2} {:>9.0} {:>7.2}  {}",
            r.idx + 1,
            r.recall * 100.0,
            r.wav_bytes / 1024,
            r.audio_secs,
            r.synth_ms_med,
            r.rtf_med,
            head,
        );
    }
    println!();

    // Exit code: 0 if all passed, 3 if some failed (CI-friendly).
    if passed < n || !synthesis_ok {
        std::process::exit(3);
    }
    Ok(())
}

#[cfg(feature = "local-mlx-whisper")]
fn usage() {
    eprintln!(
        "usage: tts_roundtrip --tts <id> --whisper <dir> [--lang vi] [--iters N]\n\
         \t[--voice <name>] [--speed 1.0] [--phrase \"...\"]* [--save-wav <dir>] [--threshold 0.6]\n\n\
         <id> is either `macos-speech` or a HuggingFace `org/repo` whose weights live\n\
         under ~/.senclaw/tts-models/<org>__<repo>/."
    );
}

#[cfg(feature = "local-mlx-whisper")]
fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

#[cfg(feature = "local-mlx-whisper")]
fn default_phrases(lang: &str) -> Vec<String> {
    match lang {
        "vi" => vec![
            "Xin chào, hôm nay là một ngày đẹp trời.".to_string(),
            "Tôi tên là Sen Claw, rất vui được gặp bạn.".to_string(),
            "Hà Nội là thủ đô của nước Việt Nam.".to_string(),
            "Trí tuệ nhân tạo đang thay đổi cách chúng ta làm việc.".to_string(),
        ],
        _ => vec![
            "Hello, today is a beautiful day.".to_string(),
            "My name is Sen Claw, nice to meet you.".to_string(),
            "Artificial intelligence is changing the way we work.".to_string(),
        ],
    }
}
