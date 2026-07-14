use anyhow::Result;
use ayx_core::envelope::Envelope;
use serde_json::json;

use crate::{UiJobsCommand, ui_command_envelope};

pub(crate) fn execute(command: UiJobsCommand) -> Result<Envelope> {
    Ok(match command {
        UiJobsCommand::Inventory => Envelope::ok_with_data(
            "one ui jobs inventory scaffolded",
            ui_command_envelope("jobs", "inventory", json!({})),
        ),
    })
}
