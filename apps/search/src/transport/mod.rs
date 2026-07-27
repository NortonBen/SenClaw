//! How the app physically reaches each backend.
//!
//! A Space App cannot call arbitrary MCP tools — the bridge's `mcp.call` action
//! is a hard-coded stub (`src/gateway/ui_server/space.rs:1704`). The documented
//! workaround (`agent.run`) puts an LLM in the middle of what is mechanically a
//! fan-out: slow, non-deterministic, expensive. So this module opens four
//! deterministic paths and keeps `agent.run` strictly as a fallback:
//!
//! | Transport | Reaches | Deterministic |
//! |---|---|---|
//! | [`browser_ws`] | `senclaw-browser` (web SERP, page text) | yes |
//! | [`app_mcp`]    | any Space App's MCP over JSON-RPC       | yes |
//! | [`core_rest`]  | daemon REST (wiki, cognitive)           | yes |
//! | [`bridge`]     | `llm.request`, `knowledge.save`         | yes |
//! | [`bridge`] `agent_run` | anything MCP-only (file memory) | **no** |
//!
//! See docs/search-app-design.md §2.

// `app_mcp` and `bridge` are complete and unit-tested, but no P0 source
// consumes them yet — the social / youtube / deepwiki sources (`app_mcp`) and
// the claim-extraction stage (`bridge.llm`) land in P1/P2. Allowing dead code
// here beats deleting working, tested transports and rewriting them later.
#[allow(dead_code)]
pub mod app_mcp;
#[allow(dead_code)]
pub mod bridge;
pub mod browser_ws;
#[allow(dead_code)]
pub mod core_rest;

#[allow(unused_imports)]
pub use app_mcp::{AppMcp, PeerApp};
pub use bridge::Bridge;
pub use browser_ws::BrowserWs;
pub use core_rest::CoreRest;

use std::sync::Arc;

/// Every transport, built once at boot and shared by all sources.
#[derive(Clone)]
#[allow(dead_code)] // `apps`/`bridge` are consumed by the P1/P2 sources
pub struct Transports {
    pub browser: BrowserWs,
    pub apps: AppMcp,
    pub core: CoreRest,
    pub bridge: Bridge,
}

impl Transports {
    pub fn from_config() -> Arc<Self> {
        Arc::new(Self {
            browser: BrowserWs::from_config(),
            apps: AppMcp::from_config(),
            core: CoreRest::from_config(),
            bridge: Bridge::from_config(),
        })
    }
}
