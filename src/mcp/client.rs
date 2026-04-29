//! Lightweight MCP client — JSON-RPC over stdio.
//!
//! Spawns senclaw MCP server subprocesses and communicates via stdin/stdout.
//! Mirrors the TS `child_process.spawn()` + JSON-RPC pattern used in the
//! original sema-core MCP harness.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tracing::{debug, info};

// ===== JSON-RPC types =====

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[allow(dead_code)]
    id: Option<u64>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: i64,
    message: String,
}

// ===== Tool info =====

#[derive(Debug, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "inputSchema")]
    pub input_schema: Value,
}

// ===== Client =====

/// A running MCP server subprocess with JSON-RPC communication.
pub struct McpClient {
    _name: String,
    child: Child,
    next_id: u64,
    reader: BufReader<tokio::process::ChildStdout>,
    writer: tokio::process::ChildStdin,
}

impl McpClient {
    /// Spawn the MCP server subprocess and perform the initialize handshake.
    pub async fn spawn(
        name: &str,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self> {
        info!("MCP spawn: {name} ({command} {})", args.join(" "));

        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd.envs(env);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit());
        cmd.kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn MCP server: {name}"))?;

        let stdout = child.stdout.take().context("MCP stdout pipe")?;
        let stdin = child.stdin.take().context("MCP stdin pipe")?;
        let reader = BufReader::new(stdout);

        let mut client = Self {
            _name: name.to_string(),
            child,
            next_id: 1,
            reader,
            writer: stdin,
        };

        // Initialize handshake
        client
            .request("initialize", Some(serde_json::json!({
                "protocolVersion": "0.1.0",
                "capabilities": {},
                "clientInfo": {
                    "name": "senclaw-engine",
                    "version": "0.1.0"
                }
            })))
            .await?;

        info!("MCP {name} initialized");
        Ok(client)
    }

    /// List available tools from this MCP server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpToolInfo>> {
        let response = self.request("tools/list", None).await?;
        let tools: Vec<McpToolInfo> = response
            .get("tools")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        debug!(
            "MCP {}: {} tool(s) available",
            self._name,
            tools.len()
        );
        Ok(tools)
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value> {
        self.request(
            "tools/call",
            Some(serde_json::json!({
                "name": name,
                "arguments": arguments,
            })),
        )
        .await
    }

    /// Send a JSON-RPC request and wait for the response.
    async fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        let mut body = serde_json::to_vec(&req)?;
        body.push(b'\n');
        self.writer.write_all(&body).await?;

        // Read response line
        let mut line = String::new();
        self.reader.read_line(&mut line).await?;

        let resp: JsonRpcResponse = serde_json::from_str(&line)
            .with_context(|| format!("MCP JSON-RPC parse error: {line}"))?;

        if let Some(err) = resp.error {
            bail!("MCP error ({}): {}", method, err.message);
        }

        Ok(resp.result.unwrap_or(Value::Null))
    }

    /// Check if process has exited.
    pub fn try_wait(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().ok().flatten()
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        // kill_on_drop is set, so the process will be killed automatically.
        info!("MCP {} — subprocess dropped", self._name);
    }
}

/// Registry of active MCP clients keyed by server name.
#[derive(Default)]
pub struct McpRegistry {
    clients: HashMap<String, McpClient>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    /// Spawn or replace an MCP server.
    pub async fn spawn(
        &mut self,
        name: &str,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Vec<McpToolInfo>> {
        // Kill old instance if present (drops it, releasing OS resources).
        self.kill(name);
        let mut client = McpClient::spawn(name, command, args, env).await?;
        let tools = client.list_tools().await.unwrap_or_default();
        self.clients.insert(name.to_string(), client);
        Ok(tools)
    }

    /// Spawn without holding &mut self across .await.
    /// Returns the client so the caller can insert it under the lock.
    pub async fn spawn_client(
        name: &str,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<(McpClient, Vec<McpToolInfo>)> {
        let mut client = McpClient::spawn(name, command, args, env).await?;
        let tools = client.list_tools().await.unwrap_or_default();
        Ok((client, tools))
    }

    /// Kill a specific MCP server by name.
    pub fn kill(&mut self, name: &str) {
        if let Some(_client) = self.clients.remove(name) {
            info!("MCP {} — killed", name);
        }
    }

    /// Kill all MCP servers.
    pub fn kill_all(&mut self) {
        let names: Vec<String> = self.clients.keys().cloned().collect();
        for name in &names {
            self.kill(name);
        }
    }
}

impl Drop for McpRegistry {
    fn drop(&mut self) {
        self.kill_all();
    }
}

/// Shared, thread-safe MCP client registry.
#[derive(Clone)]
pub struct SharedMcpRegistry {
    inner: Arc<std::sync::Mutex<McpRegistry>>,
}

impl SharedMcpRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(std::sync::Mutex::new(McpRegistry::new())),
        }
    }

    pub async fn spawn(
        &self,
        name: &str,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Vec<McpToolInfo>> {
        // Kill old instance under the lock, then spawn outside it
        // so MutexGuard doesn't live across .await.
        {
            let mut reg = self.inner.lock().unwrap();
            reg.kill(name);
        }
        let (client, tools) =
            McpRegistry::spawn_client(name, command, args, env).await?;
        let mut reg = self.inner.lock().unwrap();
        reg.clients.insert(name.to_string(), client);
        Ok(tools)
    }

    pub fn kill(&self, name: &str) {
        let mut reg = self.inner.lock().unwrap();
        reg.kill(name);
    }

    pub fn kill_all(&self) {
        let mut reg = self.inner.lock().unwrap();
        reg.kill_all();
    }
}
