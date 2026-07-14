//! `emit_widget` tool — push a one-way rich widget into the chat box.
//!
//! Unlike [`crate::tools::form_ui::FormUITool`] this is **display-only**: the
//! tool emits [`EngineEvent::WidgetEmit`] and returns immediately (no
//! [`crate::zen_core::ResponseRegistry`], no suspend, no response event). It
//! mirrors the one-way `tool:execution` push instead of the FormUI round-trip.
//!
//! See `WIDGET_CONTRACT.md` for the kind-specific `data` schemas. The backend
//! keeps `data` opaque — the web (`WidgetCard.tsx`) and desktop
//! (`widget_card.dart`) clients validate and render it.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::zen_core::{
    EngineEvent, Tool, ToolContext, ToolOutput, ToolResultMessage, WidgetEmitData, WidgetSpec,
};

/// The four supported widget kinds (contract §1).
const KINDS: [&str; 4] = ["chart", "image", "clock", "weather"];

const DESCRIPTION: &str = r#"# Rich chat widget (display-only)
Push a rich, non-interactive widget into the chat box for the user to see. This
is ONE-WAY — the widget is rendered inline; the user does NOT respond to it
(unlike FormUI/AskUserQuestion). Use it to show a chart, an image, a live clock,
or a weather card alongside your text reply.

kind must be one of: chart | image | clock | weather.
data is a kind-specific object (validated & rendered by the client):
- chart:   { chartType: bar|line|area|pie|scatter, series: [{ name, color?, points: [{x,y}] }], xLabel?, yLabel?, stacked? }
- image:   { url? | dataUrl?, caption?, alt? }   (one of url/dataUrl required)
- clock:   { tz?, label?, showSeconds?, showDate?, format24h? }
- weather: { location, unit: C|F, current: {temp,condition,icon,humidity,wind}, daily?: [{day,hi,lo,icon}] }

Returns immediately after queuing the widget — do not expect a value back."#;

pub struct EmitWidgetTool;

/// Parse + validate the raw tool input into a [`WidgetSpec`]. Kept separate so
/// both `validate_input` and `call` share one code path.
fn parse_spec(input: &Value) -> std::result::Result<WidgetSpec, String> {
    let kind = input
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or("kind is required")?
        .to_string();
    if !KINDS.contains(&kind.as_str()) {
        return Err(format!(
            "Invalid kind \"{kind}\"; expected one of chart | image | clock | weather"
        ));
    }
    let data = input
        .get("data")
        .cloned()
        .ok_or("data is required")?;
    if !data.is_object() {
        return Err("data must be an object".to_string());
    }
    let title = input
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Ok(WidgetSpec { kind, title, data })
}

