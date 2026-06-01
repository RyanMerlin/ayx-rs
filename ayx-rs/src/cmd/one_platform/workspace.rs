use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{cmd::RuntimeCtx, load_payload, OneWorkspaceCommand};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: Option<OneWorkspaceCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok(
            "one platform workspace commands available: list, current, current-configuration, configuration-v4, save-current-configuration, save-configuration-v4, configuration, configuration-schema, current-configuration-schema, delete-current-configuration, delete-configuration, people, admins, invite-users, remove-user, suspend-users, unsuspend-users, transfer, transfer-assets",
        ),
        Some(OneWorkspaceCommand::List {
            profile,
            limit,
            page_token,
            all,
            max_pages,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
        Some(OneWorkspaceCommand::ConfigurationV4 { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::CurrentConfiguration) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::SaveCurrentConfiguration { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
        Some(OneWorkspaceCommand::SaveConfigurationV4 {
            profile,
            workspace_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
        Some(OneWorkspaceCommand::Current) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::ConfigurationSchema { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::CurrentConfigurationSchema) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::DeleteCurrentConfiguration { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
        Some(OneWorkspaceCommand::DeleteConfiguration { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::Configuration { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::People { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::Admins { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::InviteUsers { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::RemoveUser {
            workspace_id,
            person_id,
        }) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::SuspendUsers { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::UnsuspendUsers { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::Transfer { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
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
        Some(OneWorkspaceCommand::TransferAssets { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
    })
}
