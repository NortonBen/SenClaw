//! One configurable source that turns **any MCP tool** into a search source.
//!
//! This is the module that makes "adding a surface" a config change rather than
//! a code change. A peer Space App (`youtube_search`, `deepwiki_search`) and a
//! user-registered third-party MCP are the same thing here — only the spec
//! differs. `sources/presets.rs` holds the built-in specs; the `mcp_sources`
//! table holds the user's.
//!
//! The hard part is not the call, it is **mapping an unknown result shape onto
//! [`Evidence`]**. Real shapes observed in this repo differ wildly:
//!
//! ```text
//! deepwiki_search → [ {name, kind, file, line, doc}, … ]        (bare array)
//! youtube_search  → { results: [ {videoId, title, channel}, …] } (no url at all!)
//! social_search   → { items: [ … ] }                             (needs platform+handle)
//! ```
//!
//! So the mapper auto-detects the item array and the field names, and a spec
//! can override any of it. `url_template` exists because youtube returns a
//! `videoId` and never a URL — without it those results would have no citation
//! target and would never dedupe against a web hit for the same video.

use crate::model::{Budget, Evidence, SourceHealth, SourceKind, SubQuery};
use crate::sources::SearchSource;
use crate::transport::AppMcp;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::time::Duration;

/// Where the MCP endpoint lives.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpTarget {
    /// A Space App installed in this daemon; the RPC URL is resolved at call
    /// time from the app registry, so a reinstalled app on a new port keeps
    /// working.
    App { app_id: String },
    /// Any MCP JSON-RPC endpoint, given verbatim.
    Url { rpc_url: String },
}

/// How to read the tool's result. Every field is optional — omitted fields fall
/// back to auto-detection.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct FieldMap {
    /// Dotted path to the item array, e.g. `results` or `data.items`.
    pub list_path: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
    /// Build a URL from item fields, e.g.
    /// `https://www.youtube.com/watch?v={videoId}`. Used when the tool returns
    /// an id but no URL.
    pub url_template: Option<String>,
    pub snippet: Option<String>,
    pub published_at: Option<String>,
}

/// A complete, self-contained description of an MCP-backed source.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpSourceSpec {
    pub id: String,
    pub label: String,
    #[serde(default = "default_kind")]
    pub kind: SourceKind,
    #[serde(default = "default_weight")]
    pub weight: f32,
    pub target: McpTarget,
    pub tool: String,
    #[serde(default = "default_query_arg")]
    pub query_arg: String,
    /// Argument name for the result cap, if the tool has one.
    #[serde(default)]
    pub limit_arg: Option<String>,
    /// Constant arguments merged into every call — this is how
    /// `social_search`'s required `platform` / `handle` get supplied.
    #[serde(default)]
    pub extra_args: Value,
    #[serde(default)]
    pub map: FieldMap,
}

fn default_kind() -> SourceKind {
    SourceKind::Custom
}
fn default_weight() -> f32 {
    1.0
}
fn default_query_arg() -> String {
    "query".to_string()
}

/// Source ids that are built in. A user-registered source may not take one of
/// these — it would shadow the real source and the user would see a working
/// name silently doing something else.
pub const RESERVED_IDS: &[&str] = &["web", "knowledge", "wiki", "memory", "corpus"];

