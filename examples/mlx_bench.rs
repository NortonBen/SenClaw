//! Decode-throughput benchmark for the native MLX engine.
//!
//! ```bash
//! cargo run --release --features local-mlx --example mlx_bench -- \
//!   ~/.senclaw/local-models/Qwen__Qwen3-0.6B Qwen/Qwen3-0.6B 5
//! ```
//!
//! Args: `<model_dir> [model_id] [iters]`. Generation params (temperature,
//! repetition_penalty, max_new_tokens, …) come from `<model_dir>/../settings.json`
//! — point at an isolated dir to control them (see `scripts/mlx_bench.sh`).
//!
//! Runs one warm-up turn (uncounted) then `iters` timed turns on a fixed prompt.
//! Decode tok/s is captured from the engine's own `[perf]` tracing line; the
//! example reports min / median / mean / max plus a determinism check (every
//! timed turn must emit byte-identical text — the correctness gate for greedy
//! decoding). Set `MLX_BENCH_OUT=<path>` to also dump each turn's text to
//! `<path>.run<N>`.

#[cfg(not(feature = "local-mlx"))]
fn main() {
    eprintln!("build with --features local-mlx");
}

#[cfg(feature = "local-mlx")]
mod perf_sink {
    use std::sync::{Arc, Mutex};
    use tracing::field::{Field, Visit};
    use tracing_subscriber::layer::{Context, Layer};

    /// Collects per-turn stats parsed from each `[local-mlx-native][perf]`
    /// turn-done line, in order.
    #[derive(Default)]
    pub struct PerfSink {
        pub decode_tok_s: Mutex<Vec<f64>>,
        pub prefill_tok_s: Mutex<Vec<f64>>,
        pub prompt_tokens: Mutex<Vec<u64>>,
    }

    impl PerfSink {
        pub fn clear(&self) {
            self.decode_tok_s.lock().unwrap().clear();
            self.prefill_tok_s.lock().unwrap().clear();
            self.prompt_tokens.lock().unwrap().clear();
        }
    }

    /// Local newtype so we can `impl Layer` without tripping the orphan rule
    /// (a foreign trait can't be implemented for `Arc<PerfSink>` directly).
    pub struct PerfLayer(pub Arc<PerfSink>);

    struct MsgVisitor(Option<String>);
    impl Visit for MsgVisitor {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.0 = Some(format!("{value:?}"));
            }
        }
    }

    /// "(137.3 tok/s)" → 137.3
    fn tok_s(seg: &str) -> Option<f64> {
        let o = seg.find('(')?;
        let c = seg[o..].find(" tok/s)")? + o;
        seg[o + 1..c].trim().parse::<f64>().ok()
    }

    /// Parse a "… prefill 67 tok / 0.49 s (137 tok/s) | decode 400 tok / 5.2 s
    /// (76.5 tok/s)" line into (prompt_tokens, prefill_tok_s, decode_tok_s).
    fn parse_perf(msg: &str) -> Option<(u64, f64, f64)> {
        let after = msg.split("prefill").nth(1)?; // " 67 tok / … | decode …"
        let prompt_tokens = after.trim_start().split(' ').next()?.parse::<u64>().ok()?;
        let prefill_seg = after.split("| decode").next()?;
        let decode_seg = after.split("| decode").nth(1)?;
        Some((prompt_tokens, tok_s(prefill_seg)?, tok_s(decode_seg)?))
    }

    impl<S: tracing::Subscriber> Layer<S> for PerfLayer {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            let mut v = MsgVisitor(None);
            event.record(&mut v);
            if let Some(m) = v.0 {
                if m.contains("[perf]") {
                    if let Some((pt, pre, dec)) = parse_perf(&m) {
                        self.0.prompt_tokens.lock().unwrap().push(pt);
                        self.0.prefill_tok_s.lock().unwrap().push(pre);
                        self.0.decode_tok_s.lock().unwrap().push(dec);
                    }
                }
            }
        }
    }
}

