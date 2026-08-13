//! Dispatch for `ayx telemetry ...`.
//!
//! Read-only operational telemetry over Alteryx One and (Phase 2) Server:
//! running jobs, run history, top workflows / plans by duration or failure
//! rate, recent errors, queue stats, weekly run-count matrices, and
//! permission summaries.
//!
//! Phase 1 implements the One side only. Phase 2 adds Server (Mongo +
//! Server-API V3); Phase 3 adds cross-source permission summaries.

use anyhow::Result;
use ayx_core::envelope::Envelope;
use clap::{Args, Subcommand};

pub mod aggregate;
pub mod errors;
pub mod jobs;
pub mod permissions;
pub mod plans;
pub mod server;
pub mod source;
pub mod summary;
pub mod weekly;
pub mod window;
pub mod workflows;

/// Shared flags every telemetry subcommand accepts. Flattened into each
/// variant so users get the same flags everywhere without us repeating
/// the declarations.
#[derive(Args, Debug, Clone)]
pub struct TelemetryArgs {
    /// Central profile name. Defaults to the active central profile.
    #[arg(long, global = false)]
    pub profile: Option<String>,
    /// Override backend selection. Default is auto-detection from the profile:
    /// pick the only configured surface, or require an explicit override if
    /// both `alteryx_one` and `server` are configured.
    #[arg(long, value_parser = ["one", "server", "auto"], default_value = "auto")]
    pub source: String,
    /// Time window for history queries: <N>{h,d,w} (default: 7d).
    #[arg(long, default_value = "7d")]
    pub since: String,
    /// Cap top-N listings (default: 10).
    #[arg(long, default_value = "10")]
    pub top: usize,
    /// Auto-paginate One list endpoints until exhausted.
    #[arg(long)]
    pub all: bool,
    /// Cap pages when --all is set (default: 50 from `OneListParams`).
    #[arg(long)]
    pub max_pages: Option<u32>,
}

/// Shared flags for telemetry subcommands that currently support only Alteryx One.
#[derive(Args, Debug, Clone)]
pub struct OneTelemetryArgs {
    /// Central profile name. Defaults to the active central profile.
    #[arg(long, global = false)]
    pub profile: Option<String>,
    /// Backend selection for this command.
    #[arg(long, value_parser = ["one"], default_value = "one")]
    pub source: String,
    /// Time window for history queries: <N>{h,d,w} (default: 7d).
    #[arg(long, default_value = "7d")]
    pub since: String,
    /// Cap top-N listings (default: 10).
    #[arg(long, default_value = "10")]
    pub top: usize,
    /// Auto-paginate One list endpoints until exhausted.
    #[arg(long)]
    pub all: bool,
    /// Cap pages when --all is set (default: 50 from `OneListParams`).
    #[arg(long)]
    pub max_pages: Option<u32>,
}

