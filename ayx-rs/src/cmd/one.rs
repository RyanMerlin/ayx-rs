//! Dispatch for `ayx one ...`.
//!
//! The largest single dispatch arm in the original main.rs — ~2000 LOC
//! covering platform / workspace / role / person / token / api / auth /
//! plans / scheduling / billing / flows / connections / connector
//! metadata / job groups / output objects / webhook flow tasks / write
//! settings / doctor.
//!
//! Each arm is verbatim from the original dispatch, wrapped in
//! `Ok(match command { ... })` so the function returns `Result<Envelope>`.
//! The `load_profile` closure replaces the same-named captured closure
//! in main.rs by delegating to the shared profile loader.

use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one::one_surface_inventory_envelope;

use crate::OneCommand;

/// Run the default email-OTP login for a named profile.
///
/// A thin entry point for `onboard`'s opt-in "log in now" step: it dispatches
/// the same `one login` a user would run (default OTP flow, no flags). The profile is passed
/// explicitly (rather than relying on the active profile) so resolution is
/// deterministic and cannot be diverted by `AYX_PROFILE`.
pub(crate) fn run_otp_login(
    environment: Option<&str>,
    profile: Option<String>,
) -> Result<Envelope> {
    let runtime = crate::cmd::RuntimeCtx::new(environment);
    super::one_platform::auth::login(
        &runtime, profile, None, false, false, None, None, None, None, None,
    )
}

/// Borrow Cli's apply + yes for the TTY confirm prompts inside delete arms.
pub struct Ctx<'a> {
    pub apply: bool,
    pub yes: bool,
    pub environment: Option<&'a str>,
}

#[allow(clippy::too_many_lines)]
pub fn execute(cli: Ctx<'_>, command: Option<OneCommand>) -> Result<Envelope> {
    // Capture `environment` up-front so `cli.environment` reads through the
    // helper don't conflict with `cli` itself being borrowed by other arms.
    let environment = cli.environment;
    let runtime = crate::cmd::RuntimeCtx::new(environment);
    Ok(match command {
        None => Envelope::ok(
            "one commands available: login, logout, whoami, auth, workspace, person, role, token, inventory, api, doctor, plans, flows, datasets, connections, scheduling, billing",
        ),
        Some(OneCommand::Login {
            profile,
            client_id,
            browser,
            device,
            refresh_token,
            access_token,
            token_endpoint,
            workspace_id,
            workspace_gid,
        }) => super::one_platform::auth::login(
            &runtime,
            profile,
            client_id,
            browser,
            device,
            refresh_token,
            access_token,
            token_endpoint,
            workspace_id,
            workspace_gid,
        )?,
        Some(OneCommand::Logout { profile }) => {
            super::one_platform::auth::logout(&runtime, profile.as_deref())?
        }
        Some(OneCommand::Whoami) => super::one_platform::person::current(&runtime, None)?,
        Some(OneCommand::Auth { command }) => {
            super::one_platform::auth::execute(&runtime, command)?
        }
        Some(OneCommand::Workspace { command }) => {
            super::one_platform::workspace::execute(&runtime, cli.apply, cli.yes, Some(command))?
        }
        Some(OneCommand::Role { command }) => {
            super::one_platform::role::execute(&runtime, cli.apply, cli.yes, command)?
        }
        Some(OneCommand::Token { command }) => {
            super::one_platform::token::execute(&runtime, cli.apply, cli.yes, command)?
        }
        Some(OneCommand::Person { command }) => {
            super::one_platform::person::execute(&runtime, cli.apply, cli.yes, command)?
        }
        Some(OneCommand::Inventory { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_surface_inventory_envelope(&config)?
        }
        Some(OneCommand::Doctor { command }) => super::one_doctor::execute(&runtime, command)?,
        Some(OneCommand::Api { command }) => super::one_api::execute(&runtime, command)?,
        Some(OneCommand::JobGroups { command }) => {
            super::one_job_groups::execute(&runtime, command)?
        }
        Some(OneCommand::OutputObjects { command }) => {
            super::one_output_objects::execute(&runtime, command)?
        }
        Some(OneCommand::WebhookFlowTasks { command }) => {
            super::one_webhook_flow_tasks::execute(&runtime, command)?
        }
        Some(OneCommand::WriteSettings { command }) => {
            super::one_write_settings::execute(&runtime, command)?
        }
        Some(OneCommand::Connections { command }) => {
            super::one_connections::execute(&runtime, cli.apply, cli.yes, command)?
        }
        Some(OneCommand::Datasets { command }) => super::one_datasets::execute(&runtime, command)?,
        Some(OneCommand::Flows { command }) => {
            super::one_flows::execute(&runtime, cli.apply, cli.yes, command)?
        }
        Some(OneCommand::Plans { command }) => {
            super::one_plans::execute(&runtime, cli.apply, cli.yes, command)?
        }
        Some(OneCommand::Scheduling { command }) => {
            super::one_scheduling::execute(&runtime, command)?
        }
        Some(OneCommand::Billing { command }) => super::one_billing::execute(&runtime, command)?,
        #[cfg(feature = "ui")]
        Some(OneCommand::Ui { command }) => super::one_ui::execute(&runtime, command)?,
    })
}
