use anyhow::{Result, anyhow};
use ayx_core::envelope::Envelope;
use ayx_core::profile::profile_storage_path;
use ayx_one_api::{
    one_api_live_request, one_api_live_request_with_body, one_api_live_request_with_query,
};
use serde_json::Value;
use serde_json::json;

use crate::{
    OneWorkspaceCommand,
    cmd::{self, RuntimeCtx},
    load_payload,
    onboard::{InlineSecretPolicy, inline_secret_warning, write_config_with_policy},
};

/// Resolve and validate the numeric workspace id required by path-scoped
/// `/v4/workspaces` operations. The profile's active workspace is a ULID/GID
/// for header scope, so it cannot be substituted into these numeric path
/// segments. Always discover the current workspace from the live read, then
/// reject an explicit path id that does not match both the current numeric id
/// and the profile's current workspace GID.
fn resolve_workspace_path_id(
    explicit: Option<String>,
    config: &ayx_core::profile::Config,
) -> Result<String> {
    let envelope = one_api_live_request(
        config,
        "workspace",
        "workspace-current-for-path-id",
        "GET",
        "/v4/workspaces/current",
        false,
        &[],
    )?;
    if !envelope.ok {
        return Err(anyhow!(
            "could not resolve the numeric workspace id from /v4/workspaces/current"
        ));
    }

    let response = envelope
        .data
        .get("response")
        .filter(|value| value.is_object())
        .unwrap_or(&envelope.data);
    validate_workspace_gid(
        config
            .alteryx_one
            .as_ref()
            .and_then(|one| one.resolved_workspace_gid()),
        response.get("gid").and_then(Value::as_str),
    )?;

    let current_id = response.get("id").and_then(|candidate| {
        candidate
            .as_i64()
            .map(|id| id.to_string())
            .or_else(|| candidate.as_u64().map(|id| id.to_string()))
            .or_else(|| {
                candidate.as_str().and_then(|id| {
                    id.chars()
                        .all(|ch| ch.is_ascii_digit())
                        .then(|| id.to_string())
                })
            })
    });
    let current_id = current_id.ok_or_else(|| {
        let response_preview = if response.is_object() {
            "current workspace response did not include a numeric id"
        } else {
            "current workspace response was not an object"
        };
        anyhow!(response_preview)
    })?;

    validate_workspace_path_id(explicit.as_deref(), current_id)
}

fn validate_workspace_path_id(explicit: Option<&str>, current_id: String) -> Result<String> {
    if let Some(explicit_id) = explicit
        && explicit_id != current_id
    {
        return Err(anyhow!(
            "--workspace-id '{}' does not match the current numeric workspace id '{}'; refusing to target a different workspace",
            explicit_id,
            current_id
        ));
    }
    Ok(current_id)
}

fn validate_workspace_gid(expected: Option<&str>, actual: Option<&str>) -> Result<()> {
    let expected = expected.ok_or_else(|| {
        anyhow!(
            "workspace preflight could not determine the profile workspace GID; refusing to target a workspace path"
        )
    })?;
    let actual = actual.ok_or_else(|| {
        anyhow!(
            "workspace preflight response did not include a workspace GID; refusing to target a workspace path"
        )
    })?;
    if expected != actual {
        return Err(anyhow!(
            "workspace preflight mismatch: profile workspace GID '{}' is not the current workspace GID '{}'; refusing to target a workspace path",
            expected,
            actual
        ));
    }
    Ok(())
}

