use anyhow::Result;
use ayx_core::envelope::Envelope;
use serde_json::json;

use crate::{UiSchedulesCommand, ui_command_envelope};

pub(crate) fn execute(command: UiSchedulesCommand) -> Result<Envelope> {
    Ok(match command {
        UiSchedulesCommand::Inventory => Envelope::ok_with_data(
            "one ui schedules inventory scaffolded",
            ui_command_envelope("schedules", "inventory", json!({})),
        ),
    })
}
