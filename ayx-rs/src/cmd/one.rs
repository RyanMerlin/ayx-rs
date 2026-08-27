//! Dispatch for `ayx one ...`.
//!
//! The largest single dispatch arm in the original main.rs — ~2000 LOC
//! covering platform / workspace / role / person / token / api / auth /
//! plans / scheduling / flows / connections / connector
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

use crate::{
    OneApiCommand, OneAuthCommand, OneCommand, OneConnectionPermissionCommand,
    OneConnectionsCommand, OneConnectorMetadataCommand, OneConnectorMetadataOverridesCommand,
    OneDatasetsCommand, OneDatasetsImportedCommand, OneDatasetsWrangledCommand,
    OneFlowFolderFlowsCommand, OneFlowFoldersCommand, OneFlowLibraryCommand, OneFlowsCommand,
    OneJobGroupCommand, OneOutputObjectCommand, OnePersonCommand, OnePlansCommand, OneRoleCommand,
    OneSchedulingCommand, OneTokenCommand, OneWebhookFlowTaskCommand, OneWorkflowsCommand,
    OneWorkspaceCommand, OneWriteSettingCommand,
};

use crate::output::{OutputDescriptor, ViewKind};

const LIST_FIELDS: &[&str] = &[
    "id",
    "name",
    "title",
    "displayName",
    "status",
    "createdAt",
    "updatedAt",
];
const DETAIL_FIELDS: &[&str] = &[
    "id",
    "name",
    "title",
    "displayName",
    "status",
    "description",
    "createdAt",
    "updatedAt",
];
const RESULT_FIELDS: &[&str] = &[
    "id",
    "name",
    "status",
    "dry_run",
    "mutating",
    "applied",
    "would_send",
    "audit_artifact",
];
const WORKFLOW_LIST_FIELDS: &[&str] = &[
    "id",
    "name",
    "owner",
    "last_updated_at",
    "workflow_version",
];

fn list(command: &'static str) -> OutputDescriptor {
    OutputDescriptor::new(command, ViewKind::List).with_fields(LIST_FIELDS)
}

fn detail(command: &'static str) -> OutputDescriptor {
    OutputDescriptor::new(command, ViewKind::Detail).with_fields(DETAIL_FIELDS)
}

fn result(command: &'static str) -> OutputDescriptor {
    OutputDescriptor::new(command, ViewKind::Result).with_fields(RESULT_FIELDS)
}

/// Exact presentation metadata for the One leaf command selected by clap.
/// Keep this beside the One dispatcher so adding a command requires choosing a
/// human/compact view deliberately rather than falling back to JSON shape
/// inference in the outer CLI.
pub(crate) fn output_descriptor(command: &OneCommand) -> OutputDescriptor {
    match command {
        OneCommand::Flows { command } => flows_descriptor(command),
        OneCommand::Workflows { command } => workflows_descriptor(command),
        OneCommand::Connections { command } => connections_descriptor(command),
        OneCommand::Plans { command } => plans_descriptor(command),
        OneCommand::Datasets { command } => datasets_descriptor(command),
        OneCommand::JobGroups { command } => job_groups_descriptor(command),
        OneCommand::OutputObjects { command } => output_objects_descriptor(command),
        OneCommand::WriteSettings { command } => write_settings_descriptor(command),
        OneCommand::Scheduling { command } => scheduling_descriptor(command),
        OneCommand::Workspace { command } => workspace_descriptor(command),
        OneCommand::Person { command } => person_descriptor(command.as_ref()),
        OneCommand::Token { command } => token_descriptor(command.as_ref()),
        OneCommand::Role { command } => role_descriptor(command),
        OneCommand::Doctor { command } => OutputDescriptor::new(
            match command {
                crate::OneDoctorCommand::Auth { .. } => "one.doctor.auth",
                crate::OneDoctorCommand::Discover { .. } => "one.doctor.discover",
                crate::OneDoctorCommand::Identity { .. } => "one.doctor.identity",
                crate::OneDoctorCommand::Plans { .. } => "one.doctor.plans",
                crate::OneDoctorCommand::Scheduling { .. } => "one.doctor.scheduling",
            },
            ViewKind::Diagnostic,
        ),
        OneCommand::Api { command } => api_descriptor(command),
        OneCommand::WebhookFlowTasks { command } => webhook_task_descriptor(command),
        OneCommand::Login { .. } => result("one.login"),
        OneCommand::Logout { .. } => result("one.logout"),
        OneCommand::Whoami => detail("one.whoami"),
        OneCommand::Auth { command } => auth_descriptor(command),
        OneCommand::Inventory { .. } => {
            OutputDescriptor::new("one.inventory", ViewKind::Diagnostic)
        }
        #[cfg(feature = "ui")]
        OneCommand::Ui { .. } => OutputDescriptor::new("one.ui", ViewKind::Raw),
    }
}

