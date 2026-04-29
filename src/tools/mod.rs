//! Concrete tool implementations that implement [`zen_core::Tool`].
//!
//! Each tool mirrors the TS sema-core tool surface:
//! - [`BashTool`] — shell command execution
//! - [`ReadTool`] — file reading
//! - [`WriteTool`] — file creation / overwrite
//! - [`EditTool`] — exact string replacement in files
//!
//! Register tools on a [`ZenEngine`] via `engine.register_tools(all_tools())`.

pub mod bash;
pub mod edit;
pub mod read;
pub mod write;

use std::sync::Arc;

use crate::zen_core::Tool;

pub use bash::BashTool;
pub use edit::EditTool;
pub use read::ReadTool;
pub use write::WriteTool;

/// All built-in tools, ready for registration on a [`ZenEngine`].
pub fn all_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(BashTool),
        Arc::new(ReadTool),
        Arc::new(WriteTool),
        Arc::new(EditTool),
    ]
}
