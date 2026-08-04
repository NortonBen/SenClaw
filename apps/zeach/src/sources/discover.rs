//! Rule-based source discovery: turn every installed Space App that exposes a
//! search-shaped MCP tool into a source — without a per-app preset.
//!
//! `presets.rs` stays the place for *curated* specs (special field maps, tuned
//! weights, URL templates). This module is the long tail: it inspects each
//! app's real `tools/list` and applies rules, so a freshly installed app shows
//! up as a source after one rescan instead of waiting for zeach to learn about
//! it.
//!
//! ## Rules
//!
//! A tool is a search surface when ALL hold:
//! 1. Its name ends in `_search` — the repo-wide convention for corpus search
//!    (`news_search`, `crm_search`, `moltbook_search`, …). A bare `search` or
//!    `*_query` name is NOT accepted: those are management/SQL surfaces
//!    (`search_query`, `crm_query`) and matching them would produce garbage
//!    sources.
//! 2. Its input schema has a string query parameter named one of
//!    [`QUERY_KEYS`].
//! 3. Every OTHER required parameter is coverable: if any required parameter
//!    besides the query is present, the tool cannot be auto-run — it becomes a
//!    [`Suggestion`] the user completes with `extra_args` (mirrors the
//!    social-template rule in presets.rs).
//!
//! Discovered sources register **disabled by default**: many app corpora are
//! private working data (CRM, email) and switching a research fan-out onto
//! them must be the user's choice. The saved per-source config re-applies on
//! every sync, so once the user enables one it stays enabled.

use crate::model::SourceKind;
use crate::sources::mcp_source::{FieldMap, McpSourceSpec, McpTarget};
use crate::transport::PeerApp;
use serde_json::Value;

/// Accepted names for the query parameter, in preference order.
const QUERY_KEYS: &[&str] = &["query", "q", "keyword", "keywords", "text", "term"];
/// Accepted names for a result-cap parameter.
const LIMIT_KEYS: &[&str] = &["limit", "max_results", "top_k", "count", "page_size"];

/// Weight for discovered sources: below neutral so a curated or core source
/// outranks an auto-detected one at equal rank.
const DISCOVERED_WEIGHT: f32 = 0.8;

/// Result of applying the rules to one app.
#[derive(Debug)]
pub enum Detection {
    /// Runnable with nothing but a query → can be auto-registered.
    Auto(McpSourceSpec),
    /// Search tool found but it needs user-supplied arguments.
    Needs(Suggestion),
    /// No search surface (the common case — not an error).
    None,
    /// App is on the denylist, with the reason.
    Denied(&'static str),
}

/// A source zeach could add if the user fills in the missing arguments.
/// Rendered by `zeach_source_templates` next to the curated templates.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Suggestion {
    pub app_id: String,
    pub app_name: String,
    pub tool: String,
    /// `(name, hint)` — hint is the schema's own description when present.
    pub required_args: Vec<(String, String)>,
}

/// Apps that must never become sources automatically.
fn denied(app_id: &str) -> Option<&'static str> {
    if app_id == crate::config::app_id() {
        return Some("chính app này — tự gọi mình là đệ quy vô hạn");
    }
    if app_id == "search" {
        // The older federated-search app fans out to the same web/app surfaces
        // zeach already covers: every result would be a duplicate, and if it
        // ever grows a zeach source the two apps recurse into each other.
        return Some("app search là meta-search trùng nguồn (và có nguy cơ gọi vòng lại zeach)");
    }
    None
}

/// Independence is counted per [`SourceKind`], so the mapping errs toward
/// under-counting: app corpora that mirror public content keep the public
/// kind; private working data is `Internal` (same kind as knowledge/wiki —
/// a fact seen in CRM and wiki is one internal confirmation, not two).
fn kind_for_app(app_id: &str) -> SourceKind {
    match app_id {
        "news" => SourceKind::Web,
        "moltbook" | "social" | "youtube" | "tiktok-activity" | "facebook-pro" => SourceKind::Social,
        "deepwiki" | "code-ide" => SourceKind::Code,
        _ => SourceKind::Internal,
    }
}

