//! Shared types for browser automation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for a browser tab.
pub type TabId = String;

/// Unique identifier for a crawl job.
pub type JobId = String;

/// Unique identifier for a pending request.
pub type RequestId = String;

/// Snapshot element representing an interactive DOM node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotElement {
    pub index: u32,
    pub tag: String,
    pub role: String,
    pub text: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, String>,
    pub bbox: BoundingBox,
    pub enabled: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Full page snapshot returned by content script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageSnapshot {
    pub url: String,
    pub title: String,
    pub elements: Vec<SnapshotElement>,
    pub text_content_summary: String,
    pub compressed_html: Option<String>,
}

/// Search result from Google/Bing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub position: u8,
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub results: Vec<SearchResultItem>,
    pub total_estimated: u64,
    pub search_url: String,
}

/// Tab state tracked by the registry.
#[derive(Debug, Clone, Serialize)]
pub struct TabState {
    pub tab_id: TabId,
    pub url: String,
    pub title: String,
    pub status: TabStatus,
    #[serde(skip)]
    pub created_at: std::time::Instant,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TabStatus {
    Loading,
    Complete,
    Error(String),
}

/// Crawl job configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlConfig {
    pub job_id: JobId,
    pub start_url: String,
    pub depth: u8,
    pub max_pages: u16,
    pub link_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub same_domain: bool,
    pub per_page_timeout_ms: u32,
    pub wait_between_pages_ms: u32,
}

/// Result from crawling a single page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlPageResult {
    pub url: String,
    pub title: String,
    pub text_content: String,
    pub extracted_data: Option<serde_json::Value>,
    pub links_found: u16,
    pub depth: u8,
    pub crawled_at: String,
}

/// Status of a crawl job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlJobStatus {
    pub job_id: JobId,
    pub status: String,
    pub pages_crawled: u16,
    pub pages_total: u16,
    pub results: Vec<CrawlPageResult>,
}

/// Scroll amount for scroll actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScrollAmount {
    Pages(f32),
    Pixels(u32),
}

/// Wait condition for wait tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WaitCondition {
    #[serde(rename = "time")]
    Time { ms: u32 },
    #[serde(rename = "text")]
    Text { text: String, timeout_ms: u32 },
    #[serde(rename = "text_gone")]
    TextGone { text: String, timeout_ms: u32 },
    #[serde(rename = "navigation")]
    Navigation { timeout_ms: u32 },
}

/// Form field definition for fill_form.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FormField {
    pub target: String,
    pub value: String,
    #[serde(rename = "type")]
    pub field_type: String,
}

/// Result of a browser action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum ActionResult {
    #[serde(rename = "ok")]
    Ok { data: serde_json::Value },
    #[serde(rename = "error")]
    Error { message: String, code: Option<String> },
}

impl ActionResult {
    pub fn ok_data(data: serde_json::Value) -> Self {
        Self::Ok { data }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
            code: None,
        }
    }
}

/// Screenshot format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenshotFormat {
    Png,
    Jpeg,
}