fn workspace_descriptor(command: &OneWorkspaceCommand) -> OutputDescriptor {
    match command {
        OneWorkspaceCommand::List { .. } => list("one.workspace.list"),
        OneWorkspaceCommand::People => list("one.workspace.people"),
        OneWorkspaceCommand::Admins => list("one.workspace.admins"),
        OneWorkspaceCommand::Groups { .. } => list("one.workspace.groups"),
        OneWorkspaceCommand::GroupsGlobal => list("one.workspace.groups-global"),
        OneWorkspaceCommand::CloudConfigs { .. } => list("one.workspace.cloud-configs"),
        OneWorkspaceCommand::Current => detail("one.workspace.current"),
        OneWorkspaceCommand::CurrentConfiguration => detail("one.workspace.current-configuration"),
        OneWorkspaceCommand::ConfigurationV4 { .. } => detail("one.workspace.configuration-v4"),
        OneWorkspaceCommand::Configuration { .. } => detail("one.workspace.configuration"),
        OneWorkspaceCommand::ConfigurationSchema { .. } => {
            detail("one.workspace.configuration-schema")
        }
        OneWorkspaceCommand::CurrentConfigurationSchema => {
            detail("one.workspace.current-configuration-schema")
        }
        OneWorkspaceCommand::InvitationLink { .. } => detail("one.workspace.invitation-link"),
        OneWorkspaceCommand::Create { .. } => result("one.workspace.create"),
        OneWorkspaceCommand::Delete { .. } => result("one.workspace.delete"),
        OneWorkspaceCommand::SaveCurrentConfiguration { .. } => {
            result("one.workspace.save-current-configuration")
        }
        OneWorkspaceCommand::SaveConfigurationV4 { .. } => {
            result("one.workspace.save-configuration-v4")
        }
        OneWorkspaceCommand::DeleteCurrentConfiguration { .. } => {
            result("one.workspace.delete-current-configuration")
        }
        OneWorkspaceCommand::DeleteConfiguration { .. } => {
            result("one.workspace.delete-configuration")
        }
        OneWorkspaceCommand::CreateGroup { .. } => result("one.workspace.create-group"),
        OneWorkspaceCommand::DeleteGroup { .. } => result("one.workspace.delete-group"),
        OneWorkspaceCommand::UpdateGroup { .. } => result("one.workspace.update-group"),
        OneWorkspaceCommand::SetGroupRoles { .. } => result("one.workspace.set-group-roles"),
        OneWorkspaceCommand::AddGroupUsers { .. } => result("one.workspace.add-group-users"),
        OneWorkspaceCommand::RemoveGroupUsers { .. } => result("one.workspace.remove-group-users"),
        OneWorkspaceCommand::Switch { .. } => result("one.workspace.switch"),
        OneWorkspaceCommand::InviteUsers { .. } => result("one.workspace.invite-users"),
        OneWorkspaceCommand::Invite { .. } => result("one.workspace.invite"),
        OneWorkspaceCommand::InviteList { .. } => result("one.workspace.invite-list"),
        OneWorkspaceCommand::ReinviteUsers { .. } => result("one.workspace.reinvite-users"),
        OneWorkspaceCommand::RemoveUser { .. } => result("one.workspace.remove-user"),
        OneWorkspaceCommand::SuspendUsers { .. } => result("one.workspace.suspend-users"),
        OneWorkspaceCommand::UnsuspendUsers { .. } => result("one.workspace.unsuspend-users"),
        OneWorkspaceCommand::SuspendUser { .. } => result("one.workspace.suspend-user"),
        OneWorkspaceCommand::Transfer { .. } => result("one.workspace.transfer"),
        OneWorkspaceCommand::TransferAssets { .. } => result("one.workspace.transfer-assets"),
        OneWorkspaceCommand::CreateCloudConfig { .. } => {
            result("one.workspace.create-cloud-config")
        }
        OneWorkspaceCommand::UpdateCloudConfig { .. } => {
            result("one.workspace.update-cloud-config")
        }
        OneWorkspaceCommand::PatchUser { .. } => result("one.workspace.patch-user"),
        OneWorkspaceCommand::UpdateUser { .. } => result("one.workspace.update-user"),
    }
}

