use anyhow::Result;
use ayx_core::envelope::Envelope;
use serde_json::json;

use crate::{UiLibraryCommand, ui_command_envelope};

pub(crate) fn execute(command: UiLibraryCommand) -> Result<Envelope> {
    Ok(match command {
        UiLibraryCommand::Inventory => Envelope::ok_with_data(
            "one ui library inventory scaffolded",
            ui_command_envelope("library", "inventory", json!({})),
        ),
    })
}