impl McpSourceSpec {
    /// Reject specs that cannot work, with a reason the user can act on.
    pub fn validate(&self) -> Result<(), String> {
        let id = self.id.trim();
        if id.is_empty() {
            return Err("thiếu `id`".into());
        }
        if RESERVED_IDS.contains(&id) {
            return Err(format!(
                "`{id}` là tên nguồn có sẵn — chọn tên khác (ví dụ `{id}-custom`)"
            ));
        }
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
        {
            return Err(format!(
                "`{id}` chứa ký tự không hợp lệ — chỉ dùng chữ, số và - _ : ."
            ));
        }
        if self.tool.trim().is_empty() {
            return Err("thiếu `tool` (tên công cụ MCP cần gọi)".into());
        }
        if self.query_arg.trim().is_empty() {
            return Err("`query_arg` không được rỗng".into());
        }
        match &self.target {
            McpTarget::App { app_id } if app_id.trim().is_empty() => Err("thiếu `app_id`".into()),
            McpTarget::App { app_id } if app_id.trim() == crate::config::app_id() => {
                Err(SELF_TARGET_MSG.into())
            }
            McpTarget::Url { rpc_url } => {
                let u = rpc_url.trim();
                if !(u.starts_with("http://") || u.starts_with("https://")) {
                    // Anything else (file:, ws:, a bare host) would either fail
                    // confusingly or reach somewhere the user did not mean.
                    return Err(format!(
                        "`rpc_url` phải bắt đầu bằng http:// hoặc https:// (nhận được `{u}`)"
                    ));
                }
                if points_at_self(u) {
                    return Err(SELF_TARGET_MSG.into());
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

const SELF_TARGET_MSG: &str =
    "không thể lấy chính app Search làm nguồn: search_query sẽ gọi lại chính nó, \
     mỗi lần lại toả ra mọi nguồn — đệ quy vô hạn cho tới khi hết thời gian chờ";

/// Does this URL point back at our own HTTP server?
///
/// Registering ourselves as a source is never useful and, for `search_query`,
/// is actively destructive: the pipeline would fan out into itself and recurse.
/// Host spellings for the same server differ (`127.0.0.1`, `localhost`,
/// `0.0.0.0`, `::1`), so match on the port plus a loopback-ish host rather than
/// on the string.
fn points_at_self(url: &str) -> bool {
    let port = crate::config::http_port();
    let port = port.trim();
    let after_scheme = match url.split_once("://") {
        Some((_, rest)) => rest,
        None => url,
    };
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let (host, url_port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.trim_matches(['[', ']']), Some(p)),
        None => (authority, None),
    };
    url_port == Some(port)
        && matches!(
            host,
            "127.0.0.1" | "localhost" | "0.0.0.0" | "::1" | "[::1]"
        )
}

pub struct McpSource {
    spec: McpSourceSpec,
    apps: AppMcp,
}

impl McpSource {
    pub fn new(spec: McpSourceSpec, apps: AppMcp) -> Self {
        Self { spec, apps }
    }

    #[allow(dead_code)] // P1 UI: show a registered source spec
    pub fn spec(&self) -> &McpSourceSpec {
        &self.spec
    }

    /// Resolve the target to a concrete JSON-RPC URL.
    async fn rpc_url(&self) -> anyhow::Result<String> {
        match &self.spec.target {
            McpTarget::Url { rpc_url } => Ok(rpc_url.clone()),
            McpTarget::App { app_id } => {
                let apps = self.apps.discover().await?;
                let peer = apps
                    .get(app_id)
                    .ok_or_else(|| anyhow::anyhow!("app `{app_id}` chưa được cài trong SenClaw"))?;
                if !peer.enabled {
                    anyhow::bail!("app `{app_id}` đang bị tắt");
                }
                Ok(peer.rpc_url())
            }
        }
    }

    fn arguments(&self, q: &SubQuery, budget: Budget) -> Value {
        let mut args = Map::new();
        // Extra args first so an explicit query/limit in the spec cannot be
        // silently overwritten by... actually the reverse: query wins, since a
        // source whose query is pinned is not a search source.
        if let Some(obj) = self.spec.extra_args.as_object() {
            for (k, v) in obj {
                args.insert(k.clone(), v.clone());
            }
        }
        args.insert(self.spec.query_arg.clone(), json!(q.text));
        if let Some(l) = &self.spec.limit_arg {
            args.insert(l.clone(), json!(budget.max_results));
        }
        Value::Object(args)
    }
}

// ---- result mapping --------------------------------------------------------

/// Read a dotted path (`a.b.c`) out of a JSON value.
fn get_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = v;
    for seg in path.split('.') {
        if seg.is_empty() {
            continue;
        }
        cur = cur.get(seg)?;
    }
    Some(cur)
}

/// Keys that commonly hold the item array, most specific first.
const LIST_KEYS: &[&str] = &[
    "results",
    "items",
    "hits",
    "rows",
    "matches",
    "entries",
    "data",
    "videos",
    "posts",
    "list",
    "documents",
    "records",
];

/// Find the array of results in an arbitrary tool response.
fn extract_items(result: &Value, list_path: Option<&str>) -> Vec<Value> {
    if let Some(p) = list_path {
        return get_path(result, p)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
    }
    if let Some(arr) = result.as_array() {
        return arr.clone(); // deepwiki_search shape
    }
    let Some(obj) = result.as_object() else {
        return vec![];
    };
    for key in LIST_KEYS {
        if let Some(arr) = obj.get(*key).and_then(Value::as_array) {
            return arr.clone();
        }
    }
    // Last resort: the first array value in the object. Better than silently
    // returning nothing for a shape nobody anticipated.
    obj.values()
        .find_map(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

const TITLE_KEYS: &[&str] = &[
    "title",
    "name",
    "headline",
    "label",
    "subject",
    "signature",
    "text",
];
const URL_KEYS: &[&str] = &["url", "link", "permalink", "href", "web_url", "source_url"];
const SNIPPET_KEYS: &[&str] = &[
    "snippet",
    "description",
    "summary",
    "doc",
    "content",
    "text",
    "body",
    "excerpt",
];

fn pick<'a>(item: &'a Value, explicit: Option<&str>, candidates: &[&str]) -> Option<&'a Value> {
    if let Some(p) = explicit {
        return get_path(item, p);
    }
    candidates.iter().find_map(|k| item.get(*k))
}

fn as_text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        // A nested object/array is not a title; stringifying it would produce
        // JSON soup in the UI.
        _ => None,
    }
}

/// Substitute `{field}` placeholders from the item.
///
/// Returns `None` if any placeholder is unresolved — emitting
/// `https://youtube.com/watch?v={videoId}` as a citation would be worse than
/// having no URL at all.
fn render_template(template: &str, item: &Value) -> Option<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}')? + open;
        out.push_str(&rest[..open]);
        let key = &rest[open + 1..close];
        let value = as_text(get_path(item, key))?;
        out.push_str(&value);
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// Map one raw item onto [`Evidence`].
pub fn map_item(
    item: &Value,
    rank: u32,
    source_id: &str,
    kind: SourceKind,
    map: &FieldMap,
) -> Option<Evidence> {
    // Some tools return arrays of plain strings.
    if let Value::String(s) = item {
        if s.trim().is_empty() {
            return None;
        }
        return Some(Evidence::new(
            source_id,
            kind,
            rank,
            1.0 / (1.0 + rank as f32),
            crate::util::truncate_chars(s, 120),
            s.clone(),
            None,
        ));
    }
    if !item.is_object() {
        return None;
    }

    let title = as_text(pick(item, map.title.as_deref(), TITLE_KEYS));
    let snippet = as_text(pick(item, map.snippet.as_deref(), SNIPPET_KEYS));
    let url = as_text(pick(item, map.url.as_deref(), URL_KEYS)).or_else(|| {
        map.url_template
            .as_deref()
            .and_then(|t| render_template(t, item))
    });

    // An item with neither a title nor a body is noise, not evidence.
    if title.is_none() && snippet.is_none() {
        return None;
    }

    let mut ev = Evidence::new(
        source_id,
        kind,
        rank,
        1.0 / (1.0 + rank as f32),
        title.clone().unwrap_or_else(|| {
            crate::util::truncate_chars(snippet.as_deref().unwrap_or_default(), 120)
        }),
        snippet.unwrap_or_default(),
        url,
    );
    ev.published_at = pick(
        item,
        map.published_at.as_deref(),
        &["published_at", "published", "date"],
    )
    .and_then(parse_timestamp);
    // Keep the raw item so downstream stages (and the UI) can show fields the
    // generic mapper had no name for.
    ev.meta = item.clone();
    Some(ev)
}

