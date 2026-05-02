//! WeChat iLink Bot channel adapter — re-exports.

mod api;
mod channel;
mod helpers;
mod types;

#[cfg(test)]
mod tests;

pub use channel::WeChatChannel;
