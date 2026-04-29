//! Feishu wiki MCP server. Port target: src-old/mcp/feishu-wiki-server.ts
//!
//! Tools (8): wiki_list_spaces, wiki_get_space, wiki_list_nodes, wiki_get_node,
//!   wiki_create_node, wiki_search, doc_read_blocks, doc_write_blocks.
//! Uses Feishu / Lark REST API via reqwest.

use anyhow::Context;
use crate::mcp::schedule_server::ToolResult;

use rmcp::ServiceExt;

// ===== Domain resolution =====

fn feishu_base_url(domain: &str) -> String {
    match domain {
        "lark" => "https://open.larksuite.com".into(),
        _ => "https://open.feishu.cn".into(),
    }
}

// ===== Lark Client =====

struct LarkClient {
    app_id: String,
    app_secret: String,
    base_url: String,
    tenant_token: std::sync::Mutex<Option<(String, i64)>>, // (token, expires_at_ms)
}

impl LarkClient {
    fn new(app_id: &str, app_secret: &str, domain: &str) -> Self {
        Self {
            app_id: app_id.to_owned(),
            app_secret: app_secret.to_owned(),
            base_url: feishu_base_url(domain),
            tenant_token: std::sync::Mutex::new(None),
        }
    }

    async fn get_token(&self) -> Result<String, String> {
        // Check cached token
        {
            let cached = self.tenant_token.lock().unwrap();
            if let Some((ref token, expires)) = *cached {
                let now = chrono::Utc::now().timestamp_millis();
                if now < expires - 60_000 {
                    return Ok(token.clone());
                }
            }
        }

        let client = reqwest::Client::new();
        let res = client
            .post(format!("{}/open-apis/auth/v3/tenant_access_token/internal", self.base_url))
            .json(&serde_json::json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            }))
            .send()
            .await
            .map_err(|e| format!("Token request failed: {e}"))?;

        let body: serde_json::Value = res.json().await.map_err(|e| format!("Token parse: {e}"))?;
        let code = body
            .get("code")
            .and_then(|c| c.as_i64())
            .unwrap_or(-1);

        if code != 0 {
            let msg = body
                .get("msg")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");
            return Err(format!("Feishu auth error ({code}): {msg}"));
        }

        let token = body
            .get("tenant_access_token")
            .and_then(|t| t.as_str())
            .ok_or("Missing tenant_access_token")?
            .to_owned();
        let expire = body
            .get("expire")
            .and_then(|e| e.as_i64())
            .unwrap_or(7200);

        let expires_at = chrono::Utc::now().timestamp_millis() + (expire as i64 * 1000);
        *self.tenant_token.lock().unwrap() = Some((token.clone(), expires_at));
        Ok(token)
    }

    async fn call_api(&self, method: &str, path: &str, body: Option<&serde_json::Value>) -> Result<serde_json::Value, String> {
        let token = self.get_token().await?;
        let client = reqwest::Client::new();
        let url = format!("{}{}", self.base_url, path);

        let req = match method {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            "PATCH" => client.patch(&url),
            _ => return Err(format!("Unknown HTTP method: {method}")),
        };

        let req = req
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json; charset=utf-8");

        let req = if let Some(b) = body {
            req.json(b)
        } else {
            req
        };

        let res = req.send().await.map_err(|e| format!("API call failed: {e}"))?;
        let status = res.status();
        let body: serde_json::Value = res.json().await.map_err(|e| format!("Response parse: {e}"))?;

        if !status.is_success() {
            let msg = body
                .get("msg")
                .and_then(|m| m.as_str())
                .unwrap_or("request failed");
            let code = body
                .get("code")
                .and_then(|c| c.as_i64())
                .unwrap_or(0);
            return Err(format!("Feishu API error ({code}): {msg}"));
        }

        Ok(body)
    }
}

// ===== Server =====

pub struct FeishuWikiServer {
    client: LarkClient,
}

