use anyhow::{Result, anyhow};
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};
use serde_json::json;

use crate::{
    OneConnectionPermissionCommand, OneConnectionsCommand, OneConnectorMetadataCommand,
    OneConnectorMetadataOverridesCommand,
    cmd::{self, RuntimeCtx},
    load_payload,
};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: Option<OneConnectionsCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok(
            "one connections commands available: list, count, create, dry-run, detail, status, update, delete, permissions, connector-metadata",
        ),
        Some(OneConnectionsCommand::List {
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
                "connection",
                "list",
                "/v4/connections",
                &[],
                &params,
            )?
        }
        Some(OneConnectionsCommand::Count { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "connection",
                "count",
                "GET",
                "/v4/connections/count",
                false,
                &[],
            )?
        }
        Some(OneConnectionsCommand::Create { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "connection",
                "create",
                "POST",
                "/v4/connections",
                true,
                &[],
                Some(payload),
            )?
        }
        Some(OneConnectionsCommand::DryRun { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "connection",
                "dry-run",
                "POST",
                "/v4/connections/dryRun",
                false,
                &[],
                Some(payload),
            )?
        }
        Some(OneConnectionsCommand::Detail {
            profile,
            connection_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let connection_id =
                connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
            one_api_live_request(
                &config,
                "connection",
                "detail",
                "GET",
                "/v4/connections/{id}",
                false,
                &[("id", connection_id.as_str())],
            )?
        }
        Some(OneConnectionsCommand::Status {
            profile,
            connection_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let connection_id =
                connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
            one_api_live_request(
                &config,
                "connection",
                "status",
                "GET",
                "/v4/connections/{id}/status",
                false,
                &[("id", connection_id.as_str())],
            )?
        }
        Some(OneConnectionsCommand::Update {
            profile,
            connection_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let connection_id =
                connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "connection",
                "update",
                "PATCH",
                "/v4/connections/{id}",
                true,
                &[("id", connection_id.as_str())],
                Some(payload),
            )?
        }
        Some(OneConnectionsCommand::Delete {
            profile,
            connection_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let connection_id =
                connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::destructive_action_message(
                        "delete",
                        &format!("connection id='{connection_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "connection",
                "delete",
                "DELETE",
                "/v4/connections/{id}",
                true,
                &[("id", connection_id.as_str())],
            )?
        }
        Some(OneConnectionsCommand::ConnectorMetadata { command }) => match command {
            None => Envelope::ok(
                "one connections connector-metadata commands available: defaults, detail, publish-info, overrides, template. \
                 Note: connector enumeration (list) is not available via the Alteryx One v4 API — use a known connector slug \
                 (e.g. 'gsheetsuser', 'remotefile') with 'detail' to discover the schema.",
            ),
            Some(OneConnectorMetadataCommand::Defaults { profile, connector }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                one_api_live_request(
                    &config,
                    "connection",
                    "connector-metadata-defaults",
                    "GET",
                    "/v4/connectorMetadata/{connector}/defaults",
                    false,
                    &[("connector", connector.as_str())],
                )?
            }
            Some(OneConnectorMetadataCommand::Detail { profile, connector }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                one_api_live_request(
                    &config,
                    "connection",
                    "connector-metadata-detail",
                    "GET",
                    "/v4/connectorMetadata/{connector}",
                    false,
                    &[("connector", connector.as_str())],
                )?
            }
            Some(OneConnectorMetadataCommand::PublishInfo { profile, connector }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                one_api_live_request(
                    &config,
                    "connection",
                    "connector-metadata-publish-info",
                    "GET",
                    "/v4/connectorMetadata/{connector}/publish/info",
                    false,
                    &[("connector", connector.as_str())],
                )?
            }
            Some(OneConnectorMetadataCommand::Template { profile, connector }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let envelope = one_api_live_request(
                    &config,
                    "connection",
                    "connector-metadata-defaults",
                    "GET",
                    "/v4/connectorMetadata/{connector}/defaults",
                    false,
                    &[("connector", connector.as_str())],
                )?;

                // If the live request itself failed (auth, network, etc.) propagate
                // it as-is so the caller sees the error envelope.
                if !envelope.ok {
                    return Ok(envelope);
                }

                let metadata = envelope
                    .data
                    .get("response")
                    .and_then(|r| r.get("connectionMetadata"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);

                let category = metadata
                    .get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let conn_type = if category == "relational" {
                    "jdbc"
                } else {
                    "remotefile"
                };

                let credential_type = metadata
                    .get("credentialTypes")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.as_str())
                    .unwrap_or("apiKey");

                let params: serde_json::Map<String, serde_json::Value> = metadata
                    .get("connectionParameters")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|p| {
                                let name = p.get("name")?.as_str()?;
                                let type_hint =
                                    p.get("type").and_then(|t| t.as_str()).unwrap_or("string");
                                let default_raw =
                                    p.get("defaultValue").and_then(|d| d.as_str()).unwrap_or("");
                                let value = if default_raw.is_empty() {
                                    serde_json::Value::String(format!("<{type_hint}>"))
                                } else {
                                    serde_json::Value::String(default_raw.to_string())
                                };
                                Some((name.to_string(), value))
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let template = json!({
                    "name": "<your connection name>",
                    "description": "",
                    "type": conn_type,
                    "vendor": connector,
                    "vendorName": connector,
                    "credentialType": credential_type,
                    "isGlobal": false,
                    "ssl": false,
                    "params": serde_json::Value::Object(params),
                });

                Envelope::ok_with_data(
                    format!(
                        "connection create template for '{connector}' — fill in placeholders and pass to 'connections create --body <file>'"
                    ),
                    template,
                )
            }
            Some(OneConnectorMetadataCommand::Overrides { command }) => match command {
                None => Envelope::ok(
                    "one connections connector-metadata overrides commands available: list, create, delete",
                ),
                Some(OneConnectorMetadataOverridesCommand::List { profile, connector }) => {
                    let config = runtime.load_profile_lenient(profile.as_deref())?;
                    one_api_live_request(
                        &config,
                        "connection",
                        "connector-metadata-overrides-list",
                        "GET",
                        "/v4/connectorMetadata/{connector}/overrides",
                        false,
                        &[("connector", connector.as_str())],
                    )?
                }
                Some(OneConnectorMetadataOverridesCommand::Create {
                    profile,
                    connector,
                    body,
                }) => {
                    let config = runtime.load_profile_lenient(profile.as_deref())?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "connection",
                        "connector-metadata-overrides-create",
                        "POST",
                        "/v4/connectorMetadata/{connector}/overrides",
                        true,
                        &[("connector", connector.as_str())],
                        Some(payload),
                    )?
                }
                Some(OneConnectorMetadataOverridesCommand::Delete { profile, connector }) => {
                    let config = runtime.load_profile_lenient(profile.as_deref())?;
                    if apply {
                        cmd::confirm::require_tty_confirmation(
                            yes,
                            &cmd::confirm::destructive_action_message(
                                "delete",
                                &format!(
                                    "connector metadata overrides for connector '{connector}'"
                                ),
                                &config.profile_name,
                            ),
                        )?;
                    }
                    one_api_live_request(
                        &config,
                        "connection",
                        "connector-metadata-overrides-delete",
                        "DELETE",
                        "/v4/connectorMetadata/{connector}/overrides",
                        true,
                        &[("connector", connector.as_str())],
                    )?
                }
            },
        },
        Some(OneConnectionsCommand::Permissions { command }) => match command {
            None => Envelope::ok(
                "one connection permissions commands available: list, create, detail, delete",
            ),
            Some(OneConnectionPermissionCommand::List {
                profile,
                connection_id,
            }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let connection_id =
                    connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
                one_api_live_request(
                    &config,
                    "connection",
                    "permissions",
                    "GET",
                    "/v4/connections/{id}/permissions",
                    false,
                    &[("id", connection_id.as_str())],
                )?
            }
            Some(OneConnectionPermissionCommand::Create {
                profile,
                connection_id,
                body,
            }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let connection_id =
                    connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
                let payload = load_payload(&body)?;
                one_api_live_request_with_body(
                    &config,
                    "connection",
                    "permissions-create",
                    "POST",
                    "/v4/connections/{id}/permissions",
                    true,
                    &[("id", connection_id.as_str())],
                    Some(payload),
                )?
            }
            Some(OneConnectionPermissionCommand::Detail {
                profile,
                connection_id,
                subject_id,
            }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let connection_id =
                    connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
                one_api_live_request(
                    &config,
                    "connection",
                    "permissions-detail",
                    "GET",
                    "/v4/connections/{id}/permissions/{aid}",
                    false,
                    &[("id", connection_id.as_str()), ("aid", subject_id.as_str())],
                )?
            }
            Some(OneConnectionPermissionCommand::Delete {
                profile,
                connection_id,
                subject_id,
            }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let connection_id =
                    connection_id.ok_or_else(|| anyhow!("--connection-id is required"))?;
                if apply {
                    cmd::confirm::require_tty_confirmation(
                        yes,
                        &cmd::confirm::destructive_action_message(
                            "delete",
                            &format!(
                                "permission subject id='{subject_id}' on connection id='{connection_id}'"
                            ),
                            &config.profile_name,
                        ),
                    )?;
                }
                one_api_live_request(
                    &config,
                    "connection",
                    "permissions-delete",
                    "DELETE",
                    "/v4/connections/{id}/permissions/{aid}",
                    true,
                    &[("id", connection_id.as_str()), ("aid", subject_id.as_str())],
                )?
            }
        },
    })
}
