use anyhow::Result;
use ayx_core::envelope::Envelope;
use serde_json::json;

use crate::{UiWorkflowCommand, ui_command_envelope};

pub(crate) fn execute(command: Option<UiWorkflowCommand>) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok(
            "one ui workflow commands available: open, create, inventory, pane-config, pane-results, tool-list, tool-select, tool-inspect, graph-get, graph-put (experimental)",
        ),
        Some(UiWorkflowCommand::Open {
            workflow_id,
            foreground,
        }) => Envelope::ok_with_data(
            "one ui workflow open scaffolded",
            ui_command_envelope(
                "workflow",
                "open",
                json!({ "workflow_id": workflow_id, "foreground": foreground }),
            ),
        ),
        Some(UiWorkflowCommand::Create { name, foreground }) => Envelope::ok_with_data(
            "one ui workflow create scaffolded",
            ui_command_envelope(
                "workflow",
                "create",
                json!({ "name": name, "foreground": foreground }),
            ),
        ),
        Some(UiWorkflowCommand::Inventory {
            workflow_id,
            foreground,
        }) => Envelope::ok_with_data(
            "one ui workflow inventory scaffolded",
            ui_command_envelope(
                "workflow",
                "inventory",
                json!({
                    "workflow_id": workflow_id,
                    "foreground": foreground,
                    "captures": ["canvas", "config-pane", "results-pane"],
                }),
            ),
        ),
        Some(UiWorkflowCommand::PaneConfig {
            workflow_id,
            tool_id,
        }) => Envelope::ok_with_data(
            "one ui workflow pane-config scaffolded",
            ui_command_envelope(
                "workflow",
                "pane-config",
                json!({ "workflow_id": workflow_id, "tool_id": tool_id }),
            ),
        ),
        Some(UiWorkflowCommand::PaneResults {
            workflow_id,
            tool_id,
        }) => Envelope::ok_with_data(
            "one ui workflow pane-results scaffolded",
            ui_command_envelope(
                "workflow",
                "pane-results",
                json!({ "workflow_id": workflow_id, "tool_id": tool_id }),
            ),
        ),
        Some(UiWorkflowCommand::ToolList { workflow_id }) => Envelope::ok_with_data(
            "one ui workflow tool-list scaffolded",
            ui_command_envelope(
                "workflow",
                "tool-list",
                json!({ "workflow_id": workflow_id }),
            ),
        ),
        Some(UiWorkflowCommand::ToolSelect {
            workflow_id,
            tool_id,
        }) => Envelope::ok_with_data(
            "one ui workflow tool-select scaffolded",
            ui_command_envelope(
                "workflow",
                "tool-select",
                json!({ "workflow_id": workflow_id, "tool_id": tool_id }),
            ),
        ),
        Some(UiWorkflowCommand::ToolInspect {
            workflow_id,
            tool_id,
        }) => Envelope::ok_with_data(
            "one ui workflow tool-inspect scaffolded",
            ui_command_envelope(
                "workflow",
                "tool-inspect",
                json!({ "workflow_id": workflow_id, "tool_id": tool_id }),
            ),
        ),
        Some(UiWorkflowCommand::GraphGet { workflow_id }) => Envelope::ok_with_data(
            "one ui workflow graph-get scaffolded",
            ui_command_envelope(
                "workflow",
                "graph-get",
                json!({ "workflow_id": workflow_id }),
            ),
        ),
        Some(UiWorkflowCommand::GraphPut { workflow_id, input }) => Envelope::ok_with_data(
            "one ui workflow graph-put scaffolded",
            ui_command_envelope(
                "workflow",
                "graph-put",
                json!({ "workflow_id": workflow_id, "input": input.display().to_string() }),
            ),
        ),
    })
}
