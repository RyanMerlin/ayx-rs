//! Dispatch for `ayx one ...`.
//!
//! The largest single dispatch arm in the original main.rs — ~2000 LOC
//! covering platform / workspace / role / person / token / api / auth /
//! plans / scheduling / billing / flows / connections / connector
//! metadata / job groups / output objects / webhook flow tasks / write
//! settings / doctor / auto-insights / desktop-exec.
//!
//! Each arm is verbatim from the original dispatch, wrapped in
//! `Ok(match command { ... })` so the function returns `Result<Envelope>`.
//! The `load_profile` closure replaces the same-named captured closure
//! in main.rs by delegating to the shared profile loader.

use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one::{api_inventory_envelope, api_status_envelope};

use crate::OneCommand;

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
    macro_rules! load_profile {
        ($profile:expr, $environment:expr) => {
            runtime.load_profile_lenient($profile)
        };
    }
    Ok(match command {
        None => Envelope::ok(
            "one commands available: platform, plans, scheduling, billing, auto-insights, desktop-exec",
        ),
        Some(OneCommand::Doctor { command }) => super::one_doctor::execute(&runtime, command)?,
        Some(OneCommand::Platform { command }) => {
            super::one_platform::execute(&runtime, cli.apply, cli.yes, command)?
        }
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
        Some(OneCommand::Status { profile }) => {
            let config = load_profile!(profile.as_deref(), environment)?;
            api_status_envelope(&config, "one")?
        }
        Some(OneCommand::Inventory { profile }) => {
            let config = load_profile!(profile.as_deref(), environment)?;
            api_inventory_envelope(&config, "one")?
        }
        Some(OneCommand::Connections { command }) => {
            super::one_connections::execute(&runtime, cli.apply, cli.yes, command)?
        }
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
        Some(OneCommand::Ui { command }) => super::one_ui::execute(&runtime, command)?,
        Some(OneCommand::AutoInsights { profile }) => {
            super::one_auto_insights::execute(&runtime, profile)?
        }
        Some(OneCommand::DesktopExec { profile }) => {
            super::one_desktop_exec::execute(&runtime, profile)?
        }
    })
}
