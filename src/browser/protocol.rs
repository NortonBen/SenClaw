//! WebSocket message types for daemon ↔ Chrome extension communication.

use serde::{Deserialize, Serialize};

use super::types::*;

// ===== Daemon → Extension =====

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DaemonMessage {
    // Tab management
    Navigate {
        request_id: RequestId,
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
    },
    NewTab {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    CloseTab {
        request_id: RequestId,
        tab_id: TabId,
    },
    SwitchTab {
        request_id: RequestId,
        tab_id: TabId,
    },
    GoBack {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
    },
    GoForward {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
    },
    Reload {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
    },

    // DOM interaction
    Click {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        index: u32,
    },
    Type {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        index: u32,
        text: String,
        #[serde(default)]
        submit: bool,
    },
    SelectOption {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        index: u32,
        option_text: String,
    },
    Scroll {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        direction: String,
        amount: ScrollAmount,
    },
    Hover {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        index: u32,
    },
    PressKey {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        key: String,
    },
    UploadFile {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        index: u32,
        file_paths: Vec<String>,
    },
    ExecuteJs {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        script: String,
    },
    WaitFor {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        condition: WaitCondition,
    },

    // Observation
    GetSnapshot {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        depth: Option<u8>,
        #[serde(default)]
        compress_html: bool,
    },
    GetScreenshot {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        #[serde(default)]
        full_page: bool,
        #[serde(default)]
        format: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        quality: Option<u8>,
    },
    ExtractText {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
    },
    ExtractLinks {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
    },
    ExtractTable {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
    },

    // Search
    Search {
        request_id: RequestId,
        query: String,
        #[serde(default = "default_search_engine")]
        engine: String,
        #[serde(default = "default_num_results")]
        num_results: u8,
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
    },

    // Crawl control
    CrawlStart {
        job_id: JobId,
        start_url: String,
        depth: u8,
        max_pages: u16,
        #[serde(default)]
        link_patterns: Vec<String>,
        #[serde(default)]
        exclude_patterns: Vec<String>,
        #[serde(default = "default_true")]
        same_domain: bool,
    },
    CrawlPause {
        job_id: JobId,
    },
    CrawlResume {
        job_id: JobId,
    },
    CrawlStop {
        job_id: JobId,
    },

    // Fill form
    FillForm {
        request_id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        fields: Vec<FormField>,
        #[serde(default)]
        submit: bool,
    },

    // List tabs
    ListTabs {
        request_id: RequestId,
    },

    // Get status
    GetStatus {
        request_id: RequestId,
    },
}

fn default_search_engine() -> String {
    "google".into()
}

fn default_num_results() -> u8 {
    10
}

fn default_true() -> bool {
    true
}

// ===== Extension → Daemon =====

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ExtensionMessage {
    // Response to requests
    Response {
        request_id: RequestId,
        #[serde(flatten)]
        result: ActionResult,
    },

    // Tab events
    TabCreated {
        tab_id: TabId,
        url: String,
        window_id: u32,
    },
    TabUpdated {
        tab_id: TabId,
        url: String,
        title: String,
        status: String,
    },
    TabClosed {
        tab_id: TabId,
    },

    // Crawl progress
    CrawlProgress {
        job_id: JobId,
        pages_crawled: u16,
        pages_total: u16,
        current_url: String,
    },
    CrawlResult {
        job_id: JobId,
        page_result: CrawlPageResult,
    },
    CrawlComplete {
        job_id: JobId,
        total_pages: u16,
        duration_ms: u64,
    },

    // Screenshot frame (for Web UI preview)
    ScreenshotFrame {
        tab_id: TabId,
        data: String,
        format: String,
    },

    // Heartbeat
    Heartbeat {
        tab_count: u16,
        active_tab_id: Option<TabId>,
    },
    UserInstruction {
        text: String,
    },
}