fn person_descriptor(command: Option<&OnePersonCommand>) -> OutputDescriptor {
    match command {
        None | Some(OnePersonCommand::List { .. }) => list("one.person.list"),
        Some(OnePersonCommand::Current) => detail("one.person.current"),
        Some(OnePersonCommand::Count) => detail("one.person.count"),
        Some(OnePersonCommand::Detail { .. }) => detail("one.person.detail"),
        Some(OnePersonCommand::Create { .. }) => result("one.person.create"),
        Some(OnePersonCommand::Update { .. }) => result("one.person.update"),
        Some(OnePersonCommand::Patch { .. }) => result("one.person.patch"),
        Some(OnePersonCommand::Delete { .. }) => result("one.person.delete"),
        Some(OnePersonCommand::UpdatePassword { .. }) => result("one.person.update-password"),
        Some(OnePersonCommand::PasswordResetRequest { .. }) => {
            result("one.person.password-reset-request")
        }
    }
}

fn token_descriptor(command: Option<&OneTokenCommand>) -> OutputDescriptor {
    match command {
        None | Some(OneTokenCommand::List) => list("one.token.list"),
        Some(OneTokenCommand::Detail { .. }) => detail("one.token.detail"),
        Some(OneTokenCommand::Create { .. }) => result("one.token.create"),
        Some(OneTokenCommand::Delete { .. }) => result("one.token.delete"),
    }
}

fn role_descriptor(command: &OneRoleCommand) -> OutputDescriptor {
    match command {
        OneRoleCommand::List | OneRoleCommand::ListAssignments { .. } => list("one.role.list"),
        OneRoleCommand::Detail { .. } => detail("one.role.detail"),
        OneRoleCommand::Assign { .. } => result("one.role.assign"),
        OneRoleCommand::Unassign { .. } => result("one.role.unassign"),
    }
}

fn api_descriptor(command: &OneApiCommand) -> OutputDescriptor {
    match command {
        OneApiCommand::Status { .. } => detail("one.api.status"),
        OneApiCommand::Diagnose { .. } => {
            OutputDescriptor::new("one.api.diagnose", ViewKind::Diagnostic)
        }
        OneApiCommand::OpenApiSpec { .. } => {
            OutputDescriptor::new("one.api.open-api-spec", ViewKind::Export)
        }
        OneApiCommand::Coverage { .. } => detail("one.api.coverage"),
    }
}

fn auth_descriptor(command: &OneAuthCommand) -> OutputDescriptor {
    let name = match command {
        OneAuthCommand::Status { .. } => "one.auth.status",
        OneAuthCommand::Diagnose { .. } => "one.auth.diagnose",
        OneAuthCommand::Protocol { .. } => "one.auth.protocol",
    };
    OutputDescriptor::new(name, ViewKind::Diagnostic)
}

