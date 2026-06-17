//! Whisper ASR functional + throughput + memory benchmark.
//!
//! ```bash
//! cargo run --release --features local-mlx-whisper --example whisper_bench -- \
//!   ~/.senclaw/whisper-models/mlx-community__whisper-large-v3-turbo vi 5 clip1.wav clip2.wav
//! ```
//!
//! Args: `<model_dir> <lang> <iters> <audio..>`.
//!
//! For each clip: decodes audio once, runs one cold pass (includes model load
//! on the first clip), then `iters` warm passes. Reports the transcript plus
//! min/median/mean of total time, real-time factor (RTF = proc/audio, <1 =
//! faster than realtime), encoder ms, decoder ms, decode tok/s, peak RAM (RSS),
//! peak MLX active memory, peak MLX cache memory, and CPU user+sys time.
//!
//! Greedy decoding is deterministic, so every warm pass must emit byte-identical
//! text — a mismatch fails the run (correctness gate). If `<clip>.txt` exists,
//! the benchmark also prints word recall against that ground truth.
//!
//! ## Environment variables
//! - `SENCLAW_WHISPER_DEBUG=1`       — verbose per-chunk debug output
//! - `SENCLAW_WHISPER_MLX_CACHE_MB=N` — cap the MLX Metal cache to N MiB

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
        eprintln!("usage: whisper_bench <model_dir> <lang> <iters> <audio..>");
        eprintln!();
        eprintln!("env:");
        eprintln!("  SENCLAW_WHISPER_DEBUG=1           verbose per-chunk logging");
        eprintln!("  SENCLAW_WHISPER_MLX_CACHE_MB=N    cap MLX Metal cache to N MiB");
        std::process::exit(2);
    }
    let model_dir = &args[1];
    let lang = args[2].clone();
    let iters: usize = args[3].parse().unwrap_or(5);
    let clips = &args[4..];

    // ── helper: (min, median, mean) of a sorted vec ──────────────────────────
    fn stat(v: &mut [f64]) -> (f64, f64, f64) {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = v.len();
        let min = v[0];
        let median = v[n / 2];
        let mean = v.iter().sum::<f64>() / n as f64;
        (min, median, mean)
    }

    fn stat_f32(v: &mut [f32]) -> (f32, f32, f32) {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = v.len();
        let min = v[0];
        let median = v[n / 2];
        let mean = v.iter().sum::<f32>() / n as f32;
        (min, median, mean)
    }

    // ── system info header ────────────────────────────────────────────────────
    println!("════════════════════════════════════════════════════════════════");
    println!(" Whisper Benchmark");
    println!("════════════════════════════════════════════════════════════════");
    println!("  model   : {model_dir}");
    println!("  language: {lang}");
    println!("  iters   : {iters}  (warm passes per clip)");
    println!("  clips   : {}", clips.len());

    // OS / hardware info
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
        {
            let cpu = String::from_utf8_lossy(&out.stdout);
            println!("  cpu     : {}", cpu.trim());
        }
        if let Ok(out) = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
        {
            let bytes: u64 = String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse()
                .unwrap_or(0);
            println!("  ram     : {:.0} GiB", bytes as f64 / (1 << 30) as f64);
        }
    }
    println!(
        "  os      : {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    if let Ok(mb) = std::env::var("SENCLAW_WHISPER_MLX_CACHE_MB") {
        println!("  mlx-cap : {mb} MiB  (SENCLAW_WHISPER_MLX_CACHE_MB)");
    }
    println!("════════════════════════════════════════════════════════════════");
    println!();

    let engine = Arc::new(WhisperEngine::new(model_dir.clone()));

    // ── summary table (filled at the end) ────────────────────────────────────
    struct ClipSummary {
        name: String,
        audio_secs: f32,
        rtf_med: f64,
        peak_ram_mb_med: f32,
        peak_mlx_mb_med: f32,
        peak_mlx_cache_mb_med: f32,
        cpu_user_ms_med: f64,
        cpu_sys_ms_med: f64,
        word_recall: Option<f64>,
        deterministic: bool,
    }
    let mut summaries: Vec<ClipSummary> = Vec::new();

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
        let (ref_text, cold_stats) =
            match transcribe_once(engine.clone(), pcm.clone(), lang.clone()).await {
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
        let mut peak_rams: Vec<f32> = Vec::new();
        let mut peak_mlxs: Vec<f32> = Vec::new();
        let mut peak_caches: Vec<f32> = Vec::new();
        let mut cpu_users: Vec<f64> = Vec::new();
        let mut cpu_syss: Vec<f64> = Vec::new();
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
            peak_rams.push(st.peak_ram_mb);
            peak_mlxs.push(st.peak_mlx_mb);
            peak_caches.push(st.peak_mlx_cache_mb);
            cpu_users.push(st.cpu_user_ms);
            cpu_syss.push(st.cpu_sys_ms);
            last = st;
        }

        let (tmin, tmed, tmean) = stat(&mut totals);
        let (_, rmed, _) = stat(&mut rtfs);
        let (_, emed, _) = stat(&mut encs);
        let (_, dmed, _) = stat(&mut decs);
        let (_, tsmed, _) = stat(&mut toks);
        let (_, ram_med, ram_max) = stat_f32(&mut peak_rams);
        let (_, mlx_med, mlx_max) = stat_f32(&mut peak_mlxs);
        let (_, cache_med, cache_max) = stat_f32(&mut peak_caches);
        let (_, user_med, _) = stat(&mut cpu_users);
        let (_, sys_med, _) = stat(&mut cpu_syss);

        let ground_truth = load_ground_truth(clip);
        let word_recall = ground_truth.as_deref().map(|t| word_accuracy(t, &ref_text));

        println!("┌─ {clip} ─────────────────────────────────────────────────");
        println!("│  transcript : {ref_text}");
        if let Some(ref truth) = ground_truth {
            println!("│  expected   : {truth}");
        }
        if let Some(acc) = word_recall {
            println!("│  word recall: {:.1}%", acc * 100.0);
        }
        println!(
            "│  audio: {audio_secs:.2} s | chunks: {} | tokens: {}",
            last.n_chunks, last.tokens
        );
        println!(
            "│  confidence: avg_logprob {:.3} | no_speech_prob {:.3}",
            last.avg_logprob, last.no_speech_prob
        );
        if first {
            println!(
                "│  cold pass (incl. model load): {cold_ms:.0} ms  (ram={:.0} MiB, mlx={:.0} MiB)",
                cold_stats.peak_ram_mb, cold_stats.peak_mlx_mb
            );
            first = false;
        }
        println!("│");
        println!("│  ── Timing (warm, {iters} passes) ─────────────────────────");
        println!("│    total ms : min {tmin:.0}  median {tmed:.0}  mean {tmean:.0}");
        println!("│    RTF      : {rmed:.3}×  (< 1 = faster than realtime)");
        println!("│    encode   : {emed:.0} ms  decode: {dmed:.0} ms  tok/s: {tsmed:.1} (median)");
        println!("│");
        println!("│  ── Memory (warm, {iters} passes) ─────────────────────────");
        println!("│    peak RAM (RSS)   : median {ram_med:.0} MiB  max {ram_max:.0} MiB");
        println!("│    peak MLX active  : median {mlx_med:.0} MiB  max {mlx_max:.0} MiB");
        println!("│    peak MLX cache   : median {cache_med:.0} MiB  max {cache_max:.0} MiB");
        if mlx_med > 0.0 && audio_secs > 0.0 {
            println!(
                "│    MLX efficiency   : {:.2} MiB / audio-sec",
                mlx_med / audio_secs
            );
        }
        println!("│");
        println!("│  ── CPU (warm, {iters} passes) ─────────────────────────────");
        println!("│    user: {user_med:.0} ms  sys: {sys_med:.0} ms  (median getrusage delta)");
        if last.total_ms > 0.0 {
            let cpu_ratio = (user_med + sys_med) / last.total_ms * 100.0;
            println!("│    CPU-to-wall: {cpu_ratio:.1}%  (> 100 % = multi-core)");
        }
        println!(
            "│  determinism: {}",
            if deterministic {
                "✓ OK (identical text)"
            } else {
                "✗ FAIL (text varied!)"
            }
        );
        println!("└──────────────────────────────────────────────────────────");
        println!();

        summaries.push(ClipSummary {
            name: clip.to_string(),
            audio_secs,
            rtf_med: rmed,
            peak_ram_mb_med: ram_med,
            peak_mlx_mb_med: mlx_med,
            peak_mlx_cache_mb_med: cache_med,
            cpu_user_ms_med: user_med,
            cpu_sys_ms_med: sys_med,
            word_recall,
            deterministic,
        });
    }

    // ── Summary table ─────────────────────────────────────────────────────────
    if summaries.len() > 1 {
        println!("════════════════════════════════════════════════════════════════");
        println!(" Summary");
        println!("════════════════════════════════════════════════════════════════");
        println!(
            "{:<30} {:>6} {:>6} {:>9} {:>9} {:>9} {:>7} {:>7} {:>7} {:>5}",
            "clip",
            "secs",
            "RTF",
            "RAM(MiB)",
            "MLX(MiB)",
            "Cache(MiB)",
            "CPUu(ms)",
            "CPUs(ms)",
            "recall",
            "det?"
        );
        println!("{}", "─".repeat(102));
        for s in &summaries {
            let recall_str = s
                .word_recall
                .map(|r| format!("{:.1}%", r * 100.0))
                .unwrap_or_else(|| "  n/a".into());
            let det_str = if s.deterministic { "✓" } else { "✗" };
            let name = if s.name.len() > 30 {
                format!("…{}", &s.name[s.name.len() - 29..])
            } else {
                s.name.clone()
            };
            println!(
                "{:<30} {:>6.1} {:>6.3} {:>9.0} {:>9.0} {:>9.0} {:>7.0} {:>7.0} {:>7} {:>5}",
                name,
                s.audio_secs,
                s.rtf_med,
                s.peak_ram_mb_med,
                s.peak_mlx_mb_med,
                s.peak_mlx_cache_mb_med,
                s.cpu_user_ms_med,
                s.cpu_sys_ms_med,
                recall_str,
                det_str,
            );
        }
        println!();
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
