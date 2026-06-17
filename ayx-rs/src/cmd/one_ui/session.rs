use anyhow::Result;
use ayx_core::envelope::Envelope;
use serde_json::json;

use crate::{UiSessionCommand, ui_command_envelope};

pub(crate) fn execute(command: Option<UiSessionCommand>) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok(
            "one ui session commands available: status, ensure, attach, inventory (experimental)",
        ),
        Some(UiSessionCommand::Status) => Envelope::ok_with_data(
            "one ui session status scaffolded",
            ui_command_envelope(
                "session",
                "status",
                json!({
                    "browser": "managed by ayx-rs",
                    "mode": "experimental hybrid pinned visible tabs plus background read-only pages",
                }),
            ),
        ),
        Some(UiSessionCommand::Ensure) => Envelope::ok_with_data(
            "one ui session ensure scaffolded",
            ui_command_envelope("session", "ensure", json!({ "result": "scaffolded" })),
        ),
        Some(UiSessionCommand::Attach { tab }) => Envelope::ok_with_data(
            "one ui session attach scaffolded",
            ui_command_envelope("session", "attach", json!({ "tab": tab })),
        ),
        Some(UiSessionCommand::Inventory) => Envelope::ok_with_data(
            "one ui session inventory scaffolded",
            ui_command_envelope(
                "session",
                "inventory",
                json!({
                    "tabs": ["workflow", "data"],
                    "policy": "foreground tabs are reusable; read-only tasks may use background pages",
                }),
            ),
        ),
    })
}
