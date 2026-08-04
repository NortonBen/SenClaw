//! Built-in [`McpSourceSpec`]s for peer Space Apps.
//!
//! A preset is registered only if the app is actually installed and enabled in
//! this daemon — a source pointing at an app that isn't there would report
//! `unavailable` on every run and add noise to every result page.
//!
//! **Not every searchable app can be a preset.** `social_search` requires
//! `platform` AND `handle` (`apps/social/src/mcp.rs`) — the handle is the
//! specific logged-in account whose session the extension replays. No default
//! can be guessed, and guessing wrong searches as the wrong identity. So social
//! is offered as a *template* the user completes (`social_template`), stored in
//! `mcp_sources` like any other user-registered source, rather than being
//! auto-registered.

use crate::model::SourceKind;
use crate::sources::mcp_source::{FieldMap, McpSourceSpec, McpTarget};

/// Specs that need no user input; registered automatically when their app is
/// installed and enabled.
pub fn auto_specs() -> Vec<McpSourceSpec> {
    vec![
        McpSourceSpec {
            id: "youtube".into(),
            label: "YouTube".into(),
            // Deliberately `Social`, not a kind of its own: independence is
            // counted per kind, and a YouTube video echoing a Threads post is
            // not two independent confirmations. Under-count, never over-claim.
            kind: SourceKind::Social,
            weight: 0.9,
            target: McpTarget::App {
                app_id: "youtube".into(),
            },
            tool: "youtube_search".into(),
            query_arg: "query".into(),
            // youtube_search takes only `query` — sending `limit` would be an
            // unknown-argument error.
            limit_arg: None,
            extra_args: serde_json::json!({}),
            map: FieldMap {
                // Returns `videoId` and no URL whatsoever; without this the
                // results could never be cited or deduped against a web hit
                // for the same video.
                url_template: Some("https://www.youtube.com/watch?v={videoId}".into()),
                snippet: Some("channel".into()),
                published_at: Some("published".into()),
                ..Default::default()
            },
        },
        McpSourceSpec {
            id: "deepwiki".into(),
            label: "Code (DeepWiki)".into(),
            kind: SourceKind::Code,
            weight: 1.1,
            target: McpTarget::App {
                app_id: "deepwiki".into(),
            },
            tool: "deepwiki_search".into(),
            query_arg: "query".into(),
            limit_arg: Some("limit".into()),
            extra_args: serde_json::json!({}),
            // Returns a bare array of rows; auto-detection handles both the
            // array and the `name`/`doc` field names.
            map: FieldMap::default(),
        },
    ]
}

/// A spec the user must complete before it can run. Returned by
/// `search_source_templates` so the UI can prompt for the missing arguments.
pub struct SourceTemplate {
    pub id: &'static str,
    pub label: &'static str,
    pub app_id: &'static str,
    pub tool: &'static str,
    /// Arguments the user must supply, with a hint for each.
    pub required_args: &'static [(&'static str, &'static str)],
    pub why: &'static str,
}

pub fn templates() -> Vec<SourceTemplate> {
    vec![SourceTemplate {
        id: "social",
        label: "Mạng xã hội",
        app_id: "social",
        tool: "social_search",
        required_args: &[
            (
                "platform",
                "nền tảng: facebook | x | threads | instagram | tiktok",
            ),
            (
                "handle",
                "tài khoản đã đăng nhập dùng để tìm, ví dụ @ten_cua_ban",
            ),
        ],
        why: "social_search tìm bằng phiên đăng nhập THẬT của một tài khoản cụ thể. \
              Không thể đoán tài khoản thay bạn — đoán sai là tìm kiếm dưới danh nghĩa người khác.",
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_is_not_sent_a_limit_argument_it_does_not_accept() {
        let yt = auto_specs()
            .into_iter()
            .find(|s| s.id == "youtube")
            .unwrap();
        assert_eq!(yt.limit_arg, None);
    }

    #[test]
    fn youtube_carries_a_url_template_because_it_returns_no_url() {
        let yt = auto_specs()
            .into_iter()
            .find(|s| s.id == "youtube")
            .unwrap();
        assert!(yt
            .map
            .url_template
            .as_deref()
            .unwrap()
            .contains("{videoId}"));
    }

    #[test]
    fn no_auto_spec_requires_arguments_the_app_cannot_default() {
        // An auto-registered spec must be runnable with nothing but a query;
        // anything else belongs in `templates()`.
        for s in auto_specs() {
            let extra = s.extra_args.as_object().cloned().unwrap_or_default();
            assert!(
                extra.is_empty(),
                "{} needs extra args — it should be a template, not an auto spec",
                s.id
            );
        }
    }

    #[test]
    fn social_is_a_template_not_an_auto_spec() {
        assert!(auto_specs().iter().all(|s| s.id != "social"));
        assert!(templates().iter().any(|t| t.id == "social"));
    }
}
