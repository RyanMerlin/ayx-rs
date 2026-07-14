use anyhow::Result;
use ayx_core::envelope::Envelope;
use serde_json::json;

use crate::{UiDataCommand, ui_command_envelope};

pub(crate) fn execute(command: UiDataCommand) -> Result<Envelope> {
    Ok(match command {
        UiDataCommand::ListDatasets { foreground } => Envelope::ok_with_data(
            "one ui data list-datasets scaffolded",
            ui_command_envelope(
                "data",
                "list-datasets",
                json!({
                    "foreground": foreground,
                    "tab_policy": "use pinned tab when warm; background page for read-only refresh is allowed",
                }),
            ),
        ),
        UiDataCommand::DatasetDetail {
            dataset_id,
            foreground,
        } => Envelope::ok_with_data(
            "one ui data dataset-detail scaffolded",
            ui_command_envelope(
                "data",
                "dataset-detail",
                json!({ "dataset_id": dataset_id, "foreground": foreground }),
            ),
        ),
        UiDataCommand::DatasetPreview {
            dataset_id,
            foreground,
        } => Envelope::ok_with_data(
            "one ui data dataset-preview scaffolded",
            ui_command_envelope(
                "data",
                "dataset-preview",
                json!({ "dataset_id": dataset_id, "foreground": foreground }),
            ),
        ),
        UiDataCommand::Upload { input, foreground } => Envelope::ok_with_data(
            "one ui data upload scaffolded",
            ui_command_envelope(
                "data",
                "upload",
                json!({ "input": input.display().to_string(), "foreground": foreground }),
            ),
        ),
        UiDataCommand::ListConnections { foreground } => Envelope::ok_with_data(
            "one ui data list-connections scaffolded",
            ui_command_envelope(
                "data",
                "list-connections",
                json!({ "foreground": foreground }),
            ),
        ),
    })
}