fn flows_descriptor(command: &OneFlowsCommand) -> OutputDescriptor {
    match command {
        OneFlowsCommand::List { .. } => list("one.flows.list"),
        OneFlowsCommand::Count { .. } => detail("one.flows.count"),
        OneFlowsCommand::Library { command } => match command {
            OneFlowLibraryCommand::List { .. } => list("one.flows.library.list"),
            OneFlowLibraryCommand::Count { .. } => detail("one.flows.library.count"),
        },
        OneFlowsCommand::Folders { command } => match command {
            OneFlowFoldersCommand::List { .. } => list("one.flows.folders.list"),
            OneFlowFoldersCommand::Count { .. } => detail("one.flows.folders.count"),
            OneFlowFoldersCommand::Detail { .. } => detail("one.flows.folders.detail"),
            OneFlowFoldersCommand::Create { .. } => result("one.flows.folders.create"),
            OneFlowFoldersCommand::Update { .. } => result("one.flows.folders.update"),
            OneFlowFoldersCommand::Delete { .. } => result("one.flows.folders.delete"),
            OneFlowFoldersCommand::Flows { command } => match command {
                OneFlowFolderFlowsCommand::List { .. } => list("one.flows.folders.flows.list"),
                OneFlowFolderFlowsCommand::Count { .. } => detail("one.flows.folders.flows.count"),
            },
        },
        OneFlowsCommand::Detail { .. } => detail("one.flows.detail"),
        OneFlowsCommand::Inputs { .. } => list("one.flows.inputs"),
        OneFlowsCommand::Outputs { .. } => list("one.flows.outputs"),
        OneFlowsCommand::PermissionsGet { .. } => list("one.flows.permissions-get"),
        OneFlowsCommand::Export { .. } => {
            OutputDescriptor::new("one.flows.export", ViewKind::Export)
        }
        OneFlowsCommand::Create { .. } => result("one.flows.create"),
        OneFlowsCommand::Update { .. } => result("one.flows.update"),
        OneFlowsCommand::Delete { .. } => result("one.flows.delete"),
        OneFlowsCommand::Copy { .. } => result("one.flows.copy"),
        OneFlowsCommand::Run { .. } => result("one.flows.run"),
        OneFlowsCommand::Validate { .. } => result("one.flows.validate"),
        OneFlowsCommand::Parameters { .. } => detail("one.flows.parameters"),
        OneFlowsCommand::Permissions { .. } => result("one.flows.permissions"),
        OneFlowsCommand::Move { .. } => result("one.flows.move"),
        OneFlowsCommand::ReplaceDataset { .. } => result("one.flows.replace-dataset"),
        OneFlowsCommand::Import { .. } => result("one.flows.import"),
        OneFlowsCommand::ImportDryRun { .. } => result("one.flows.import-dry-run"),
        OneFlowsCommand::ExportDryRun { .. } => result("one.flows.export-dry-run"),
    }
}

fn workflows_descriptor(command: &OneWorkflowsCommand) -> OutputDescriptor {
    match command {
        OneWorkflowsCommand::List { .. } => {
            OutputDescriptor::new("one.workflows.list", ViewKind::List)
                .with_fields(WORKFLOW_LIST_FIELDS)
        }
        OneWorkflowsCommand::Assets { .. } => list("one.workflows.assets"),
        OneWorkflowsCommand::Tools { .. } => list("one.workflows.tools"),
        OneWorkflowsCommand::Dependencies { .. } => list("one.workflows.dependencies"),
        OneWorkflowsCommand::Count { .. } => detail("one.workflows.count"),
        OneWorkflowsCommand::Detail { .. } => detail("one.workflows.detail"),
        OneWorkflowsCommand::Engines { .. } => detail("one.workflows.engines"),
        OneWorkflowsCommand::Delete { .. } => result("one.workflows.delete"),
        OneWorkflowsCommand::Copy { .. } => result("one.workflows.copy"),
        OneWorkflowsCommand::Share { .. } => result("one.workflows.share"),
    }
}

fn connections_descriptor(command: &OneConnectionsCommand) -> OutputDescriptor {
    match command {
        OneConnectionsCommand::List { .. } => list("one.connections.list"),
        OneConnectionsCommand::Count { .. } => detail("one.connections.count"),
        OneConnectionsCommand::Detail { .. } => detail("one.connections.detail"),
        OneConnectionsCommand::Status { .. } => detail("one.connections.status"),
        OneConnectionsCommand::Create { .. } => result("one.connections.create"),
        OneConnectionsCommand::DryRun { .. } => result("one.connections.dry-run"),
        OneConnectionsCommand::Update { .. } => result("one.connections.update"),
        OneConnectionsCommand::Delete { .. } => result("one.connections.delete"),
        OneConnectionsCommand::ConnectorMetadata { command } => {
            connector_metadata_descriptor(command)
        }
        OneConnectionsCommand::Permissions { command } => {
            connection_permissions_descriptor(command)
        }
    }
}

