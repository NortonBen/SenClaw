use serde::{Deserialize, Serialize};

/// A source file that has been indexed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRec {
    pub id: i64,
    pub path: String,
    pub lang: String,
    pub hash: String,
    pub mtime: i64,
    pub loc: i64,
}

/// A symbol (definition) extracted from a source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: i64,
    pub file_id: i64,
    pub path: String,
    pub name: String,
    /// function | method | class | struct | enum | interface | trait | type | const | module | impl | macro
    pub kind: String,
    pub parent: Option<String>,
    pub start_line: i64,
    pub end_line: i64,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
}

/// A relationship between code locations: a call, import, or inheritance edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    /// call | import | extends | implements
    pub kind: String,
    pub src_path: String,
    /// Enclosing symbol at the source site, if any.
    pub src_symbol: Option<String>,
    /// Callee name, import path, or parent type.
    pub target: String,
    pub line: i64,
}

/// A resolved caller/callee link in the call graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallLink {
    pub name: String,
    pub path: String,
    pub start_line: i64,
    pub kind: String,
}

/// Aggregate counts for a repository index.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IndexStats {
    pub files: i64,
    pub symbols: i64,
    pub edges: i64,
    pub by_lang: Vec<(String, i64)>,
    pub last_indexed: i64,
}
