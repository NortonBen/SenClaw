//! Feed a captured raw model stream (the file produced by `MLX_BENCH_OUT=...`)
//! through the unified [`stream_parser`] and dump the canonical output. Used to
//! verify on real model output that markers (`<|channel>`, `<|tool_call>`, …)
//! never leak past the normalizer regardless of which arch produced the file.
//!
//! Usage:
//!   parse_raw_stream <raw-file> <model-id>
//!
//! Example (after running mlx_bench):
//!   cargo run --release --features local-mlx --example parse_raw_stream -- \
//!     /tmp/g4_final.run1 mlx-community/gemma-4-e2b-it-4bit

#[cfg(not(feature = "local-mlx"))]
fn main() {
    eprintln!("build with --features local-mlx");
    std::process::exit(1);
}

#[cfg(feature = "local-mlx")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: parse_raw_stream <raw-file> <model-id>")?;
    let model_id = args.next().unwrap_or_default();
    let raw = std::fs::read_to_string(&path)?;

    use senclaw::local_model::stream_parser::{dialect_for_model_id, parse_complete};
    let dialect = dialect_for_model_id(&model_id);
    let (visible, reasoning, tool_calls) = parse_complete(&raw, dialect);

    eprintln!("── raw input ──");
    eprintln!("path:    {path}");
    eprintln!("bytes:   {}", raw.len());
    eprintln!("dialect: {dialect:?}");
    eprintln!();
    eprintln!("── canonical (post-parser) ──");
    eprintln!("reasoning ({} chars):", reasoning.len());
    eprintln!("{}", truncate(&reasoning, 600));
    eprintln!();
    eprintln!("visible   ({} chars):", visible.len());
    eprintln!("{}", truncate(&visible, 600));
    eprintln!();
    eprintln!("tool_calls: {} parsed", tool_calls.len());
    for tc in &tool_calls {
        eprintln!("  - {}", serde_json::to_string(tc).unwrap_or_default());
    }
    eprintln!();

    // The whole point of the normalizer: NO marker may survive past it.
    let markers = [
        "<|channel>",
        "<channel|>",
        "<|tool_call>",
        "<tool_call|>",
        "<|\"|>",
    ];
    let mut leaks = 0usize;
    for m in markers {
        if visible.contains(m) {
            eprintln!("LEAK in visible: {m}");
            leaks += 1;
        }
        if reasoning.contains(m) {
            eprintln!("LEAK in reasoning: {m}");
            leaks += 1;
        }
    }
    if leaks == 0 {
        eprintln!("✓ no marker leaks — visible/reasoning are clean OpenAI-shape");
        Ok(())
    } else {
        Err(format!("{leaks} marker leak(s) detected").into())
    }
}

#[cfg(feature = "local-mlx")]
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…[truncated]", s.chars().take(n).collect::<String>())
    }
}
