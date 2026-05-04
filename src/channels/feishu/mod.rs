//! Feishu/Lark channel adapter. Mirrors `src-old/channels/feishu.ts` + `feishu-client.ts`.
//!
//! JID format: `feishu:user:{open_id}` (private) or `feishu:group:{chat_id}` (group).
//!
//! Multi-app support: each appId/appSecret pair maintains its own REST client & WS listener.
//! WebSocket event receiving is stubbed (requires Feishu WS framing protocol).
//!
//! Connection mode: REST for sending, WS stub for receiving (TODO: full WS protocol).

mod api;
mod channel;
mod helpers;
#[cfg(test)]
mod tests;
mod token;
mod types;
mod ws;

pub use channel::FeishuChannel;
pub use types::FeishuDomain;

// ===== Constants =====

pub(crate) const FEISHU_MAX_LEN: usize = 4000;
pub(crate) const FEISHU_CARD_MAX_LEN: usize = 20_000;
pub(crate) const APP_INIT_TIMEOUT_SECS: u64 = 15;
pub(crate) const DEDUP_TTL_MS: i64 = 30 * 60 * 1000;
pub(crate) const DEDUP_MAX_SIZE: usize = 1000;
pub(crate) const DEDUP_CLEANUP_INTERVAL_MS: i64 = 5 * 60 * 1000;
pub(crate) const SENDER_NAME_TTL_SECS: u64 = 10 * 60;
pub(crate) const TOKEN_REFRESH_MARGIN_SECS: i64 = 60;