/// Process resident set size (MiB) — total RAM held by the bench process
/// (weights + KV + MLX pool + everything). Mirrors the engine's own probe.
#[cfg(all(feature = "local-mlx", target_os = "macos"))]
fn rss_mib() -> f64 {
    unsafe {
        #[repr(C)]
        struct MachTaskBasicInfo {
            virtual_size: u64,
            resident_size: u64,
            resident_size_max: u64,
            user_time: libc::time_value_t,
            system_time: libc::time_value_t,
            policy: i32,
            suspend_count: i32,
        }
        const MACH_TASK_BASIC_INFO: libc::c_int = 20;
        const COUNT: u32 = 12;
        let mut info: MachTaskBasicInfo = std::mem::zeroed();
        let mut count = COUNT;
        let kr = libc::task_info(
            libc::mach_task_self(),
            MACH_TASK_BASIC_INFO as _,
            &mut info as *mut _ as _,
            &mut count,
        );
        if kr == 0 {
            info.resident_size as f64 / (1024.0 * 1024.0)
        } else {
            0.0
        }
    }
}
#[cfg(all(feature = "local-mlx", not(target_os = "macos")))]
fn rss_mib() -> f64 {
    0.0
}

/// MLX Metal allocator stats (active, cache, peak) in MiB.
#[cfg(feature = "local-mlx")]
fn mlx_mem_mib() -> (f64, f64, f64) {
    unsafe {
        let (mut a, mut c, mut p) = (0usize, 0usize, 0usize);
        mlx_sys::mlx_get_active_memory(&mut a);
        mlx_sys::mlx_get_cache_memory(&mut c);
        mlx_sys::mlx_get_peak_memory(&mut p);
        const M: f64 = 1024.0 * 1024.0;
        (a as f64 / M, c as f64 / M, p as f64 / M)
    }
}

/// Memory-growth mode: fire `n` sequential requests with *distinct* prompts
/// (so the prefix cache can't simply replay one conversation) and report how
/// RSS and the MLX allocator pool move across requests. A flat trajectory after
/// the first couple of requests = bounded; a steady climb = a leak / unbounded
/// cache. Run after warm-up so weights are already resident in the baseline.
#[cfg(feature = "local-mlx")]
async fn mem_mode(
    engine: &senclaw::local_model::MlxNativeEngine,
    n: usize,
) -> anyhow::Result<()> {
    use std::time::Instant;
    use tokio::sync::mpsc;

    const TOPICS: [&str; 10] = [
        "the Roman Empire's road network",
        "how photosynthesis converts light to sugar",
        "quantum entanglement and Bell inequalities",
        "the causes of the French Revolution",
        "how black holes bend spacetime",
        "the adaptive immune system",
        "blockchain consensus algorithms",
        "plate tectonics and continental drift",
        "the Krebs cycle in cellular respiration",
        "how neural machine translation works",
    ];

    // Optional machine-readable trace for offline study: `MLX_BENCH_MEM_CSV=path`.
    let mut csv = std::env::var("MLX_BENCH_MEM_CSV").ok().and_then(|p| {
        let mut f = std::fs::File::create(&p).ok()?;
        use std::io::Write;
        let _ = writeln!(f, "req,pre_rss,post_rss,d_rss,mlx_active,mlx_cache,mlx_peak,wall_s");
        eprintln!("(mem CSV → {p})");
        Some(f)
    });

    let base_rss = rss_mib();
    let (ba, bc, _bp) = mlx_mem_mib();
    eprintln!(
        "\n──────── memory mode: {n} requests ────────\nbaseline (post warm-up): rss={base_rss:.0} MiB | mlx active={ba:.0} cache={bc:.0} MiB"
    );
    eprintln!("{:>3}  {:>8}  {:>8}  {:>7}  {:>9}  {:>9}  {:>9}  {:>6}", "req", "pre", "post", "Δrss", "mlx_act", "mlx_cache", "mlx_peak", "wall");

    let mut prev_rss = base_rss;
    for i in 1..=n {
        let topic = TOPICS[(i - 1) % TOPICS.len()];
        let messages = vec![
            serde_json::json!({"role": "system", "content": "You are a helpful, verbose assistant. Answer at length."}),
            serde_json::json!({"role": "user", "content": format!("Write a detailed ~400-word explanation of {topic}. Be thorough and specific.")}),
        ];
        let tools: Vec<serde_json::Value> = vec![];

        let pre_rss = rss_mib();
        let (tx, mut rx) = mpsc::channel::<String>(64);
        let start = Instant::now();
        let gen = async { engine.stream_openai_to_channel(&messages, &tools, tx).await };
        let drain = async { while rx.recv().await.is_some() {} };
        let (res, ()) = tokio::join!(gen, drain);
        res?;
        let wall = start.elapsed().as_secs_f64();

        let rss = rss_mib();
        let (a, c, p) = mlx_mem_mib();
        eprintln!(
            "{i:>3}  {pre_rss:>6.0} M  {rss:>6.0} M  {:>+6.0}  {a:>7.0} M  {c:>7.0} M  {p:>7.0} M  {wall:>4.2}s",
            rss - prev_rss
        );
        if let Some(f) = csv.as_mut() {
            use std::io::Write;
            let _ = writeln!(
                f,
                "{i},{pre_rss:.1},{rss:.1},{:.1},{a:.1},{c:.1},{p:.1},{wall:.3}",
                rss - prev_rss
            );
        }
        prev_rss = rss;
    }

    let end_rss = rss_mib();
    let (ea, ec, ep) = mlx_mem_mib();
    eprintln!(
        "\nsummary: rss {base_rss:.0} → {end_rss:.0} MiB  (Δ {:+.0} MiB total, {:+.1} MiB/req)",
        end_rss - base_rss,
        (end_rss - base_rss) / n as f64,
    );
    eprintln!("         mlx active={ea:.0} cache={ec:.0} peak={ep:.0} MiB");
    Ok(())
}

