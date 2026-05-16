//! Qwen / DeepSeek-style thinking tag parsing (no MLX dependency).

const THINK_OPEN: &str = concat!("<", "think", ">");
const THINK_CLOSE: &str = concat!("</", "think", ">");

const THINKING_TAG_PAIRS: &[(&str, &str)] = &[
    ("<think>", "</think>"),
    (THINK_OPEN, THINK_CLOSE),
    ("<redacted_reasoning>", "</redacted_reasoning>"),
];

/// Split assistant output into `(reasoning, visible_answer)`.
///
/// Handles closed and unclosed thinking tags (Qwen3 often stops before `</think>`).
pub fn split_thinking_blocks(text: &str) -> (String, String) {
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut answer = text.to_string();
    for (open, close) in THINKING_TAG_PAIRS {
        if open.is_empty() {
            continue;
        }
        loop {
            let Some(i) = answer.find(open) else {
                break;
            };
            let after_open = &answer[i + open.len()..];
            if let Some(j) = after_open.find(close) {
                let inner = after_open[..j].trim();
                if !inner.is_empty() {
                    reasoning_parts.push(inner.to_string());
                }
                answer = format!("{}{}", &answer[..i], &after_open[j + close.len()..]);
            } else {
                let inner = after_open.trim();
                if !inner.is_empty() {
                    reasoning_parts.push(inner.to_string());
                }
                answer = answer[..i].to_string();
                break;
            }
        }
    }
    let reasoning = reasoning_parts.join("\n\n");
    let answer = collapse_blank_lines(answer.trim());
    (reasoning, answer)
}

/// Remove thinking wrappers; keep only the visible answer.
pub fn strip_thinking_blocks(text: &str) -> String {
    split_thinking_blocks(text).1
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_blank = false;
    for line in s.lines() {
        let blank = line.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
        prev_blank = blank;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_redacted_thinking() {
        let s = "<think>\nlong chain\n</think>\n\nHello!";
        assert_eq!(strip_thinking_blocks(s), "Hello!");
    }

    #[test]
    fn split_unclosed_thinking() {
        let s = "<think>\n10 + 22 = 32\n";
        let (r, a) = split_thinking_blocks(s);
        assert_eq!(r, "10 + 22 = 32");
        assert!(a.is_empty());
    }

    #[test]
    fn split_closed_then_answer() {
        let s = "<think>\nwork\n</think>\n\n32";
        let (r, a) = split_thinking_blocks(s);
        assert_eq!(r, "work");
        assert_eq!(a, "32");
    }
}
