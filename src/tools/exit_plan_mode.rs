//! ExitPlanMode tool — the agent calls this to request user approval of a plan,
//! mirroring sema-core's `plan_to_agent` tool.
//!
//! Behaviour: emits a [`crate::zen_core::EngineEvent::PlanExitRequest`] event
//! with the plan file path and content; suspends until the user picks
//! `startEditing` or `clearContextAndStart`. The chosen option is delivered
//! via the engine's response registry.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::zen_core::{
    EngineEvent, PlanExitOptions, PlanExitRequestData, Tool, ToolContext, ToolOutput,
    ToolPermissionInfo, ToolResultMessage,
};

pub struct ExitPlanModeTool;

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }

    fn description(&self) -> &str {
        "Use this tool when you are in plan mode and have finished presenting your plan and are \
         ready to code. Prompts the user to approve the plan."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "plan": {
                    "type": "string",
                    "description": "Markdown plan content to present to the user for approval."
                },
                "planFilePath": {
                    "type": "string",
                    "description": "Absolute path of the plan file you wrote. Optional but recommended."
                }
            },
            "required": ["plan"]
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext<'_>) -> Result<Vec<ToolOutput>> {
        let plan_content = input
            .get("plan")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let plan_file_path = input
            .get("planFilePath")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let (bus, registry) = match (ctx.event_bus, ctx.response_registry) {
            (Some(b), Some(r)) => (b, r),
            _ => {
                return Ok(vec![ToolOutput::Result {
                    data: serde_json::json!({"error": "no_event_bus"}),
                    result_for_assistant: "Plan-mode approval is unavailable in this context.".to_string(),
                }]);
            }
        };

        let rx = registry.register_ask_question(ctx.agent_id);
        bus.emit(EngineEvent::PlanExitRequest(PlanExitRequestData {
            agent_id: ctx.agent_id.to_string(),
            plan_file_path: plan_file_path.clone(),
            plan_content: plan_content.clone(),
            options: PlanExitOptions {
                start_editing: "Approve plan and start editing".to_string(),
                clear_context_and_start: "Clear context and start fresh".to_string(),
            },
        }));

        let selected = tokio::select! {
            _ = ctx.abort.cancelled() => "cancelled".to_string(),
            resp = rx => match resp {
                Ok(answer) => {
                    answer.answers
                        .get("selected")
                        .cloned()
                        .unwrap_or_else(|| "startEditing".to_string())
                }
                Err(_) => "cancelled".to_string(),
            }
        };

        let result_text = match selected.as_str() {
            "clearContextAndStart" => "User chose to clear context and start fresh from the plan.".to_string(),
            "cancelled" => "Plan approval was cancelled.".to_string(),
            _ => "Plan approved. Begin implementation.".to_string(),
        };

        Ok(vec![ToolOutput::Result {
            data: serde_json::json!({
                "selected": selected,
                "planFilePath": plan_file_path,
            }),
            result_for_assistant: result_text,
        }])
    }

    fn gen_tool_result_message(&self, data: &Value, _input: &Value) -> ToolResultMessage {
        let selected = data
            .get("selected")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        ToolResultMessage {
            title: "ExitPlanMode".to_string(),
            summary: selected,
            content: data.clone(),
        }
    }

    fn get_display_title(&self, _input: &Value) -> String {
        "ExitPlanMode".to_string()
    }

    fn gen_tool_permission(&self, _input: &Value) -> Option<ToolPermissionInfo> {
        None // approval flow is handled by the plan-exit event, not the permission gate.
    }
}
