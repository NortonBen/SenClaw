//! Determine whether the agent should respond to a message.
//! Mirrors `src-old/gateway/TriggerChecker.ts`.

use crate::types::{Agent, Binding, GroupBinding, IncomingMessage};

/// Rule priority (high to low):
///   1. is_from_me -> never respond
///   2. is_admin group -> always respond
///   3. private chat -> always respond
///   4. requires_trigger=false -> always respond
///   5. group/supergroup -> respond only when mentioned
pub fn should_trigger(msg: &IncomingMessage, group: &GroupBinding) -> bool {
    if msg.is_from_me {
        return false;
    }
    if group.is_admin {
        return true;
    }
    if msg.chat_type == crate::types::ChatType::Private {
        return true;
    }
    if !group.requires_trigger {
        return true;
    }
    msg.mentions_bot_username.unwrap_or(false)
}

/// Entity-model variant: uses [`Agent`] for config + [`Binding`] for routing flags.
pub fn should_trigger_entity(msg: &IncomingMessage, agent: &Agent, binding: &Binding) -> bool {
    if msg.is_from_me {
        return false;
    }
    if binding.is_admin {
        return true;
    }
    if msg.chat_type == crate::types::ChatType::Private {
        return true;
    }
    if !agent.requires_trigger {
        return true;
    }
    msg.mentions_bot_username.unwrap_or(false)
}
