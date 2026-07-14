use anyhow::{Result, anyhow};
use ayx_core::envelope::Envelope;
use ayx_core::profile::profile_storage_path;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};
use serde_json::json;

use crate::{
    OneWorkspaceCommand,
    cmd::{self, RuntimeCtx},
    load_payload,
    onboard::{InlineSecretPolicy, inline_secret_warning, write_config_with_policy},
};

/// Resolve a workspace id from the explicit arg or fall back to the profile's
/// configured `workspace_gid`. Returns an error if neither is available.
///
/// When an explicit workspace-id is supplied AND the profile has an active
/// workspace, they must match — the token is workspace-bound. Passing the
/// wrong id would silently mutate a different workspace than the caller
/// expected, so we fail closed with a clear remediation message.
fn resolve_workspace_id(
    explicit: Option<String>,
    config: &ayx_core::profile::Config,
) -> Result<String> {
    let active = config
        .alteryx_one
        .as_ref()
        .and_then(|o| o.resolved_workspace_gid())
        .map(str::to_string);

    match (explicit, active) {
        (Some(exp), Some(act)) if exp != act => Err(anyhow!(
            "--workspace-id '{}' does not match the active workspace '{}'. \
             The token is workspace-bound; switch with `ayx one workspace switch` \
             or re-authenticate. Omit --workspace-id to use the active workspace.",
            exp,
            act
        )),
        (Some(exp), _) => Ok(exp),
        (None, Some(act)) => Ok(act),
        (None, None) => Err(anyhow!(
            "workspace-id not specified and could not be inferred from profile; \
             pass --workspace-id explicitly"
        )),
    }
}

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: Option<OneWorkspaceCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok(
            "one workspace commands available: list, current, current-configuration, configuration-v4, save-current-configuration, save-configuration-v4, configuration, configuration-schema, current-configuration-schema, delete-current-configuration, delete-configuration, people, admins, switch, invite-users, remove-user, suspend-users, unsuspend-users, transfer, transfer-assets",
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
                "workspace",
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
                "workspace",
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
                "workspace",
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
                "workspace",
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
                "workspace",
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
                "workspace",
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
                "workspace",
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
                "workspace",
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
                "workspace",
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
                "workspace",
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
                "workspace",
                "workspace-configuration",
                "GET",
                "/v4/workspaces/{id}/configuration",
                false,
                &[("id", &workspace_id)],
            )?
        }
        Some(OneWorkspaceCommand::People) => {
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
        Some(OneWorkspaceCommand::Admins) => {
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
        Some(OneWorkspaceCommand::Switch {
            profile,
            workspace_id,
        }) => {
            let mut config = runtime.load_profile_lenient(profile.as_deref())?;
            let one = config
                .alteryx_one
                .as_mut()
                .ok_or_else(|| anyhow!("no alteryx_one section in profile"))?;
            let available: Vec<String> = one.workspace_credentials.keys().cloned().collect();
            if !one.workspace_credentials.contains_key(&workspace_id) {
                let profile_name = config.profile_name.clone();
                return Err(anyhow!(
                    "no stored credential for workspace '{}' in profile '{}'. \
                     Authenticate into it first with `ayx one login` \
                     (the active workspace is determined by which workspace you logged into — \
                     the token is workspace-bound). \
                     Available: {}. \
                     Run `ayx one workspace list` to see workspaces.",
                    workspace_id,
                    profile_name,
                    if available.is_empty() {
                        "(none)".to_string()
                    } else {
                        available.join(", ")
                    }
                ));
            }
            one.expected_workspace_id = Some(workspace_id.clone());
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
                format!("active workspace set to '{workspace_id}'"),
                json!({
                    "active_workspace_id": workspace_id,
                    "available_workspace_ids": available_after,
                    "profile": profile_name,
                }),
            )
        }
        Some(OneWorkspaceCommand::InviteUsers { workspace_id }) => {
            let config = runtime.load_profile_lenient(None)?;
            let ws_id = resolve_workspace_id(workspace_id, &config)?;
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
                "workspace",
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
                "workspace",
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
                "workspace",
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
                "workspace",
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
                "workspace",
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

#[cfg(test)]
mod tests {
    use super::resolve_workspace_id;
    use ayx_core::profile::{
        AlteryxOneProfile, Config, MongoDatabases, MongoMode, MongoProfile, WorkspaceCredential,
    };

    /// Minimal Config with an Alteryx One section and optionally a workspace credential
    /// so `resolved_workspace_gid()` resolves via `active_workspace_id()`.
    fn base_config(active_gid: Option<&str>) -> Config {
        let mut one = AlteryxOneProfile {
            account_email: "test@example.com".into(),
            base_url: Some("https://example.alteryxcloud.com".into()),
            ..Default::default()
        };
        if let Some(gid) = active_gid {
            let cred = WorkspaceCredential {
                access_token: Some("tok".into()),
                workspace_gid: Some(gid.into()),
                ..Default::default()
            };
            one.workspace_credentials.insert(gid.into(), cred);
            one.expected_workspace_id = Some(gid.into());
        }
        Config {
            profile_name: "test-profile".into(),
            mongo: MongoProfile {
                mode: MongoMode::Embedded,
                databases: MongoDatabases {
                    gallery_name: "g".into(),
                    service_name: "s".into(),
                },
                embedded: None,
                managed: None,
            },
            alteryx_one: Some(one),
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        }
    }

    fn config_no_one() -> Config {
        Config {
            profile_name: "test-profile".into(),
            mongo: MongoProfile {
                mode: MongoMode::Embedded,
                databases: MongoDatabases {
                    gallery_name: "g".into(),
                    service_name: "s".into(),
                },
                embedded: None,
                managed: None,
            },
            alteryx_one: None,
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        }
    }

    #[test]
    fn explicit_id_matching_active_gid_returns_it() {
        let config = base_config(Some("ws-001"));
        let result = resolve_workspace_id(Some("ws-001".into()), &config);
        assert_eq!(result.unwrap(), "ws-001");
    }

    #[test]
    fn explicit_id_differing_from_active_gid_returns_mismatch_error() {
        let config = base_config(Some("ws-001"));
        let err = resolve_workspace_id(Some("ws-OTHER".into()), &config).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ws-OTHER") && msg.contains("ws-001"),
            "error message should reference both ids: {msg}"
        );
        assert!(
            msg.contains("workspace-bound") || msg.contains("switch"),
            "error should mention workspace-bound or switch: {msg}"
        );
    }

    #[test]
    fn no_explicit_id_profile_has_active_gid_returns_it() {
        let config = base_config(Some("ws-002"));
        let result = resolve_workspace_id(None, &config);
        assert_eq!(result.unwrap(), "ws-002");
    }

    #[test]
    fn no_explicit_id_no_active_gid_returns_error() {
        // Profile with alteryx_one block but no workspace credentials → no active gid.
        let config = base_config(None);
        let err = resolve_workspace_id(None, &config).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("workspace-id") || msg.contains("specify"),
            "error should tell user to specify workspace-id: {msg}"
        );
    }

    #[test]
    fn no_explicit_id_no_alteryx_one_section_returns_error() {
        let config = config_no_one();
        let err = resolve_workspace_id(None, &config).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("workspace-id") || msg.contains("specify"),
            "error should tell user to specify workspace-id: {msg}"
        );
    }
}