fn connector_metadata_descriptor(command: &OneConnectorMetadataCommand) -> OutputDescriptor {
    match command {
        OneConnectorMetadataCommand::Defaults { .. } => {
            detail("one.connections.connector-metadata.defaults")
        }
        OneConnectorMetadataCommand::PublishInfo { .. } => {
            detail("one.connections.connector-metadata.publish-info")
        }
        OneConnectorMetadataCommand::Detail { .. } => {
            detail("one.connections.connector-metadata.detail")
        }
        OneConnectorMetadataCommand::Template { .. } => OutputDescriptor::new(
            "one.connections.connector-metadata.template",
            ViewKind::Export,
        ),
        OneConnectorMetadataCommand::Overrides { command } => match command {
            OneConnectorMetadataOverridesCommand::List { .. } => {
                list("one.connections.connector-metadata.overrides.list")
            }
            OneConnectorMetadataOverridesCommand::Create { .. } => {
                result("one.connections.connector-metadata.overrides.create")
            }
            OneConnectorMetadataOverridesCommand::Delete { .. } => {
                result("one.connections.connector-metadata.overrides.delete")
            }
        },
    }
}

fn connection_permissions_descriptor(command: &OneConnectionPermissionCommand) -> OutputDescriptor {
    match command {
        OneConnectionPermissionCommand::List { .. } => list("one.connections.permissions.list"),
        OneConnectionPermissionCommand::Detail { .. } => {
            detail("one.connections.permissions.detail")
        }
        OneConnectionPermissionCommand::Create { .. } => {
            result("one.connections.permissions.create")
        }
        OneConnectionPermissionCommand::Delete { .. } => {
            result("one.connections.permissions.delete")
        }
    }
}

fn plans_descriptor(command: &OnePlansCommand) -> OutputDescriptor {
    match command {
        OnePlansCommand::List { .. } | OnePlansCommand::Schedules { .. } => list("one.plans.list"),
        OnePlansCommand::Count { .. } => detail("one.plans.count"),
        OnePlansCommand::Detail { .. } => detail("one.plans.detail"),
        OnePlansCommand::Full { .. } => detail("one.plans.full"),
        OnePlansCommand::RunParameters { .. } => detail("one.plans.run-parameters"),
        OnePlansCommand::Export { .. } => {
            OutputDescriptor::new("one.plans.export", ViewKind::Export)
        }
        OnePlansCommand::Permissions { .. } => {
            OutputDescriptor::new("one.plans.permissions", ViewKind::Raw)
        }
        OnePlansCommand::Create { .. } => result("one.plans.create"),
        OnePlansCommand::Run { .. } => result("one.plans.run"),
        OnePlansCommand::Update { .. } => result("one.plans.update"),
        OnePlansCommand::Delete { .. } => result("one.plans.delete"),
        OnePlansCommand::Share { .. } => result("one.plans.share"),
        OnePlansCommand::Import { .. } => result("one.plans.import"),
    }
}

fn datasets_descriptor(command: &OneDatasetsCommand) -> OutputDescriptor {
    match command {
        OneDatasetsCommand::List { .. } => list("one.datasets.list"),
        OneDatasetsCommand::Count { .. } => detail("one.datasets.count"),
        OneDatasetsCommand::Wrangled { command } => match command {
            OneDatasetsWrangledCommand::List { .. } => list("one.datasets.wrangled.list"),
            OneDatasetsWrangledCommand::Count { .. } => detail("one.datasets.wrangled.count"),
            OneDatasetsWrangledCommand::Detail { .. } => detail("one.datasets.wrangled.detail"),
        },
        OneDatasetsCommand::Imported { command } => match command {
            OneDatasetsImportedCommand::Detail { .. } => detail("one.datasets.imported.detail"),
        },
    }
}