fn schema_props(tool: &Value) -> Option<&serde_json::Map<String, Value>> {
    tool.get("inputSchema")?.get("properties")?.as_object()
}

fn required_of(tool: &Value) -> Vec<String> {
    tool.get("inputSchema")
        .and_then(|s| s.get("required"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The best `*_search` tool of an app, or None. Prefers `<app>_search` (the
/// dominant convention), then the shortest matching name — `foo_search` beats
/// `foo_search_advanced` because the plain one is the corpus search.
fn best_search_tool<'a>(app_id: &str, tools: &'a [Value]) -> Option<&'a Value> {
    let mut candidates: Vec<(&str, &Value)> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str).map(|n| (n, t)))
        .filter(|(n, _)| n.ends_with("_search"))
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let canonical = format!("{}_search", app_id.replace('-', "_"));
    candidates.sort_by_key(|(n, _)| (*n != canonical, n.len()));
    Some(candidates[0].1)
}

/// Apply the rules to one installed app.
pub fn detect(app: &PeerApp, tools: &[Value]) -> Detection {
    if let Some(reason) = denied(&app.id) {
        return Detection::Denied(reason);
    }
    let Some(tool) = best_search_tool(&app.id, tools) else {
        return Detection::None;
    };
    let name = tool.get("name").and_then(Value::as_str).unwrap_or_default();
    let Some(props) = schema_props(tool) else {
        return Detection::None;
    };

    let Some(query_arg) = QUERY_KEYS.iter().find(|k| props.contains_key(**k)) else {
        // A `*_search` tool with no recognizable query parameter — treat as
        // not-a-source rather than guessing an argument name.
        return Detection::None;
    };
    let limit_arg = LIMIT_KEYS
        .iter()
        .find(|k| props.contains_key(**k))
        .map(|k| k.to_string());

    let missing: Vec<(String, String)> = required_of(tool)
        .into_iter()
        .filter(|r| r != *query_arg)
        .map(|r| {
            let hint = props
                .get(&r)
                .and_then(|p| p.get("description"))
                .and_then(Value::as_str)
                .unwrap_or("giá trị bắt buộc của công cụ")
                .to_string();
            (r, hint)
        })
        .collect();
    if !missing.is_empty() {
        return Detection::Needs(Suggestion {
            app_id: app.id.clone(),
            app_name: app.name.clone(),
            tool: name.to_string(),
            required_args: missing,
        });
    }

    let spec = McpSourceSpec {
        id: app.id.clone(),
        label: app.name.clone(),
        kind: kind_for_app(&app.id),
        weight: DISCOVERED_WEIGHT,
        target: McpTarget::App {
            app_id: app.id.clone(),
        },
        tool: name.to_string(),
        query_arg: query_arg.to_string(),
        limit_arg,
        extra_args: serde_json::json!({}),
        map: FieldMap::default(),
    };
    match spec.validate() {
        Ok(()) => Detection::Auto(spec),
        // validate() failing here means the app id collides with a reserved
        // name or similar — not a source, and not worth a suggestion either.
        Err(_) => Detection::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn app(id: &str) -> PeerApp {
        PeerApp {
            id: id.into(),
            name: format!("App {id}"),
            origin: "http://127.0.0.1:1".into(),
            mcp_path: "/api/mcp/message".into(),
            mcp_name: None,
            enabled: true,
        }
    }

    fn tool(name: &str, props: Value, required: Value) -> Value {
        json!({ "name": name, "inputSchema": { "type": "object", "properties": props, "required": required } })
    }

    #[test]
    fn a_plain_search_tool_becomes_an_auto_spec() {
        // crm_search: { q (required), limit } — the real shape.
        let tools = vec![tool(
            "crm_search",
            json!({ "q": { "type": "string" }, "limit": { "type": "number" } }),
            json!(["q"]),
        )];
        match detect(&app("crm"), &tools) {
            Detection::Auto(spec) => {
                assert_eq!(spec.id, "crm");
                assert_eq!(spec.tool, "crm_search");
                assert_eq!(spec.query_arg, "q");
                assert_eq!(spec.limit_arg.as_deref(), Some("limit"));
                assert_eq!(spec.kind, SourceKind::Internal);
                assert!(spec.weight < 1.0, "discovered sources rank below curated");
            }
            other => panic!("expected Auto, got {other:?}"),
        }
    }

    #[test]
    fn extra_required_args_demote_the_tool_to_a_suggestion() {
        // predict_topic_search requires `topic` — no default can be guessed.
        let tools = vec![tool(
            "predict_topic_search",
            json!({ "topic": { "type": "string", "description": "Tên chủ đề." },
                    "q": { "type": "string" }, "limit": { "type": "number" } }),
            json!(["topic"]),
        )];
        match detect(&app("predict"), &tools) {
            Detection::Needs(s) => {
                assert_eq!(s.tool, "predict_topic_search");
                assert_eq!(s.required_args.len(), 1);
                assert_eq!(s.required_args[0].0, "topic");
                assert!(s.required_args[0].1.contains("chủ đề"));
            }
            other => panic!("expected Needs, got {other:?}"),
        }
    }

    #[test]
    fn management_and_query_tools_are_not_sources() {
        // `*_query` (SQL/chart surfaces) and CRUD tools must never match.
        let tools = vec![
            tool("crm_query", json!({ "sql": { "type": "string" } }), json!(["sql"])),
            tool("crm_update", json!({ "id": { "type": "number" } }), json!(["id"])),
        ];
        assert!(matches!(detect(&app("crm"), &tools), Detection::None));
    }

    #[test]
    fn a_search_tool_without_a_query_parameter_is_skipped() {
        let tools = vec![tool(
            "weird_search",
            json!({ "vector": { "type": "array" } }),
            json!([]),
        )];
        assert!(matches!(detect(&app("weird"), &tools), Detection::None));
    }

    #[test]
    fn the_canonical_app_search_tool_wins_over_longer_variants() {
        let tools = vec![
            tool("news_archive_search", json!({ "q": { "type": "string" } }), json!([])),
            tool("news_search", json!({ "q": { "type": "string" } }), json!([])),
        ];
        match detect(&app("news"), &tools) {
            Detection::Auto(spec) => assert_eq!(spec.tool, "news_search"),
            other => panic!("expected Auto, got {other:?}"),
        }
    }

    #[test]
    fn hyphenated_app_ids_still_find_their_canonical_tool() {
        let tools = vec![tool(
            "code_ide_search",
            json!({ "query": { "type": "string" } }),
            json!(["query"]),
        )];
        match detect(&app("code-ide"), &tools) {
            Detection::Auto(spec) => {
                assert_eq!(spec.tool, "code_ide_search");
                assert_eq!(spec.kind, SourceKind::Code);
            }
            other => panic!("expected Auto, got {other:?}"),
        }
    }

    #[test]
    fn self_and_the_federated_search_app_are_denied() {
        let tools = vec![tool("x_search", json!({ "q": { "type": "string" } }), json!([]))];
        assert!(matches!(
            detect(&app(&crate::config::app_id()), &tools),
            Detection::Denied(_)
        ));
        assert!(matches!(detect(&app("search"), &tools), Detection::Denied(_)));
    }

    #[test]
    fn known_apps_map_to_public_kinds_and_unknown_apps_stay_internal() {
        assert_eq!(kind_for_app("news"), SourceKind::Web);
        assert_eq!(kind_for_app("moltbook"), SourceKind::Social);
        assert_eq!(kind_for_app("deepwiki"), SourceKind::Code);
        assert_eq!(kind_for_app("kanban"), SourceKind::Internal);
        assert_eq!(kind_for_app("một-app-tương-lai"), SourceKind::Internal);
    }
}
