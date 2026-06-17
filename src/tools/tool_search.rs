//! ToolSearch — discover deferred tools by keyword.
//!
//! Mirrors the `ToolSearchTool` pattern in `yasasbanukaofficial/claude-code`:
//! tools marked `should_defer() = true` are excluded from the initial tool
//! list sent to the LLM each turn (saves ~80% of tool-definition tokens).
//! The LLM then calls this tool with a query to find and load specialized
//! tools on demand.
//!
//! Result format: full tool schemas (name, description, input_schema) so the
//! LLM can call them directly in subsequent turns — no separate "load" step
//! needed; the next prompt will include the discovered tools automatically.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::zen_core::{Tool, ToolContext, ToolOutput, ToolResultMessage};

const DEFAULT_MAX_RESULTS: usize = 5;
const MAX_RESULTS_HARD_CAP: usize = 20;
const SELECT_PREFIX: &str = "select:";

/// Normalize alternate MCP naming schemes to the canonical bridge form.
/// e.g. `mcp__senclaw-browser__browser_search` → `mcp__browser__search`
pub fn normalize_mcp_tool_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("mcp__senclaw-") {
        if let Some((server, tool)) = rest.split_once("__") {
            let prefix = format!("{server}_");
            let clean_tool = tool.strip_prefix(&prefix).unwrap_or(tool);
            return format!("mcp__{server}__{clean_tool}");
        }
    }
    name.to_string()
}

fn mcp_name_parts(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("mcp__")?;
    rest.split_once("__")
}

/// Canonicalize a tool name for hyphen/underscore-insensitive comparison.
///
/// Models frequently emit `mcp__ssh-manager_mcp__foo` (or all underscores) for
/// a server registered as `ssh-manager-mcp`. The MCP bridge keeps hyphens in
/// the server segment, so an exact match misses. Folding `-` to `_` lets a tool
/// call resolve regardless of which separator the model chose.
fn canonical_tool_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Resolve a tool by exact name, alias, or normalized MCP alias.
pub fn resolve_tool_by_name(name: &str, tools: &[Arc<dyn Tool>]) -> Option<Arc<dyn Tool>> {
    if let Some(t) = tools.iter().find(|t| t.name() == name) {
        return Some(Arc::clone(t));
    }
    let normalized = normalize_mcp_tool_name(name);
    if normalized != name {
        if let Some(t) = tools.iter().find(|t| t.name() == normalized) {
            return Some(Arc::clone(t));
        }
    }
    // Bridge the stripped form against the registered full form. Tools register
    // under their full server prefix (`mcp__senclaw-browser__browser_search`),
    // but the model — and the skill docs — call them by the stripped bridge
    // form (`mcp__browser__search`). Normalizing the tool's OWN name too makes
    // the two meet, so the documented short name resolves to whatever long name
    // the manager actually registered.
    if let Some(t) = tools
        .iter()
        .find(|t| normalize_mcp_tool_name(t.name()) == normalized)
    {
        return Some(Arc::clone(t));
    }
    for t in tools {
        if t.aliases()
            .iter()
            .any(|a| *a == name || normalize_mcp_tool_name(a) == normalized)
        {
            return Some(Arc::clone(t));
        }
    }
    // Hyphen/underscore-insensitive match: `mcp__ssh-manager_mcp__x` should
    // resolve to a tool registered as `mcp__ssh-manager-mcp__x`.
    let canon = canonical_tool_name(&normalized);
    if let Some(t) = tools.iter().find(|t| canonical_tool_name(t.name()) == canon) {
        return Some(Arc::clone(t));
    }
    for t in tools {
        if t.aliases()
            .iter()
            .any(|a| canonical_tool_name(a) == canon)
        {
            return Some(Arc::clone(t));
        }
    }
    // Last resort: match MCP server + verb suffix (handles unstripped names).
    if let Some((server, verb)) = mcp_name_parts(&normalized) {
        let needle = format!("__{verb}");
        let canon_server = canonical_tool_name(server);
        tools
            .iter()
            .find(|t| {
                let n = t.name();
                n.ends_with(&needle)
                    && (canonical_tool_name(n).contains(&format!("mcp__{canon_server}__"))
                        || canonical_tool_name(n)
                            .contains(&format!("mcp__senclaw_{canon_server}__")))
            })
            .map(Arc::clone)
    } else {
        // Bare name without `mcp__` prefix — models sometimes strip the
        // `mcp__{server}__` prefix and emit just the verb or server+verb:
        //   - `event_create` for `mcp__space__event_create` (verb only)
        //   - `space_event_create` for `mcp__space__event_create` (server_verb)
        let canon_bare = canonical_tool_name(&normalized);

        // Strategy 1: exact verb match — bare name IS the verb segment.
        // e.g. `event_create` → unique tool ending with `__event_create`.
        let suffix = format!("__{canon_bare}");
        let matches: Vec<_> = tools
            .iter()
            .filter(|t| canonical_tool_name(t.name()).ends_with(&suffix))
            .collect();
        if matches.len() == 1 {
            return Some(Arc::clone(matches[0]));
        }

        // Strategy 2: server_verb concatenation — model concatenated server
        // and verb with `_` instead of `mcp__{server}__{verb}`.
        // e.g. `space_event_create` → `mcp__space__event_create` where
        // server=space, verb=event_create.
        let matches: Vec<_> = tools
            .iter()
            .filter(|t| {
                let norm = normalize_mcp_tool_name(t.name());
                if let Some((server, verb)) = mcp_name_parts(&norm) {
                    let concat = format!(
                        "{}_{}",
                        canonical_tool_name(server),
                        canonical_tool_name(verb)
                    );
                    concat == canon_bare
                } else {
                    false
                }
            })
            .collect();
        if matches.len() == 1 {
            Some(Arc::clone(matches[0]))
        } else {
            None
        }
    }
}

