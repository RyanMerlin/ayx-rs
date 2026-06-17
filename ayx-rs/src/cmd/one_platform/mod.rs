use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one::{api_status_envelope, one_surface_inventory_envelope};
use ayx_one_api::one_api_live_request;

use crate::{OnePlatformCommand, cmd::RuntimeCtx};

mod api;
mod auth;
mod person;
mod role;
mod token;
mod workspace;

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: Option<OnePlatformCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok(
            "one platform commands available: api, auth, status, inventory, workspace, role, user, token, person",
        ),
        Some(OnePlatformCommand::Api { command }) => api::execute(runtime, command)?,
        Some(OnePlatformCommand::Auth { command }) => auth::execute(runtime, command)?,
        Some(OnePlatformCommand::Status { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            api_status_envelope(&config, "one platform")?
        }
        Some(OnePlatformCommand::Inventory { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_surface_inventory_envelope(&config)?
        }
        Some(OnePlatformCommand::Workspace { command }) => {
            workspace::execute(runtime, apply, yes, Some(command))?
        }
        Some(OnePlatformCommand::Role { command }) => role::execute(runtime, apply, yes, command)?,
        Some(OnePlatformCommand::User) => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "platform",
                "user-current",
                "GET",
                "/v4/people/current",
                false,
                &[],
            )?
        }
        Some(OnePlatformCommand::Token { command }) => {
            token::execute(runtime, apply, yes, command)?
        }
        Some(OnePlatformCommand::Person { command }) => {
            person::execute(runtime, apply, yes, command)?
        }
    })
}
