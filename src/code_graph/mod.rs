//! Code Knowledge Graph — biến codebase thành đồ thị tri thức có thể query.
//!
//! Inspired by GitNexus (https://github.com/abhigyanpatwari/GitNexus).
//!
//! Pipeline:
//!   1. Parsing     — tree-sitter: văn bản → AST (parser.rs)
//!   2. Extraction  — AST → Nodes (symbols) + Edges (relationships) (parser.rs)
//!   3. Resolution  — cross-file edge linking (indexer.rs)
//!   4. Storage     — SQLite (schema.rs + indexer.rs)
//!
//! Graph entities:
//!   Nodes — Function, Class, Struct, Trait, Interface, Enum, Type, Const, Module
//!   Edges — CALLS, IMPORTS, EXTENDS, IMPLEMENTS, DEFINES

pub mod indexer;
pub mod parser;
pub mod query;
pub mod schema;
pub mod types;

pub use indexer::CodeGraphIndexer;
pub use query::GraphQuery;
pub use types::{CallerInfo, ImpactNode, IndexStats, Language, NodeKind};
