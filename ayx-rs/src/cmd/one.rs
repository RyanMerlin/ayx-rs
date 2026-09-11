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
    OneAgentAssetsCommand, OneAgentDatasetsCommand, OneAgentWorkflowsCommand, OneAgentsCommand,
    OneApiCommand, OneAuthCommand, OneCommand, OneConnectionPermissionCommand,
    OneConnectionsCommand, OneConnectorMetadataCommand, OneConnectorMetadataOverridesCommand,
    OneDatasetsCommand, OneDatasetsImportedCommand, OneDatasetsWrangledCommand,
    OneFlowFolderFlowsCommand, OneFlowFoldersCommand, OneFlowLibraryCommand, OneFlowsCommand,
    OneJobGroupCommand, OneJobsCommand, OneOutputObjectCommand, OnePersonCommand, OnePlansCommand,
    OneRoleCommand, OneSchedulingCommand, OneTokenCommand, OneWebhookFlowTaskCommand,
    OneWorkflowsCommand, OneWorkspaceCommand, OneWriteSettingCommand,
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
const WORKFLOW_LIST_FIELDS: &[&str] =
    &["id", "name", "owner", "last_updated_at", "workflow_version"];
const GROUP_LIST_COLLECTION_KEYS: &[&str] = &["groups"];
/// People are identified and disambiguated by email, not only a display name.
/// These are deliberately distinct from the generic list columns.
///
/// `isAdmin` and `isDisabled` are deliberately NOT projected. `/v4/people` sets
/// `isAdmin` only on the caller's own record (see `docs/one-endpoint-matrix.md`),
/// so the column rendered blank for every other person and, for the caller,
/// contradicted `one workspace admins` — which is the authoritative answer to
/// who administers a workspace. A column that is empty 17 rows out of 18 and
/// wrong on the 18th is worse than no column.
const PEOPLE_LIST_FIELDS: &[&str] = &["id", "name", "fullName", "email"];
const WORKSPACE_ADMIN_LIST_FIELDS: &[&str] = &["id", "name", "email", "createdAt", "updatedAt"];
const WORKSPACE_CURRENT_FIELDS: &[&str] = &[
    "id",
    "name",
    "displayName",
    "state",
    "workspace_member_count",
    "workspace_tier",
    "gid",
    "custom_url",
];

fn list(command: &'static str) -> OutputDescriptor {
    OutputDescriptor::new(command, ViewKind::List).with_fields(LIST_FIELDS)
}

fn group_list(command: &'static str) -> OutputDescriptor {
    list(command).with_collection_keys(GROUP_LIST_COLLECTION_KEYS)
}