fn parse_select_names(query: &str) -> Vec<String> {
    query[SELECT_PREFIX.len()..]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn select_matches(names: &[String], tools: &[Arc<dyn Tool>]) -> Vec<Arc<dyn Tool>> {
    let mut out = Vec::new();
    for name in names {
        if let Some(t) = resolve_tool_by_name(name, tools) {
            if !out.iter().any(|x: &Arc<dyn Tool>| x.name() == t.name()) {
                out.push(t);
            }
        }
    }
    out
}

/// Closure that returns the full list of currently deferred tools. Engine
/// supplies this so `ToolSearch` always sees the live registry.
pub type DeferredToolsFn = Arc<dyn Fn() -> Vec<Arc<dyn Tool>> + Send + Sync>;

/// Closure that registers a tool name as "discovered" — the engine then
/// includes it in the active tool list for subsequent LLM turns. Without
/// this, the model can read schemas but can't actually invoke the tool.
pub type RegisterDiscoveredFn = Arc<dyn Fn(&str) + Send + Sync>;

pub struct ToolSearchTool {
    deferred_resolver: DeferredToolsFn,
    register_discovered: Option<RegisterDiscoveredFn>,
}

impl ToolSearchTool {
    pub fn new(deferred_resolver: DeferredToolsFn) -> Self {
        Self {
            deferred_resolver,
            register_discovered: None,
        }
    }

    /// Inject the discovery callback. Engine calls this immediately after
    /// constructing the tool so each search result is auto-loaded for the
    /// rest of the session.
    pub fn with_discovery(mut self, cb: RegisterDiscoveredFn) -> Self {
        self.register_discovered = Some(cb);
        self
    }

    fn rank_matches(query: &str, tools: &[Arc<dyn Tool>], limit: usize) -> Vec<Arc<dyn Tool>> {
        let q_lower = query.to_lowercase();
        let q_terms: Vec<&str> = q_lower
            .split_whitespace()
            .filter(|t| !t.is_empty())
            .collect();
        if q_terms.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<(i32, Arc<dyn Tool>)> = tools
            .iter()
            .filter_map(|t| {
                let name = t.name().to_lowercase();
                let hint = t.search_hint().to_lowercase();
                let desc = t.description().to_lowercase();
                let mut score = 0i32;
                // Boost entire MCP server families when the query names a server
                // (e.g. "browser search" → all `mcp__browser__*` tools). Match on
                // the normalized name so the full registered form
                // (`mcp__senclaw-browser__browser_search`) is boosted under its
                // stripped server name (`browser`) just like the short form.
                let family_name = normalize_mcp_tool_name(&name);
                if family_name.starts_with("mcp__") {
                    for term in &q_terms {
                        let family = format!("mcp__{term}__");
                        if family_name.starts_with(&family) {
                            score += 80;
                        }
                    }
                }

                for term in &q_terms {
                    // Highest weight: exact name substring (e.g. user asks "screenshot" → "browser_screenshot")
                    if name.contains(term) {
                        score += 100;
                    }
                    if hint.contains(term) {
                        score += 25;
                    }
                    if desc.contains(term) {
                        score += 5;
                    }
                    for alias in t.aliases() {
                        if alias.to_lowercase().contains(term) {
                            score += 60;
                        }
                    }
                }
                if score > 0 {
                    Some((score, Arc::clone(t)))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| {
            // higher score first; then alphabetical name for cache-stable order
            b.0.cmp(&a.0).then_with(|| a.1.name().cmp(b.1.name()))
        });
        scored.into_iter().take(limit).map(|(_, t)| t).collect()
    }
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "ToolSearch"
    }

    fn description(&self) -> &str {
        "Search for specialized tools that aren't loaded by default. Returns \
         full schemas of matching tools so they can be called in subsequent \
         turns. Use when a task needs capabilities beyond the core toolset \
         (e.g. browser screenshots, calendar events, code graph queries)."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keywords describing the capability you need. Examples: 'browser screenshot', 'calendar event', 'wiki search', 'code graph symbols'."
                },
                "max_results": {
                    "type": "number",
                    "description": "Max tools to return (default 5, hard cap 20)."
                }
            },
            "required": ["query"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn always_load(&self) -> bool {
        // ToolSearch is the discovery mechanism itself — must be in every prompt.
        true
    }

    async fn validate_input(
        &self,
        input: &Value,
        _ctx: &ToolContext<'_>,
    ) -> std::result::Result<(), String> {
        let q = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
        if q.trim().is_empty() {
            return Err("query is required".to_string());
        }
        Ok(())
    }

    async fn call(&self, input: Value, _ctx: &ToolContext<'_>) -> Result<Vec<ToolOutput>> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let limit = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_MAX_RESULTS as u64)
            .min(MAX_RESULTS_HARD_CAP as u64) as usize;

        let deferred = (self.deferred_resolver)();
        let matches = if query.starts_with(SELECT_PREFIX) {
            let names = parse_select_names(&query);
            if names.is_empty() {
                Vec::new()
            } else {
                select_matches(&names, &deferred)
            }
        } else {
            Self::rank_matches(&query, &deferred, limit)
        };

        // Register each match as discovered — engine will include them in
        // subsequent `tools_for_main_agent()` calls. Without this, the model
        // gets the schema here but can't actually call the tool next turn.
        if let Some(ref cb) = self.register_discovered {
            for t in &matches {
                cb(t.name());
            }
        }

        let payload: Vec<Value> = matches
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "input_schema": t.input_schema(),
                })
            })
            .collect();

        let text_summary = if matches.is_empty() {
            format!(
                "No tools matched query '{query}'. {} deferred tools available — try broader keywords.",
                deferred.len()
            )
        } else {
            let mut s = format!("Found {} tool(s) matching '{}':\n", matches.len(), query);
            for t in &matches {
                s.push_str(&format!("  - {}: {}\n", t.name(), t.search_hint()));
            }
            s.push_str("\nThese tools are now usable. Call them directly in your next turn.");
            s
        };

        Ok(vec![ToolOutput::Result {
            data: serde_json::json!({
                "query": query,
                "matches": payload,
                "deferred_total": deferred.len(),
            }),
            result_for_assistant: text_summary,
        }])
    }

    fn gen_tool_result_message(&self, data: &Value, _input: &Value) -> ToolResultMessage {
        let count = data
            .get("matches")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let query = data
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        ToolResultMessage {
            title: "ToolSearch".to_string(),
            summary: format!("{count} matches for '{query}'"),
            content: data.clone(),
        }
    }

    fn get_display_title(&self, input: &Value) -> String {
        let q = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
        if q.is_empty() {
            "ToolSearch".to_string()
        } else {
            format!("ToolSearch: \"{}\"", q)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zen_core::{Tool, ToolPermissionInfo};
    use std::sync::Mutex;

    /// Stub tool used by tests — implements the bare minimum.
    struct StubTool {
        name: &'static str,
        desc: &'static str,
        hint: &'static str,
        deferred: bool,
    }

    #[async_trait]
    impl Tool for StubTool {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            self.desc
        }
        fn input_schema(&self) -> Value {
            serde_json::json!({"type":"object"})
        }
        fn is_read_only(&self) -> bool {
            true
        }
        async fn call(&self, _input: Value, _ctx: &ToolContext<'_>) -> Result<Vec<ToolOutput>> {
            Ok(vec![])
        }
        fn gen_tool_result_message(&self, _d: &Value, _i: &Value) -> ToolResultMessage {
            ToolResultMessage {
                title: String::new(),
                summary: String::new(),
                content: Value::Null,
            }
        }
        fn get_display_title(&self, _i: &Value) -> String {
            self.name.to_string()
        }
        fn gen_tool_permission(&self, _i: &Value) -> Option<ToolPermissionInfo> {
            None
        }
        fn search_hint(&self) -> String {
            self.hint.to_string()
        }
        fn should_defer(&self) -> bool {
            self.deferred
        }
    }

    fn fixtures() -> Vec<Arc<dyn Tool>> {
        vec![
            Arc::new(StubTool {
                name: "browser_screenshot",
                desc: "Take a screenshot of the current browser tab.",
                hint: "screenshot browser tab capture",
                deferred: true,
            }),
            Arc::new(StubTool {
                name: "calendar_create",
                desc: "Create a calendar event.",
                hint: "calendar event create",
                deferred: true,
            }),
            Arc::new(StubTool {
                name: "wiki_search",
                desc: "Search the wiki.",
                hint: "wiki search documents",
                deferred: true,
            }),
        ]
    }

    #[test]
    fn rank_matches_prefers_name_hits() {
        let tools = fixtures();
        let hits = ToolSearchTool::rank_matches("screenshot", &tools, 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name(), "browser_screenshot");
    }

    #[test]
    fn resolve_tolerates_hyphen_underscore_in_mcp_server() {
        // Tool registered with hyphens in the server segment (Space App MCP).
        let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(StubTool {
            name: "mcp__ssh-manager-mcp__ssh_list_hosts",
            desc: "List SSH hosts.",
            hint: "ssh list hosts",
            deferred: true,
        })];
        // Model emits underscores instead of hyphens — must still resolve.
        for called in [
            "mcp__ssh-manager-mcp__ssh_list_hosts", // exact
            "mcp__ssh-manager_mcp__ssh_list_hosts", // observed failure
            "mcp__ssh_manager_mcp__ssh_list_hosts", // all underscores
        ] {
            let t = resolve_tool_by_name(called, &tools);
            assert!(t.is_some(), "should resolve {called}");
            assert_eq!(t.unwrap().name(), "mcp__ssh-manager-mcp__ssh_list_hosts");
        }
    }

    #[test]
    fn rank_matches_returns_empty_for_empty_query() {
        let tools = fixtures();
        let hits = ToolSearchTool::rank_matches("", &tools, 5);
        assert!(hits.is_empty());
    }

    #[test]
    fn rank_matches_combines_multi_term_score() {
        let tools = fixtures();
        let hits = ToolSearchTool::rank_matches("calendar event", &tools, 5);
        assert_eq!(hits.first().map(|t| t.name()), Some("calendar_create"));
    }

    #[test]
    fn rank_matches_caps_at_limit() {
        let tools = fixtures();
        let hits = ToolSearchTool::rank_matches("create event search", &tools, 1);
        assert_eq!(hits.len(), 1);
    }

    #[tokio::test]
    async fn call_returns_serialized_matches() {
        let resolver: DeferredToolsFn = Arc::new(|| fixtures());
        let tool = ToolSearchTool::new(resolver);
        let ctx = ToolContext {
            agent_id: "main",
            working_dir: "/tmp",
            agent_data_dir: "/tmp",
            abort: tokio_util::sync::CancellationToken::new(),
            event_bus: None,
            response_registry: None,
        };
        let out = tool
            .call(serde_json::json!({"query": "screenshot"}), &ctx)
            .await
            .unwrap();
        let ToolOutput::Result { data, .. } = &out[0] else {
            panic!("unexpected variant");
        };
        let matches = data["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["name"], "browser_screenshot");
        assert!(matches[0]["input_schema"].is_object());
    }

    #[tokio::test]
    async fn call_no_match_reports_total_deferred() {
        let resolver: DeferredToolsFn = Arc::new(|| fixtures());
        let tool = ToolSearchTool::new(resolver);
        let ctx = ToolContext {
            agent_id: "main",
            working_dir: "/tmp",
            agent_data_dir: "/tmp",
            abort: tokio_util::sync::CancellationToken::new(),
            event_bus: None,
            response_registry: None,
        };
        let out = tool
            .call(
                serde_json::json!({"query": "nonexistent-feature-xyzqq"}),
                &ctx,
            )
            .await
            .unwrap();
        let ToolOutput::Result {
            data,
            result_for_assistant,
        } = &out[0]
        else {
            panic!();
        };
        assert_eq!(data["matches"].as_array().unwrap().len(), 0);
        assert_eq!(data["deferred_total"], 3);
        assert!(result_for_assistant.contains("No tools matched"));
    }

    #[test]
    fn normalize_mcp_tool_name_strips_senclaw_prefix() {
        assert_eq!(
            super::normalize_mcp_tool_name("mcp__senclaw-browser__browser_search"),
            "mcp__browser__search"
        );
    }

    #[test]
    fn resolve_stripped_bridge_name_to_registered_full_name() {
        // The manager registers MCP tools under the FULL server prefix, but the
        // agent-browser skill (and the model) call them by the stripped bridge
        // form. The documented short name must resolve to the registered tool,
        // both for direct dispatch and for `select:` loading via ToolSearch.
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(StubTool {
                name: "mcp__senclaw-browser__browser_search",
                desc: "Search the web.",
                hint: "browser search web",
                deferred: true,
            }),
            Arc::new(StubTool {
                name: "mcp__senclaw-browser__browser_close_tab",
                desc: "Close a browser tab.",
                hint: "browser close tab",
                deferred: true,
            }),
        ];
        for called in [
            "mcp__browser__search",                  // skill-documented short form
            "mcp__senclaw-browser__browser_search",  // registered full form
        ] {
            let t = resolve_tool_by_name(called, &tools);
            assert!(t.is_some(), "should resolve {called}");
            assert_eq!(
                t.unwrap().name(),
                "mcp__senclaw-browser__browser_search"
            );
        }
        // The exact `select:` query from the skill must load the tool too.
        let hits = select_matches(
            &[
                "mcp__browser__search".to_string(),
                "mcp__browser__close_tab".to_string(),
            ],
            &tools,
        );
        assert_eq!(hits.len(), 2, "select: should load both stripped names");
    }

    #[test]
    fn select_query_loads_exact_tools() {
        let tools = fixtures();
        let hits = select_matches(
            &["browser_screenshot".to_string(), "wiki_search".to_string()],
            &tools,
        );
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().any(|t| t.name() == "browser_screenshot"));
        assert!(hits.iter().any(|t| t.name() == "wiki_search"));
    }

    #[tokio::test]
    async fn call_select_prefix_registers_tools() {
        let discovered = Arc::new(Mutex::new(Vec::<String>::new()));
        let disc = Arc::clone(&discovered);
        let resolver: DeferredToolsFn = Arc::new(|| fixtures());
        let register: RegisterDiscoveredFn =
            Arc::new(move |name| disc.lock().unwrap().push(name.to_string()));
        let tool = ToolSearchTool::new(resolver).with_discovery(register);
        let ctx = ToolContext {
            agent_id: "main",
            working_dir: "/tmp",
            agent_data_dir: "/tmp",
            abort: tokio_util::sync::CancellationToken::new(),
            event_bus: None,
            response_registry: None,
        };
        let out = tool
            .call(
                serde_json::json!({"query": "select:browser_screenshot,wiki_search"}),
                &ctx,
            )
            .await
            .unwrap();
        let ToolOutput::Result { data, .. } = &out[0] else {
            panic!("unexpected variant");
        };
        assert_eq!(data["matches"].as_array().unwrap().len(), 2);
        let names = discovered.lock().unwrap();
        assert!(names.contains(&"browser_screenshot".to_string()));
        assert!(names.contains(&"wiki_search".to_string()));
    }

    #[test]
    fn rank_matches_boosts_browser_family() {
        let tools = fixtures();
        let hits = ToolSearchTool::rank_matches("browser search", &tools, 5);
        assert_eq!(hits.first().map(|t| t.name()), Some("browser_screenshot"));
    }

    #[test]
    fn resolve_bare_name_server_verb_concat() {
        // Model emits `space_event_create` for `mcp__space__event_create`
        // (server + "_" + verb, no mcp__ prefix).
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(StubTool {
                name: "mcp__space__event_create",
                desc: "Create event",
                hint: "event create",
                deferred: true,
            }),
            Arc::new(StubTool {
                name: "mcp__space__event_delete",
                desc: "Delete event",
                hint: "event delete",
                deferred: true,
            }),
        ];

        // server_verb concatenation: space + _ + event_create
        let t = resolve_tool_by_name("space_event_create", &tools);
        assert!(t.is_some(), "should resolve space_event_create");
        assert_eq!(t.unwrap().name(), "mcp__space__event_create");

        // Also works for other verbs
        let t = resolve_tool_by_name("space_event_delete", &tools);
        assert!(t.is_some(), "should resolve space_event_delete");
        assert_eq!(t.unwrap().name(), "mcp__space__event_delete");

        // Pure verb match (only if unique)
        let single: Vec<Arc<dyn Tool>> = vec![Arc::new(StubTool {
            name: "mcp__space__event_create",
            desc: "Create event",
            hint: "event create",
            deferred: true,
        })];
        let t = resolve_tool_by_name("event_create", &single);
        assert!(t.is_some(), "should resolve bare verb event_create");

        // Ambiguous verb → None (two tools share the suffix)
        let ambig: Vec<Arc<dyn Tool>> = vec![
            Arc::new(StubTool {
                name: "mcp__space__event_create",
                desc: "Create event",
                hint: "",
                deferred: true,
            }),
            Arc::new(StubTool {
                name: "mcp__calendar__event_create",
                desc: "Create event",
                hint: "",
                deferred: true,
            }),
        ];
        let t = resolve_tool_by_name("event_create", &ambig);
        assert!(t.is_none(), "ambiguous verb should return None");

        // But server_verb is still unique even with ambiguous verb
        let t = resolve_tool_by_name("space_event_create", &ambig);
        assert!(t.is_some(), "server_verb should resolve even when verb alone is ambiguous");
        assert_eq!(t.unwrap().name(), "mcp__space__event_create");
    }

    #[test]
    fn resolve_bare_name_with_senclaw_prefix_tools() {
        // Real-world case: tool registered as mcp__senclaw-space__space_event_create
        // which normalizes to mcp__space__event_create
        let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(StubTool {
            name: "mcp__senclaw-space__space_event_create",
            desc: "Create event",
            hint: "event create",
            deferred: true,
        })];

        let t = resolve_tool_by_name("space_event_create", &tools);
        assert!(t.is_some(), "should resolve via normalize + server_verb");
        assert_eq!(t.unwrap().name(), "mcp__senclaw-space__space_event_create");
    }

    #[test]
    fn always_load_is_true_so_tool_search_never_deferred() {
        let resolver: DeferredToolsFn = Arc::new(Vec::new);
        let t = ToolSearchTool::new(resolver);
        assert!(t.always_load());
        // Sanity: should_defer default is false; ToolSearch never opts in.
        assert!(!t.should_defer());
    }
}
