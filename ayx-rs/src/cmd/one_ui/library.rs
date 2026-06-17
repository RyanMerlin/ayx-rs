use anyhow::Result;
use ayx_core::envelope::Envelope;
use serde_json::json;

use crate::{UiLibraryCommand, ui_command_envelope};

pub(crate) fn execute(command: Option<UiLibraryCommand>) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok("one ui library commands available: inventory (experimental)"),
        Some(UiLibraryCommand::Inventory) => Envelope::ok_with_data(
            "one ui library inventory scaffolded",
            ui_command_envelope("library", "inventory", json!({})),
        ),
    })
}
