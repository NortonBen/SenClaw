//! Verify the full Gemma-4 channel → `<think>` UI chain on real captured
//! model output. Mirrors the daemon's runtime path step-by-step:
//!
//!   raw stream
//!     ↓  stream_parser::parse_complete_with_markers
//!   (visible, reasoning, tool_calls)
//!     ↓  build assistant message (ContentBlock::Thinking + Text)
//!     ↓  extract_content
//!   (text, reasoning) — what conversation.rs emits via MessageComplete
//!     ↓  merge_assistant_reasoning_for_web_ui equivalent
//!   final WS payload — what the UI's extractLeadingReasoningBlocks sees
//!
//! The chain is correct iff the final string starts with `<think>` so the UI
//! regex `^(\s*)<think\b[^>]*>([\s\S]*?)</think>` matches.

#[cfg(not(feature = "local-mlx"))]
fn main() {
    eprintln!("build with --features local-mlx");
    std::process::exit(1);
}

#[cfg(feature = "local-mlx")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: verify_think_chain <raw-file>")?;
    let raw = std::fs::read_to_string(&path)?;

    use senclaw::local_model::stream_parser::{parse_complete_with_markers, MarkerSet};
    let markers = MarkerSet::gemma4();
    let (visible, reasoning, _tcs) = parse_complete_with_markers(&raw, &markers);

    eprintln!("── step 1: parser ──");
    eprintln!("reasoning chars: {}", reasoning.len());
    eprintln!("visible chars:   {}", visible.len());
    eprintln!();

    // Step 2: simulate `merge_assistant_reasoning_for_web_ui` from pool.rs.
    // The function wraps reasoning in `<think>…</think>` UNLESS the visible
    // content already contains `<think` (defense against double-wrapping).
    fn merge_for_ui(reasoning: &str, content: &str) -> String {
        let r = reasoning.trim();
        if r.is_empty() {
            return content.to_string();
        }
        let c = content.trim();
        let sniff = if c.len() > 4096 { &c[..4096] } else { c };
        let lower = sniff.to_ascii_lowercase();
        if lower.contains("<think")
            || lower.contains("redacted_reasoning")
            || lower.contains("redacted_thinking")
        {
            eprintln!("⚠ merge bypass: visible content already contains <think — UI won't see reasoning wrap");
            return content.to_string();
        }
        format!("<think>\n{r}\n</think>\n\n{c}")
    }
    let merged = merge_for_ui(&reasoning, &visible);

    eprintln!("── step 2: merge ──");
    eprintln!("merged starts with: {:?}", &merged.chars().take(40).collect::<String>());
    eprintln!();

    // Step 3: simulate the UI's `extractLeadingReasoningBlocks` Regex match.
    // The UI regex is `^(\s*)<think\b[^>]*>([\s\S]*?)</think>` (case-insensitive).
    let merged_lower = merged.to_ascii_lowercase();
    let opens_with_think = merged_lower.trim_start().starts_with("<think");
    let has_close = merged_lower.contains("</think>");

    eprintln!("── step 3: UI extraction simulation ──");
    eprintln!("starts with <think:   {opens_with_think}");
    eprintln!("contains </think>:    {has_close}");
    eprintln!();

    if opens_with_think && has_close {
        eprintln!("✓ chain works — UI will render reasoning as collapsible thinking block");
        Ok(())
    } else {
        eprintln!("✗ chain broken — UI will NOT render reasoning as <think> block");
        Err("verification failed".into())
    }
}
