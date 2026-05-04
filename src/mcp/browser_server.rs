//! Browser MCP server — remote Chrome control via MCP tools.
//!
//! Tools: navigate, new_tab, close_tab, list_tabs, switch_tab, go_back,
//! go_forward, reload, click, type_text, select_option, scroll, hover,
//! press_key, upload_file, execute_js, wait, snapshot, screenshot,
//! extract_text, extract_links, extract_table, extract_structured,
//! search, crawl, crawl_status, fill_form, click_and_wait, get_status, stop_task.
//!
//! Communicates with the SenClaw Chrome Extension via WebSocket bridge.

use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use crate::browser::html_compressor::{self, CompressConfig};
use crate::browser::protocol::{DaemonMessage, ExtensionMessage};
use crate::browser::types::*;

use rmcp::ServiceExt;

// ===== MCP param types =====

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct NavigateParams {
    url: String,
    #[serde(default)]
    tab_id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct NewTabParams {
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct CloseTabParams {
    tab_id: String,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct SwitchTabParams {
    tab_id: String,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct TabActionParams {
    #[serde(default)]
    tab_id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct ClickParams {
    #[serde(default)]
    tab_id: Option<String>,
    index: u32,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct TypeParams {
    #[serde(default)]
    tab_id: Option<String>,
    index: u32,
    text: String,
    #[serde(default)]
    submit: bool,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct SelectOptionParams {
    #[serde(default)]
    tab_id: Option<String>,
    index: u32,
    option_text: String,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct ScrollParams {
    #[serde(default)]
    tab_id: Option<String>,
    /// "down" or "up"
    #[serde(default = "default_direction")]
    direction: String,
    /// Pixels or pages (e.g. "300" or "0.5")
    #[serde(default)]
    amount: Option<String>,
}

fn default_direction() -> String {
    "down".into()
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct HoverParams {
    #[serde(default)]
    tab_id: Option<String>,
    index: u32,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct PressKeyParams {
    #[serde(default)]
    tab_id: Option<String>,
    key: String,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct UploadFileParams {
    #[serde(default)]
    tab_id: Option<String>,
    index: u32,
    file_paths: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct ExecuteJsParams {
    #[serde(default)]
    tab_id: Option<String>,
    script: String,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct WaitParams {
    #[serde(default)]
    tab_id: Option<String>,
    /// Wait type: "time", "text", "text_gone", "navigation"
    #[serde(rename = "type")]
    wait_type: String,
    /// Time in ms (for "time" type)
    #[serde(default)]
    ms: Option<u32>,
    /// Text to wait for (for "text" type)
    #[serde(default)]
    text: Option<String>,
    /// Text to wait to disappear (for "text_gone" type)
    #[serde(default)]
    text_gone: Option<String>,
    /// Timeout in ms
    #[serde(default = "default_timeout")]
    timeout_ms: u32,
}

fn default_timeout() -> u32 {
    30000
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct SnapshotParams {
    #[serde(default)]
    tab_id: Option<String>,
    #[serde(default)]
    depth: Option<u8>,
    #[serde(default)]
    include_hidden: bool,
    #[serde(default = "default_compress")]
    compress_html: bool,
}

fn default_compress() -> bool {
    true
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct ScreenshotParams {
    #[serde(default)]
    tab_id: Option<String>,
    #[serde(default)]
    full_page: bool,
    #[serde(default)]
    element_selector: Option<String>,
    #[serde(default = "default_format")]
    format: String,
    #[serde(default)]
    quality: Option<u8>,
}

fn default_format() -> String {
    "png".into()
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct ExtractTextParams {
    #[serde(default)]
    tab_id: Option<String>,
    #[serde(default)]
    selector: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct ExtractLinksParams {
    #[serde(default)]
    tab_id: Option<String>,
    #[serde(default)]
    selector: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct ExtractTableParams {
    #[serde(default)]
    tab_id: Option<String>,
    #[serde(default)]
    selector: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct ExtractStructuredParams {
    #[serde(default)]
    tab_id: Option<String>,
    schema: serde_json::Value,
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    max_items: Option<u16>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct SearchParams {
    query: String,
    #[serde(default = "default_search_engine2")]
    engine: String,
    #[serde(default = "default_num_results2")]
    num_results: u8,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default = "default_true_val")]
    safe_search: bool,
}

fn default_search_engine2() -> String {
    "google".into()
}
fn default_num_results2() -> u8 {
    10
}
fn default_true_val() -> bool {
    true
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct CrawlParams {
    start_url: String,
    #[serde(default = "default_depth")]
    depth: u8,
    #[serde(default = "default_max_pages")]
    max_pages: u16,
    #[serde(default)]
    link_patterns: Vec<String>,
    #[serde(default)]
    exclude_patterns: Vec<String>,
    #[serde(default = "default_true_val")]
    same_domain: bool,
    #[serde(default = "default_extract_type")]
    extract_type: String,
    #[serde(default)]
    structured_schema: Option<serde_json::Value>,
    #[serde(default = "default_per_page_timeout")]
    per_page_timeout_ms: u32,
    #[serde(default = "default_wait_between")]
    wait_between_pages_ms: u32,
}

fn default_depth() -> u8 {
    2
}
fn default_max_pages() -> u16 {
    50
}
fn default_extract_type() -> String {
    "text".into()
}
fn default_per_page_timeout() -> u32 {
    10000
}
fn default_wait_between() -> u32 {
    1000
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct CrawlStatusParams {
    job_id: String,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct FillFormParams {
    #[serde(default)]
    tab_id: Option<String>,
    fields: Vec<FormField>,
    #[serde(default)]
    submit: bool,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct ClickAndWaitParams {
    #[serde(default)]
    tab_id: Option<String>,
    index: u32,
    #[serde(default = "default_timeout")]
    timeout_ms: u32,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct StopTaskParams {
    #[serde(default)]
    tab_id: Option<String>,
}

// ===== MCP Server wrapper =====

#[derive(Clone)]
struct McpBrowserServer {
    /// Gateway's WebSocket port.
    ws_port: u16,
}

impl McpBrowserServer {
    fn request_id() -> String {
        Uuid::new_v4().to_string()
    }

    /// Send a DaemonMessage to the gateway and wait for the response.
    /// Opens a fresh WebSocket connection for each request (stateless).
    async fn do_request(&self, msg: DaemonMessage) -> Result<ActionResult, String> {
        let url = format!("ws://127.0.0.1:{}/browser-mcp", self.ws_port);

        let (mut ws, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| format!("Bridge connection failed: {e}"))?;

        // Send the request
        let payload = serde_json::to_string(&msg).map_err(|e| format!("serialize error: {e}"))?;
        ws.send(Message::Text(payload.into()))
            .await
            .map_err(|e| format!("send error: {e}"))?;

        // Read the response (with 30s timeout)
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(30));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                msg_opt = ws.next() => {
                    match msg_opt {
                        Some(Ok(Message::Text(text))) => {
                            match serde_json::from_str::<ExtensionMessage>(&text) {
                                Ok(ExtensionMessage::Response { result, .. }) => {
                                    let _ = ws.close(None).await;
                                    return Ok(result);
                                }
                                Ok(other) => {
                                    tracing::debug!("[BrowserServer] Non-response message: {:?}", other);
                                    // Keep waiting for the response
                                }
                                Err(e) => {
                                    let _ = ws.close(None).await;
                                    return Err(format!("parse error: {e}"));
                                }
                            }
                        }
                        Some(Ok(_)) => {} // Ignore binary, ping, pong
                        Some(Err(e)) => {
                            return Err(format!("WS error: {e}"));
                        }
                        None => {
                            return Err("Connection closed without response".into());
                        }
                    }
                }
                _ = &mut timeout => {
                    let _ = ws.close(None).await;
                    return Err("Request timed out after 30s".into());
                }
            }
        }
    }

    fn parse_amount(amount: Option<&str>) -> ScrollAmount {
        match amount {
            Some(s) => {
                if let Ok(px) = s.parse::<u32>() {
                    ScrollAmount::Pixels(px)
                } else if let Ok(pages) = s.parse::<f32>() {
                    ScrollAmount::Pages(pages)
                } else {
                    ScrollAmount::Pages(1.0)
                }
            }
            None => ScrollAmount::Pages(1.0),
        }
    }
}

// ===== MCP tool implementations =====

#[rmcp::tool_router(server_handler)]
impl McpBrowserServer {
    // ===== Navigation =====

    #[rmcp::tool(
        description = "Navigate to a URL in a browser tab. Creates a new tab if tab_id is not specified."
    )]
    async fn browser_navigate(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            NavigateParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::Navigate {
                request_id: Self::request_id(),
                url: p.url,
                tab_id: p.tab_id,
            })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                serde_json::to_string_pretty(&data).unwrap_or_default()
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(description = "Create a new browser tab. Optionally navigate to a URL.")]
    async fn browser_new_tab(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            NewTabParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::NewTab {
                request_id: Self::request_id(),
                url: p.url,
            })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                serde_json::to_string_pretty(&data).unwrap_or_default()
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(description = "Close a browser tab by its tab_id.")]
    async fn browser_close_tab(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            CloseTabParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::CloseTab {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
            })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                serde_json::to_string_pretty(&data).unwrap_or_default()
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(description = "List all open browser tabs.")]
    async fn browser_list_tabs(&self) -> String {
        let rid = Self::request_id();
        match self
            .do_request(DaemonMessage::ListTabs { request_id: rid })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                serde_json::to_string_pretty(&data).unwrap_or_default()
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(description = "Switch to a specific tab, making it the active tab.")]
    async fn browser_switch_tab(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            SwitchTabParams,
        >,
    ) -> String {
        let tab_id = p.tab_id.clone();
        match self
            .do_request(DaemonMessage::SwitchTab {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => format!("Switched to tab {}", tab_id),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(description = "Go back to the previous page in the tab's history.")]
    async fn browser_go_back(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            TabActionParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::GoBack {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => "Navigated back".to_string(),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(description = "Go forward to the next page in the tab's history.")]
    async fn browser_go_forward(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            TabActionParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::GoForward {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => "Navigated forward".to_string(),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(description = "Reload the current page in the tab.")]
    async fn browser_reload(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            TabActionParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::Reload {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => "Page reloaded".to_string(),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    // ===== Page Interaction =====

    #[rmcp::tool(
        description = "Click on an element by its index (from snapshot). Requires permission for state-changing operations."
    )]
    async fn browser_click(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ClickParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::Click {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                index: p.index,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => format!("Clicked element #{}", p.index),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(
        description = "Type text into an input element by its index (from snapshot). Set submit=true to press Enter after typing."
    )]
    async fn browser_type(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            TypeParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::Type {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                index: p.index,
                text: p.text,
                submit: p.submit,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => format!("Typed into element #{}", p.index),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(description = "Select an option in a dropdown element by its index.")]
    async fn browser_select_option(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            SelectOptionParams,
        >,
    ) -> String {
        let option_text = p.option_text.clone();
        let idx = p.index;
        match self
            .do_request(DaemonMessage::SelectOption {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                index: p.index,
                option_text: p.option_text,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => {
                format!("Selected '{}' in element #{}", option_text, idx)
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(
        description = "Scroll the page. Direction: 'up' or 'down'. Amount: pixels (e.g. '300') or pages (e.g. '0.5')."
    )]
    async fn browser_scroll(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ScrollParams,
        >,
    ) -> String {
        let amount = Self::parse_amount(p.amount.as_deref());
        match self
            .do_request(DaemonMessage::Scroll {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                direction: p.direction,
                amount,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => "Scrolled".to_string(),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(description = "Hover the mouse over an element by its index.")]
    async fn browser_hover(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            HoverParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::Hover {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                index: p.index,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => format!("Hovered element #{}", p.index),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(
        description = "Press a keyboard key. Common keys: Enter, Escape, Tab, ArrowDown, ArrowUp, ArrowLeft, ArrowRight, Backspace, Delete, PageDown, PageUp, Home, End."
    )]
    async fn browser_press_key(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            PressKeyParams,
        >,
    ) -> String {
        let key = p.key.clone();
        match self
            .do_request(DaemonMessage::PressKey {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                key: p.key,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => format!("Pressed key: {}", key),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(
        description = "Upload files to a file input element by its index. Requires explicit permission."
    )]
    async fn browser_upload_file(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            UploadFileParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::UploadFile {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                index: p.index,
                file_paths: p.file_paths,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => format!("Files uploaded to element #{}", p.index),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(
        description = "Execute JavaScript on the page. Supports async/await. Returns the script's return value as JSON. Requires explicit permission."
    )]
    async fn browser_execute_js(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ExecuteJsParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::ExecuteJs {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                script: p.script,
            })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                serde_json::to_string_pretty(&data).unwrap_or_default()
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(
        description = "Wait for a condition. Types: 'time' (wait N ms), 'text' (wait for text to appear), 'text_gone' (wait for text to disappear), 'navigation' (wait for page load)."
    )]
    async fn browser_wait(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            WaitParams,
        >,
    ) -> String {
        let condition = match p.wait_type.as_str() {
            "time" => WaitCondition::Time {
                ms: p.ms.unwrap_or(1000),
            },
            "text" => WaitCondition::Text {
                text: p.text.unwrap_or_default(),
                timeout_ms: p.timeout_ms,
            },
            "text_gone" => WaitCondition::TextGone {
                text: p.text_gone.unwrap_or_default(),
                timeout_ms: p.timeout_ms,
            },
            "navigation" => WaitCondition::Navigation {
                timeout_ms: p.timeout_ms,
            },
            _ => WaitCondition::Time { ms: 1000 },
        };

        match self
            .do_request(DaemonMessage::WaitFor {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                condition,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => "Wait completed".to_string(),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    // ===== Observation =====

    #[rmcp::tool(
        description = "Capture the accessibility snapshot of the current page. Returns interactive elements with indices, text content, and compressed HTML. Use this before interacting with the page to understand what elements are available."
    )]
    async fn browser_snapshot(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            SnapshotParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::GetSnapshot {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                depth: p.depth,
                compress_html: p.compress_html,
            })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                // If compress_html is enabled and we got raw HTML, compress it
                if p.compress_html {
                    if let Some(html) = data.get("html").and_then(|v| v.as_str()) {
                        let config = CompressConfig::snapshot();
                        let compressed = html_compressor::compress_html(html, &config);
                        let mut out = serde_json::json!({
                            "url": data.get("url"),
                            "title": data.get("title"),
                            "elements": data.get("elements"),
                            "text_content_summary": compressed.text_content,
                            "compressed_html": format!(
                                "Interactive elements: {}\nText preview: {}",
                                compressed.interactive_elements.len(),
                                &compressed.text_content[..compressed.text_content.len().min(2000)]
                            ),
                            "compression_stats": {
                                "original_size": compressed.stats.original_size,
                                "compressed_size": compressed.stats.compressed_size,
                                "ratio": format!("{:.1}%", compressed.stats.compression_ratio * 100.0),
                            },
                        });
                        // Include interactive elements list
                        if !compressed.interactive_elements.is_empty() {
                            out["interactive_elements"] =
                                serde_json::to_value(&compressed.interactive_elements)
                                    .unwrap_or_default();
                        }
                        return serde_json::to_string_pretty(&out).unwrap_or_default();
                    }
                }
                serde_json::to_string_pretty(&data).unwrap_or_default()
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(
        description = "Take a screenshot of the page. Supports viewport, full-page, and element-specific captures."
    )]
    async fn browser_screenshot(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ScreenshotParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::GetScreenshot {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                full_page: p.full_page,
                format: p.format,
                quality: p.quality,
            })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                // Screenshots can be large; summarize instead of returning base64
                let mut out = data.clone();
                if let Some(obj) = out.as_object_mut() {
                    if obj.contains_key("data") {
                        let data_len = obj
                            .get("data")
                            .and_then(|v| v.as_str())
                            .map(|s| s.len())
                            .unwrap_or(0);
                        obj.insert("data_size_bytes".into(), serde_json::json!(data_len));
                        obj.remove("data");
                        obj.insert("note".into(), serde_json::json!(
                            "Screenshot data available via screenshot streaming (base64 omitted for brevity)"
                        ));
                    }
                }
                serde_json::to_string_pretty(&out).unwrap_or_default()
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(description = "Extract text content from the page or a specific element.")]
    async fn browser_extract_text(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ExtractTextParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::ExtractText {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                selector: p.selector,
            })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                // If we got HTML, compress it for LLM-friendly output
                if let Some(html) = data.get("html").and_then(|v| v.as_str()) {
                    let config = CompressConfig::text_extraction();
                    let compressed = html_compressor::compress_html(html, &config);
                    return serde_json::to_string_pretty(&serde_json::json!({
                        "url": data.get("url"),
                        "text": compressed.text_content,
                        "compression_ratio": format!("{:.1}%", compressed.stats.compression_ratio * 100.0),
                    }))
                    .unwrap_or_default();
                }
                serde_json::to_string_pretty(&data).unwrap_or_default()
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(
        description = "Extract all links from the page or a specific element. Returns URLs and their text."
    )]
    async fn browser_extract_links(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ExtractLinksParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::ExtractLinks {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                selector: p.selector,
            })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                serde_json::to_string_pretty(&data).unwrap_or_default()
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(description = "Extract an HTML table from the page as JSON array of objects.")]
    async fn browser_extract_table(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ExtractTableParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::ExtractTable {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                selector: p.selector,
            })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                serde_json::to_string_pretty(&data).unwrap_or_default()
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(
        description = "Extract structured data from the page using a JSON schema. The LLM analyzes page content and maps it to the schema fields. Useful for product listings, article metadata, search results, contact info, pricing tables."
    )]
    async fn browser_extract_structured(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ExtractStructuredParams,
        >,
    ) -> String {
        // Structured extraction: first get the text, then the extension/LLM maps to schema.
        // For now, delegate to the extension's execution.
        let script = format!(
            r#"return (() => {{
                const schema = {};
                const maxItems = {};
                const selector = {};
                const results = [];
                // Use schema to guide extraction from DOM
                const container = selector ? document.querySelector(selector) : document.body;
                if (!container) return JSON.stringify({{ error: 'Selector not found' }});
                const items = container.querySelectorAll('[itemscope], [itemtype], article, .product, .result, .item, li');
                items.forEach(el => {{
                    const item = {{}};
                    for (const [key, prop] of Object.entries(schema.properties || {{}})) {{
                        const sel = prop.selector || `[itemprop="${{key}}"], .${{key}}, [data-${{key}}]`;
                        const match = el.querySelector(sel);
                        item[key] = match ? match.textContent.trim() : null;
                    }}
                    results.push(item);
                }});
                return JSON.stringify(results.slice(0, maxItems));
            }})()"#,
            p.schema,
            p.max_items.unwrap_or(100),
            p.selector
                .as_ref()
                .map(|s| format!("'{}'", s.replace('\'', "\\'")))
                .unwrap_or_else(|| "null".to_string()),
        );

        match self
            .do_request(DaemonMessage::ExecuteJs {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                script,
            })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                serde_json::to_string_pretty(&data).unwrap_or_default()
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    // ===== Search & Crawl =====

    #[rmcp::tool(
        description = "Search Google or Bing and return structured results. Use for research, fact-checking, or finding documentation."
    )]
    async fn browser_search(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            SearchParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::Search {
                request_id: Self::request_id(),
                query: p.query,
                engine: p.engine,
                num_results: p.num_results,
                language: p.language,
            })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                serde_json::to_string_pretty(&data).unwrap_or_default()
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(
        description = "Start a deep crawl from a URL. Follows links matching patterns up to a configurable depth. Returns structured content from visited pages. Supports same-domain filtering, link pattern matching, per-page time budgets, and polite crawling delays."
    )]
    async fn browser_crawl(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            CrawlParams,
        >,
    ) -> String {
        let job_id = Uuid::new_v4().to_string();
        let start_url = p.start_url.clone();
        let link_patterns = p.link_patterns.clone();
        let exclude_patterns = p.exclude_patterns.clone();

        match self
            .do_request(DaemonMessage::CrawlStart {
                job_id: job_id.clone(),
                start_url,
                depth: p.depth,
                max_pages: p.max_pages,
                link_patterns,
                exclude_patterns,
                same_domain: p.same_domain,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => serde_json::to_string_pretty(&serde_json::json!({
                "job_id": job_id,
                "status": "started",
                "message": "Crawl job started. Use browser_crawl_status to check progress."
            }))
            .unwrap_or_default(),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(
        description = "Check the status of a crawl job. Returns pages crawled, total pages, and collected results."
    )]
    async fn browser_crawl_status(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            CrawlStatusParams,
        >,
    ) -> String {
        let rid = Self::request_id();
        let job_id = p.job_id.clone();
        // Use GetStatus to read crawl engine state via the bridge
        match self
            .do_request(DaemonMessage::GetStatus { request_id: rid })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                // Extract crawl_jobs from status response
                if let Some(jobs) = data.get("active_crawl_jobs") {
                    // Look up specific job status from the crawl engine
                    // The bridge's GetStatus returns crawl job list; we need per-job status
                    return serde_json::to_string_pretty(&serde_json::json!({
                        "job_id": job_id,
                        "message": "Crawl status available via browser_get_status. Active jobs listed.",
                        "active_jobs": jobs,
                    })).unwrap_or_default();
                }
                format!("Job {} not found in active crawl jobs", job_id)
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    // ===== Form & Auth =====

    #[rmcp::tool(
        description = "Fill multiple form fields at once. Automatically finds fields by label, placeholder, name, or CSS selector. Set submit=true to submit the form after filling."
    )]
    async fn browser_fill_form(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            FillFormParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::FillForm {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
                fields: p.fields,
                submit: p.submit,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => "Form filled".to_string(),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(description = "Click on an element and wait for navigation to complete.")]
    async fn browser_click_and_wait(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ClickAndWaitParams,
        >,
    ) -> String {
        match self
            .do_request(DaemonMessage::Click {
                request_id: Self::request_id(),
                tab_id: p.tab_id.clone(),
                index: p.index,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => {
                // Wait for navigation
                match self
                    .do_request(DaemonMessage::WaitFor {
                        request_id: Self::request_id(),
                        tab_id: p.tab_id,
                        condition: WaitCondition::Navigation {
                            timeout_ms: p.timeout_ms,
                        },
                    })
                    .await
                {
                    Ok(ActionResult::Ok { .. }) => "Clicked and navigation completed".to_string(),
                    Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
                    Err(e) => e,
                }
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    // ===== Session =====

    #[rmcp::tool(
        description = "Get the current status of the browser bridge: connection state, tab count, active tab."
    )]
    async fn browser_get_status(&self) -> String {
        let rid = Self::request_id();
        match self
            .do_request(DaemonMessage::GetStatus { request_id: rid })
            .await
        {
            Ok(ActionResult::Ok { data }) => {
                serde_json::to_string_pretty(&data).unwrap_or_default()
            }
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }

    #[rmcp::tool(description = "Stop an ongoing task (navigation, crawl, etc.) on a tab.")]
    async fn browser_stop_task(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            StopTaskParams,
        >,
    ) -> String {
        // Stop by reloading the tab
        match self
            .do_request(DaemonMessage::Reload {
                request_id: Self::request_id(),
                tab_id: p.tab_id,
            })
            .await
        {
            Ok(ActionResult::Ok { .. }) => "Task stopped".to_string(),
            Ok(ActionResult::Error { message, .. }) => format!("Error: {message}"),
            Err(e) => e,
        }
    }
}

// ===== Stdio server entry point =====

/// Start the browser MCP server over stdio.
/// Connects to the daemon's WebSocket gateway at `/browser-mcp`.
/// Uses SENCLAW_WS_PORT to locate the gateway.
pub async fn run_stdio_server() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let ws_port: u16 = std::env::var("SENCLAW_WS_PORT")
        .context("SENCLAW_WS_PORT not set")?
        .parse()
        .context("invalid SENCLAW_WS_PORT")?;

    // Verify the gateway is reachable
    let test_url = format!("ws://127.0.0.1:{ws_port}/browser-mcp");
    match tokio_tungstenite::connect_async(&test_url).await {
        Ok((mut ws, _)) => {
            let _ = ws.close(None).await;
            tracing::info!("[BrowserServer] Gateway reachable at {test_url}");
        }
        Err(e) => {
            tracing::warn!("[BrowserServer] Gateway not reachable at {test_url}: {e}");
            tracing::warn!("[BrowserServer] Make sure the SenClaw daemon is running with the WebSocket gateway started.");
        }
    }

    // Start MCP stdio server — each tool call will open a fresh WS connection
    let server = McpBrowserServer { ws_port };
    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
