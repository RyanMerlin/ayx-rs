use anyhow::Result;
use ayx_core::envelope::Envelope;

use crate::{UiCommand, cmd::RuntimeCtx};

mod data;
mod jobs;
mod library;
mod schedules;
mod session;
mod workflow;

pub(crate) fn execute(_runtime: &RuntimeCtx<'_>, command: Option<UiCommand>) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok(
            "one ui commands available: session, workflow, data, library, schedules, jobs (experimental)",
        ),
        Some(UiCommand::Session { command }) => session::execute(command)?,
        Some(UiCommand::Workflow { command }) => workflow::execute(command)?,
        Some(UiCommand::Data { command }) => data::execute(command)?,
        Some(UiCommand::Library { command }) => library::execute(command)?,
        Some(UiCommand::Schedules { command }) => schedules::execute(command)?,
        Some(UiCommand::Jobs { command }) => jobs::execute(command)?,
    })
}