impl From<&OneTelemetryArgs> for TelemetryArgs {
    fn from(args: &OneTelemetryArgs) -> Self {
        Self {
            profile: args.profile.clone(),
            source: args.source.clone(),
            since: args.since.clone(),
            top: args.top,
            all: args.all,
            max_pages: args.max_pages,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum TelemetryCommand {
    /// Job-group telemetry: running, history, top.
    Jobs {
        #[command(subcommand)]
        command: TelemetryJobsCommand,
    },
    /// Workflow telemetry: top by run-count / failure-rate / duration, errors.
    Workflows {
        #[command(subcommand)]
        command: TelemetryWorkflowsCommand,
    },
    /// Plan telemetry: top by run-count, performance percentiles.
    Plans {
        #[command(subcommand)]
        command: TelemetryPlansCommand,
    },
    /// Recent failed-job messages with timestamps.
    Errors {
        #[command(subcommand)]
        command: TelemetryErrorsCommand,
    },
    /// Weekly run-count matrix (7×24 buckets) — data feed for the deferred
    /// heatmap phase.
    Weekly {
        #[command(subcommand)]
        command: TelemetryWeeklyCommand,
    },
    /// Queue depth and wait-time stats (Server source only in Phase 2).
    Queue {
        #[command(subcommand)]
        command: TelemetryQueueCommand,
    },
    /// Who has access to which connections, workflows, and collections.
    Permissions {
        #[command(subcommand)]
        command: TelemetryPermissionsCommand,
    },
    /// One-shot overview composing the above into a single envelope.
    Summary {
        #[command(flatten)]
        args: OneTelemetryArgs,
    },
}

#[derive(Subcommand, Debug)]
pub enum TelemetryJobsCommand {
    /// List job groups currently in Running or Queued state.
    Running {
        #[command(flatten)]
        args: TelemetryArgs,
    },
    /// Recent job-group history (succeeded + failed + cancelled) in --since window.
    History {
        #[command(flatten)]
        args: TelemetryArgs,
    },
    /// Top flows by run count over --since window.
    Top {
        #[command(flatten)]
        args: TelemetryArgs,
    },
}

#[derive(Subcommand, Debug)]
pub enum TelemetryWorkflowsCommand {
    /// Top flows by run count, failure rate, or duration over --since window.
    Top {
        #[command(flatten)]
        args: OneTelemetryArgs,
        /// Sort key: run-count | failure-rate | p95-duration (default run-count).
        #[arg(long, default_value = "run-count")]
        by: String,
    },
    /// Per-flow duration percentiles (p50/p95/p99) over --since window.
    Performance {
        #[command(flatten)]
        args: OneTelemetryArgs,
    },
    /// Flows ordered by failure count over --since window.
    Errors {
        #[command(flatten)]
        args: OneTelemetryArgs,
    },
}

#[derive(Subcommand, Debug)]
pub enum TelemetryPlansCommand {
    /// Top plans by run count over --since window.
    Top {
        #[command(flatten)]
        args: TelemetryArgs,
    },
    /// Per-plan duration percentiles.
    Performance {
        #[command(flatten)]
        args: TelemetryArgs,
    },
}

#[derive(Subcommand, Debug)]
pub enum TelemetryErrorsCommand {
    /// Recent failed job groups with error messages.
    Recent {
        #[command(flatten)]
        args: TelemetryArgs,
    },
}

#[derive(Subcommand, Debug)]
pub enum TelemetryPermissionsCommand {
    /// DCM connections and the subjects with access to each.
    Connections {
        #[command(flatten)]
        args: TelemetryArgs,
        /// Expand per-connection by iterating
        /// `/v4/connections/{id}/permissions/sharedSubjects`. Cost is
        /// O(connections); on large tenants pair with `--max-pages` to cap
        /// blast radius.
        #[arg(long)]
        deep: bool,
    },
    /// Who has workflow access. On One that's workspace people (no per-flow
    /// ACL endpoint); on Server it's the collections.appinfos surface.
    Workflows {
        #[command(flatten)]
        args: TelemetryArgs,
        /// Workspace id for `/iam/v1/workspaces/{id}/people` (One only).
        /// Falls back to `alteryx_one.expected_workspace_id` if unset.
        #[arg(long)]
        workspace_id: Option<String>,
    },
    /// Collections / Gallery item-membership ACLs (Server only).
    Collections {
        #[command(flatten)]
        args: TelemetryArgs,
    },
    /// Roll up access counts: connections per subject, people per workspace.
    Summary {
        #[command(flatten)]
        args: TelemetryArgs,
        #[arg(long)]
        workspace_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum TelemetryQueueCommand {
    /// Currently running + queued jobs (Server side).
    Status {
        #[command(flatten)]
        args: TelemetryArgs,
    },
    /// Wait-time stats over recent queue entries.
    WaitTime {
        #[command(flatten)]
        args: TelemetryArgs,
    },
}

#[derive(Subcommand, Debug)]
pub enum TelemetryWeeklyCommand {
    /// Emit a stable 168-bucket run-count matrix (day_of_week × hour).
    RunCounts {
        #[command(flatten)]
        args: OneTelemetryArgs,
    },
}

pub fn execute(environment: Option<&str>, command: TelemetryCommand) -> Result<Envelope> {
    match command {
        TelemetryCommand::Jobs { command } => match command {
            TelemetryJobsCommand::Running { args } => jobs::running(environment, &args),
            TelemetryJobsCommand::History { args } => jobs::history(environment, &args),
            TelemetryJobsCommand::Top { args } => jobs::top(environment, &args),
        },
        TelemetryCommand::Workflows { command } => match command {
            TelemetryWorkflowsCommand::Top { args, by } => workflows::top(environment, &args, &by),
            TelemetryWorkflowsCommand::Performance { args } => {
                workflows::performance(environment, &args)
            }
            TelemetryWorkflowsCommand::Errors { args } => workflows::errors(environment, &args),
        },
        TelemetryCommand::Plans { command } => match command {
            TelemetryPlansCommand::Top { args } => plans::top(environment, &args),
            TelemetryPlansCommand::Performance { args } => plans::performance(environment, &args),
        },
        TelemetryCommand::Errors { command } => match command {
            TelemetryErrorsCommand::Recent { args } => errors::recent(environment, &args),
        },
        TelemetryCommand::Weekly { command } => match command {
            TelemetryWeeklyCommand::RunCounts { args } => weekly::run_counts(environment, &args),
        },
        TelemetryCommand::Queue { command } => match command {
            TelemetryQueueCommand::Status { args } => {
                let (config, src) = load_and_pick_source(&args, environment)?;
                if src != source::TelemetrySource::Server {
                    return Err(anyhow::anyhow!(
                        "validation: telemetry queue requires --source server; the One surface does not expose a queue endpoint"
                    ));
                }
                server::queue_status(&config)
            }
            TelemetryQueueCommand::WaitTime { args } => {
                let (config, src) = load_and_pick_source(&args, environment)?;
                if src != source::TelemetrySource::Server {
                    return Err(anyhow::anyhow!(
                        "validation: telemetry queue requires --source server; the One surface does not expose a queue endpoint"
                    ));
                }
                server::queue_wait_time(&config, &args)
            }
        },
        TelemetryCommand::Permissions { command } => match command {
            TelemetryPermissionsCommand::Connections { args, deep } => {
                permissions::connections(environment, &args, deep)
            }
            TelemetryPermissionsCommand::Workflows { args, workspace_id } => {
                permissions::workflows(environment, &args, workspace_id.as_deref())
            }
            TelemetryPermissionsCommand::Collections { args } => {
                permissions::collections(environment, &args)
            }
            TelemetryPermissionsCommand::Summary { args, workspace_id } => {
                permissions::summary(environment, &args, workspace_id.as_deref())
            }
        },
        TelemetryCommand::Summary { args } => summary::summary(environment, &args),
    }
}

/// Helper used by every subcommand: load the profile under the active
/// environment, then resolve the requested telemetry source.
pub fn load_and_pick_source(
    args: &TelemetryArgs,
    environment: Option<&str>,
) -> Result<(ayx_core::profile::Config, source::TelemetrySource)> {
    let runtime = crate::cmd::RuntimeCtx::new(environment);
    let config = runtime.load_profile_lenient(args.profile.as_deref())?;
    let src = source::pick(&config, Some(&args.source))?;
    Ok((config, src))
}