#[cfg(feature = "local-mlx")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use perf_sink::{PerfLayer, PerfSink};
    use senclaw::local_model::MlxNativeEngine;
    use std::sync::Arc;
    use std::time::Instant;
    use tokio::sync::mpsc;
    use tracing_subscriber::prelude::*;

    let sink = Arc::new(PerfSink::default());
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer().with_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "warn".into()),
            ),
        )
        .with(PerfLayer(sink.clone()))
        .init();

    let mut args = std::env::args().skip(1);
    let model_dir = args.next().unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}/.senclaw/local-models/Qwen__Qwen3-0.6B")
    });
    let model_id = args.next().unwrap_or_else(|| "Qwen/Qwen3-0.6B".to_string());
    let iters: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);

    eprintln!("model_dir = {model_dir}\nmodel_id  = {model_id}\niters     = {iters} (+1 warm-up)");

    let engine = MlxNativeEngine::new(std::path::Path::new(&model_dir), &model_id, None);
    let t = Instant::now();
    engine.warm_up()?;
    eprintln!("warm_up (weights load): {:.2}s", t.elapsed().as_secs_f64());

    // Memory-growth mode: `MLX_BENCH_REQUESTS=N` → N distinct requests, track RAM.
    if let Some(n) = std::env::var("MLX_BENCH_REQUESTS").ok().and_then(|s| s.parse::<usize>().ok()) {
        if n > 0 {
            return mem_mode(&engine, n).await;
        }
    }

    // `MLX_BENCH_PROMPT_TOKENS=1000` → synthesize a ~N-token document and ask for
    // a summary (stresses prefill / chunked-prefill at ≥512 tokens). Default = a
    // short ~70-token prompt (decode-dominated). Actual prompt size is reported
    // from the engine's `[perf]` line, so the char→token estimate need not be exact.
    let prompt_target = std::env::var("MLX_BENCH_PROMPT_TOKENS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&t| t > 0);
    let user_content = match prompt_target {
        Some(t) => {
            let sentence = "The transformer processes every token in parallel, attending to \
                each other position through scaled dot-product attention, while residual \
                connections and layer normalization stabilize the deep stacks that make large \
                language models effective. ";
            // ~5.7 chars/token for this repetitive filler (repeated text packs
            // into fewer BPE tokens than prose). Actual size is reported below.
            let target_chars = (t as f64 * 5.7) as usize;
            let mut body = String::with_capacity(target_chars + sentence.len());
            let mut k = 0usize;
            while body.len() < target_chars {
                body.push_str(&format!("[para {k}] {sentence}"));
                k += 1;
            }
            eprintln!("prompt    = ~{t} tokens (synthesized, {} chars)", body.len());
            format!("Read the following document and summarize it in about 150 words.\n\n{body}")
        }
        None => "Write a detailed, ~600-word explanation of how the transformer neural network \
            architecture works, covering attention, positional encoding, feed-forward layers, \
            and why it scales. Be thorough."
            .to_string(),
    };
    let messages = vec![
        serde_json::json!({
            "role": "system",
            "content": "You are a helpful, verbose assistant. Always answer at length."
        }),
        serde_json::json!({ "role": "user", "content": user_content }),
    ];
    let tools: Vec<serde_json::Value> = vec![];

    // One run; returns (ttft_s, wall_s, output_text).
    let run_once = |idx: usize| {
        let eng = &engine;
        let msgs = messages.clone();
        let tls = tools.clone();
        async move {
            let (tx, mut rx) = mpsc::channel::<String>(64);
            let start = Instant::now();
            let mut ttft: Option<f64> = None;
            let mut full = String::new();
            let gen = async move { eng.stream_openai_to_channel(&msgs, &tls, tx).await };
            let drain = async {
                while let Some(chunk) = rx.recv().await {
                    if ttft.is_none() {
                        ttft = Some(start.elapsed().as_secs_f64());
                    }
                    full.push_str(&chunk);
                }
            };
            let (res, ()) = tokio::join!(gen, drain);
            res?;
            let wall = start.elapsed().as_secs_f64();
            if let Ok(base) = std::env::var("MLX_BENCH_OUT") {
                let _ = std::fs::write(format!("{base}.run{idx}"), &full);
            }
            anyhow::Ok((ttft.unwrap_or(wall), wall, full))
        }
    };

    // Warm-up turn (caches kernels / prefix). Its prefill is the only *cold*
    // one — subsequent turns hit the prefix cache (≥1024-token prompts) and
    // report near-instant prefill — so capture it before clearing.
    let _ = run_once(0).await?;
    let cold_prompt_tokens = sink.prompt_tokens.lock().unwrap().last().copied().unwrap_or(0);
    let cold_prefill_tok_s = sink.prefill_tok_s.lock().unwrap().last().copied().unwrap_or(0.0);
    sink.clear();

    let mut ttfts = Vec::new();
    let mut outputs = Vec::new();
    for i in 1..=iters {
        let (ttft, wall, out) = run_once(i).await?;
        eprintln!("  run {i}: ttft={ttft:.2}s wall={wall:.2}s chars={}", out.chars().count());
        ttfts.push(ttft);
        outputs.push(out);
    }

    let mut decode = sink.decode_tok_s.lock().unwrap().clone();
    let identical = outputs.windows(2).all(|w| w[0] == w[1]);

    let stats = |v: &mut Vec<f64>| -> (f64, f64, f64, f64) {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = v.len().max(1);
        let min = *v.first().unwrap_or(&0.0);
        let max = *v.last().unwrap_or(&0.0);
        let mean = v.iter().sum::<f64>() / n as f64;
        let median = v[n / 2];
        (min, median, mean, max)
    };

    eprintln!("\n──────── prompt {cold_prompt_tokens} tok / {} timed turns ────────", decode.len());
    eprintln!("  cold prefill: {cold_prefill_tok_s:.0} tok/s (warm-up turn, no prefix-cache hit)");
    let mut prefill = sink.prefill_tok_s.lock().unwrap().clone();
    if !prefill.is_empty() {
        let (_, pmed, _, _) = stats(&mut prefill);
        eprintln!("  warm prefill: median={pmed:.0} tok/s (timed turns — prefix-cache hits if ≥1024 tok)");
    }
    if decode.is_empty() {
        eprintln!("  (no decode [perf] lines captured)");
    } else {
        let (min, median, mean, max) = stats(&mut decode);
        eprintln!("  decode tok/s  min={min:.1}  median={median:.1}  mean={mean:.1}  max={max:.1}");
        eprintln!("  samples: {decode:?}");
    }
    let mut tt = ttfts.clone();
    let (tmin, tmed, _, tmax) = stats(&mut tt);
    eprintln!("  ttft (s)      min={tmin:.2}  median={tmed:.2}  max={tmax:.2}");
    eprintln!(
        "  determinism:  {}",
        if identical { "all timed turns byte-identical ✓" } else { "OUTPUTS DIFFER ✗" }
    );

    if !identical {
        std::process::exit(1);
    }
    Ok(())
}
