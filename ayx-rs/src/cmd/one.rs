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

use anyhow::{anyhow, Result};
use ayx_core::envelope::Envelope;
use ayx_one::{
    api_diagnose_envelope, api_inventory_envelope, api_status_envelope,
    one_surface_inventory_envelope,
};
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};
use serde_json::json;

use crate::cmd;
use crate::{
    load_payload, one_platform_auth_diagnose_envelope, one_platform_auth_status_envelope,
    ui_command_envelope, OneBillingCommand, OneCommand, OneJobGroupCommand,
    OneOutputObjectCommand, OnePlatformApiCommand, OnePlatformAuthCommand, OnePlatformCommand,
    OnePlatformPersonCommand, OnePlatformTokenCommand, OneRoleCommand, OneSchedulingCommand,
    OneWebhookFlowTaskCommand, OneWorkspaceCommand, OneWriteSettingCommand, UiCommand,
    UiDataCommand, UiJobsCommand, UiLibraryCommand, UiSchedulesCommand, UiSessionCommand,
    UiWorkflowCommand,
};

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
            Some(OneCommand::Platform { command }) => match command {
                Some(OnePlatformCommand::Api { command }) => match command {
                    OnePlatformApiCommand::Status { profile } => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        api_status_envelope(&config, "one platform")?
                    }
                    OnePlatformApiCommand::Diagnose { profile } => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        api_diagnose_envelope(&config, "one platform")?
                    }
                    OnePlatformApiCommand::OpenApiSpec { profile } => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "open-api-spec",
                            "GET",
                            "/v4/open-api-spec",
                            false,
                            &[],
                        )?
                    }
                },
                Some(OnePlatformCommand::Status { profile }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    api_status_envelope(&config, "one platform")?
                }
                Some(OnePlatformCommand::Inventory { profile }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    one_surface_inventory_envelope(&config)?
                }
                Some(OnePlatformCommand::Workspace { command }) => match command {
                    OneWorkspaceCommand::List {
                        profile,
                        limit,
                        page_token,
                        all,
                        max_pages,
                    } => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        let params = ayx_one_api::OneListParams::new()
                            .with_limit(limit)
                            .with_page_token(page_token)
                            .with_all(all, max_pages);
                        ayx_one_api::one_api_list_request(
                            &config,
                            "platform",
                            "workspace-list",
                            "/v4/workspaces",
                            &[],
                            &params,
                        )?
                    }
                    OneWorkspaceCommand::ConfigurationV4 { workspace_id } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-configuration-v4",
                            "GET",
                            "/v4/workspaces/{id}/configuration",
                            false,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::CurrentConfiguration => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-current-configuration",
                            "GET",
                            "/v4/workspaces/current/configuration",
                            false,
                            &[],
                        )?
                    }
                    OneWorkspaceCommand::SaveCurrentConfiguration { profile, body } => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "workspace-save-current-configuration",
                            "PATCH",
                            "/v4/workspaces/current/configuration",
                            true,
                            &[],
                            Some(payload),
                        )?
                    }
                    OneWorkspaceCommand::SaveConfigurationV4 {
                        profile,
                        workspace_id,
                        body,
                    } => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "workspace-save-configuration-v4",
                            "PATCH",
                            "/v4/workspaces/{id}/configuration",
                            true,
                            &[("id", &workspace_id)],
                            Some(payload),
                        )?
                    }
                    OneWorkspaceCommand::Current => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-current",
                            "GET",
                            "/v4/workspaces/current",
                            false,
                            &[],
                        )?
                    }
                    OneWorkspaceCommand::ConfigurationSchema { workspace_id } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-configuration-schema",
                            "GET",
                            "/v4/workspaces/{id}/configuration-schema",
                            false,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::CurrentConfigurationSchema => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-current-configuration-schema",
                            "GET",
                            "/v4/workspaces/current/configuration-schema",
                            false,
                            &[],
                        )?
                    }
                    OneWorkspaceCommand::DeleteCurrentConfiguration { profile } => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-delete-current-configuration",
                            "POST",
                            "/v4/workspaces/current/delete-configuration",
                            true,
                            &[],
                        )?
                    }
                    OneWorkspaceCommand::DeleteConfiguration { workspace_id } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-delete-configuration",
                            "POST",
                            "/v4/workspaces/{id}/delete-configuration",
                            true,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::Configuration { workspace_id } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-configuration",
                            "GET",
                            "/iam/v1/workspaces/{id}/configuration",
                            false,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::People { workspace_id } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-people",
                            "GET",
                            "/iam/v1/workspaces/{id}/people",
                            false,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::Admins { workspace_id } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-admins",
                            "GET",
                            "/iam/v1/workspaces/{workspaceId}/admins",
                            false,
                            &[("workspaceId", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::InviteUsers { workspace_id } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-invite-users",
                            "POST",
                            "/iam/v1/workspaces/{id}/people/batch",
                            true,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::RemoveUser {
                        workspace_id,
                        person_id,
                    } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-remove-user",
                            "DELETE",
                            "/iam/v1/workspaces/{id}/people/{personId}",
                            true,
                            &[("id", &workspace_id), ("personId", &person_id)],
                        )?
                    }
                    OneWorkspaceCommand::SuspendUsers { workspace_id } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-suspend-users",
                            "POST",
                            "/iam/v1/workspaces/{id}/people/suspend",
                            true,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::UnsuspendUsers { workspace_id } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-unsuspend-users",
                            "POST",
                            "/iam/v1/workspaces/{id}/people/unsuspend",
                            true,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::Transfer { workspace_id } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "workspace-transfer",
                            "POST",
                            "/iam/v1/workspaces/{id}/transfer",
                            true,
                            &[("id", &workspace_id)],
                        )?
                    }
                    OneWorkspaceCommand::TransferAssets { profile, body } => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "workspace-transfer-assets",
                            "PATCH",
                            "/v4/workspaces/current/transfer",
                            true,
                            &[],
                            Some(payload),
                        )?
                    }
                },
                Some(OnePlatformCommand::Role { command }) => match command {
                    OneRoleCommand::ListAssignments { role_id } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "role-list-assignments",
                            "GET",
                            "/iam/v1/authorization/roles/{id}/people",
                            false,
                            &[("id", &role_id)],
                        )?
                    }
                    OneRoleCommand::Assign { role_id, subject_id } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "role-assign",
                            "POST",
                            "/iam/v1/authorization/roles/{id}/people/{subjectId}",
                            true,
                            &[("id", &role_id), ("subjectId", &subject_id)],
                        )?
                    }
                    OneRoleCommand::Unassign { role_id, subject_id } => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "role-unassign",
                            "DELETE",
                            "/iam/v1/authorization/roles/{id}/people/{subjectId}",
                            true,
                            &[("id", &role_id), ("subjectId", &subject_id)],
                        )?
                    }
                },
                Some(OnePlatformCommand::Auth { command }) => match command {
                    OnePlatformAuthCommand::Status { profile } => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        one_platform_auth_status_envelope(&config)?
                    }
                    OnePlatformAuthCommand::Diagnose { profile } => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        one_platform_auth_diagnose_envelope(&config)?
                    }
                },
                Some(OnePlatformCommand::User) => {
                    let config = load_profile!(None, environment)?;
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
                Some(OnePlatformCommand::Person { command }) => match command {
                    None => {
                        // Bare `ayx one platform person` runs an unpaginated list
                        // against the default config.yaml for back-compat.
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "person-list",
                            "GET",
                            "/v4/people",
                            false,
                            &[],
                        )?
                    }
                    Some(OnePlatformPersonCommand::List {
                        profile,
                        limit,
                        page_token,
                        all,
                        max_pages,
                    }) => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        let params = ayx_one_api::OneListParams::new()
                            .with_limit(limit)
                            .with_page_token(page_token)
                            .with_all(all, max_pages);
                        ayx_one_api::one_api_list_request(
                            &config,
                            "platform",
                            "person-list",
                            "/v4/people",
                            &[],
                            &params,
                        )?
                    }
                    Some(OnePlatformPersonCommand::Count) => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "person-count",
                            "GET",
                            "/v4/people/count",
                            false,
                            &[],
                        )?
                    }
                    Some(OnePlatformPersonCommand::Current) => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "person-current",
                            "GET",
                            "/v4/people/current",
                            false,
                            &[],
                        )?
                    }
                    Some(OnePlatformPersonCommand::Detail { profile, person_id }) => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "person-detail",
                            "GET",
                            "/v4/people/{id}",
                            false,
                            &[("id", &person_id)],
                        )?
                    }
                    Some(OnePlatformPersonCommand::Update {
                        profile,
                        person_id,
                        body,
                    }) => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "person-update",
                            "PUT",
                            "/v4/people/{id}",
                            true,
                            &[("id", &person_id)],
                            Some(payload),
                        )?
                    }
                    Some(OnePlatformPersonCommand::Patch {
                        profile,
                        person_id,
                        body,
                    }) => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "person-patch",
                            "PATCH",
                            "/v4/people/{id}",
                            true,
                            &[("id", &person_id)],
                            Some(payload),
                        )?
                    }
                    Some(OnePlatformPersonCommand::Delete { profile, person_id }) => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        if cli.apply {
                            cmd::confirm::require_tty_confirmation(
                                cli.yes,
                                &format!("About to DELETE person id='{person_id}' on profile '{}'. This cannot be undone.", config.profile_name),
                            )?;
                        }
                        one_api_live_request(
                            &config,
                            "platform",
                            "person-delete",
                            "DELETE",
                            "/v4/people/{id}",
                            true,
                            &[("id", &person_id)],
                        )?
                    }
                    Some(OnePlatformPersonCommand::Create { profile, body }) => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "person-create",
                            "POST",
                            "/v4/people",
                            true,
                            &[],
                            Some(payload),
                        )?
                    }
                    Some(OnePlatformPersonCommand::UpdatePassword { profile, body }) => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "person-update-password",
                            "PATCH",
                            "/v4/people/current/updatePassword",
                            true,
                            &[],
                            Some(payload),
                        )?
                    }
                    Some(OnePlatformPersonCommand::PasswordResetRequest { profile, body }) => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "person-password-reset-request",
                            "POST",
                            "/v4/passwordresetrequest",
                            true,
                            &[],
                            Some(payload),
                        )?
                    }
                },
                Some(OnePlatformCommand::Token { command }) => match command {
                    None | Some(OnePlatformTokenCommand::List) => {
                        let config = load_profile!(None, environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "api-access-tokens-list",
                            "GET",
                            "/v4/apiAccessTokens",
                            false,
                            &[],
                        )?
                    }
                    Some(OnePlatformTokenCommand::Create { profile, body }) => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        let payload = load_payload(&body)?;
                        one_api_live_request_with_body(
                            &config,
                            "platform",
                            "api-access-tokens-create",
                            "POST",
                            "/v4/apiAccessTokens",
                            true,
                            &[],
                            Some(payload),
                        )?
                    }
                    Some(OnePlatformTokenCommand::Detail { profile, token_id }) => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "api-access-tokens-detail",
                            "GET",
                            "/v4/apiAccessTokens/{tokenId}",
                            false,
                            &[("tokenId", &token_id)],
                        )?
                    }
                    Some(OnePlatformTokenCommand::Delete { profile, token_id }) => {
                        let config = load_profile!(profile.as_deref(), environment)?;
                        one_api_live_request(
                            &config,
                            "platform",
                            "api-access-tokens-delete",
                            "DELETE",
                            "/v4/apiAccessTokens/{tokenId}",
                            true,
                            &[("tokenId", &token_id)],
                        )?
                    }
                },
                None => Envelope::ok("one platform commands available: api, auth, status, inventory, workspace, role, user, token, person"),
            },
            Some(OneCommand::JobGroups { command }) => match command {
                None => Envelope::ok(
                    "one job-group commands available: list, count, pdf-results, run, publish, detail, cancel, status, inputs, outputs, jobs, publications, profile, profile-results",
                ),
                Some(OneJobGroupCommand::List {
                    profile,
                    limit,
                    page_token,
                    all,
                    max_pages,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let params = ayx_one_api::OneListParams::new()
                        .with_limit(limit)
                        .with_page_token(page_token)
                        .with_all(all, max_pages);
                    ayx_one_api::one_api_list_request(
                        &config,
                        "jobGroup",
                        "list",
                        "/v4/jobLibrary",
                        &[],
                        &params,
                    )?
                }
                Some(OneJobGroupCommand::Count { profile }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "count",
                        "GET",
                        "/v4/jobLibrary/count",
                        false,
                        &[],
                    )?
                }
                Some(OneJobGroupCommand::Run { profile, body }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "jobGroup",
                        "run",
                        "POST",
                        "/v4/jobGroups",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OneJobGroupCommand::Publish {
                    profile,
                    job_group_id,
                    body,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let job_group_id =
                        job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "jobGroup",
                        "publish",
                        "PUT",
                        "/v4/jobGroups/{id}/publish",
                        true,
                        &[("id", job_group_id.as_str())],
                        Some(payload),
                    )?
                }
                Some(OneJobGroupCommand::PdfResults { profile, job_group_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let job_group_id =
                        job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "pdf-results",
                        "GET",
                        "/v4/jobGroups/{id}/pdfResults",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Detail { profile, job_group_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "detail",
                        "GET",
                        "/v4/jobGroups/{id}",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Cancel { profile, job_group_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "cancel",
                        "POST",
                        "/v4/jobGroups/{id}/cancel",
                        true,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Status { profile, job_group_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "status",
                        "GET",
                        "/v4/jobGroups/{id}/status",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Inputs { profile, job_group_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "inputs",
                        "GET",
                        "/v4/jobGroups/{id}/inputs",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Outputs { profile, job_group_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "outputs",
                        "GET",
                        "/v4/jobGroups/{id}/outputs",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Jobs { profile, job_group_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "jobs",
                        "GET",
                        "/v4/jobGroups/{id}/jobs",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Publications { profile, job_group_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "publications",
                        "GET",
                        "/v4/jobGroups/{id}/publications",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::Profile { profile, job_group_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "profile",
                        "GET",
                        "/v4/jobGroups/{id}/profile",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
                Some(OneJobGroupCommand::ProfileResults { profile, job_group_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
                    one_api_live_request(
                        &config,
                        "jobGroup",
                        "profile-results",
                        "GET",
                        "/v4/jobGroups/{id}/profileResults",
                        false,
                        &[("id", job_group_id.as_str())],
                    )?
                }
            },
            Some(OneCommand::OutputObjects { command }) => match command {
                None => Envelope::ok(
                    "one output-object commands available: list, count, create, detail, update, delete, inputs, wrangle-to-python",
                ),
                Some(OneOutputObjectCommand::List {
                    profile,
                    limit,
                    page_token,
                    all,
                    max_pages,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let params = ayx_one_api::OneListParams::new()
                        .with_limit(limit)
                        .with_page_token(page_token)
                        .with_all(all, max_pages);
                    ayx_one_api::one_api_list_request(
                        &config,
                        "outputObject",
                        "list",
                        "/v4/outputObjects",
                        &[],
                        &params,
                    )?
                }
                Some(OneOutputObjectCommand::Count { profile }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    one_api_live_request(
                        &config,
                        "outputObject",
                        "count",
                        "GET",
                        "/v4/outputObjects/count",
                        false,
                        &[],
                    )?
                }
                Some(OneOutputObjectCommand::Create { profile, body }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "outputObject",
                        "create",
                        "POST",
                        "/v4/outputObjects",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OneOutputObjectCommand::Detail { profile, output_object_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let output_object_id = output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
                    one_api_live_request(
                        &config,
                        "outputObject",
                        "detail",
                        "GET",
                        "/v4/outputObjects/{id}",
                        false,
                        &[("id", output_object_id.as_str())],
                    )?
                }
                Some(OneOutputObjectCommand::Update {
                    profile,
                    output_object_id,
                    body,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let output_object_id = output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "outputObject",
                        "update",
                        "PATCH",
                        "/v4/outputObjects/{id}",
                        true,
                        &[("id", output_object_id.as_str())],
                        Some(payload),
                    )?
                }
                Some(OneOutputObjectCommand::Delete { profile, output_object_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let output_object_id = output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
                    one_api_live_request(
                        &config,
                        "outputObject",
                        "delete",
                        "DELETE",
                        "/v4/outputObjects/{id}",
                        true,
                        &[("id", output_object_id.as_str())],
                    )?
                }
                Some(OneOutputObjectCommand::Inputs { profile, output_object_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let output_object_id = output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
                    one_api_live_request(
                        &config,
                        "outputObject",
                        "inputs",
                        "GET",
                        "/v4/outputObjects/{id}/inputs",
                        false,
                        &[("id", output_object_id.as_str())],
                    )?
                }
                Some(OneOutputObjectCommand::WrangleToPython {
                    profile,
                    output_object_id,
                    body,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let output_object_id = output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
                    match body {
                        Some(body) => {
                            let payload = load_payload(&body)?;
                            one_api_live_request_with_body(
                                &config,
                                "outputObject",
                                "wrangle-to-python",
                                "POST",
                                "/v4/outputObjects/{id}/wrangleToPython",
                                true,
                                &[("id", output_object_id.as_str())],
                                Some(payload),
                            )?
                        }
                        None => one_api_live_request(
                            &config,
                            "outputObject",
                            "wrangle-to-python",
                            "POST",
                            "/v4/outputObjects/{id}/wrangleToPython",
                            false,
                            &[("id", output_object_id.as_str())],
                        )?,
                    }
                }
            },
            Some(OneCommand::WebhookFlowTasks { command }) => match command {
                None => Envelope::ok("one webhook-flow-task commands available: create, detail, delete, test"),
                Some(OneWebhookFlowTaskCommand::Create { profile, body }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "webhookFlowTask",
                        "create",
                        "POST",
                        "/v4/webhookFlowTasks",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OneWebhookFlowTaskCommand::Detail {
                    profile,
                    webhook_flow_task_id,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let webhook_flow_task_id =
                        webhook_flow_task_id.ok_or_else(|| anyhow!("--webhook-flow-task-id is required"))?;
                    one_api_live_request(
                        &config,
                        "webhookFlowTask",
                        "detail",
                        "GET",
                        "/v4/webhookFlowTasks/{id}",
                        false,
                        &[("id", webhook_flow_task_id.as_str())],
                    )?
                }
                Some(OneWebhookFlowTaskCommand::Delete {
                    profile,
                    webhook_flow_task_id,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let webhook_flow_task_id =
                        webhook_flow_task_id.ok_or_else(|| anyhow!("--webhook-flow-task-id is required"))?;
                    one_api_live_request(
                        &config,
                        "webhookFlowTask",
                        "delete",
                        "DELETE",
                        "/v4/webhookFlowTasks/{id}",
                        true,
                        &[("id", webhook_flow_task_id.as_str())],
                    )?
                }
                Some(OneWebhookFlowTaskCommand::Test { profile, body }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "webhookFlowTask",
                        "test",
                        "POST",
                        "/v4/webhooks/test",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
            },
            Some(OneCommand::WriteSettings { command }) => match command {
                None => Envelope::ok("one write-setting commands available: list, count, create, detail, update, delete"),
                Some(OneWriteSettingCommand::List {
                    profile,
                    limit,
                    page_token,
                    all,
                    max_pages,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let params = ayx_one_api::OneListParams::new()
                        .with_limit(limit)
                        .with_page_token(page_token)
                        .with_all(all, max_pages);
                    ayx_one_api::one_api_list_request(
                        &config,
                        "writeSetting",
                        "list",
                        "/v4/writeSettings",
                        &[],
                        &params,
                    )?
                }
                Some(OneWriteSettingCommand::Count { profile }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    one_api_live_request(
                        &config,
                        "writeSetting",
                        "count",
                        "GET",
                        "/v4/writeSettings/count",
                        false,
                        &[],
                    )?
                }
                Some(OneWriteSettingCommand::Create { profile, body }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "writeSetting",
                        "create",
                        "POST",
                        "/v4/writeSettings",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OneWriteSettingCommand::Detail {
                    profile,
                    write_setting_id,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let write_setting_id =
                        write_setting_id.ok_or_else(|| anyhow!("--write-setting-id is required"))?;
                    one_api_live_request(
                        &config,
                        "writeSetting",
                        "detail",
                        "GET",
                        "/v4/writeSettings/{id}",
                        false,
                        &[("id", write_setting_id.as_str())],
                    )?
                }
                Some(OneWriteSettingCommand::Update {
                    profile,
                    write_setting_id,
                    body,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let write_setting_id =
                        write_setting_id.ok_or_else(|| anyhow!("--write-setting-id is required"))?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "writeSetting",
                        "update",
                        "PATCH",
                        "/v4/writeSettings/{id}",
                        true,
                        &[("id", write_setting_id.as_str())],
                        Some(payload),
                    )?
                }
                Some(OneWriteSettingCommand::Delete {
                    profile,
                    write_setting_id,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let write_setting_id =
                        write_setting_id.ok_or_else(|| anyhow!("--write-setting-id is required"))?;
                    one_api_live_request(
                        &config,
                        "writeSetting",
                        "delete",
                        "DELETE",
                        "/v4/writeSettings/{id}",
                        true,
                        &[("id", write_setting_id.as_str())],
                    )?
                }
            },
            Some(OneCommand::Status { profile }) => {
                let config = load_profile!(profile.as_deref(), environment)?;
                api_status_envelope(&config, "one")?
            }
            Some(OneCommand::Inventory { profile }) => {
                let config = load_profile!(profile.as_deref(), environment)?;
                api_inventory_envelope(&config, "one")?
            }
            Some(OneCommand::Connections { command }) => {
                super::one_connections::execute(&runtime, command)?
            },
            Some(OneCommand::Flows { command }) => {
                super::one_flows::execute(&runtime, cli.apply, cli.yes, command)?
            },
            Some(OneCommand::Plans { command }) => {
                super::one_plans::execute(&runtime, cli.apply, cli.yes, command)?
            },
            Some(OneCommand::Scheduling { command }) => match command {
                None => Envelope::ok(
                    "one scheduling commands available: list, detail, enable, disable, count",
                ),
                Some(OneSchedulingCommand::List {
                    profile,
                    limit,
                    page_token,
                    all,
                    max_pages,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let params = ayx_one_api::OneListParams::new()
                        .with_limit(limit)
                        .with_page_token(page_token)
                        .with_all(all, max_pages);
                    ayx_one_api::one_api_list_request(
                        &config,
                        "scheduling",
                        "list",
                        "/scheduling/v1/schedules",
                        &[],
                        &params,
                    )?
                }
                Some(OneSchedulingCommand::Detail { profile, schedule_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let schedule_id = schedule_id.ok_or_else(|| anyhow!("--schedule-id is required"))?;
                    one_api_live_request(
                        &config,
                        "scheduling",
                        "detail",
                        "GET",
                        "/scheduling/v1/schedules/{id}",
                        false,
                        &[("id", schedule_id.as_str())],
                    )?
                }
                Some(OneSchedulingCommand::Enable { profile, schedule_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let schedule_id = schedule_id.ok_or_else(|| anyhow!("--schedule-id is required"))?;
                    one_api_live_request(
                        &config,
                        "scheduling",
                        "enable",
                        "POST",
                        "/scheduling/v1/schedules/{id}/enable",
                        true,
                        &[("id", schedule_id.as_str())],
                    )?
                }
                Some(OneSchedulingCommand::Disable { profile, schedule_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let schedule_id = schedule_id.ok_or_else(|| anyhow!("--schedule-id is required"))?;
                    one_api_live_request(
                        &config,
                        "scheduling",
                        "disable",
                        "POST",
                        "/scheduling/v1/schedules/{id}/disable",
                        true,
                        &[("id", schedule_id.as_str())],
                    )?
                }
                Some(OneSchedulingCommand::Count { profile }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    one_api_live_request(&config, "scheduling", "count", "GET", "/scheduling/v1/schedules/count", false, &[])?
                }
            },
            Some(OneCommand::Billing { command }) => match command {
                None => Envelope::ok("one billing commands available: current-account, usage-export"),
                Some(OneBillingCommand::CurrentAccount { profile }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    one_api_live_request(&config, "billing", "current-account", "GET", "/billing/v1/my/billing-accounts/current", false, &[])?
                }
                Some(OneBillingCommand::UsageExport { profile }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    one_api_live_request(&config, "billing", "usage-export", "GET", "/billing/v1/usage/export", false, &[])?
                }
            },
            Some(OneCommand::Ui { command }) => match command {
                None => Envelope::ok("one ui commands available: session, workflow, data, library, schedules, jobs (experimental)"),
                Some(UiCommand::Session { command }) => match command {
                    None => Envelope::ok("one ui session commands available: status, ensure, attach, inventory (experimental)"),
                    Some(UiSessionCommand::Status) => Envelope::ok_with_data(
                        "one ui session status scaffolded",
                        ui_command_envelope("session", "status", json!({
                            "browser": "managed by ayx-rs",
                            "mode": "experimental hybrid pinned visible tabs plus background read-only pages",
                        })),
                    ),
                    Some(UiSessionCommand::Ensure) => Envelope::ok_with_data(
                        "one ui session ensure scaffolded",
                        ui_command_envelope("session", "ensure", json!({ "result": "scaffolded" })),
                    ),
                    Some(UiSessionCommand::Attach { tab }) => Envelope::ok_with_data(
                        "one ui session attach scaffolded",
                        ui_command_envelope("session", "attach", json!({ "tab": tab })),
                    ),
                    Some(UiSessionCommand::Inventory) => Envelope::ok_with_data(
                        "one ui session inventory scaffolded",
                        ui_command_envelope("session", "inventory", json!({
                            "tabs": ["workflow", "data"],
                            "policy": "foreground tabs are reusable; read-only tasks may use background pages",
                        })),
                    ),
                },
                Some(UiCommand::Workflow { command }) => match command {
                    None => Envelope::ok("one ui workflow commands available: open, create, inventory, pane-config, pane-results, tool-list, tool-select, tool-inspect, graph-get, graph-put (experimental)"),
                    Some(UiWorkflowCommand::Open { workflow_id, foreground }) => Envelope::ok_with_data(
                        "one ui workflow open scaffolded",
                        ui_command_envelope("workflow", "open", json!({ "workflow_id": workflow_id, "foreground": foreground })),
                    ),
                    Some(UiWorkflowCommand::Create { name, foreground }) => Envelope::ok_with_data(
                        "one ui workflow create scaffolded",
                        ui_command_envelope("workflow", "create", json!({ "name": name, "foreground": foreground })),
                    ),
                    Some(UiWorkflowCommand::Inventory { workflow_id, foreground }) => Envelope::ok_with_data(
                        "one ui workflow inventory scaffolded",
                        ui_command_envelope("workflow", "inventory", json!({
                            "workflow_id": workflow_id,
                            "foreground": foreground,
                            "captures": ["canvas", "config-pane", "results-pane"],
                        })),
                    ),
                    Some(UiWorkflowCommand::PaneConfig { workflow_id, tool_id }) => Envelope::ok_with_data(
                        "one ui workflow pane-config scaffolded",
                        ui_command_envelope("workflow", "pane-config", json!({ "workflow_id": workflow_id, "tool_id": tool_id })),
                    ),
                    Some(UiWorkflowCommand::PaneResults { workflow_id, tool_id }) => Envelope::ok_with_data(
                        "one ui workflow pane-results scaffolded",
                        ui_command_envelope("workflow", "pane-results", json!({ "workflow_id": workflow_id, "tool_id": tool_id })),
                    ),
                    Some(UiWorkflowCommand::ToolList { workflow_id }) => Envelope::ok_with_data(
                        "one ui workflow tool-list scaffolded",
                        ui_command_envelope("workflow", "tool-list", json!({ "workflow_id": workflow_id })),
                    ),
                    Some(UiWorkflowCommand::ToolSelect { workflow_id, tool_id }) => Envelope::ok_with_data(
                        "one ui workflow tool-select scaffolded",
                        ui_command_envelope("workflow", "tool-select", json!({ "workflow_id": workflow_id, "tool_id": tool_id })),
                    ),
                    Some(UiWorkflowCommand::ToolInspect { workflow_id, tool_id }) => Envelope::ok_with_data(
                        "one ui workflow tool-inspect scaffolded",
                        ui_command_envelope("workflow", "tool-inspect", json!({ "workflow_id": workflow_id, "tool_id": tool_id })),
                    ),
                    Some(UiWorkflowCommand::GraphGet { workflow_id }) => Envelope::ok_with_data(
                        "one ui workflow graph-get scaffolded",
                        ui_command_envelope("workflow", "graph-get", json!({ "workflow_id": workflow_id })),
                    ),
                    Some(UiWorkflowCommand::GraphPut { workflow_id, input }) => Envelope::ok_with_data(
                        "one ui workflow graph-put scaffolded",
                        ui_command_envelope("workflow", "graph-put", json!({ "workflow_id": workflow_id, "input": input.display().to_string() })),
                    ),
                },
                Some(UiCommand::Data { command }) => match command {
                    None => Envelope::ok("one ui data commands available: list-datasets, dataset-detail, dataset-preview, upload, list-connections (experimental)"),
                    Some(UiDataCommand::ListDatasets { foreground }) => Envelope::ok_with_data(
                        "one ui data list-datasets scaffolded",
                        ui_command_envelope("data", "list-datasets", json!({
                            "foreground": foreground,
                            "tab_policy": "use pinned tab when warm; background page for read-only refresh is allowed",
                        })),
                    ),
                    Some(UiDataCommand::DatasetDetail { dataset_id, foreground }) => Envelope::ok_with_data(
                        "one ui data dataset-detail scaffolded",
                        ui_command_envelope("data", "dataset-detail", json!({ "dataset_id": dataset_id, "foreground": foreground })),
                    ),
                    Some(UiDataCommand::DatasetPreview { dataset_id, foreground }) => Envelope::ok_with_data(
                        "one ui data dataset-preview scaffolded",
                        ui_command_envelope("data", "dataset-preview", json!({ "dataset_id": dataset_id, "foreground": foreground })),
                    ),
                    Some(UiDataCommand::Upload { input, foreground }) => Envelope::ok_with_data(
                        "one ui data upload scaffolded",
                        ui_command_envelope("data", "upload", json!({ "input": input.display().to_string(), "foreground": foreground })),
                    ),
                    Some(UiDataCommand::ListConnections { foreground }) => Envelope::ok_with_data(
                        "one ui data list-connections scaffolded",
                        ui_command_envelope("data", "list-connections", json!({ "foreground": foreground })),
                    ),
                },
                Some(UiCommand::Library { command }) => match command {
                    None => Envelope::ok("one ui library commands available: inventory (experimental)"),
                    Some(UiLibraryCommand::Inventory) => Envelope::ok_with_data(
                        "one ui library inventory scaffolded",
                        ui_command_envelope("library", "inventory", json!({})),
                    ),
                },
                Some(UiCommand::Schedules { command }) => match command {
                    None => Envelope::ok("one ui schedules commands available: inventory (experimental)"),
                    Some(UiSchedulesCommand::Inventory) => Envelope::ok_with_data(
                        "one ui schedules inventory scaffolded",
                        ui_command_envelope("schedules", "inventory", json!({})),
                    ),
                },
                Some(UiCommand::Jobs { command }) => match command {
                    None => Envelope::ok("one ui jobs commands available: inventory (experimental)"),
                    Some(UiJobsCommand::Inventory) => Envelope::ok_with_data(
                        "one ui jobs inventory scaffolded",
                        ui_command_envelope("jobs", "inventory", json!({})),
                    ),
                },
            },
            Some(OneCommand::AutoInsights { profile }) => {
                let config = load_profile!(profile.as_deref(), environment)?;
                api_diagnose_envelope(&config, "one auto-insights")?
            }
            Some(OneCommand::DesktopExec { profile }) => {
                let config = load_profile!(profile.as_deref(), environment)?;
                api_status_envelope(&config, "one desktop-exec")?
            }
    })
}