impl FeishuWikiServer {
    pub fn new(app_id: &str, app_secret: &str, domain: Option<&str>) -> Self {
        Self {
            client: LarkClient::new(app_id, app_secret, domain.unwrap_or("feishu")),
        }
    }

    // ===== wiki_list_spaces =====

    pub async fn wiki_list_spaces(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> ToolResult {
        let mut query = format!("page_size={}", page_size.unwrap_or(20));
        if let Some(t) = page_token {
            query.push_str(&format!("&page_token={t}"));
        }
        match self.client.call_api("GET", &format!("/open-apis/wiki/v2/spaces?{query}"), None).await {
            Ok(res) => {
                let data = res.get("data");
                let items = data.and_then(|d| d.get("items")).and_then(|i| i.as_array());
                match items {
                    Some(items) if !items.is_empty() => {
                        let lines: Vec<String> = items
                            .iter()
                            .map(|s| {
                                let name = s.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                                let space_id = s.get("space_id").and_then(|n| n.as_str()).unwrap_or("?");
                                let desc = s.get("description").and_then(|n| n.as_str()).unwrap_or("No description");
                                let vis = s.get("visibility").and_then(|n| n.as_str()).unwrap_or("private");
                                format!("- **{name}** (space_id: `{space_id}`) — {desc} [{vis}]")
                            })
                            .collect();
                        let mut result = lines.join("\n");
                        if data.and_then(|d| d.get("has_more")).and_then(|h| h.as_bool()).unwrap_or(false) {
                            if let Some(pt) = data.and_then(|d| d.get("page_token")).and_then(|t| t.as_str()) {
                                result.push_str(&format!("\n\n_More results available. Use page_token: `{pt}`_"));
                            }
                        }
                        ToolResult::ok(result)
                    }
                    _ => ToolResult::ok("No accessible wiki spaces found. Please ensure the bot is added as a wiki member.".into()),
                }
            }
            Err(e) => ToolResult::err(format!("{e}")),
        }
    }

    // ===== wiki_get_space =====

    pub async fn wiki_get_space(&self, space_id: &str) -> ToolResult {
        match self
            .client
            .call_api("GET", &format!("/open-apis/wiki/v2/spaces/{space_id}"), None)
            .await
        {
            Ok(res) => {
                let space = res.get("data").and_then(|d| d.get("space"));
                match space {
                    Some(s) => ToolResult::ok(serde_json::to_string_pretty(s).unwrap_or_default()),
                    None => ToolResult::err("Space not found".into()),
                }
            }
            Err(e) => ToolResult::err(format!("{e}")),
        }
    }

    // ===== wiki_list_nodes =====

    pub async fn wiki_list_nodes(
        &self,
        space_id: &str,
        parent_node_token: Option<&str>,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> ToolResult {
        let mut params = vec![("page_size", page_size.unwrap_or(20).to_string())];
        if let Some(t) = parent_node_token {
            params.push(("parent_node_token", t.to_owned()));
        }
        if let Some(t) = page_token {
            params.push(("page_token", t.to_owned()));
        }
        let query = params.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
        match self
            .client
            .call_api("GET", &format!("/open-apis/wiki/v2/spaces/{space_id}/nodes?{query}"), None)
            .await
        {
            Ok(res) => {
                let data = res.get("data");
                let items = data.and_then(|d| d.get("items")).and_then(|i| i.as_array());
                match items {
                    Some(items) if !items.is_empty() => {
                        let lines: Vec<String> = items
                            .iter()
                            .map(|n| {
                                let title = n.get("title").and_then(|t| t.as_str()).unwrap_or("(Untitled)");
                                let obj_type = n.get("obj_type").and_then(|t| t.as_str()).unwrap_or("?");
                                let node_token = n.get("node_token").and_then(|t| t.as_str()).unwrap_or("?");
                                let obj_token = n.get("obj_token").and_then(|t| t.as_str()).unwrap_or("?");
                                let has_child = n.get("has_child").and_then(|c| c.as_bool()).unwrap_or(false);
                                format!(
                                    "- {title} — type: {obj_type}, node_token: `{node_token}`, obj_token: `{obj_token}`{}",
                                    if has_child { " (has children)" } else { "" }
                                )
                            })
                            .collect();
                        let mut result = lines.join("\n");
                        if data.and_then(|d| d.get("has_more")).and_then(|h| h.as_bool()).unwrap_or(false) {
                            if let Some(pt) = data.and_then(|d| d.get("page_token")).and_then(|t| t.as_str()) {
                                result.push_str(&format!("\n\n_More results: page_token=`{pt}`_"));
                            }
                        }
                        ToolResult::ok(result)
                    }
                    _ => ToolResult::ok("No child nodes under this node.".into()),
                }
            }
            Err(e) => ToolResult::err(format!("{e}")),
        }
    }

    // ===== wiki_get_node =====

    pub async fn wiki_get_node(&self, token: &str) -> ToolResult {
        match self
            .client
            .call_api("GET", &format!("/open-apis/wiki/v2/nodes/{token}"), None)
            .await
        {
            Ok(res) => {
                let node = res.get("data").and_then(|d| d.get("node"));
                match node {
                    Some(n) => ToolResult::ok(serde_json::to_string_pretty(n).unwrap_or_default()),
                    None => ToolResult::err("Node not found".into()),
                }
            }
            Err(e) => ToolResult::err(format!("{e}")),
        }
    }

    // ===== wiki_create_node =====

    pub async fn wiki_create_node(
        &self,
        space_id: &str,
        obj_type: &str,
        title: Option<&str>,
        parent_node_token: Option<&str>,
    ) -> ToolResult {
        match self
            .client
            .call_api(
                "POST",
                &format!("/open-apis/wiki/v2/spaces/{space_id}/nodes"),
                Some(&serde_json::json!({
                    "obj_type": obj_type,
                    "node_type": "origin",
                    "title": title,
                    "parent_node_token": parent_node_token,
                })),
            )
            .await
        {
            Ok(res) => {
                let node = res.get("data").and_then(|d| d.get("node"));
                match node {
                    Some(n) => {
                        let nt = n.get("node_token").and_then(|t| t.as_str()).unwrap_or("?");
                        let ot = n.get("obj_token").and_then(|t| t.as_str()).unwrap_or("?");
                        let mut lines = vec![
                            "Node created".to_owned(),
                            format!("  node_token: `{nt}`"),
                            format!("  obj_token: `{ot}`"),
                            format!("  type: {obj_type}"),
                        ];
                        if let Some(t) = title {
                            lines.push(format!("  title: {t}"));
                        }
                        if obj_type == "docx" {
                            lines.push(String::new());
                            lines.push("Use doc_write_blocks to write content (document_id = obj_token)".into());
                        }
                        ToolResult::ok(lines.join("\n"))
                    }
                    None => ToolResult::err("Failed to create node: node info not returned".into()),
                }
            }
            Err(e) => ToolResult::err(format!("{e}")),
        }
    }

    // ===== wiki_search =====

    pub async fn wiki_search(
        &self,
        query: &str,
        space_id: Option<&str>,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> ToolResult {
        let mut params = vec![
            ("page_size".to_owned(), page_size.unwrap_or(20).to_string()),
        ];
        if let Some(t) = page_token {
            params.push(("page_token".to_owned(), t.to_owned()));
        }
        let qs = params.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
        let url = format!("/open-apis/wiki/v2/search?{qs}");

        let mut body = serde_json::json!({
            "query": query,
        });
        if let Some(sid) = space_id {
            body["space_id"] = serde_json::Value::String(sid.to_owned());
        }

        match self.client.call_api("POST", &url, Some(&body)).await {
            Ok(res) => {
                let data = res.get("data");
                let items = data.and_then(|d| d.get("items")).and_then(|i| i.as_array());
                match items {
                    Some(items) if !items.is_empty() => {
                        let lines: Vec<String> = items
                            .iter()
                            .map(|n| {
                                let title = n.get("title").and_then(|t| t.as_str()).unwrap_or("(Untitled)");
                                let space = n.get("space_id").and_then(|t| t.as_str()).unwrap_or("?");
                                let node_token = n.get("node_token").and_then(|t| t.as_str()).unwrap_or("?");
                                let obj_type = n.get("obj_type").and_then(|t| t.as_str()).unwrap_or("?");
                                format!("- **{title}** — space: {space}, node_token: `{node_token}`, type: {obj_type}")
                            })
                            .collect();
                        let mut result = format!("Search \"{query}\" found {} results:\n\n", items.len())
                            + &lines.join("\n");
                        if data.and_then(|d| d.get("has_more")).and_then(|h| h.as_bool()).unwrap_or(false) {
                            if let Some(pt) = data.and_then(|d| d.get("page_token")).and_then(|t| t.as_str()) {
                                result.push_str(&format!("\n\n_More results: page_token=`{pt}`_"));
                            }
                        }
                        ToolResult::ok(result)
                    }
                    _ => ToolResult::ok(format!("No matching nodes found for \"{query}\".")),
                }
            }
            Err(e) => ToolResult::err(format!("{e}")),
        }
    }

    // ===== doc_read_blocks =====

    pub async fn doc_read_blocks(
        &self,
        document_id: &str,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> ToolResult {
        let mut params = vec![("page_size", page_size.unwrap_or(100).to_string())];
        if let Some(t) = page_token {
            params.push(("page_token", t.to_owned()));
        }
        let query = params.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
        match self
            .client
            .call_api(
                "GET",
                &format!("/open-apis/docx/v1/documents/{document_id}/blocks?{query}"),
                None,
            )
            .await
        {
            Ok(res) => {
                let data = res.get("data");
                let items = data.and_then(|d| d.get("items")).and_then(|i| i.as_array());
                match items {
                    Some(items) if !items.is_empty() => {
                        let blocks: Vec<serde_json::Value> = items
                            .iter()
                            .map(|b| {
                                let mut out = serde_json::json!({
                                    "block_id": b.get("block_id"),
                                    "block_type": b.get("block_type"),
                                    "parent_id": b.get("parent_id"),
                                });
                                // Extract text content
                                if let Some(obj) = b.as_object() {
                                    for (k, v) in obj {
                                        if !matches!(k.as_str(), "block_id" | "block_type" | "parent_id" | "children")
                                            && v.is_object()
                                        {
                                            out["content"] = v.clone();
                                            break;
                                        }
                                    }
                                    if let Some(children) = obj.get("children") {
                                        out["children"] = children.clone();
                                    }
                                }
                                out
                            })
                            .collect();
                        let mut result = serde_json::to_string_pretty(&blocks).unwrap_or_default();
                        if data.and_then(|d| d.get("has_more")).and_then(|h| h.as_bool()).unwrap_or(false) {
                            if let Some(pt) = data.and_then(|d| d.get("page_token")).and_then(|t| t.as_str()) {
                                result.push_str(&format!("\n\n_More blocks: page_token=`{pt}`_"));
                            }
                        }
                        ToolResult::ok(result)
                    }
                    _ => ToolResult::ok("Document is empty, no blocks found.".into()),
                }
            }
            Err(e) => ToolResult::err(format!("{e}")),
        }
    }

    // ===== doc_write_blocks =====

    pub async fn doc_write_blocks(
        &self,
        document_id: &str,
        parent_block_id: &str,
        blocks: Vec<DocBlockInput>,
        index: Option<u32>,
    ) -> ToolResult {
        let type_map: std::collections::HashMap<&str, u32> = [
            ("text", 2), ("heading1", 3), ("heading2", 4), ("heading3", 5),
            ("heading4", 6), ("heading5", 7), ("heading6", 8), ("heading7", 9),
            ("heading8", 10), ("heading9", 11), ("bullet", 12), ("ordered", 13),
            ("code", 14), ("todo", 17), ("divider", 22),
        ].iter().cloned().collect();

        let mut children = Vec::new();
        for b in &blocks {
            let block_type = match type_map.get(b.block_type.as_str()) {
                Some(t) => *t,
                None => return ToolResult::err(format!(
                    "Unsupported block type: \"{}\". Supported: {}",
                    b.block_type,
                    type_map.keys().copied().collect::<Vec<_>>().join(", ")
                )),
            };

            if b.block_type == "divider" {
                children.push(serde_json::json!({
                    "block_type": block_type,
                    "divider": {},
                }));
                continue;
            }

            if let Some(ref raw) = b.raw {
                children.push(serde_json::json!({
                    "block_type": block_type,
                    &b.block_type: raw,
                }));
                continue;
            }

            let text_content = b.content.as_deref().unwrap_or("");
            let elements = vec![serde_json::json!({
                "text_run": {
                    "content": text_content,
                    "text_element_style": {},
                }
            })];

            let body_key = if b.block_type.starts_with("heading") {
                b.block_type.clone()
            } else {
                b.block_type.clone()
            };

            let mut body = serde_json::json!({ "elements": elements });
            if b.block_type == "code" {
                body["style"] = serde_json::json!({ "language": 1, "wrap": true });
            }
            if b.block_type == "todo" {
                body["style"] = serde_json::json!({ "done": false });
            }

            children.push(serde_json::json!({
                "block_type": block_type,
                &body_key: body,
            }));
        }

        let mut data = serde_json::json!({ "children": children });
        if let Some(idx) = index {
            data["index"] = serde_json::Value::Number(idx.into());
        }

        match self
            .client
            .call_api(
                "POST",
                &format!("/open-apis/docx/v1/documents/{document_id}/blocks/{parent_block_id}/children"),
                Some(&data),
            )
            .await
        {
            Ok(res) => {
                let created = res.get("data")
                    .and_then(|d| d.get("children"))
                    .and_then(|c| c.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let new_ids: Vec<String> = res
                    .get("data")
                    .and_then(|d| d.get("children"))
                    .and_then(|c| c.as_array())
                    .map(|children| {
                        children
                            .iter()
                            .filter_map(|c| {
                                c.get("block_id").and_then(|id| id.as_str()).map(|s| format!("`{s}`"))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let mut lines = vec![
                    format!("Successfully wrote {created} blocks"),
                    format!("  document_id: `{document_id}`"),
                    format!("  parent_block_id: `{parent_block_id}`"),
                ];
                if !new_ids.is_empty() {
                    lines.push(format!("  new block_ids: {}", new_ids.join(", ")));
                }
                ToolResult::ok(lines.join("\n"))
            }
            Err(e) => ToolResult::err(format!("{e}")),
        }
    }
}

// ===== DocBlockInput =====

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct DocBlockInput {
    #[serde(rename = "blockType")]
    pub block_type: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub raw: Option<serde_json::Value>,
}

// ===== MCP stdio server =====

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct ListSpacesParams {
    #[serde(default)]
    #[serde(rename = "pageSize")]
    page_size: Option<u32>,
    #[serde(default)]
    #[serde(rename = "pageToken")]
    page_token: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct SpaceIdParams {
    #[serde(rename = "spaceId")]
    space_id: String,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct ListNodesParams {
    #[serde(rename = "spaceId")]
    space_id: String,
    #[serde(default)]
    #[serde(rename = "parentNodeToken")]
    parent_node_token: Option<String>,
    #[serde(default)]
    #[serde(rename = "pageSize")]
    page_size: Option<u32>,
    #[serde(default)]
    #[serde(rename = "pageToken")]
    page_token: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct TokenParams {
    token: String,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct CreateNodeParams {
    #[serde(rename = "spaceId")]
    space_id: String,
    #[serde(rename = "objType")]
    obj_type: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    #[serde(rename = "parentNodeToken")]
    parent_node_token: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct SearchParams {
    query: String,
    #[serde(default)]
    #[serde(rename = "spaceId")]
    space_id: Option<String>,
    #[serde(default)]
    #[serde(rename = "pageSize")]
    page_size: Option<u32>,
    #[serde(default)]
    #[serde(rename = "pageToken")]
    page_token: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct DocBlocksParams {
    #[serde(rename = "documentId")]
    document_id: String,
    #[serde(default)]
    #[serde(rename = "pageSize")]
    page_size: Option<u32>,
    #[serde(default)]
    #[serde(rename = "pageToken")]
    page_token: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct WriteBlocksParams {
    #[serde(rename = "documentId")]
    document_id: String,
    #[serde(rename = "parentBlockId")]
    parent_block_id: String,
    blocks: Vec<DocBlockInput>,
    #[serde(default)]
    index: Option<u32>,
}

#[derive(Clone)]
struct McpFeishuWikiServer {
    app_id: String,
    app_secret: String,
    domain: String,
}

impl McpFeishuWikiServer {
    fn inner(&self) -> FeishuWikiServer {
        FeishuWikiServer::new(&self.app_id, &self.app_secret, Some(&self.domain))
    }
}

#[rmcp::tool_router(server_handler)]
impl McpFeishuWikiServer {
    #[rmcp::tool(description = "List accessible Feishu/Lark wiki spaces")]
    async fn wiki_list_spaces(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<ListSpacesParams>,
    ) -> String {
        self.inner().wiki_list_spaces(p.page_size, p.page_token.as_deref()).await.content
    }

    #[rmcp::tool(description = "Get details of a specific wiki space")]
    async fn wiki_get_space(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<SpaceIdParams>,
    ) -> String {
        self.inner().wiki_get_space(&p.space_id).await.content
    }

    #[rmcp::tool(description = "List child nodes in a wiki space or parent node")]
    async fn wiki_list_nodes(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<ListNodesParams>,
    ) -> String {
        self.inner()
            .wiki_list_nodes(&p.space_id, p.parent_node_token.as_deref(), p.page_size, p.page_token.as_deref())
            .await
            .content
    }

    #[rmcp::tool(description = "Get details of a specific wiki node by token")]
    async fn wiki_get_node(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<TokenParams>,
    ) -> String {
        self.inner().wiki_get_node(&p.token).await.content
    }

    #[rmcp::tool(description = "Create a new node (doc or folder) in a wiki space")]
    async fn wiki_create_node(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<CreateNodeParams>,
    ) -> String {
        self.inner()
            .wiki_create_node(&p.space_id, &p.obj_type, p.title.as_deref(), p.parent_node_token.as_deref())
            .await
            .content
    }

    #[rmcp::tool(description = "Search wiki nodes by query text")]
    async fn wiki_search(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<SearchParams>,
    ) -> String {
        self.inner()
            .wiki_search(&p.query, p.space_id.as_deref(), p.page_size, p.page_token.as_deref())
            .await
            .content
    }

    #[rmcp::tool(description = "Read blocks from a Feishu/Lark document")]
    async fn doc_read_blocks(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<DocBlocksParams>,
    ) -> String {
        self.inner()
            .doc_read_blocks(&p.document_id, p.page_size, p.page_token.as_deref())
            .await
            .content
    }

    #[rmcp::tool(description = "Write content blocks to a Feishu/Lark document")]
    async fn doc_write_blocks(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<WriteBlocksParams>,
    ) -> String {
        self.inner()
            .doc_write_blocks(&p.document_id, &p.parent_block_id, p.blocks, p.index)
            .await
            .content
    }
}

/// Start the Feishu wiki MCP server over stdio.
pub async fn run_stdio_server() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let app_id = std::env::var("FEISHU_APP_ID").context("FEISHU_APP_ID not set")?;
    let app_secret = std::env::var("FEISHU_APP_SECRET").context("FEISHU_APP_SECRET not set")?;
    let domain = std::env::var("FEISHU_DOMAIN").unwrap_or_else(|_| "feishu".into());

    let server = McpFeishuWikiServer {
        app_id,
        app_secret,
        domain,
    };

    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
