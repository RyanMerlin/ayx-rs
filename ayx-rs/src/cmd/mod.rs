//! Per-top-level-command dispatch modules.
//!
//! Each file in this directory owns the body of one `Command::X` match arm
//! from `main.rs`. The `Cli` struct + the top-level clap tree still live in
//! `main.rs`, but the dispatch lives here — this lets the parent stay a
//! shallow router and gives each command family its own file to grow in.
//!
//! Convention: every cmd module exposes one `execute(...)` entry point that
//! returns `anyhow::Result<Envelope>`. They take whatever Cli state they
//! need (apply flag, environment override, etc.) as parameters rather than
//! reaching back into a shared `cli` struct, so the boundary is explicit.

use anyhow::Result;

pub mod catalog;
pub(crate) mod command_surface;
pub mod confirm;
pub mod discover;
pub(crate) mod headless;
pub mod mongo;
pub mod one;
mod one_agent_assets;
mod one_api;
mod one_connections;
pub mod one_datasets;
mod one_doctor;
mod one_flows;
mod one_job_groups;
mod one_open;
mod one_output_objects;
mod one_plans;
mod one_platform;
mod one_scheduling;
#[cfg(feature = "ui")]
mod one_ui;
mod one_webhook_flow_tasks;
mod one_workflows;
mod one_write_settings;
pub mod registry;
pub mod select;
pub mod server;
pub mod sqlserver;
pub mod telemetry;
pub mod tools;
pub mod workflow;

/// Shared runtime context for command families that need to load profiles.
pub(crate) struct RuntimeCtx<'a> {
    pub environment: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub workspace_source: ayx_core::profile::WorkspaceResolutionSource,
    pub no_input: bool,
    pub page_size: Option<u32>,
}

impl<'a> RuntimeCtx<'a> {
    pub(crate) fn new(environment: Option<&'a str>) -> Self {
        Self {
            environment,
            workspace: None,
            workspace_source: ayx_core::profile::WorkspaceResolutionSource::ActiveProfile,
            no_input: false,
            page_size: None,
        }
    }

    fn apply_workspace(
        &self,
        mut config: ayx_core::profile::Config,
    ) -> Result<ayx_core::profile::Config> {
        if let Some(one) = config.alteryx_one.as_mut() {
            one.migrate_workspace_credentials().map_err(|message| {
                anyhow::anyhow!("workspace credential migration failed: {message}")
            })?;
        }
        if let Some(selector) = self.workspace {
            let one = config
                .alteryx_one
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("--workspace requires an alteryx_one profile"))?;
            one.validate_workspace_identities()
                .map_err(|message| anyhow::anyhow!("--workspace: {message}"))?;
            let target = one
                .resolve_workspace_target(selector, self.workspace_source)
                .map_err(|message| anyhow::anyhow!("--workspace: {message}"))?;
            one.active_workspace_id = Some(target.workspace_id);
        }
        Ok(config)
    }

    pub(crate) fn load_profile<P>(&self, profile: P) -> Result<ayx_core::profile::Config>
    where
        P: Into<crate::ProfileInput<'a>>,
    {
        self.apply_workspace(crate::load_profile_with_env(profile, self.environment)?)
    }

    pub(crate) fn load_profile_lenient<P>(&self, profile: P) -> Result<ayx_core::profile::Config>
    where
        P: Into<crate::ProfileInput<'a>>,
    {
        self.apply_workspace(crate::load_profile_with_env_lenient(
            profile,
            self.environment,
        )?)
    }

    /// Login resolves its rollout before validating bound keyring references,
    /// so a CLI flow override can take precedence over rollout environment.
    pub(crate) fn load_profile_lenient_for_auth<P>(
        &self,
        profile: P,
    ) -> Result<ayx_core::profile::Config>
    where
        P: Into<crate::ProfileInput<'a>>,
    {
        self.apply_workspace(crate::load_profile_with_env_lenient_unvalidated(
            profile,
            self.environment,
        )?)
    }
}