fn confirm_workspace_mutation(
    apply: bool,
    yes: bool,
    action: &str,
    subject: &str,
    profile: &str,
) -> Result<()> {
    if apply {
        cmd::confirm::require_tty_confirmation(
            yes,
            &cmd::confirm::access_change_message(action, subject, profile),
        )?;
    }
    Ok(())
}

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: OneWorkspaceCommand,
) -> Result<Envelope> {
    Ok(match command {
        OneWorkspaceCommand::List {
            profile,
            limit,
            page_token,
            all,
            max_pages,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let params = ayx_one_api::OneListParams::new()
                .with_limit(limit)
                .with_page_token(page_token)
                .with_all(all, max_pages);
            ayx_one_api::one_api_list_request(
                &config,
                "workspace",
                "workspace-list",
                "/v4/workspaces",
                &[],
                &params,
            )?
        }
        OneWorkspaceCommand::Create { profile, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            confirm_workspace_mutation(apply, yes, "create", "a workspace", &config.profile_name)?;
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-create",
                "POST",
                "/v4/workspaces",
                true,
                &[],
                Some(payload),
            )?
        }
        OneWorkspaceCommand::Delete { id } => {
            let config = runtime.load_profile_lenient(None)?;
            let path_id = resolve_workspace_path_id(Some(id), &config)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "delete",
                        &format!("workspace id='{path_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "workspace",
                "workspace-delete",
                "DELETE",
                "/v4/workspaces/{id}",
                true,
                &[("id", &path_id)],
            )?
        }
        OneWorkspaceCommand::ConfigurationV4 { id } => {
            let config = runtime.load_profile_lenient(None)?;
            let path_id = resolve_workspace_path_id(Some(id), &config)?;
            one_api_live_request(
                &config,
                "workspace",
                "workspace-configuration-v4",
                "GET",
                "/v4/workspaces/{id}/configuration",
                false,
                &[("id", &path_id)],
            )?
        }
        OneWorkspaceCommand::CurrentConfiguration => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "workspace",
                "workspace-current-configuration",
                "GET",
                "/v4/workspaces/current/configuration",
                false,
                &[],
            )?
        }
        OneWorkspaceCommand::SaveCurrentConfiguration { profile, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            confirm_workspace_mutation(
                apply,
                yes,
                "update",
                "the current workspace configuration",
                &config.profile_name,
            )?;
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-save-current-configuration",
                "PATCH",
                "/v4/workspaces/current/configuration",
                true,
                &[],
                Some(payload),
            )?
        }
        OneWorkspaceCommand::SaveConfigurationV4 { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let path_id = resolve_workspace_path_id(Some(id), &config)?;
            let payload = load_payload(&body)?;
            confirm_workspace_mutation(
                apply,
                yes,
                "update",
                &format!("workspace configuration id='{path_id}'"),
                &config.profile_name,
            )?;
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-save-configuration-v4",
                "PATCH",
                "/v4/workspaces/{id}/configuration",
                true,
                &[("id", &path_id)],
                Some(payload),
            )?
        }
        OneWorkspaceCommand::Current => {
            if ayx_one_api::debug_trace() {
                eprintln!("[one-debug] workspace current: loading profile");
            }
            let config = runtime.load_profile_lenient(None)?;
            if ayx_one_api::debug_trace() {
                eprintln!("[one-debug] workspace current: calling live request");
            }
            one_api_live_request(
                &config,
                "workspace",
                "workspace-current",
                "GET",
                "/v4/workspaces/current",
                false,
                &[],
            )?
        }
        OneWorkspaceCommand::ConfigurationSchema { id } => {
            let config = runtime.load_profile_lenient(None)?;
            let path_id = resolve_workspace_path_id(Some(id), &config)?;
            one_api_live_request(
                &config,
                "workspace",
                "workspace-configuration-schema",
                "GET",
                "/v4/workspaces/{id}/configuration-schema",
                false,
                &[("id", &path_id)],
            )?
        }
        OneWorkspaceCommand::CurrentConfigurationSchema => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "workspace",
                "workspace-current-configuration-schema",
                "GET",
                "/v4/workspaces/current/configuration-schema",
                false,
                &[],
            )?
        }
        OneWorkspaceCommand::DeleteCurrentConfiguration { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            confirm_workspace_mutation(
                apply,
                yes,
                "delete",
                "the current workspace configuration",
                &config.profile_name,
            )?;
            one_api_live_request(
                &config,
                "workspace",
                "workspace-delete-current-configuration",
                "POST",
                "/v4/workspaces/current/delete-configuration",
                true,
                &[],
            )?
        }
        OneWorkspaceCommand::DeleteConfiguration { id } => {
            let config = runtime.load_profile_lenient(None)?;
            let path_id = resolve_workspace_path_id(Some(id), &config)?;
            confirm_workspace_mutation(
                apply,
                yes,
                "delete",
                &format!("workspace configuration id='{path_id}'"),
                &config.profile_name,
            )?;
            one_api_live_request(
                &config,
                "workspace",
                "workspace-delete-configuration",
                "POST",
                "/v4/workspaces/{id}/delete-configuration",
                true,
                &[("id", &path_id)],
            )?
        }
        OneWorkspaceCommand::Configuration { id } => {
            let config = runtime.load_profile_lenient(None)?;
            let path_id = resolve_workspace_path_id(Some(id), &config)?;
            one_api_live_request(
                &config,
                "workspace",
                "workspace-configuration",
                "GET",
                "/v4/workspaces/{id}/configuration",
                false,
                &[("id", &path_id)],
            )?
        }
        OneWorkspaceCommand::People => {
            let config = runtime.load_profile_lenient(None)?;
            // The workspace context is conveyed via the x-alteryx-workspace-gid
            // header (set by the transport layer); /v4/people is the correct
            // live-verified endpoint. /v4/workspaces/{id}/people returns 404.
            one_api_live_request(
                &config,
                "workspace",
                "workspace-people",
                "GET",
                "/v4/people",
                false,
                &[],
            )?
        }
        OneWorkspaceCommand::Admins => {
            let config = runtime.load_profile_lenient(None)?;
            // Same: workspace context via header; filter admins with role query
            // param. /v4/workspaces/{id}/admins returns 404.
            one_api_live_request(
                &config,
                "workspace",
                "workspace-admins",
                "GET",
                "/v4/people?role=admin",
                false,
                &[],
            )?
        }
        OneWorkspaceCommand::Groups { workspace_id } => {
            let config = runtime.load_profile_lenient(None)?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            one_api_live_request(
                &config,
                "workspace",
                "workspace-groups",
                "GET",
                "/v4/workspaces/{id}/groups",
                false,
                &[("id", &path_id)],
            )?
        }
        OneWorkspaceCommand::GroupsGlobal => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "workspace",
                "workspace-groups-global",
                "GET",
                "/v4/groups",
                false,
                &[],
            )?
        }
        OneWorkspaceCommand::CreateGroup {
            profile,
            workspace_id,
            body,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            let payload = load_payload(&body)?;
            confirm_workspace_mutation(
                apply,
                yes,
                "create",
                &format!("a group in workspace id='{path_id}'"),
                &config.profile_name,
            )?;
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-group-create",
                "POST",
                "/v4/workspaces/{id}/groups",
                true,
                &[("id", &path_id)],
                Some(payload),
            )?
        }
        OneWorkspaceCommand::DeleteGroup {
            workspace_id,
            group_id,
        } => {
            let config = runtime.load_profile_lenient(None)?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "delete",
                        &format!("group id='{group_id}' from workspace id='{path_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "workspace",
                "workspace-group-delete",
                "DELETE",
                "/v4/workspaces/{id}/groups/{groupId}",
                true,
                &[("id", &path_id), ("groupId", &group_id)],
            )?
        }
        OneWorkspaceCommand::UpdateGroup {
            profile,
            workspace_id,
            group_id,
            body,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            let payload = load_payload(&body)?;
            confirm_workspace_mutation(
                apply,
                yes,
                "update",
                &format!("group id='{group_id}' in workspace id='{path_id}'"),
                &config.profile_name,
            )?;
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-group-update",
                "PUT",
                "/v4/workspaces/{id}/groups/{groupId}",
                true,
                &[("id", &path_id), ("groupId", &group_id)],
                Some(payload),
            )?
        }
        OneWorkspaceCommand::SetGroupRoles {
            profile,
            workspace_id,
            group_id,
            body,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            let payload = load_payload(&body)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "update",
                        &format!("roles for group id='{group_id}' in workspace id='{path_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-group-set-roles",
                "PUT",
                "/v4/workspaces/{id}/groups/{groupId}/roles",
                true,
                &[("id", &path_id), ("groupId", &group_id)],
                Some(payload),
            )?
        }
        OneWorkspaceCommand::AddGroupUsers {
            workspace_id,
            group_id,
            user_ids,
        } => {
            let config = runtime.load_profile_lenient(None)?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "add",
                        &format!("users to group id='{group_id}' in workspace id='{path_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            let query_params: Vec<(&str, &str)> = user_ids
                .iter()
                .map(|user_id| ("userIds", user_id.as_str()))
                .collect();
            one_api_live_request_with_query(
                &config,
                "workspace",
                "workspace-group-add-users",
                "POST",
                "/v4/workspaces/{id}/groups/{groupId}/users",
                true,
                &[("id", &path_id), ("groupId", &group_id)],
                &query_params,
            )?
        }
        OneWorkspaceCommand::RemoveGroupUsers {
            workspace_id,
            group_id,
            user_ids,
        } => {
            let config = runtime.load_profile_lenient(None)?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "remove",
                        &format!("users from group id='{group_id}' in workspace id='{path_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            let query_params: Vec<(&str, &str)> = user_ids
                .iter()
                .map(|user_id| ("userIds", user_id.as_str()))
                .collect();
            one_api_live_request_with_query(
                &config,
                "workspace",
                "workspace-group-remove-users",
                "DELETE",
                "/v4/workspaces/{id}/groups/{groupId}/users",
                true,
                &[("id", &path_id), ("groupId", &group_id)],
                &query_params,
            )?
        }
        OneWorkspaceCommand::InvitationLink {
            workspace_id,
            person_id,
        } => {
            let config = runtime.load_profile_lenient(None)?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            one_api_live_request(
                &config,
                "workspace",
                "workspace-invitation-link",
                "GET",
                "/v4/workspaces/{id}/invitationLink?personId={personId}",
                false,
                &[("id", &path_id), ("personId", &person_id)],
            )?
        }
        OneWorkspaceCommand::CloudConfigs { workspace_id } => {
            let config = runtime.load_profile_lenient(None)?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            one_api_live_request(
                &config,
                "workspace",
                "workspace-cloud-configs",
                "GET",
                "/v4/workspaces/{workspaceId}/cloudConfigs",
                false,
                &[("workspaceId", &path_id)],
            )?
        }
        OneWorkspaceCommand::Switch { profile, id } => {
            let mut config = runtime.load_profile_lenient(profile.as_deref())?;
            let one = config
                .alteryx_one
                .as_mut()
                .ok_or_else(|| anyhow!("no alteryx_one section in profile"))?;
            let available: Vec<String> = one.workspace_credentials.keys().cloned().collect();
            if !one.workspace_credentials.contains_key(&id) {
                let profile_name = config.profile_name.clone();
                return Err(anyhow!(
                    "no stored credential for workspace '{}' in profile '{}'. \
                     Authenticate into it first with `ayx one login` \
                     (the active workspace is determined by which workspace you logged into — \
                     the token is workspace-bound). \
                     Available: {}. \
                     Run `ayx one workspace list` to see workspaces.",
                    id,
                    profile_name,
                    if available.is_empty() {
                        "(none)".to_string()
                    } else {
                        available.join(", ")
                    }
                ));
            }
            one.expected_workspace_id = Some(id.clone());
            let profile_name = config.profile_name.clone();
            let available_after: Vec<String> = config
                .alteryx_one
                .as_ref()
                .map(|o| o.workspace_credentials.keys().cloned().collect())
                .unwrap_or_default();
            let path = profile_storage_path(&profile_name)
                .map_err(|e| anyhow!("could not resolve profile path: {e}"))?;
            let secretize = write_config_with_policy(&path, &config, InlineSecretPolicy::Allow)
                .map_err(|e| anyhow!("failed to save profile: {e}"))?;
            if let Some(msg) = inline_secret_warning(&secretize.inline_fields) {
                eprintln!("warning: {msg}");
            }
            Envelope::ok_with_data(
                format!("active workspace set to '{id}'"),
                json!({
                    "active_workspace_id": id,
                    "available_workspace_ids": available_after,
                    "profile": profile_name,
                }),
            )
        }
        OneWorkspaceCommand::InviteUsers { workspace_id } => {
            let config = runtime.load_profile_lenient(None)?;
            let ws_id = resolve_workspace_path_id(workspace_id, &config)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "invite",
                        &format!("users to workspace id='{ws_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "workspace",
                "workspace-invite-users",
                "POST",
                "/v4/workspaces/{id}/people/batch",
                true,
                &[("id", &ws_id)],
            )?
        }
        OneWorkspaceCommand::Invite {
            profile,
            workspace_id,
            body,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            let payload = load_payload(&body)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "invite",
                        &format!("user(s) to workspace id='{path_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-invite",
                "POST",
                "/v4/workspaces/{id}/people",
                true,
                &[("id", &path_id)],
                Some(payload),
            )?
        }
        OneWorkspaceCommand::InviteList {
            profile,
            workspace_id,
            body,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            let payload = load_payload(&body)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "invite",
                        &format!("user list to workspace id='{path_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-invite-list",
                "POST",
                "/v4/workspaces/{id}/people/batch",
                true,
                &[("id", &path_id)],
                Some(payload),
            )?
        }
        OneWorkspaceCommand::ReinviteUsers {
            profile,
            workspace_id,
            body,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            let payload = load_payload(&body)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "reinvite",
                        &format!("users in workspace id='{path_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-reinvite-users",
                "PATCH",
                "/v4/workspaces/{id}/people/batch",
                true,
                &[("id", &path_id)],
                Some(payload),
            )?
        }
        OneWorkspaceCommand::RemoveUser { workspace_id, id } => {
            let config = runtime.load_profile_lenient(None)?;
            let ws_id = resolve_workspace_path_id(workspace_id, &config)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "remove",
                        &format!("user person id='{id}' from workspace id='{ws_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "workspace",
                "workspace-remove-user",
                "DELETE",
                "/v4/workspaces/{workspaceId}/people/{id}",
                true,
                &[("workspaceId", &ws_id), ("id", &id)],
            )?
        }
        OneWorkspaceCommand::SuspendUsers { workspace_id } => {
            let config = runtime.load_profile_lenient(None)?;
            let ws_id = resolve_workspace_path_id(workspace_id, &config)?;
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
                "workspace",
                "workspace-suspend-users",
                "POST",
                "/v4/workspaces/{id}/people/suspend",
                true,
                &[("id", &ws_id)],
            )?
        }
        OneWorkspaceCommand::UnsuspendUsers { workspace_id } => {
            let config = runtime.load_profile_lenient(None)?;
            let ws_id = resolve_workspace_path_id(workspace_id, &config)?;
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
                "workspace",
                "workspace-unsuspend-users",
                "POST",
                "/v4/workspaces/{id}/people/unsuspend",
                true,
                &[("id", &ws_id)],
            )?
        }
        OneWorkspaceCommand::SuspendUser {
            workspace_id,
            person_id,
        } => {
            let config = runtime.load_profile_lenient(None)?;
            let ws_id = resolve_workspace_path_id(workspace_id, &config)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "suspend",
                        &format!("user person id='{person_id}' in workspace id='{ws_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "workspace",
                "workspace-suspend-user",
                "PUT",
                "/v4/workspaces/{id}/people/{personId}/suspended",
                true,
                &[("id", &ws_id), ("personId", &person_id)],
            )?
        }
        OneWorkspaceCommand::Transfer { workspace_id } => {
            let config = runtime.load_profile_lenient(None)?;
            let ws_id = resolve_workspace_path_id(workspace_id, &config)?;
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
                "workspace",
                "workspace-transfer",
                "PATCH",
                "/v4/workspaces/{id}/transfer",
                true,
                &[("id", &ws_id)],
            )?
        }
        OneWorkspaceCommand::TransferAssets { profile, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            confirm_workspace_mutation(
                apply,
                yes,
                "transfer",
                "workspace assets",
                &config.profile_name,
            )?;
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-transfer-assets",
                "PATCH",
                "/v4/workspaces/current/transfer",
                true,
                &[],
                Some(payload),
            )?
        }
        OneWorkspaceCommand::CreateCloudConfig {
            profile,
            workspace_id,
            cloud_provider,
            body,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            let payload = load_payload(&body)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "create",
                        &format!("cloud config for workspace id='{path_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-cloud-config-create",
                "POST",
                "/v4/workspaces/{workspaceId}/cloudConfigs/{cloudProvider}",
                true,
                &[
                    ("workspaceId", &path_id),
                    ("cloudProvider", &cloud_provider),
                ],
                Some(payload),
            )?
        }
        OneWorkspaceCommand::UpdateCloudConfig {
            profile,
            workspace_id,
            cloud_provider,
            body,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            let payload = load_payload(&body)?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "update",
                        &format!("cloud config for workspace id='{path_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-cloud-config-update",
                "PATCH",
                "/v4/workspaces/{workspaceId}/cloudConfigs/{cloudProvider}",
                true,
                &[
                    ("workspaceId", &path_id),
                    ("cloudProvider", &cloud_provider),
                ],
                Some(payload),
            )?
        }
        OneWorkspaceCommand::PatchUser {
            profile,
            workspace_id,
            person_id,
            body,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            let payload = load_payload(&body)?;
            confirm_workspace_mutation(
                apply,
                yes,
                "update",
                &format!("user person id='{person_id}' in workspace id='{path_id}'"),
                &config.profile_name,
            )?;
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-user-patch",
                "PATCH",
                "/v4/workspaces/{workspaceId}/people/{id}",
                true,
                &[("workspaceId", &path_id), ("id", &person_id)],
                Some(payload),
            )?
        }
        OneWorkspaceCommand::UpdateUser {
            profile,
            workspace_id,
            person_id,
            body,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let path_id = resolve_workspace_path_id(Some(workspace_id), &config)?;
            let payload = load_payload(&body)?;
            confirm_workspace_mutation(
                apply,
                yes,
                "update",
                &format!("user person id='{person_id}' in workspace id='{path_id}'"),
                &config.profile_name,
            )?;
            one_api_live_request_with_body(
                &config,
                "workspace",
                "workspace-user-update",
                "PUT",
                "/v4/workspaces/{workspaceId}/people/{id}",
                true,
                &[("workspaceId", &path_id), ("id", &person_id)],
                Some(payload),
            )?
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{confirm_workspace_mutation, validate_workspace_gid, validate_workspace_path_id};

    #[test]
    fn workspace_mutation_confirmation_is_skipped_for_dry_run() {
        confirm_workspace_mutation(false, false, "update", "a workspace", "test")
            .expect("dry-run should not prompt");
    }

    #[test]
    fn workspace_mutation_confirmation_accepts_yes() {
        confirm_workspace_mutation(true, true, "update", "a workspace", "test")
            .expect("--yes should bypass the prompt");
    }

    #[test]
    fn explicit_workspace_path_id_must_match_current_numeric_id() {
        let error = validate_workspace_path_id(Some("90002"), "90001".to_string())
            .expect_err("a different workspace path must be rejected");
        assert!(error.to_string().contains("90001"));
        assert_eq!(
            validate_workspace_path_id(Some("90001"), "90001".to_string()).unwrap(),
            "90001"
        );
    }

    #[test]
    fn workspace_gid_validation_fails_closed() {
        assert!(validate_workspace_gid(Some("gid-1"), Some("gid-2")).is_err());
        assert!(validate_workspace_gid(None, Some("gid-1")).is_err());
        assert!(validate_workspace_gid(Some("gid-1"), None).is_err());
        assert!(validate_workspace_gid(Some("gid-1"), Some("gid-1")).is_ok());
    }
}
