use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};
use serde_json::json;

use crate::{
    OneConnectionPermissionCommand, OneConnectionsCommand, OneConnectorMetadataCommand,
    OneConnectorMetadataOverridesCommand,
    cmd::{self, RuntimeCtx},
    load_payload,
};

/// Build a connection create-body template from connector metadata returned by
/// `/v4/connectorMetadata/{connector}/defaults`.
///
/// `connector` is the slug (e.g. `"bigquery"`, `"gsheetsuser"`).
/// `metadata` is the `connectionMetadata` sub-object from the live response, or
/// `Value::Null` / any non-object when the response was unavailable.
///
/// This is pure data transformation with no I/O — extracted so it can be
/// unit-tested without a live API call.
pub(crate) fn build_connection_template(
    connector: &str,
    metadata: &serde_json::Value,
) -> serde_json::Value {
    let category = metadata
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    const FILE_CATEGORIES: &[&str] = &[
        "file",
        "remotefile",
        "filesystem",
        "cloud_storage",
        "storage",
    ];
    let (conn_type, type_guessed) = if category == "relational" {
        ("jdbc".to_string(), false)
    } else if FILE_CATEGORIES.contains(&category) || category.contains("file") {
        ("remotefile".to_string(), false)
    } else if category.is_empty() {
        ("<jdbc|remotefile|...>".to_string(), true)
    } else {
        // Unknown non-empty category: produce a placeholder so the
        // operator can choose the right value.
        ("<jdbc|remotefile|...>".to_string(), true)
    };

    let credential_types: Vec<&str> = metadata
        .get("credentialTypes")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let credential_type = credential_types.first().copied().unwrap_or("apiKey");
    let multiple_credential_types = credential_types.len() > 1;

    let params: serde_json::Map<String, serde_json::Value> = metadata
        .get("connectionParameters")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    let name = p.get("name")?.as_str()?;
                    let type_hint = p.get("type").and_then(|t| t.as_str()).unwrap_or("string");
                    let default_raw = p.get("defaultValue").and_then(|d| d.as_str()).unwrap_or("");
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

    let mut notes: Vec<String> = Vec::new();
    if type_guessed {
        notes.push(format!(
            "type was guessed from category='{}'; set to 'jdbc' for database connections or 'remotefile' for file/cloud-storage connections",
            category
        ));
    }
    if multiple_credential_types {
        notes.push(format!(
            "multiple credentialTypes available: {}; template uses the first",
            credential_types.join(", ")
        ));
    }

    let mut template = json!({
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
    if !notes.is_empty() {
        template["_note"] =
            serde_json::Value::Array(notes.into_iter().map(serde_json::Value::String).collect());
    }
    template
}

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
        Some(OneConnectionsCommand::Detail { profile, id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "connection",
                "detail",
                "GET",
                "/v4/connections/{id}",
                false,
                &[("id", id.as_str())],
            )?
        }
        Some(OneConnectionsCommand::Status { profile, id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "connection",
                "status",
                "GET",
                "/v4/connections/{id}/status",
                false,
                &[("id", id.as_str())],
            )?
        }
        Some(OneConnectionsCommand::Update { profile, id, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "connection",
                "update",
                "PATCH",
                "/v4/connections/{id}",
                true,
                &[("id", id.as_str())],
                Some(payload),
            )?
        }
        Some(OneConnectionsCommand::Delete { profile, id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::destructive_action_message(
                        "delete",
                        &format!("connection id='{id}'"),
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
                &[("id", id.as_str())],
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

                let template = build_connection_template(&connector, &metadata);

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
            Some(OneConnectionPermissionCommand::List { profile, id }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                one_api_live_request(
                    &config,
                    "connection",
                    "permissions",
                    "GET",
                    "/v4/connections/{id}/permissions",
                    false,
                    &[("id", id.as_str())],
                )?
            }
            Some(OneConnectionPermissionCommand::Create { profile, id, body }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let payload = load_payload(&body)?;
                one_api_live_request_with_body(
                    &config,
                    "connection",
                    "permissions-create",
                    "POST",
                    "/v4/connections/{id}/permissions",
                    true,
                    &[("id", id.as_str())],
                    Some(payload),
                )?
            }
            Some(OneConnectionPermissionCommand::Detail {
                profile,
                connection_id,
                subject_id,
            }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
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

#[cfg(test)]
mod tests {
    use super::build_connection_template;
    use serde_json::json;

    #[test]
    fn bigquery_relational_category_produces_jdbc_type_with_params() {
        let metadata = json!({
            "category": "relational",
            "credentialTypes": ["apiKey", "oauth2"],
            "connectionParameters": [
                {
                    "name": "projectId",
                    "type": "string",
                    "required": true,
                    "defaultValue": ""
                }
            ]
        });
        let tmpl = build_connection_template("bigquery", &metadata);

        assert_eq!(tmpl["type"], json!("jdbc"), "relational → jdbc");
        assert_eq!(tmpl["vendor"], json!("bigquery"));
        // First credential type used.
        assert_eq!(tmpl["credentialType"], json!("apiKey"));
        // Parameter placeholder emitted because defaultValue is empty.
        assert_eq!(tmpl["params"]["projectId"], json!("<string>"));
        // Multiple credential types → _note is present.
        let note = tmpl["_note"].as_array().expect("_note must be array");
        assert!(
            note.iter().any(|n| n
                .as_str()
                .unwrap_or("")
                .contains("multiple credentialTypes")),
            "_note should mention multiple credentialTypes"
        );
    }

    #[test]
    fn gsheets_file_category_produces_remotefile_type() {
        let metadata = json!({
            "category": "remotefile",
            "credentialTypes": ["oauth2"],
            "connectionParameters": []
        });
        let tmpl = build_connection_template("gsheetsuser", &metadata);

        assert_eq!(
            tmpl["type"],
            json!("remotefile"),
            "remotefile category → remotefile type"
        );
        assert_eq!(tmpl["credentialType"], json!("oauth2"));
        // Single credential type → no _note about multiple types.
        let has_multi_note = tmpl["_note"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .any(|n| n.as_str().unwrap_or("").contains("multiple"))
            })
            .unwrap_or(false);
        assert!(
            !has_multi_note,
            "single credentialType should not produce a multiple-types note"
        );
    }

    #[test]
    fn unknown_category_produces_placeholder_type_and_note() {
        let metadata = json!({
            "category": "exotic",
            "credentialTypes": ["apiKey"],
            "connectionParameters": []
        });
        let tmpl = build_connection_template("myconnector", &metadata);

        assert_eq!(
            tmpl["type"],
            json!("<jdbc|remotefile|...>"),
            "unknown category → placeholder"
        );
        let note = tmpl["_note"]
            .as_array()
            .expect("_note must be array for unknown category");
        assert!(
            note.iter()
                .any(|n| n.as_str().unwrap_or("").contains("type was guessed")),
            "_note should mention type guessing"
        );
    }

    #[test]
    fn null_metadata_produces_graceful_generic_template_no_panic() {
        // Null metadata (e.g. request failed or field absent) → generic template.
        let tmpl = build_connection_template("unknown", &serde_json::Value::Null);

        // Type becomes the placeholder because category is absent (empty string).
        assert_eq!(tmpl["type"], json!("<jdbc|remotefile|...>"));
        // credentialType falls back to "apiKey" when no credentialTypes array.
        assert_eq!(tmpl["credentialType"], json!("apiKey"));
        // params is an empty object — no panic from iterating a missing array.
        assert_eq!(tmpl["params"], json!({}));
        assert_eq!(tmpl["vendor"], json!("unknown"));
    }

    #[test]
    fn parameter_with_non_empty_default_value_uses_default() {
        let metadata = json!({
            "category": "relational",
            "credentialTypes": ["apiKey"],
            "connectionParameters": [
                { "name": "port", "type": "integer", "required": false, "defaultValue": "5432" }
            ]
        });
        let tmpl = build_connection_template("postgres", &metadata);
        assert_eq!(
            tmpl["params"]["port"],
            json!("5432"),
            "non-empty defaultValue must be used as-is"
        );
    }
}
