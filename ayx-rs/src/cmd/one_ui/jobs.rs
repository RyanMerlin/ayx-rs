use anyhow::Result;
use ayx_core::envelope::Envelope;
use serde_json::json;

use crate::{ui_command_envelope, UiJobsCommand};

pub(crate) fn execute(command: Option<UiJobsCommand>) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok("one ui jobs commands available: inventory (experimental)"),
        Some(UiJobsCommand::Inventory) => Envelope::ok_with_data(
            "one ui jobs inventory scaffolded",
            ui_command_envelope("jobs", "inventory", json!({})),
        ),
    })
}