#[async_trait]
impl Tool for EmitWidgetTool {
    fn name(&self) -> &str {
        "emit_widget"
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["chart", "image", "clock", "weather"],
                    "description": "Which widget to render."
                },
                "title": {
                    "type": "string",
                    "description": "Optional card header shown above the widget."
                },
                "data": {
                    "type": "object",
                    "description": "Kind-specific payload. See the tool description for each kind's shape."
                },
                "chat_jid": {
                    "type": "string",
                    "description": "Optional target chat JID; defaults to the current chat."
                }
            },
            "required": ["kind", "data"]
        })
    }

    fn is_read_only(&self) -> bool {
        // Display-only push; no filesystem/network side effects.
        true
    }

    async fn validate_input(
        &self,
        input: &Value,
        _ctx: &ToolContext<'_>,
    ) -> std::result::Result<(), String> {
        parse_spec(input)?;
        Ok(())
    }

    async fn call(&self, input: Value, ctx: &ToolContext<'_>) -> Result<Vec<ToolOutput>> {
        let widget = parse_spec(&input).map_err(|e| anyhow::anyhow!(e))?;
        let chat_jid = input
            .get("chat_jid")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let event_bus = ctx
            .event_bus
            .ok_or_else(|| anyhow::anyhow!("EventBus not available"))?;

        let id = format!("widget-{}", uuid::Uuid::new_v4());
        let kind = widget.kind.clone();
        event_bus.emit(EngineEvent::WidgetEmit(WidgetEmitData {
            agent_id: ctx.agent_id.to_string(),
            chat_jid,
            widget,
            id: id.clone(),
        }));

        let result_for_assistant =
            format!("Rendered a {kind} widget in the chat. It is display-only; the user will not reply to it.");
        Ok(vec![ToolOutput::Result {
            data: serde_json::json!({ "kind": kind, "id": id }),
            result_for_assistant,
        }])
    }

    fn gen_tool_result_message(&self, data: &Value, _input: &Value) -> ToolResultMessage {
        let kind = data.get("kind").and_then(|v| v.as_str()).unwrap_or("widget");
        ToolResultMessage {
            title: "Widget".into(),
            summary: format!("Rendered {kind} widget"),
            content: data.clone(),
        }
    }

    fn get_display_title(&self, input: &Value) -> String {
        match input.get("kind").and_then(|v| v.as_str()) {
            Some(kind) => format!("Widget: {kind}"),
            None => "Widget".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(
        bus: &'a crate::zen_core::EventBus,
        abort: &tokio_util::sync::CancellationToken,
    ) -> ToolContext<'a> {
        ToolContext {
            agent_id: "main",
            working_dir: "/tmp",
            agent_data_dir: "/tmp",
            abort: abort.clone(),
            event_bus: Some(bus),
            response_registry: None,
        }
    }

    fn sample() -> Value {
        serde_json::json!({
            "kind": "chart",
            "title": "Doanh thu",
            "data": {
                "chartType": "bar",
                "series": [{"name": "Q1", "points": [{"x": "T1", "y": 30}]}]
            }
        })
    }

    #[test]
    fn parse_spec_ok() {
        let spec = parse_spec(&sample()).unwrap();
        assert_eq!(spec.kind, "chart");
        assert_eq!(spec.title.as_deref(), Some("Doanh thu"));
        assert!(spec.data.is_object());
    }

    #[test]
    fn parse_spec_rejects_bad_kind() {
        let bad = serde_json::json!({"kind": "video", "data": {}});
        assert!(parse_spec(&bad).unwrap_err().contains("Invalid kind"));
    }

    #[test]
    fn parse_spec_rejects_non_object_data() {
        let bad = serde_json::json!({"kind": "chart", "data": [1, 2, 3]});
        assert!(parse_spec(&bad).unwrap_err().contains("data must be an object"));
    }

    #[test]
    fn parse_spec_requires_kind_and_data() {
        assert!(parse_spec(&serde_json::json!({"data": {}})).is_err());
        assert!(parse_spec(&serde_json::json!({"kind": "clock"})).is_err());
    }

    #[tokio::test]
    async fn call_emits_widget_event_and_returns_immediately() {
        let bus = crate::zen_core::EventBus::new();
        let abort = tokio_util::sync::CancellationToken::new();
        let mut rx = bus.subscribe();

        let outputs = EmitWidgetTool
            .call(sample(), &ctx(&bus, &abort))
            .await
            .unwrap();

        // Returns a Result synchronously (no blocking).
        let ToolOutput::Result { data, .. } = &outputs[0] else {
            panic!("expected Result output");
        };
        assert_eq!(data["kind"], "chart");
        let id = data["id"].as_str().unwrap().to_string();
        assert!(id.starts_with("widget-"));

        // And emitted exactly one WidgetEmit event carrying the spec.
        let event = rx.try_recv().unwrap();
        let EngineEvent::WidgetEmit(emitted) = event else {
            panic!("expected WidgetEmit");
        };
        assert_eq!(emitted.agent_id, "main");
        assert_eq!(emitted.widget.kind, "chart");
        assert_eq!(emitted.id, id);
        assert!(emitted.chat_jid.is_none());
    }

    #[tokio::test]
    async fn call_passes_through_chat_jid() {
        let bus = crate::zen_core::EventBus::new();
        let abort = tokio_util::sync::CancellationToken::new();
        let mut rx = bus.subscribe();
        let mut input = sample();
        input["chat_jid"] = serde_json::json!("telegram:42");
        EmitWidgetTool.call(input, &ctx(&bus, &abort)).await.unwrap();
        let EngineEvent::WidgetEmit(emitted) = rx.try_recv().unwrap() else {
            panic!("expected WidgetEmit");
        };
        assert_eq!(emitted.chat_jid.as_deref(), Some("telegram:42"));
    }
}
