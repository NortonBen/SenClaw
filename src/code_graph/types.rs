//! Core types for the Code Knowledge Graph.

use serde::{Deserialize, Serialize};

// ─── Node (Symbol) ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Function,
    AsyncFunction,
    Method,
    Class,
    Struct,
    Trait,
    Interface,
    Enum,
    Type,
    Const,
    Module,
    File,
}

impl NodeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeKind::Function => "function",
            NodeKind::AsyncFunction => "async_function",
            NodeKind::Method => "method",
            NodeKind::Class => "class",
            NodeKind::Struct => "struct",
            NodeKind::Trait => "trait",
            NodeKind::Interface => "interface",
            NodeKind::Enum => "enum",
            NodeKind::Type => "type",
            NodeKind::Const => "const",
            NodeKind::Module => "module",
            NodeKind::File => "file",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "function" => NodeKind::Function,
            "async_function" => NodeKind::AsyncFunction,
            "method" => NodeKind::Method,
            "class" => NodeKind::Class,
            "struct" => NodeKind::Struct,
            "trait" => NodeKind::Trait,
            "interface" => NodeKind::Interface,
            "enum" => NodeKind::Enum,
            "type" => NodeKind::Type,
            "const" => NodeKind::Const,
            "module" => NodeKind::Module,
            _ => NodeKind::File,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeNode {
    pub id: i64,
    pub project_id: String,
    pub file_path: String,
    pub name: String,
    pub kind: NodeKind,
    pub signature: Option<String>,
    pub doc_comment: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub language: Language,
}

// ─── Edge (Relationship) ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// A calls B
    Calls,
    /// A imports from B
    Imports,
    /// A extends/inherits B
    Extends,
    /// A implements B (interface)
    Implements,
    /// File A defines symbol B
    Defines,
    /// A uses type B
    UsesType,
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::Calls => "calls",
            EdgeKind::Imports => "imports",
            EdgeKind::Extends => "extends",
            EdgeKind::Implements => "implements",
            EdgeKind::Defines => "defines",
            EdgeKind::UsesType => "uses_type",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "calls" => EdgeKind::Calls,
            "imports" => EdgeKind::Imports,
            "extends" => EdgeKind::Extends,
            "implements" => EdgeKind::Implements,
            "defines" => EdgeKind::Defines,
            "uses_type" => EdgeKind::UsesType,
            _ => EdgeKind::Calls,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeEdge {
    pub id: i64,
    pub project_id: String,
    pub from_id: Option<i64>,
    pub from_name: String,
    pub from_file: String,
    pub to_id: Option<i64>,
    pub to_name: String,
    pub to_file: Option<String>,
    pub kind: EdgeKind,
    pub at_line: u32,
}

// ─── Language ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    // Compiled / systems
    #[default]
    Rust,
    C,
    Cpp,
    CSharp,
    Go,
    Verilog,
    // JVM
    Java,
    Scala,
    // Scripting
    JavaScript,
    TypeScript,
    Python,
    Ruby,
    PHP,
    Bash,
    Julia,
    // Functional
    Haskell,
    OCaml,
    Agda,
    // Markup / data (symbols extracted where possible)
    HTML,
    CSS,
    JSON,
    // Not indexed for symbol graph
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Language::Rust,
            "c" | "h" => Language::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Language::Cpp,
            "cs" => Language::CSharp,
            "go" => Language::Go,
            "v" | "sv" | "svh" => Language::Verilog,
            "java" => Language::Java,
            "scala" | "sc" => Language::Scala,
            "js" | "jsx" | "mjs" | "cjs" => Language::JavaScript,
            "ts" | "tsx" => Language::TypeScript,
            "py" | "pyi" => Language::Python,
            "rb" | "rake" | "gemspec" => Language::Ruby,
            "php" | "php4" | "php5" | "phps" => Language::PHP,
            "sh" | "bash" | "zsh" | "fish" => Language::Bash,
            "jl" => Language::Julia,
            "hs" | "lhs" => Language::Haskell,
            "ml" | "mli" => Language::OCaml,
            "agda" => Language::Agda,
            "html" | "htm" | "erb" | "ejs" => Language::HTML,
            "css" | "scss" | "sass" => Language::CSS,
            "json" => Language::JSON,
            _ => Language::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::CSharp => "c_sharp",
            Language::Go => "go",
            Language::Verilog => "verilog",
            Language::Java => "java",
            Language::Scala => "scala",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::Python => "python",
            Language::Ruby => "ruby",
            Language::PHP => "php",
            Language::Bash => "bash",
            Language::Julia => "julia",
            Language::Haskell => "haskell",
            Language::OCaml => "ocaml",
            Language::Agda => "agda",
            Language::HTML => "html",
            Language::CSS => "css",
            Language::JSON => "json",
            Language::Unknown => "unknown",
        }
    }

    /// Languages where we extract meaningful symbols/edges for the code graph.
    pub fn is_indexable(self) -> bool {
        !matches!(
            self,
            Language::HTML | Language::CSS | Language::JSON | Language::Unknown
        )
    }
}

// ─── Parse output ────────────────────────────────────────────────────────────

/// Raw extraction result from parsing one file — before DB ids are assigned.
#[derive(Debug, Default)]
pub struct ParsedFile {
    pub file_path: String,
    pub language: Language,
    pub nodes: Vec<RawNode>,
    pub edges: Vec<RawEdge>,
}

#[derive(Debug, Clone)]
pub struct RawNode {
    pub name: String,
    pub kind: NodeKind,
    pub signature: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone)]
pub struct RawEdge {
    /// Name of the symbol doing the call/import/extends (within this file)
    pub from_name: String,
    /// Name of the target symbol/module
    pub to_name: String,
    /// Hint at which file the target lives (for imports where we know the path)
    pub to_file: Option<String>,
    pub kind: EdgeKind,
    pub at_line: u32,
}

// ─── Query results ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct CallerInfo {
    pub caller_name: String,
    pub caller_file: String,
    pub caller_kind: String,
    pub at_line: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactNode {
    pub name: String,
    pub file: String,
    pub kind: String,
    pub depth: u32,
    pub via: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub files_skipped: usize,
    pub symbols: usize,
    pub edges: usize,
}
