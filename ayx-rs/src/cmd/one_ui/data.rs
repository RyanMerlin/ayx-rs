use anyhow::Result;
use ayx_core::envelope::Envelope;
use serde_json::json;

use crate::{ui_command_envelope, UiDataCommand};

pub(crate) fn execute(command: Option<UiDataCommand>) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok("one ui data commands available: list-datasets, dataset-detail, dataset-preview, upload, list-connections (experimental)"),
        Some(UiDataCommand::ListDatasets { foreground }) => Envelope::ok_with_data(
            "one ui data list-datasets scaffolded",
            ui_command_envelope("data", "list-datasets", json!({
                "foreground": foreground,
                "tab_policy": "use pinned tab when warm; background page for read-only refresh is allowed",
            })),
        ),
        Some(UiDataCommand::DatasetDetail { dataset_id, foreground }) => Envelope::ok_with_data(
            "one ui data dataset-detail scaffolded",
            ui_command_envelope("data", "dataset-detail", json!({ "dataset_id": dataset_id, "foreground": foreground })),
        ),
        Some(UiDataCommand::DatasetPreview { dataset_id, foreground }) => Envelope::ok_with_data(
            "one ui data dataset-preview scaffolded",
            ui_command_envelope("data", "dataset-preview", json!({ "dataset_id": dataset_id, "foreground": foreground })),
        ),
        Some(UiDataCommand::Upload { input, foreground }) => Envelope::ok_with_data(
            "one ui data upload scaffolded",
            ui_command_envelope("data", "upload", json!({ "input": input.display().to_string(), "foreground": foreground })),
        ),
        Some(UiDataCommand::ListConnections { foreground }) => Envelope::ok_with_data(
            "one ui data list-connections scaffolded",
            ui_command_envelope("data", "list-connections", json!({ "foreground": foreground })),
        ),
    })
}
