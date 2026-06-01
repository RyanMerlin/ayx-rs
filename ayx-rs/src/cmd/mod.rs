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
pub mod confirm;
pub mod dashboard;
pub mod mongo;
pub mod one;
mod one_connections;
mod one_doctor;
mod one_flows;
mod one_job_groups;
mod one_plans;
mod one_platform;
pub mod registry;
pub mod server;
pub mod sqlserver;
pub mod telemetry;
pub mod tools;
pub mod workflow;

/// Shared runtime context for command families that need to load profiles.
pub(crate) struct RuntimeCtx<'a> {
    pub environment: Option<&'a str>,
}

impl<'a> RuntimeCtx<'a> {
    pub(crate) fn new(environment: Option<&'a str>) -> Self {
        Self { environment }
    }

    pub(crate) fn load_profile<P>(&self, profile: P) -> Result<ayx_core::profile::Config>
    where
        P: Into<crate::ProfileInput<'a>>,
    {
        crate::load_profile_with_env(profile, self.environment)
    }

    pub(crate) fn load_profile_lenient<P>(&self, profile: P) -> Result<ayx_core::profile::Config>
    where
        P: Into<crate::ProfileInput<'a>>,
    {
        crate::load_profile_with_env_lenient(profile, self.environment)
    }
}