fn list_with(command: &'static str, fields: &'static [&'static str]) -> OutputDescriptor {
    OutputDescriptor::new(command, ViewKind::List).with_fields(fields)
}

fn detail(command: &'static str) -> OutputDescriptor {
    OutputDescriptor::new(command, ViewKind::Detail).with_fields(DETAIL_FIELDS)
}

fn detail_with(command: &'static str, fields: &'static [&'static str]) -> OutputDescriptor {
    OutputDescriptor::new(command, ViewKind::Detail).with_fields(fields)
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
        OneCommand::AgentAssets { command } => agent_assets_descriptor(command),
        OneCommand::Jobs { id, command, .. } => jobs_descriptor(id.as_deref(), command.as_ref()),
        OneCommand::JobGroups { command } => legacy_job_groups_descriptor(command),
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
        OneCommand::Open { .. } => OutputDescriptor::new("one.open", ViewKind::Result)
            .with_fields(&["kind", "id", "url", "launched"]),
        #[cfg(feature = "ui")]
        OneCommand::Ui { .. } => OutputDescriptor::new("one.ui", ViewKind::Raw),
    }
}

fn workspace_descriptor(command: &OneWorkspaceCommand) -> OutputDescriptor {
    match command {
        OneWorkspaceCommand::List { .. } => list("one.workspace.list"),
        OneWorkspaceCommand::People => list_with("one.workspace.people", PEOPLE_LIST_FIELDS),
        OneWorkspaceCommand::Admins => {
            list_with("one.workspace.admins", WORKSPACE_ADMIN_LIST_FIELDS)
        }
        OneWorkspaceCommand::Groups { .. } => group_list("one.workspace.groups"),
        OneWorkspaceCommand::GroupsGlobal => group_list("one.workspace.groups-global"),
        OneWorkspaceCommand::CloudConfigs { .. } => list("one.workspace.cloud-configs"),
        OneWorkspaceCommand::Current => {
            detail_with("one.workspace.current", WORKSPACE_CURRENT_FIELDS)
        }
        // Same resource as `current`, just addressed by id, so it gets the same
        // projection. The generic detail fields omit `state`, `gid`,
        // `workspace_member_count`, `workspace_tier`, and `custom_url`, which
        // would hand an agent a thinner object here than `workspace current`
        // returns for the very same workspace.
        OneWorkspaceCommand::Detail { .. } => {
            detail_with("one.workspace.detail", WORKSPACE_CURRENT_FIELDS)
        }
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
        None | Some(OnePersonCommand::List { .. }) => {
            list_with("one.person.list", PEOPLE_LIST_FIELDS)
        }
        Some(OnePersonCommand::Current) => detail("one.person.current"),
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
        OneRoleCommand::List => list("one.role.list"),
        OneRoleCommand::ListAssignments { .. } => list("one.role.list-assignments"),
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
        // The workflow service returns two sibling arrays rather than a
        // conventional `items` wrapper. Declare both explicitly so terminal
        // output renders them as separate tables instead of calling the shape
        // unrecognized.
        OneWorkflowsCommand::Tools { .. } => list("one.workflows.tools")
            .with_named_collections(&["tools", "toolsByProductCapabilityId"]),
        OneWorkflowsCommand::Upload { .. } => result("one.workflows.upload"),
        OneWorkflowsCommand::Dependencies { .. } => list("one.workflows.dependencies"),
        OneWorkflowsCommand::Count { .. } => detail("one.workflows.count"),
        OneWorkflowsCommand::Detail { .. } => detail("one.workflows.detail"),
        OneWorkflowsCommand::Engines { .. } => detail("one.workflows.engines"),
        OneWorkflowsCommand::Run { .. } => result("one.workflows.run"),
        OneWorkflowsCommand::Cancel { .. } => result("one.workflows.cancel"),
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
        OneConnectionPermissionCommand::List { .. } => list("one.connections.permissions.list")
            .with_collection_keys(&["permission_rows"])
            .with_fields(&[
                "subject_type",
                "subject_id",
                "display_identity",
                "role",
                "policy",
                "created",
                "source",
            ]),
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
        OnePlansCommand::List { .. } => list("one.plans.list"),
        OnePlansCommand::Schedules { .. } => list("one.plans.schedules"),
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
        OneDatasetsCommand::Create { .. } => result("one.datasets.create"),
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

fn agent_assets_descriptor(command: &OneAgentAssetsCommand) -> OutputDescriptor {
    match command {
        OneAgentAssetsCommand::Agents { command } => match command {
            OneAgentsCommand::List { .. } => list("one.agent-assets.agents.list"),
            OneAgentsCommand::Detail { .. } => detail("one.agent-assets.agents.detail"),
            OneAgentsCommand::Prompt { .. } => result("one.agent-assets.agents.prompt"),
            OneAgentsCommand::Create { .. } => result("one.agent-assets.agents.create"),
            OneAgentsCommand::Update { .. } => result("one.agent-assets.agents.update"),
            OneAgentsCommand::Delete { .. } => result("one.agent-assets.agents.delete"),
        },
        OneAgentAssetsCommand::Datasets { command } => match command {
            OneAgentDatasetsCommand::List { .. } => list("one.agent-assets.datasets.list"),
            OneAgentDatasetsCommand::Set { .. } => result("one.agent-assets.datasets.set"),
        },
        OneAgentAssetsCommand::Workflows { command } => match command {
            OneAgentWorkflowsCommand::List { .. } => list("one.agent-assets.workflows.list"),
            OneAgentWorkflowsCommand::Enable { .. } => result("one.agent-assets.workflows.enable"),
            OneAgentWorkflowsCommand::Disable { .. } => {
                result("one.agent-assets.workflows.disable")
            }
        },
    }
}

const JOB_RUN_FIELDS: &[&str] = &[
    "id",
    "jobGroup",
    "jobType",
    "status",
    "percentComplete",
    "createdAt",
    "startedAt",
    "finishedAt",
    "lastHeartbeatAt",
    "hasWarnings",
    "errorMessage",
    "executionLanguage",
    "sampleSize",
    "cpJobId",
    "emrcluster",
    "wrangleScript",
];

/// Every `ayx one jobs <VERB>` subcommand name, as clap spells it (kebab-case
/// where relevant). Used only to detect the `--profile`-before-verb mix-up in
/// `execute` above — see the comment there for why clap can silently swallow
/// one of these words as the `[JOB-ID]` positional instead of dispatching to
/// the subcommand.
const JOBS_VERB_NAMES: &[&str] = &[
    "list",
    "count",
    "execute",
    "publish",
    "cancel",
    "status",
    "runs",
    "inputs",
    "outputs",
    "publications",
    "profile",
    "profile-results",
    "pdf-results",
];

fn is_jobs_verb_name(candidate: &str) -> bool {
    JOBS_VERB_NAMES.contains(&candidate)
}

fn jobs_descriptor(id: Option<&str>, command: Option<&OneJobsCommand>) -> OutputDescriptor {
    match (id, command) {
        (Some(_), None) => detail("one.jobs.detail"),
        (None, Some(OneJobsCommand::List { .. })) => list("one.jobs.list"),
        (None, Some(OneJobsCommand::Count { .. })) => detail("one.jobs.count"),
        (None, Some(OneJobsCommand::Execute { .. })) => result("one.jobs.execute"),
        (None, Some(OneJobsCommand::Publish { .. })) => result("one.jobs.publish"),
        (None, Some(OneJobsCommand::Cancel { .. })) => result("one.jobs.cancel"),
        (None, Some(OneJobsCommand::Status { .. })) => detail_with("one.jobs.status", &["status"]),
        (None, Some(OneJobsCommand::Runs { .. })) => list("one.jobs.runs")
            .with_fields(JOB_RUN_FIELDS)
            .with_detailed_rows(),
        (None, Some(OneJobsCommand::Inputs { .. })) => list("one.jobs.inputs"),
        (None, Some(OneJobsCommand::Outputs { .. })) => {
            list("one.jobs.outputs").with_named_collections(&["files", "tables"])
        }
        (None, Some(OneJobsCommand::Publications { .. })) => list("one.jobs.publications"),
        (None, Some(OneJobsCommand::Profile { .. })) => detail("one.jobs.profile"),
        (None, Some(OneJobsCommand::ProfileResults { .. })) => detail("one.jobs.profile-results"),
        (None, Some(OneJobsCommand::PdfResults { .. })) => detail("one.jobs.pdf-results"),
        // Clap accepts either a positional JOB-ID or one subcommand, and the
        // parent requires one of them. Keep this arm total for future parser
        // changes rather than allowing presentation metadata to panic.
        _ => OutputDescriptor::new("one.jobs", ViewKind::Raw),
    }
}

fn legacy_job_groups_descriptor(command: &OneJobGroupCommand) -> OutputDescriptor {
    // The view *kind* is shared -- these really do render as lists or details.
    // The `command` name is not: it is how a caller correlates a result back
    // to the invocation that produced it, so each leaf names itself.
    match command {
        OneJobGroupCommand::List { .. } => list("one.jobs.list"),
        OneJobGroupCommand::Inputs { .. } => list("one.jobs.inputs"),
        // `{files: [...], tables: [...]}` -- two lists, both shown.
        OneJobGroupCommand::Outputs { .. } => {
            list("one.jobs.outputs").with_named_collections(&["files", "tables"])
        }
        OneJobGroupCommand::Jobs { .. } => list("one.jobs.runs")
            .with_fields(JOB_RUN_FIELDS)
            .with_detailed_rows(),
        OneJobGroupCommand::Publications { .. } => list("one.jobs.publications"),
        OneJobGroupCommand::Count { .. } => detail("one.jobs.count"),
        OneJobGroupCommand::Detail { .. } => detail("one.jobs.detail"),
        // The body is a bare string ("Complete"); the one field labels it.
        OneJobGroupCommand::Status { .. } => detail_with("one.jobs.status", &["status"]),
        OneJobGroupCommand::Profile { .. } => detail("one.jobs.profile"),
        OneJobGroupCommand::ProfileResults { .. } => detail("one.jobs.profile-results"),
        OneJobGroupCommand::PdfResults { .. } => detail("one.jobs.pdf-results"),
        OneJobGroupCommand::Run { .. } => result("one.jobs.execute"),
        OneJobGroupCommand::Publish { .. } => result("one.jobs.publish"),
        OneJobGroupCommand::Cancel { .. } => result("one.jobs.cancel"),
    }
}

fn output_objects_descriptor(command: &OneOutputObjectCommand) -> OutputDescriptor {
    match command {
        OneOutputObjectCommand::List { .. } => list("one.output-objects.list"),
        OneOutputObjectCommand::Inputs { .. } => list("one.output-objects.inputs"),
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
    // `explicit_reauth: true`. The onboarding wizard only reaches here after a
    // human answered its "Log in now" prompt, so this must actually
    // authenticate. Treating it as a bare `ayx one login` let the
    // existing-credential short-circuit return "already configured" without
    // contacting the server, which left the profile holding the legacy keyring
    // references onboard had just written and broke every later One command.
    super::one_platform::auth::login(
        &runtime, profile, None, false, false, None, None, None, None, None, None, None, false,
        None, false, None, None, false, None, false, true,
    )
}

/// Borrow Cli's apply + yes for the TTY confirm prompts inside delete arms.
pub struct Ctx<'a> {
    pub apply: bool,
    pub yes: bool,
    pub environment: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub workspace_source: ayx_core::profile::WorkspaceResolutionSource,
    pub no_input: bool,
    pub page_size: Option<u32>,
}

#[allow(clippy::too_many_lines)]
pub fn execute(cli: Ctx<'_>, command: OneCommand) -> Result<Envelope> {
    // Capture `environment` up-front so `cli.environment` reads through the
    // helper don't conflict with `cli` itself being borrowed by other arms.
    let environment = cli.environment;
    let mut runtime = crate::cmd::RuntimeCtx::new(environment);
    runtime.workspace = cli.workspace;
    runtime.workspace_source = cli.workspace_source;
    runtime.no_input = cli.no_input;
    runtime.page_size = cli.page_size;
    Ok(match command {
        OneCommand::Login {
            profile,
            client_id,
            browser,
            device,
            oauth_api_token,
            auth_method,
            refresh_token,
            refresh_token_env,
            refresh_token_stdin,
            access_token,
            access_token_env,
            access_token_stdin,
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
            oauth_api_token,
            auth_method,
            refresh_token_env,
            refresh_token_stdin,
            access_token_env,
            access_token_stdin,
            // A user typing `ayx one login` gets the existing-credential
            // short-circuit; only the onboarding wizard opts out of it.
            false,
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
        OneCommand::Open { kind, id, print } => {
            super::one_open::execute(&runtime, kind, id, print)?
        }
        OneCommand::Doctor { command } => super::one_doctor::execute(&runtime, command)?,
        OneCommand::Api { command } => super::one_api::execute(&runtime, command)?,
        OneCommand::Jobs {
            id,
            profile,
            command,
        } => match (id, command) {
            // `args_conflicts_with_subcommands` on the `Jobs` variant locks
            // clap into "no subcommand" parsing as soon as any other arg
            // (`--profile` included) is seen ahead of the verb, so a verb
            // name typed after a leading `--profile` is swallowed as the
            // optional `[JOB-ID]` positional instead of matching a
            // subcommand — for a zero-further-args verb like `list`/`count`
            // this parses silently; for one that itself takes more tokens
            // (`runs <ID>`, `status <ID>`, `execute --body …`) clap already
            // rejects the leftover token as "unexpected argument" (exit 2).
            // Catch the silent case here: an id that is exactly a known
            // `jobs` verb name, combined with an explicit `--profile`, is
            // essentially always this mix-up rather than a literal JOB-ID.
            // A typed `UsageError` (not message-text sniffing) is what tells
            // `classify_anyhow_error` this is a validation error, so the
            // message can stay plain instead of being contorted to contain a
            // keyword the classifier scans for.
            (Some(id), None) if profile.is_some() && is_jobs_verb_name(&id) => {
                return Err(anyhow::Error::new(super::UsageError(format!(
                    "put --profile after the jobs verb: `ayx one jobs {id} <JOB-ID> --profile <PROFILE>` \
                     (for example `ayx one jobs runs <JOB-ID> --profile <PROFILE>`); if `{id}` is meant \
                     as a literal JOB-ID, move --profile after it instead: `ayx one jobs {id} --profile <PROFILE>`"
                ))));
            }
            (Some(id), None) => super::one_job_groups::execute(
                &runtime,
                OneJobGroupCommand::Detail {
                    profile,
                    id: Some(id),
                },
            )?,
            // Whenever a subcommand matched, the `Jobs`-level `--profile`
            // could not also have been consumed (see the comment above):
            // consuming any arg at the `Jobs` level locks clap out of
            // subcommand parsing for the rest of the invocation. Each
            // `OneJobsCommand` variant carries its own `profile` field for
            // the documented `ayx one jobs <VERB> <JOB-ID> --profile
            // <PROFILE>` form, converted along with the rest of `command`.
            (None, Some(command)) => super::one_job_groups::execute(&runtime, command.into())?,
            // Reachable only as `ayx one jobs --profile <PROFILE>` with no
            // JOB-ID and no subcommand — clap accepts it syntactically since
            // `--profile` alone doesn't trigger the subcommand conflict, but
            // there is nothing for the profile to scope. A typed
            // `UsageError` (see above) keeps this a validation error (exit
            // 2) without the message having to spell out a classifier
            // keyword.
            (None, None) => {
                return Err(anyhow::Error::new(super::UsageError(
                    "`ayx one jobs` needs a JOB-ID or a subcommand, e.g. `ayx one jobs <JOB-ID>` \
                     or `ayx one jobs runs <JOB-ID>`; see `ayx one jobs --help`"
                        .to_string(),
                )));
            }
            // clap's `args_conflicts_with_subcommands` on the `Jobs` variant
            // rejects a JOB-ID together with a subcommand before dispatch
            // ever reaches here.
            (Some(_), Some(_)) => unreachable!(
                "clap rejects a JOB-ID and a `jobs` subcommand together via args_conflicts_with_subcommands"
            ),
        },
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
        OneCommand::AgentAssets { command } => {
            super::one_agent_assets::execute(&runtime, cli.apply, cli.yes, command)?
        }
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

    /// The same defect class in the other families that had it. Kept as a
    /// separate test so a regression names the family it broke.
    #[test]
    fn sibling_families_do_not_share_a_command_name_either() {
        let plans_list = plans_descriptor(&OnePlansCommand::List {
            profile: None,
            limit: None,
            page_token: None,
            all: false,
            max_pages: None,
        });
        let plans_schedules = plans_descriptor(&OnePlansCommand::Schedules {
            profile: None,
            id: "1".to_string(),
        });
        assert_eq!(plans_list.command, "one.plans.list");
        assert_eq!(plans_schedules.command, "one.plans.schedules");
        assert_ne!(plans_list.command, plans_schedules.command);

        let oo_list = output_objects_descriptor(&OneOutputObjectCommand::List {
            profile: None,
            limit: None,
            page_token: None,
            all: false,
            max_pages: None,
        });
        let oo_inputs = output_objects_descriptor(&OneOutputObjectCommand::Inputs {
            profile: None,
            id: "1".to_string(),
        });
        assert_eq!(oo_list.command, "one.output-objects.list");
        assert_eq!(oo_inputs.command, "one.output-objects.inputs");
        assert_ne!(oo_list.command, oo_inputs.command);

        // `role list` and `role list-assignments` are separate user commands
        // hitting different endpoints (/v4/authorization/roles versus
        // .../roles/{id}/people), so they must not report the same name.
        let role_list = role_descriptor(&OneRoleCommand::List);
        let role_assignments = role_descriptor(&OneRoleCommand::ListAssignments {
            id: "1".to_string(),
        });
        assert_eq!(role_list.command, "one.role.list");
        assert_eq!(role_assignments.command, "one.role.list-assignments");
        assert_ne!(role_list.command, role_assignments.command);
    }

    /// `command` is part of the machine-readable envelope contract: it is how a
    /// caller correlates a result back to the invocation that produced it.
    /// Sharing one name across several leaves breaks that correlation. It was
    /// found live -- `ayx one job-groups profile <id>` reported
    /// `one.job-groups.detail`, and `inputs` reported `one.job-groups.list`,
    /// so a failure could not be traced to the command that caused it.
    ///
    /// The view *kind* is legitimately shared; the name is not.
    #[test]
    fn every_job_group_compatibility_leaf_reports_its_own_canonical_command_name() {
        let leaves = vec![
            (
                "list",
                OneJobGroupCommand::List {
                    profile: None,
                    limit: None,
                    page_token: None,
                    all: false,
                    max_pages: None,
                },
            ),
            ("count", OneJobGroupCommand::Count { profile: None }),
            (
                "detail",
                OneJobGroupCommand::Detail {
                    profile: None,
                    id: Some("1".to_string()),
                },
            ),
            (
                "status",
                OneJobGroupCommand::Status {
                    profile: None,
                    id: "1".to_string(),
                },
            ),
            (
                "inputs",
                OneJobGroupCommand::Inputs {
                    profile: None,
                    id: "1".to_string(),
                },
            ),
            (
                "outputs",
                OneJobGroupCommand::Outputs {
                    profile: None,
                    id: "1".to_string(),
                },
            ),
            (
                "runs",
                OneJobGroupCommand::Jobs {
                    profile: None,
                    id: "1".to_string(),
                },
            ),
            (
                "publications",
                OneJobGroupCommand::Publications {
                    profile: None,
                    id: "1".to_string(),
                },
            ),
            (
                "profile",
                OneJobGroupCommand::Profile {
                    profile: None,
                    id: "1".to_string(),
                },
            ),
            (
                "profile-results",
                OneJobGroupCommand::ProfileResults {
                    profile: None,
                    id: "1".to_string(),
                },
            ),
            (
                "pdf-results",
                OneJobGroupCommand::PdfResults {
                    profile: None,
                    id: "1".to_string(),
                },
            ),
        ];

        let mut seen: std::collections::BTreeMap<&'static str, String> =
            std::collections::BTreeMap::new();
        for (leaf, command) in leaves {
            let descriptor = legacy_job_groups_descriptor(&command);
            assert_eq!(
                descriptor.command,
                format!("one.jobs.{leaf}"),
                "the compatibility alias must report the canonical `one jobs {leaf}` envelope name"
            );
            if let Some(previous) = seen.insert(descriptor.command, leaf.to_string()) {
                panic!(
                    "'{}' is reported by both '{previous}' and '{leaf}'; a caller cannot tell them apart",
                    descriptor.command
                );
            }
        }
    }

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

        let workflow_tools = output_descriptor(&OneCommand::Workflows {
            command: OneWorkflowsCommand::Tools { profile: None },
        });
        assert_eq!(workflow_tools.command, "one.workflows.tools");
        assert_eq!(
            workflow_tools.named_collections,
            &["tools", "toolsByProductCapabilityId"],
            "workflow tools must explicitly declare its two sibling collections"
        );

        let groups = output_descriptor(&OneCommand::Workspace {
            command: OneWorkspaceCommand::Groups { workspace_id: None },
        });
        assert_eq!(groups.command, "one.workspace.groups");
        assert_eq!(groups.collection_keys, &["groups"]);

        let plan = output_descriptor(&OneCommand::Plans {
            command: OnePlansCommand::Run {
                profile: None,
                id: "plan_1".to_string(),
            },
        });
        assert_eq!(plan.command, "one.plans.run");
        assert_eq!(plan.kind, ViewKind::Result);
        assert!(plan.fields.contains(&"dry_run"));

        let people = output_descriptor(&OneCommand::Workspace {
            command: OneWorkspaceCommand::People,
        });
        assert_eq!(people.command, "one.workspace.people");
        assert!(people.fields.contains(&"email"));
        // `/v4/people` decorates only the caller's own record with `isAdmin`, so
        // projecting it rendered blank for everyone else and disagreed with
        // `one workspace admins` for the caller. `admins` is the authoritative
        // answer; this list must not imply a second, contradictory one.
        assert!(
            !people.fields.contains(&"isAdmin"),
            "workspace people must not project an admin flag the endpoint does not populate"
        );
        assert!(!people.fields.contains(&"isDisabled"));

        let admins = output_descriptor(&OneCommand::Workspace {
            command: OneWorkspaceCommand::Admins,
        });
        assert_eq!(admins.command, "one.workspace.admins");
        assert!(admins.fields.contains(&"email"));
        assert!(!admins.fields.contains(&"isAdmin"));

        let current = output_descriptor(&OneCommand::Workspace {
            command: OneWorkspaceCommand::Current,
        });
        assert!(current.fields.contains(&"workspace_member_count"));
        assert!(current.fields.contains(&"workspace_tier"));
    }
}