fn job_groups_descriptor(command: &OneJobGroupCommand) -> OutputDescriptor {
    match command {
        OneJobGroupCommand::List { .. }
        | OneJobGroupCommand::Inputs { .. }
        | OneJobGroupCommand::Outputs { .. }
        | OneJobGroupCommand::Jobs { .. }
        | OneJobGroupCommand::Publications { .. } => list("one.job-groups.list"),
        OneJobGroupCommand::Count { .. } => detail("one.job-groups.count"),
        OneJobGroupCommand::Detail { .. }
        | OneJobGroupCommand::Status { .. }
        | OneJobGroupCommand::Profile { .. }
        | OneJobGroupCommand::ProfileResults { .. }
        | OneJobGroupCommand::PdfResults { .. } => detail("one.job-groups.detail"),
        OneJobGroupCommand::Run { .. } => result("one.job-groups.run"),
        OneJobGroupCommand::Publish { .. } => result("one.job-groups.publish"),
        OneJobGroupCommand::Cancel { .. } => result("one.job-groups.cancel"),
    }
}

fn output_objects_descriptor(command: &OneOutputObjectCommand) -> OutputDescriptor {
    match command {
        OneOutputObjectCommand::List { .. } | OneOutputObjectCommand::Inputs { .. } => {
            list("one.output-objects.list")
        }
        OneOutputObjectCommand::Count { .. } => detail("one.output-objects.count"),
        OneOutputObjectCommand::Detail { .. } => detail("one.output-objects.detail"),
        OneOutputObjectCommand::Create { .. } => result("one.output-objects.create"),
        OneOutputObjectCommand::Update { .. } => result("one.output-objects.update"),
        OneOutputObjectCommand::Delete { .. } => result("one.output-objects.delete"),
        OneOutputObjectCommand::WrangleToPython { .. } => {
            result("one.output-objects.wrangle-to-python")
        }
    }
}

fn write_settings_descriptor(command: &OneWriteSettingCommand) -> OutputDescriptor {
    match command {
        OneWriteSettingCommand::List { .. } => list("one.write-settings.list"),
        OneWriteSettingCommand::Count { .. } => detail("one.write-settings.count"),
        OneWriteSettingCommand::Detail { .. } => detail("one.write-settings.detail"),
        OneWriteSettingCommand::Create { .. } => result("one.write-settings.create"),
        OneWriteSettingCommand::Update { .. } => result("one.write-settings.update"),
        OneWriteSettingCommand::Delete { .. } => result("one.write-settings.delete"),
    }
}

fn scheduling_descriptor(command: &OneSchedulingCommand) -> OutputDescriptor {
    match command {
        OneSchedulingCommand::List { .. } => list("one.scheduling.list"),
        OneSchedulingCommand::Count { .. } => detail("one.scheduling.count"),
        OneSchedulingCommand::Detail { .. } => detail("one.scheduling.detail"),
        OneSchedulingCommand::Create { .. } => result("one.scheduling.create"),
        OneSchedulingCommand::Update { .. } => result("one.scheduling.update"),
        OneSchedulingCommand::Enable { .. } => result("one.scheduling.enable"),
        OneSchedulingCommand::Disable { .. } => result("one.scheduling.disable"),
        OneSchedulingCommand::Delete { .. } => result("one.scheduling.delete"),
    }
}

fn webhook_task_descriptor(command: &OneWebhookFlowTaskCommand) -> OutputDescriptor {
    match command {
        OneWebhookFlowTaskCommand::Detail { .. } => detail("one.webhook-flow-tasks.detail"),
        OneWebhookFlowTaskCommand::Create { .. } => result("one.webhook-flow-tasks.create"),
        OneWebhookFlowTaskCommand::Delete { .. } => result("one.webhook-flow-tasks.delete"),
        OneWebhookFlowTaskCommand::Test { .. } => result("one.webhook-flow-tasks.test"),
    }
}

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
        &runtime, profile, None, false, false, None, None, None, None, None, None, None, false,
        None,
    )
}

/// Borrow Cli's apply + yes for the TTY confirm prompts inside delete arms.
pub struct Ctx<'a> {
    pub apply: bool,
    pub yes: bool,
    pub environment: Option<&'a str>,
}

