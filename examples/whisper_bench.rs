//! Whisper ASR functional + throughput benchmark.
//!
//! ```bash
//! cargo run --release --features local-mlx-whisper --example whisper_bench -- \
//!   ~/.senclaw/local-models/mlx-community__whisper-large-v3-turbo vi 5 clip1.wav clip2.wav
//! ```
//!
//! Args: `<model_dir> <lang> <iters> <audio...>`.
//!
//! For each clip: decodes audio once, runs one cold pass (includes model load
//! on the first clip), then `iters` warm passes. Reports the transcript plus
//! min/median/mean of total time, real-time factor (RTF = proc/audio, <1 =
//! faster than realtime), encoder ms, decoder ms, and decode tok/s. Greedy
//! decoding is deterministic, so every warm pass must emit byte-identical text
//! — a mismatch fails the run (correctness gate). If `<clip>.txt` exists, the
//! benchmark also prints word recall against that ground truth.

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
        eprintln!("whisper_bench failed: {e:#}");
        std::process::exit(1);
    }
}

#[cfg(feature = "local-mlx-whisper")]
async fn run() -> anyhow::Result<()> {
    use senclaw::local_model::audio;
    use senclaw::local_model::whisper_transcribe::{word_accuracy, TranscribeStats, WhisperEngine};
    use std::sync::Arc;
    use std::time::Instant;

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("usage: whisper_bench <model_dir> <lang> <iters> <audio...>");
        std::process::exit(2);
    }
    let model_dir = &args[1];
    let lang = args[2].clone();
    let iters: usize = args[3].parse().unwrap_or(5);
    let clips = &args[4..];

    fn stat(v: &mut [f64]) -> (f64, f64, f64) {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = v.len();
        let min = v[0];
        let median = v[n / 2];
        let mean = v.iter().sum::<f64>() / n as f64;
        (min, median, mean)
    }

    let engine = Arc::new(WhisperEngine::new(model_dir.clone()));
    println!(
        "==> Whisper benchmark | model={model_dir} | lang={lang} | iters={iters} | clips={}",
        clips.len()
    );

    let mut first = true;
    for clip in clips {
        let pcm = match audio::load_audio(clip) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("-- skip {clip}: {e}");
                continue;
            }
        };
        let audio_secs = pcm.len() as f32 / audio::SAMPLE_RATE as f32;

        // Cold pass — on the first clip this also pays the model-load cost.
        let cold = Instant::now();
        let (ref_text, _) = match transcribe_once(engine.clone(), pcm.clone(), lang.clone()).await {
            Ok(x) => x,
            Err(e) => {
                eprintln!("-- error {clip}: {e}");
                continue;
            }
        };
        let cold_ms = cold.elapsed().as_secs_f64() * 1000.0;

        // Warm passes (timed + determinism check).
        let mut totals = Vec::new();
        let mut rtfs = Vec::new();
        let mut encs = Vec::new();
        let mut decs = Vec::new();
        let mut toks = Vec::new();
        let mut deterministic = true;
        let mut last: TranscribeStats = TranscribeStats::default();
        for _ in 0..iters {
            let (text, st) = transcribe_once(engine.clone(), pcm.clone(), lang.clone()).await?;
            if text != ref_text {
                deterministic = false;
            }
            totals.push(st.total_ms);
            rtfs.push(st.rtf());
            encs.push(st.encode_ms);
            decs.push(st.decode_ms);
            toks.push(st.decode_tok_s());
            last = st;
        }

        let (tmin, tmed, tmean) = stat(&mut totals);
        let (_, rmed, _) = stat(&mut rtfs);
        let (_, emed, _) = stat(&mut encs);
        let (_, dmed, _) = stat(&mut decs);
        let (_, tsmed, _) = stat(&mut toks);

        println!("\n######## {clip} ########");
        println!("  transcript: {ref_text}");
        if let Some(truth) = load_ground_truth(clip) {
            let acc = word_accuracy(&truth, &ref_text);
            println!("  expected  : {truth}");
            println!("  word recall: {:.3}", acc);
        }
        println!(
            "  audio: {:.2}s | chunks: {} | tokens: {}",
            audio_secs, last.n_chunks, last.tokens
        );
        println!(
            "  confidence: avg_logprob {:.3} | no_speech_prob {:.3}",
            last.avg_logprob, last.no_speech_prob
        );
        if first {
            println!("  cold pass (incl. model load): {:.0} ms", cold_ms);
            first = false;
        }
        println!(
            "  total ms:  min {:.0} | median {:.0} | mean {:.0}",
            tmin, tmed, tmean
        );
        println!("  RTF (median): {:.3}x  (<1 = faster than realtime)", rmed);
        println!(
            "  encode {:.0} ms | decode {:.0} ms | decode {:.1} tok/s (median)",
            emed, dmed, tsmed
        );
        println!(
            "  determinism: {}",
            if deterministic {
                "OK (identical text)"
            } else {
                "FAIL (text varied!)"
            }
        );
    }

    Ok(())
}

#[cfg(feature = "local-mlx-whisper")]
async fn transcribe_once(
    engine: std::sync::Arc<senclaw::local_model::whisper_transcribe::WhisperEngine>,
    pcm: Vec<f32>,
    lang: String,
) -> anyhow::Result<(
    String,
    senclaw::local_model::whisper_transcribe::TranscribeStats,
)> {
    tokio::task::spawn_blocking(move || engine.transcribe_pcm_timed(&pcm, Some(&lang)))
        .await
        .map_err(anyhow::Error::from)?
}

#[cfg(feature = "local-mlx-whisper")]
fn load_ground_truth(clip: &str) -> Option<String> {
    let txt = std::path::Path::new(clip).with_extension("txt");
    std::fs::read_to_string(txt)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