fn parse_timestamp(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        // Heuristic: seconds vs milliseconds.
        return Some(if n < 100_000_000_000 { n * 1000 } else { n });
    }
    let s = v.as_str()?;
    chrono::DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|d| d.timestamp_millis())
}

#[async_trait]
impl SearchSource for McpSource {
    fn id(&self) -> &str {
        &self.spec.id
    }
    fn label(&self) -> &str {
        &self.spec.label
    }
    fn kind(&self) -> SourceKind {
        self.spec.kind
    }
    fn weight(&self) -> f32 {
        self.spec.weight
    }

    async fn health(&self) -> SourceHealth {
        let url = match self.rpc_url().await {
            Ok(u) => u,
            Err(e) => return SourceHealth::unavailable(e.to_string()),
        };
        match self.apps.list_tools(&url, Duration::from_secs(5)).await {
            Err(e) => SourceHealth::unavailable(format!("không gọi được MCP ({e})")),
            Ok(tools) => {
                // A renamed tool is the single most likely way this source rots.
                // Say so explicitly instead of returning zero results forever.
                let found = tools.iter().any(|t| {
                    t.get("name").and_then(Value::as_str) == Some(self.spec.tool.as_str())
                });
                if found {
                    SourceHealth::Ready
                } else {
                    SourceHealth::unavailable(format!(
                        "MCP không còn công cụ `{}` (hiện có: {})",
                        self.spec.tool,
                        tools
                            .iter()
                            .filter_map(|t| t.get("name").and_then(Value::as_str))
                            .take(8)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                }
            }
        }
    }

    async fn search(&self, q: &SubQuery, budget: Budget) -> anyhow::Result<Vec<Evidence>> {
        let url = self.rpc_url().await?;
        let result = self
            .apps
            .call(
                &url,
                &self.spec.tool,
                self.arguments(q, budget),
                Duration::from_millis(budget.timeout_ms),
            )
            .await?;

        let items = extract_items(&result, self.spec.map.list_path.as_deref());
        Ok(items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| {
                map_item(
                    item,
                    i as u32,
                    &self.spec.id,
                    self.spec.kind,
                    &self.spec.map,
                )
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> FieldMap {
        FieldMap::default()
    }

    #[test]
    fn a_bare_array_is_the_item_list() {
        // deepwiki_search returns `json!(rows)` — no wrapper object.
        let r = json!([{ "name": "run", "doc": "starts it" }]);
        assert_eq!(extract_items(&r, None).len(), 1);
    }

    #[test]
    fn a_wrapped_array_is_found_by_key() {
        let r = json!({ "results": [{ "title": "a" }, { "title": "b" }] });
        assert_eq!(extract_items(&r, None).len(), 2);
    }

    #[test]
    fn an_explicit_list_path_wins_over_auto_detection() {
        let r = json!({ "results": [{ "title": "wrong" }], "data": { "items": [{ "title": "right" }] } });
        let items = extract_items(&r, Some("data.items"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "right");
    }

    #[test]
    fn an_unanticipated_shape_falls_back_to_the_first_array() {
        let r = json!({ "weird_key": [{ "title": "found anyway" }] });
        assert_eq!(extract_items(&r, None).len(), 1);
    }

    #[test]
    fn a_shape_with_no_array_yields_no_items_rather_than_panicking() {
        assert!(extract_items(&json!({ "ok": true }), None).is_empty());
        assert!(extract_items(&json!("a string"), None).is_empty());
    }

    #[test]
    fn deepwiki_rows_map_onto_evidence() {
        let item =
            json!({ "name": "Db::open", "doc": "opens the sqlite file", "file": "src/db.rs" });
        let ev = map_item(&item, 0, "deepwiki", SourceKind::Internal, &map()).unwrap();
        assert_eq!(ev.title, "Db::open");
        assert_eq!(ev.snippet, "opens the sqlite file");
        assert_eq!(ev.url, None);
        // The raw row survives so `file` is not lost.
        assert_eq!(ev.meta["file"], "src/db.rs");
    }

    #[test]
    fn a_url_template_builds_the_missing_citation_target() {
        // youtube_search returns a videoId and no URL at all.
        let item = json!({ "videoId": "dQw4w9WgXcQ", "title": "Never Gonna Give You Up" });
        let m = FieldMap {
            url_template: Some("https://www.youtube.com/watch?v={videoId}".into()),
            ..Default::default()
        };
        let ev = map_item(&item, 0, "youtube", SourceKind::Social, &m).unwrap();
        assert_eq!(
            ev.url.as_deref(),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
        );
    }

    #[test]
    fn an_unresolvable_template_yields_no_url_not_a_broken_one() {
        let item = json!({ "title": "no id here" });
        let m = FieldMap {
            url_template: Some("https://www.youtube.com/watch?v={videoId}".into()),
            ..Default::default()
        };
        let ev = map_item(&item, 0, "youtube", SourceKind::Social, &m).unwrap();
        assert_eq!(ev.url, None, "a literal {{videoId}} must never be cited");
    }

    #[test]
    fn a_real_url_field_beats_the_template() {
        let item = json!({ "videoId": "abc", "url": "https://example.com/canonical" });
        let m = FieldMap {
            url_template: Some("https://www.youtube.com/watch?v={videoId}".into()),
            snippet: Some("videoId".into()),
            ..Default::default()
        };
        let ev = map_item(&item, 0, "yt", SourceKind::Social, &m).unwrap();
        assert_eq!(ev.url.as_deref(), Some("https://example.com/canonical"));
    }

    #[test]
    fn items_with_neither_title_nor_body_are_dropped() {
        let item = json!({ "score": 0.9, "vector": [1, 2, 3] });
        assert!(map_item(&item, 0, "x", SourceKind::Custom, &map()).is_none());
    }

    #[test]
    fn a_nested_object_is_not_used_as_a_title() {
        // `text` is a title candidate, but an object under it is JSON soup.
        let item = json!({ "text": { "raw": "x" }, "description": "the real body" });
        let ev = map_item(&item, 0, "x", SourceKind::Custom, &map()).unwrap();
        assert_eq!(ev.snippet, "the real body");
        assert_eq!(ev.title, "the real body", "falls back to the body");
    }

    #[test]
    fn arrays_of_plain_strings_still_produce_evidence() {
        let ev = map_item(
            &json!("một kết quả dạng chuỗi"),
            2,
            "x",
            SourceKind::Custom,
            &map(),
        )
        .unwrap();
        assert_eq!(ev.snippet, "một kết quả dạng chuỗi");
        assert_eq!(ev.hits[0].rank, 2);
    }

    #[test]
    fn timestamps_accept_seconds_millis_and_rfc3339() {
        assert_eq!(
            parse_timestamp(&json!(1_700_000_000)),
            Some(1_700_000_000_000)
        );
        assert_eq!(
            parse_timestamp(&json!(1_700_000_000_000i64)),
            Some(1_700_000_000_000)
        );
        assert!(parse_timestamp(&json!("2026-07-20T10:00:00Z")).is_some());
        assert_eq!(parse_timestamp(&json!("hôm qua")), None);
    }

    fn spec(extra: Value) -> McpSourceSpec {
        McpSourceSpec {
            id: "social:threads".into(),
            label: "Threads".into(),
            kind: SourceKind::Social,
            weight: 1.0,
            target: McpTarget::App {
                app_id: "social".into(),
            },
            tool: "social_search".into(),
            query_arg: "query".into(),
            limit_arg: Some("limit".into()),
            extra_args: extra,
            map: FieldMap::default(),
        }
    }

    #[test]
    fn extra_args_supply_required_parameters_the_query_alone_cannot() {
        // social_search requires platform + handle; without extra_args the call
        // would always fail with "missing platform".
        let s = McpSource::new(
            spec(json!({ "platform": "threads", "handle": "@me" })),
            AppMcp::new("http://127.0.0.1:1"),
        );
        let args = s.arguments(
            &SubQuery::new("giá vàng"),
            Budget {
                max_results: 7,
                timeout_ms: 1,
            },
        );
        assert_eq!(args["platform"], "threads");
        assert_eq!(args["handle"], "@me");
        assert_eq!(args["query"], "giá vàng");
        assert_eq!(args["limit"], 7);
    }

    #[test]
    fn extra_args_cannot_pin_the_query() {
        // A "search source" whose query is fixed is not a search source.
        let s = McpSource::new(
            spec(json!({ "query": "pinned" })),
            AppMcp::new("http://127.0.0.1:1"),
        );
        let args = s.arguments(
            &SubQuery::new("thật"),
            Budget {
                max_results: 5,
                timeout_ms: 1,
            },
        );
        assert_eq!(args["query"], "thật");
    }

    #[test]
    fn a_user_source_cannot_shadow_a_built_in_one() {
        let mut sp = spec(json!({}));
        sp.id = "web".into();
        assert!(sp.validate().unwrap_err().contains("có sẵn"));
    }

    #[test]
    fn a_non_http_rpc_url_is_rejected() {
        let mut sp = spec(json!({}));
        sp.target = McpTarget::Url {
            rpc_url: "file:///etc/passwd".into(),
        };
        assert!(sp.validate().is_err());
        sp.target = McpTarget::Url {
            rpc_url: "https://mcp.example/rpc".into(),
        };
        assert!(sp.validate().is_ok());
    }

    #[test]
    fn registering_this_app_as_its_own_source_is_refused() {
        // search_query calling itself fans out into itself — unbounded
        // recursion. Every spelling of "us" must be caught.
        let port = crate::config::http_port();
        for host in ["127.0.0.1", "localhost", "0.0.0.0", "::1"] {
            let mut sp = spec(json!({}));
            sp.target = McpTarget::Url {
                rpc_url: format!("http://{host}:{port}/api/mcp/message"),
            };
            assert!(
                sp.validate().is_err(),
                "self-target via {host} must be refused"
            );
        }
        let mut by_app = spec(json!({}));
        by_app.target = McpTarget::App {
            app_id: crate::config::app_id(),
        };
        assert!(by_app.validate().is_err());
    }

    #[test]
    fn a_different_app_on_another_port_is_still_allowed() {
        let mut sp = spec(json!({}));
        sp.target = McpTarget::Url {
            rpc_url: "http://127.0.0.1:4520/api/mcp/message".into(),
        };
        assert!(sp.validate().is_ok(), "peer apps must stay registerable");
    }

    #[test]
    fn a_remote_host_reusing_our_port_number_is_not_mistaken_for_us() {
        let mut sp = spec(json!({}));
        sp.target = McpTarget::Url {
            rpc_url: format!("https://example.com:{}/rpc", crate::config::http_port()),
        };
        assert!(sp.validate().is_ok());
    }

    #[test]
    fn a_spec_missing_its_tool_is_rejected() {
        let mut sp = spec(json!({}));
        sp.tool = "  ".into();
        assert!(sp.validate().unwrap_err().contains("tool"));
    }

    #[test]
    fn a_valid_spec_round_trips_through_json() {
        // The DB stores the whole spec as JSON; a field that fails to survive
        // the round-trip would silently reset on restart.
        let sp = spec(json!({ "platform": "threads", "handle": "@me" }));
        let back: McpSourceSpec =
            serde_json::from_str(&serde_json::to_string(&sp).unwrap()).unwrap();
        assert_eq!(back.id, sp.id);
        assert_eq!(back.tool, sp.tool);
        assert_eq!(back.target, sp.target);
        assert_eq!(back.extra_args["handle"], "@me");
        assert_eq!(back.limit_arg, sp.limit_arg);
    }

    #[test]
    fn a_tool_with_no_limit_arg_is_not_sent_one() {
        let mut sp = spec(json!({}));
        sp.limit_arg = None;
        let s = McpSource::new(sp, AppMcp::new("http://127.0.0.1:1"));
        let args = s.arguments(
            &SubQuery::new("q"),
            Budget {
                max_results: 5,
                timeout_ms: 1,
            },
        );
        assert!(args.get("limit").is_none());
    }
}
