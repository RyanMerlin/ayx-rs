use anyhow::{Result, anyhow};
use ayx_core::envelope::Envelope;
use ayx_core::profile::profile_storage_path;
use ayx_one_api::{
    one_api_live_request, one_api_live_request_with_body, one_api_live_request_with_query,
};
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

    let response = envelope.data.get("response").unwrap_or(&envelope.data);
    let identity = ayx_one_api::parse_current_workspace_identity(response)
        .map_err(|err| anyhow!("workspace path preflight failed: {err}"))?;
    validate_workspace_gid(
        config
            .alteryx_one
            .as_ref()
            .and_then(|one| one.resolved_workspace_gid()),
        Some(identity.workspace_gid.as_str()),
    )?;
    validate_workspace_path_id(explicit.as_deref(), identity.workspace_id)
}

/// Is this person record flagged as a workspace admin?
///
/// The live `/v4/people` payload spells the flag `isAdmin`; `is_admin` is
/// accepted for the snake_case normalization used elsewhere in the workspace.
/// A record with no flag at all is **not** an admin — the filter fails closed
/// so an unknown payload never widens the admin list.
fn person_is_admin(person: &serde_json::Value) -> bool {
    for key in ["isAdmin", "is_admin"] {
        match person.get(key) {
            Some(serde_json::Value::Bool(flag)) => return *flag,
            Some(serde_json::Value::String(flag)) => return flag.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    false
}

/// Retain only admin records in a people collection, in place.
///
/// Accepts either a bare array or the list-object shapes the One gateway and
/// the CLI's pagination aggregator produce. Returns `Some((before, after))`
/// when a collection was actually found and filtered.
fn retain_admins(value: &mut serde_json::Value) -> Option<(usize, usize)> {
    match value {
        serde_json::Value::Array(items) => {
            let before = items.len();
            items.retain(person_is_admin);
            Some((before, items.len()))
        }
        serde_json::Value::Object(map) => {
            for key in ["items", "results", "data", "records", "value"] {
                if let Some(serde_json::Value::Array(items)) = map.get_mut(key) {
                    let before = items.len();
                    items.retain(person_is_admin);
                    let after = items.len();
                    // A payload-reported count would otherwise still describe
                    // the unfiltered list and contradict the items beside it.
                    if map.contains_key("total") {
                        map.insert("total".to_string(), serde_json::Value::from(after as u64));
                    }
                    return Some((before, after));
                }
            }
            None
        }
        _ => None,
    }
}

/// Apply the client-side admin filter to a `workspace admins` envelope.
///
/// `GET /v4/people?role=admin` returns HTTP 200 with the *complete* people
/// list — the gateway ignores `role=admin` — so without this the command was
/// an alias for `workspace people` (docs/ayx-cli-testing-issues.md Issue 1).
/// Per-person fields are left exactly as the API returned them; only
/// non-admins are dropped. Both `data.response` (single request) and
/// `data.items` (CLI-normalized pagination, aggregated across every page) are
/// filtered, so the filter cannot miss a page. A failed envelope is returned
/// untouched — a failure must never be rewritten into an empty admin list.
fn filter_admins_envelope(mut envelope: Envelope) -> Envelope {
    if !envelope.ok {
        return envelope;
    }

    let mut before = 0usize;
    let mut after = 0usize;
    let mut filtered = false;
    for key in ["response", "items"] {
        if let Some(target) = envelope.data.get_mut(key)
            && let Some((page_before, page_after)) = retain_admins(target)
        {
            before += page_before;
            after += page_after;
            filtered = true;
        }
    }

    if filtered && let serde_json::Value::Object(ref mut map) = envelope.data {
        map.insert(
            "admin_filter".to_string(),
            json!({
                "applied": "client_side",
                "field": "isAdmin",
                "items_before": before,
                "items_after": after,
                "reason": "GET /v4/people?role=admin returns the unfiltered people list",
            }),
        );
    }

    envelope
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
                .with_page_size(runtime.page_size)
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
            // Same: workspace context via header; /v4/workspaces/{id}/admins
            // returns 404. `role=admin` is sent but the gateway accepts and
            // ignores it — the response is the full people list — so the admin
            // filter has to be applied client-side on `isAdmin`.
            let envelope = one_api_live_request(
                &config,
                "workspace",
                "workspace-admins",
                "GET",
                "/v4/people?role=admin",
                false,
                &[],
            )?;
            filter_admins_envelope(envelope)
        }
        OneWorkspaceCommand::Groups { workspace_id } => {
            let config = runtime.load_profile_lenient(None)?;
            let path_id = resolve_workspace_path_id(workspace_id, &config)?;
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
            one.validate_workspace_identities()
                .map_err(|message| anyhow!("workspace switch: {message}"))?;
            if id.is_none() {
                let rows = one.workspace_credentials.iter().map(|(id, credential)| {
                    json!({
                        "workspace_id": credential.workspace_id.as_deref().unwrap_or(id),
                        "workspace_gid": credential.workspace_gid,
                        "workspace_name": credential.workspace_name,
                        "active": one.active_workspace_id.as_deref() == Some(id.as_str()),
                        "credential_health": credential.credential_health.as_deref().unwrap_or("unknown"),
                        "has_access_token": credential.access_token.is_some() || credential.access_token_ref.is_some(),
                    })
                }).collect::<Vec<_>>();
                return Ok(Envelope::ok_with_data(
                    "saved credential workspaces",
                    json!({
                        "items": rows,
                        "active_workspace_id": one.active_workspace_id,
                    }),
                ));
            }
            let selector = id.expect("checked above");
            let key = one
                .resolve_workspace_selector(&selector)
                .map_err(|message| anyhow!("workspace switch: {message}"))?;
            let credential = one
                .workspace_credentials
                .get(&key)
                .ok_or_else(|| anyhow!("workspace switch resolved to a missing credential"))?;
            let target = ayx_core::profile::WorkspaceTarget::from_credential(
                &key,
                credential,
                ayx_core::profile::WorkspaceResolutionSource::SavedCredential,
            )
            .ok_or_else(|| {
                anyhow!("workspace switch requires saved ID, GID, and exact name metadata")
            })?;
            let available: Vec<String> = one.workspace_credentials.keys().cloned().collect();
            if credential.access_token.is_none() && credential.access_token_ref.is_none() {
                let profile_name = config.profile_name.clone();
                return Err(anyhow!(
                    "no stored credential for workspace '{}' in profile '{}'. \
                     Authenticate into it first with `ayx one login` \
                     (the active workspace is determined by which workspace you logged into — \
                     the token is workspace-bound). \
                     Available: {}. \
                     Run `ayx one workspace list` to see workspaces.",
                    selector,
                    profile_name,
                    if available.is_empty() {
                        "(none)".to_string()
                    } else {
                        available.join(", ")
                    }
                ));
            }
            // Select the credential only in memory first. The current-workspace
            // endpoint is authoritative for token identity; durable state is
            // changed only after that probe succeeds.
            one.active_workspace_id = Some(key.clone());
            let _ = one;
            let identity = ayx_one_api::probe_config_current_workspace(&config).map_err(|err| {
                anyhow!("workspace switch verification failed: {err}; active state was not changed")
            })?;
            if identity.workspace_id != target.workspace_id
                || identity.workspace_gid != target.workspace_gid
            {
                return Err(anyhow!(
                    "selected workspace token identity ('{}', '{}') does not match saved workspace ('{}', '{}'); active state was not changed",
                    identity.workspace_id,
                    identity.workspace_gid,
                    target.workspace_id,
                    target.workspace_gid
                ));
            }
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
                format!("active workspace set to '{}'", target.workspace_id),
                json!({
                    "active_workspace_id": target.workspace_id,
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
    use super::{
        confirm_workspace_mutation, filter_admins_envelope, validate_workspace_gid,
        validate_workspace_path_id,
    };
    use ayx_core::envelope::Envelope;
    use serde_json::json;

    /// `GET /v4/people?role=admin` returns the full people list — the gateway
    /// accepts `role=admin` and ignores it (docs/ayx-cli-testing-issues.md
    /// Issue 1). `workspace admins` must therefore filter on the payload's
    /// live-payload admin flag, spelled `isAdmin`.
    #[test]
    fn workspace_admins_filters_people_payload_on_is_admin() {
        let envelope = Envelope::ok_with_data(
            "workspace workspace-admins ok",
            json!({
                "surface": "workspace",
                "operation": "workspace-admins",
                "status_code": 200,
                "response": {
                    "items": [
                        {
                            "id": "u_1",
                            "email": "admin@example.com",
                            "fullName": "Ada Admin",
                            "isAdmin": true,
                            "isSuspended": false,
                            "createdAt": "2026-01-02T03:04:05Z"
                        },
                        {
                            "id": "u_2",
                            "email": "member@example.com",
                            "fullName": "Mel Member",
                            "isAdmin": false,
                            "isSuspended": false,
                            "createdAt": "2026-02-02T03:04:05Z"
                        },
                        {
                            "id": "u_3",
                            "email": "other@example.com",
                            "fullName": "Otto Other",
                            "isAdmin": false,
                            "isSuspended": true,
                            "createdAt": "2026-03-02T03:04:05Z"
                        }
                    ],
                    "total": 3
                }
            }),
        );

        let filtered = filter_admins_envelope(envelope);
        let items = filtered.data["response"]["items"]
            .as_array()
            .expect("items array is preserved");
        assert_eq!(
            items.len(),
            1,
            "only the isAdmin person survives the filter"
        );
        assert_eq!(items[0]["id"], "u_1");
        // The per-person shape is untouched by the filter.
        assert_eq!(items[0]["email"], "admin@example.com");
        assert_eq!(items[0]["fullName"], "Ada Admin");
        assert_eq!(items[0]["isAdmin"], true);
        assert_eq!(items[0]["createdAt"], "2026-01-02T03:04:05Z");
        // A payload-reported total is re-stated for the filtered set.
        assert_eq!(filtered.data["response"]["total"], 1);
        assert_eq!(filtered.data["admin_filter"]["field"], "isAdmin");
        assert_eq!(filtered.data["admin_filter"]["items_before"], 3);
        assert_eq!(filtered.data["admin_filter"]["items_after"], 1);
    }

    /// A person record with no admin flag at all is not an admin.
    #[test]
    fn workspace_admins_treats_missing_admin_flag_as_not_admin() {
        let envelope = Envelope::ok_with_data(
            "workspace workspace-admins ok",
            json!({
                "response": {
                    "items": [
                        { "id": "u_1", "email": "nobody@example.com" },
                        { "id": "u_2", "email": "admin@example.com", "is_admin": true }
                    ]
                }
            }),
        );

        let filtered = filter_admins_envelope(envelope);
        let items = filtered.data["response"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "u_2");
    }

    /// The CLI-normalized pagination shape puts every page's records under a
    /// top-level `data.items`; the filter must apply there too, so it covers
    /// all pages rather than only the first.
    #[test]
    fn workspace_admins_filters_aggregated_pagination_items() {
        let envelope = Envelope::ok_with_data(
            "workspace workspace-admins ok",
            json!({
                "items": [
                    { "id": "p1_a", "isAdmin": false },
                    { "id": "p1_b", "isAdmin": true },
                    { "id": "p2_a", "isAdmin": false },
                    { "id": "p2_b", "isAdmin": true }
                ],
                "pages_fetched": 2
            }),
        );

        let filtered = filter_admins_envelope(envelope);
        let items = filtered.data["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["id"], "p1_b");
        assert_eq!(items[1]["id"], "p2_b");
        assert_eq!(filtered.data["pages_fetched"], 2);
    }

    /// A failed request is passed through untouched — never rewritten into a
    /// clean, empty admin list.
    #[test]
    fn workspace_admins_filter_leaves_failures_untouched() {
        let envelope = Envelope::err_coded(
            ayx_core::envelope::ErrorCode::PermissionDenied,
            "workspace workspace-admins failed",
            json!({ "response": { "items": [{ "id": "u_1", "isAdmin": false }] } }),
        );

        let filtered = filter_admins_envelope(envelope);
        assert!(!filtered.ok);
        assert_eq!(
            filtered.data["response"]["items"].as_array().unwrap().len(),
            1
        );
        assert!(filtered.data.get("admin_filter").is_none());
    }

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
