//! How a Space App runs: its lifecycle mode, what the machine must provide for
//! it, how it is confined, and what has to be installed before it starts.
//!
//! The launcher ([`crate::gateway::ui_server::space_mcp`]) is about *processes*
//! — spawning, health-checking, killing, reclaiming ports. This module is about
//! the *decisions* it makes, kept separate because each one is a pure function
//! of a manifest and a machine, and is worth being able to test that way.
//!
//! - [`manifest`] — the typed view of the `runtime`, `requires` and `sandbox`
//!   blocks: background vs session, the runner, the idle timeout.
//! - [`requirements`] — does this machine have Node 18 / Python 3.10 / ffmpeg?
//! - [`prepare`] — `npm ci` / `pip install` into a venv, once per install.
//! - [`sandbox_decl`] — applying the confinement an app declares for itself.

pub mod manifest;
pub mod prepare;
pub mod requirements;
pub mod sandbox_decl;

pub use manifest::{RunMode, Runner, Requires, RuntimeSpec, SandboxDecl};
pub use requirements::{CheckResult, RequirementsReport};
