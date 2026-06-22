//! Dispatch for `ayx tools workspace ...`.
//!
//! Pure delegation to the four `tools_workspace_*_envelope` helpers in
//! main.rs. No `load_profile` dependency — workspace operations take an
//! explicit `--workspace` argument and pre-parse it themselves.

use anyhow::Result;
use ayx_core::envelope::Envelope;

use crate::{
    ToolsCommand, ToolsWorkspaceCommand, tools_workspace_compare_envelope,
    tools_workspace_init_envelope, tools_workspace_migrate_envelope,
    tools_workspace_resolve_envelope,
};

pub fn execute(command: Option<ToolsCommand>) -> Result<Envelope> {
    let help = "tools workspace commands available: init, resolve, compare, migrate-workflows, check-dcm-connections";
    match command {
        None => Ok(Envelope::ok(help)),
        Some(ToolsCommand::Workspace { command }) => match command {
            None => Ok(Envelope::ok(help)),
            Some(ToolsWorkspaceCommand::Init {
                output_file,
                active_environment,
                source_environment,
                target_environment,
            }) => tools_workspace_init_envelope(
                &output_file,
                &active_environment,
                &source_environment,
                &target_environment,
            ),
            Some(ToolsWorkspaceCommand::Resolve {
                workspace,
                source,
                target,
            }) => tools_workspace_resolve_envelope(&workspace, &source, &target),
            Some(ToolsWorkspaceCommand::Compare {
                workspace,
                source,
                target,
            }) => tools_workspace_compare_envelope(&workspace, &source, &target),
            Some(ToolsWorkspaceCommand::MigrateWorkflows {
                workspace,
                source,
                target,
            }) => tools_workspace_migrate_envelope(&workspace, &source, &target, "workflows"),
            Some(ToolsWorkspaceCommand::CheckDcmConnections {
                workspace,
                source,
                target,
            }) => tools_workspace_migrate_envelope(&workspace, &source, &target, "dcm-connections"),
        },
    }
}
