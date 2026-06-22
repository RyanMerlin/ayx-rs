use anyhow::{Result, anyhow};
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{
    OneWorkspaceCommand,
    cmd::{self, RuntimeCtx},
    load_payload,
};

/// Resolve a workspace id from the explicit arg or fall back to the profile's
/// configured `workspace_gid`. Returns an error if neither is available.
fn resolve_workspace_id(
    explicit: Option<String>,
    config: &ayx_core::profile::Config,
) -> Result<String> {
    explicit
        .or_else(|| {
            config
                .alteryx_one
                .as_ref()
                .and_then(|o| o.resolved_workspace_gid())
                .map(str::to_string)
        })
        .ok_or_else(|| {
            anyhow!(
                "workspace-id not specified and could not be inferred from profile; \
                 pass --workspace-id explicitly"
            )
        })
}

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
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
            if ayx_one_api::debug_trace() {
                eprintln!("[one-debug] workspace current: loading profile");
            }
            let config = runtime.load_profile_lenient(None)?;
            if ayx_one_api::debug_trace() {
                eprintln!("[one-debug] workspace current: calling live request");
            }
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
                "/v4/workspaces/{id}/configuration",
                false,
                &[("id", &workspace_id)],
            )?
        }
        Some(OneWorkspaceCommand::People { workspace_id: _ }) => {
            let config = runtime.load_profile_lenient(None)?;
            // The workspace context is conveyed via the x-alteryx-workspace-gid
            // header (set by the transport layer); /v4/people is the correct
            // live-verified endpoint. /v4/workspaces/{id}/people returns 404.
            one_api_live_request(
                &config,
                "platform",
                "workspace-people",
                "GET",
                "/v4/people",
                false,
                &[],
            )?
        }
        Some(OneWorkspaceCommand::Admins { workspace_id: _ }) => {
            let config = runtime.load_profile_lenient(None)?;
            // Same: workspace context via header; filter admins with role query
            // param. /v4/workspaces/{id}/admins returns 404.
            one_api_live_request(
                &config,
                "platform",
                "workspace-admins",
                "GET",
                "/v4/people?role=admin",
                false,
                &[],
            )?
        }
        Some(OneWorkspaceCommand::InviteUsers { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
            let ws_id = resolve_workspace_id(workspace_id, &config)?;
            one_api_live_request(
                &config,
                "platform",
                "workspace-invite-users",
                "POST",
                "/v4/workspaces/{id}/people/batch",
                true,
                &[("id", &ws_id)],
            )?
        }
        Some(OneWorkspaceCommand::RemoveUser {
            workspace_id,
            person_id,
        }) => {
            let config = runtime.load_profile_lenient(None)?;
            let ws_id = resolve_workspace_id(workspace_id, &config)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "remove",
                        &format!("user person id='{person_id}' from workspace id='{ws_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "platform",
                "workspace-remove-user",
                "DELETE",
                "/v4/workspaces/{workspaceId}/people/{id}",
                true,
                &[("workspaceId", &ws_id), ("id", &person_id)],
            )?
        }
        Some(OneWorkspaceCommand::SuspendUsers { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
            let ws_id = resolve_workspace_id(workspace_id, &config)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "suspend",
                        &format!("users in workspace id='{ws_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "platform",
                "workspace-suspend-users",
                "POST",
                "/iam/v1/workspaces/{id}/people/suspend",
                true,
                &[("id", &ws_id)],
            )?
        }
        Some(OneWorkspaceCommand::UnsuspendUsers { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
            let ws_id = resolve_workspace_id(workspace_id, &config)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "unsuspend",
                        &format!("users in workspace id='{ws_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "platform",
                "workspace-unsuspend-users",
                "POST",
                "/iam/v1/workspaces/{id}/people/unsuspend",
                true,
                &[("id", &ws_id)],
            )?
        }
        Some(OneWorkspaceCommand::Transfer { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
            let ws_id = resolve_workspace_id(workspace_id, &config)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "transfer",
                        &format!("workspace id='{ws_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "platform",
                "workspace-transfer",
                "POST",
                "/v4/workspaces/{id}/transfer",
                true,
                &[("id", &ws_id)],
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
