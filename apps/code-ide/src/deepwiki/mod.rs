//! DeepWiki, vendored in-process.
//!
//! The DeepWiki App Space (tree-sitter code index + call graph + source-grounded
//! wiki/Q&A) folded into SenClaw Code so the IDE ships code intelligence in the
//! same binary. Its Axum router is nested under `/api/deepwiki` and its built web
//! UI is served at `/deepwiki` (embedded as a tab). The standalone `apps/deepwiki`
//! App Space is kept as-is; this is a copy, namespaced under `crate::deepwiki`.

pub mod api;
pub mod db;
pub mod index;
pub mod lang;
pub mod mcp;
pub mod model;
pub mod parse;
pub mod query;
pub mod watch;
pub mod wiki;
