//! Prompt preprocessing for native MLX inference (RAM / token control).

use super::mlx_lm_utils::tokenizer::{Conversation, Role};

pub use super::thinking_parse::{split_thinking_blocks, strip_thinking_blocks};

/// User-assistant turns capped before templating; a leading `Role::System` is preserved with the first user turn when present.
pub const MLX_MAX_HISTORY_TURNS: usize = 4;

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

    // Keep an optional leading system plus the first user chunk (Gemma-3 Jinja), or
    // the first merged user turn (no system), matching the old single-prefix rule.
    let trim_prefix = if convs.first().is_some_and(|c| c.role == Role::System) {
        convs
            .iter()
            .enumerate()
            .find(|(i, c)| *i > 0 && c.role == Role::User)
            .map(|(i, _)| i + 1)
            .unwrap_or(convs.len())
            .max(1)
    } else {
        1
    };

    if start > trim_prefix {
        let before = convs.len();
        convs.drain(trim_prefix..start);
        tracing::info!(
            "[local-mlx-native] trimming chat history: {before} → {} messages (keep last {max_turns} user turns, preserve {} leading message(s))",
            convs.len(),
            trim_prefix
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_keeps_last_n_user_turns() {
        let mut convs: Vec<Conversation<Role, String>> = vec![
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
        assert_eq!(convs[0].content, "1", "first user turn preserved");
        assert!(convs.len() >= 2);
        assert_eq!(convs.last().unwrap().content, "3");
    }

    #[test]
    fn trim_preserves_system_and_first_user_when_capping() {
        let mut convs: Vec<Conversation<Role, String>> = vec![
            Conversation {
                role: Role::System,
                content: "sys".into(),
            },
            Conversation {
                role: Role::User,
                content: "u0".into(),
            },
            Conversation {
                role: Role::Assistant,
                content: "a0".into(),
            },
            Conversation {
                role: Role::User,
                content: "u1".into(),
            },
            Conversation {
                role: Role::Assistant,
                content: "a1".into(),
            },
            Conversation {
                role: Role::User,
                content: "u2".into(),
            },
        ];
        trim_conversation_history(&mut convs, 2);
        assert_eq!(convs.len(), 5);
        assert_eq!(convs[0].role, Role::System);
        assert_eq!(convs[0].content, "sys");
        assert_eq!(convs[1].content, "u0");
        assert_eq!(convs.last().unwrap().content, "u2");
    }
}
