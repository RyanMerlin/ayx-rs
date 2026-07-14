use anyhow::Result;
use ayx_core::envelope::Envelope;

use crate::{UiCommand, cmd::RuntimeCtx};

mod data;
mod jobs;
mod library;
mod schedules;
mod session;
mod workflow;

pub(crate) fn execute(_runtime: &RuntimeCtx<'_>, command: UiCommand) -> Result<Envelope> {
    Ok(match command {
        UiCommand::Session { command } => session::execute(command)?,
        UiCommand::Workflow { command } => workflow::execute(command)?,
        UiCommand::Data { command } => data::execute(command)?,
        UiCommand::Library { command } => library::execute(command)?,
        UiCommand::Schedules { command } => schedules::execute(command)?,
        UiCommand::Jobs { command } => jobs::execute(command)?,
    })
}
