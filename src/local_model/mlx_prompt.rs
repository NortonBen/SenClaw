//! Prompt preprocessing for native MLX inference (RAM / token control).

use super::mlx_lm_utils::tokenizer::{Conversation, Role};

/// User/assistant turn pairs kept in chat history before templating (excludes system).
pub const MLX_MAX_HISTORY_TURNS: usize = 4;

const THINK_OPEN: &str = concat!("<", "think", ">");
const THINK_CLOSE: &str = concat!("</", "think", ">");

const THINKING_TAG_PAIRS: &[(&str, &str)] = &[
    ("<think>", "</think>"),
    (THINK_OPEN, THINK_CLOSE),
    ("<redacted_reasoning>", "</redacted_reasoning>"),
];

/// Remove Qwen/DeepSeek-style reasoning wrappers from assistant text (anywhere in the string).
pub fn strip_thinking_blocks(text: &str) -> String {
    let mut out = text.to_string();
    for (open, close) in THINKING_TAG_PAIRS {
        if open.is_empty() {
            continue;
        }
        out = remove_all_tag_pairs(&out, open, close);
    }
    collapse_blank_lines(out.trim())
}

fn remove_all_tag_pairs(s: &str, open: &str, close: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(open) {
        result.push_str(&rest[..i]);
        let after_open = &rest[i + open.len()..];
        if let Some(j) = after_open.find(close) {
            rest = &after_open[j + close.len()..];
        } else {
            result.push_str(&rest[i..]);
            return result;
        }
    }
    result.push_str(rest);
    result
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

/// Keep the last `max_turns` user messages and all messages from the first kept user onward.
pub fn trim_conversation_history<T>(convs: &mut Vec<Conversation<Role, T>>, max_turns: usize) {
    if max_turns == 0 || convs.is_empty() {
        return;
    }

    let mut user_seen = 0usize;
    let mut start = 0usize;
    for (i, c) in convs.iter().enumerate().rev() {
        if c.role == Role::User {
            user_seen += 1;
            if user_seen == max_turns {
                start = i;
                break;
            }
        }
    }

    if start > 0 {
        tracing::info!(
            "[local-mlx-native] trimming chat history: {} → {} messages (keep last {max_turns} user turns)",
            convs.len(),
            convs.len() - start
        );
        convs.drain(..start);
    }
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
    fn strip_think_tags() {
        let s = concat!(THINK_OPEN, "\nstep\n", THINK_CLOSE, "\n\nAnswer.");
        assert_eq!(strip_thinking_blocks(&s), "Answer.");
    }

    #[test]
    fn trim_keeps_last_n_user_turns() {
        let mut convs = vec![
            Conversation {
                role: Role::User,
                content: "1".into(),
            },
            Conversation {
                role: Role::Assistant,
                content: "a".into(),
            },
            Conversation {
                role: Role::User,
                content: "2".into(),
            },
            Conversation {
                role: Role::Assistant,
                content: "b".into(),
            },
            Conversation {
                role: Role::User,
                content: "3".into(),
            },
        ];
        trim_conversation_history(&mut convs, 2);
        assert_eq!(convs.len(), 3);
        assert_eq!(convs[0].content, "2");
    }
}