#[allow(clippy::too_many_lines)]
pub fn execute(cli: Ctx<'_>, command: OneCommand) -> Result<Envelope> {
    // Capture `environment` up-front so `cli.environment` reads through the
    // helper don't conflict with `cli` itself being borrowed by other arms.
    let environment = cli.environment;
    let runtime = crate::cmd::RuntimeCtx::new(environment);
    Ok(match command {
        OneCommand::Login {
            profile,
            client_id,
            browser,
            device,
            refresh_token,
            access_token,
            token_endpoint,
            base_url,
            workspace_id,
            workspace_gid,
            auth_flow,
            save_workspace_password,
            secret_policy,
        } => super::one_platform::auth::login(
            &runtime,
            profile,
            client_id,
            browser,
            device,
            refresh_token,
            access_token,
            token_endpoint,
            base_url,
            workspace_id,
            workspace_gid,
            auth_flow,
            save_workspace_password,
            secret_policy,
        )?,
        OneCommand::Logout { profile } => {
            super::one_platform::auth::logout(&runtime, profile.as_deref())?
        }
        OneCommand::Whoami => super::one_platform::person::current(&runtime, None)?,
        OneCommand::Auth { command } => super::one_platform::auth::execute(&runtime, command)?,
        OneCommand::Workspace { command } => {
            super::one_platform::workspace::execute(&runtime, cli.apply, cli.yes, command)?
        }
        OneCommand::Role { command } => {
            super::one_platform::role::execute(&runtime, cli.apply, cli.yes, command)?
        }
        OneCommand::Token { command } => {
            super::one_platform::token::execute(&runtime, cli.apply, cli.yes, command)?
        }
        OneCommand::Person { command } => {
            super::one_platform::person::execute(&runtime, cli.apply, cli.yes, command)?
        }
        OneCommand::Inventory { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_surface_inventory_envelope(&config)?
        }
        OneCommand::Doctor { command } => super::one_doctor::execute(&runtime, command)?,
        OneCommand::Api { command } => super::one_api::execute(&runtime, command)?,
        OneCommand::JobGroups { command } => super::one_job_groups::execute(&runtime, command)?,
        OneCommand::OutputObjects { command } => {
            super::one_output_objects::execute(&runtime, command)?
        }
        OneCommand::WebhookFlowTasks { command } => {
            super::one_webhook_flow_tasks::execute(&runtime, command)?
        }
        OneCommand::WriteSettings { command } => {
            super::one_write_settings::execute(&runtime, command)?
        }
        OneCommand::Connections { command } => {
            super::one_connections::execute(&runtime, cli.apply, cli.yes, command)?
        }
        OneCommand::Workflows { command } => {
            super::one_workflows::execute(&runtime, cli.apply, cli.yes, command)?
        }
        OneCommand::Datasets { command } => super::one_datasets::execute(&runtime, command)?,
        OneCommand::Flows { command } => {
            super::one_flows::execute(&runtime, cli.apply, cli.yes, command)?
        }
        OneCommand::Plans { command } => {
            super::one_plans::execute(&runtime, cli.apply, cli.yes, command)?
        }
        OneCommand::Scheduling { command } => {
            super::one_scheduling::execute(&runtime, cli.apply, cli.yes, command)?
        }
        #[cfg(feature = "ui")]
        OneCommand::Ui { command } => super::one_ui::execute(&runtime, command)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OneFlowsCommand, OnePlansCommand, OneWorkflowsCommand};

    #[test]
    fn descriptors_name_one_leaf_commands_and_views() {
        let flow = output_descriptor(&OneCommand::Flows {
            command: OneFlowsCommand::Count { profile: None },
        });
        assert_eq!(flow.command, "one.flows.count");
        assert_eq!(flow.kind, ViewKind::Detail);

        let workflow = output_descriptor(&OneCommand::Workflows {
            command: OneWorkflowsCommand::List {
                profile: None,
                limit: None,
                page_token: None,
                all: false,
                max_pages: None,
            },
        });
        assert_eq!(workflow.command, "one.workflows.list");
        assert_eq!(workflow.kind, ViewKind::List);
        assert!(workflow.fields.contains(&"owner"));
        assert!(workflow.fields.contains(&"last_updated_at"));
        assert!(workflow.fields.contains(&"workflow_version"));

        let plan = output_descriptor(&OneCommand::Plans {
            command: OnePlansCommand::Run {
                profile: None,
                id: "plan_1".to_string(),
            },
        });
        assert_eq!(plan.command, "one.plans.run");
        assert_eq!(plan.kind, ViewKind::Result);
        assert!(plan.fields.contains(&"dry_run"));
    }
}
