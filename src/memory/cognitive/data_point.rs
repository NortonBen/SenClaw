//! DataPoint — node payload in the cognitive graph.
//!
//! Port of cognee `DataPoint` + shodh `EntityNode`. A DataPoint is the unit
//! of memory: an entity, a chunk, a summary, or a user-defined type.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Entity,
    Chunk,
    Summary,
    Custom,
}

impl NodeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Entity => "entity",
            Self::Chunk => "chunk",
            Self::Summary => "summary",
            Self::Custom => "custom",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "entity" => Self::Entity,
            "chunk" => Self::Chunk,
            "summary" => Self::Summary,
            _ => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    pub id: Uuid,
    pub kind: NodeKind,
    /// User-defined type name (e.g. `"ScientificPaper"`) — only meaningful
    /// when `kind == Custom`. Empty string otherwise.
    pub type_name: String,
    pub name: String,
    pub summary: String,
    /// Content hash (e.g. SHA-256 hex) for dedupe.
    pub content_hash: Option<String>,
    pub props: Value,
    /// shodh dynamics
    pub salience: f32,
    pub mention_count: u32,
    pub is_proper_noun: bool,
    pub selectivity: Option<f32>,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_seen_at: i64,
}

impl DataPoint {
    /// New chunk node (most common path during `add()`).
    pub fn chunk(text: impl Into<String>, content_hash: Option<String>, now: i64) -> Self {
        let text = text.into();
        Self {
            id: Uuid::new_v4(),
            kind: NodeKind::Chunk,
            type_name: String::new(),
            name: String::new(),
            summary: text,
            content_hash,
            props: Value::Object(Default::default()),
            salience: 0.5,
            mention_count: 1,
            is_proper_noun: false,
            selectivity: None,
            created_at: now,
            updated_at: now,
            last_seen_at: now,
        }
    }

    /// New entity node (produced by `cognify` / triplet extraction).
    pub fn entity(name: impl Into<String>, now: i64) -> Self {
        let name = name.into();
        let proper = name.chars().next().is_some_and(|c| c.is_uppercase());
        Self {
            id: Uuid::new_v4(),
            kind: NodeKind::Entity,
            type_name: String::new(),
            name,
            summary: String::new(),
            content_hash: None,
            props: Value::Object(Default::default()),
            salience: 0.5,
            mention_count: 1,
            is_proper_noun: proper,
            selectivity: None,
            created_at: now,
            updated_at: now,
            last_seen_at: now,
        }
    }
}
