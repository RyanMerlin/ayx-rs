use anyhow::{Result, bail};
use ayx_core::envelope::{Envelope, ErrorCode};
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};
use serde_json::{Value, json};

use crate::{
    ConnectionSharePolicy, OneConnectionPermissionCommand, OneConnectionsCommand,
    OneConnectorMetadataCommand, OneConnectorMetadataOverridesCommand, ShareSubjectType,
    cmd::{self, RuntimeCtx},
    load_payload,
};

/// Body for `POST /v4/connections/share`.
///
/// The connection id travels in the BODY, not the path — unlike every other
/// connection endpoint. The CLI still takes it positionally (consistent with its
/// siblings) and this moves it into place.
///
/// Pure data transformation, no I/O, so it can be unit-tested without a live call.
pub(crate) fn build_connection_share_body(
    connection_id: &str,
    policy: ConnectionSharePolicy,
    person_ids: &[String],
    group_ids: &[String],
) -> Result<serde_json::Value> {
    if person_ids.is_empty() && group_ids.is_empty() {
        bail!("no share recipients: pass at least one --to-person or --to-group");
    }
    let mut subjects = serde_json::Map::new();
    if !person_ids.is_empty() {
        subjects.insert("person".to_string(), json!(person_ids));
    }
    if !group_ids.is_empty() {
        subjects.insert("group".to_string(), json!(group_ids));
    }
    Ok(json!({
        "connectionId": connection_id,
        "policy": policy.as_api_str(),
        "subjects": subjects,
    }))
}

/// Bind a raw permission body to the positional connection id.
///
/// The share endpoint takes the resource id from the JSON body rather than
/// the URL. Refusing a conflicting body value prevents an operator from
/// confirming one connection while actually sharing another. A missing id is
/// filled from the positional argument so raw bodies remain convenient.
pub(crate) fn validate_connection_share_body(
    connection_id: &str,
    mut payload: Value,
) -> Result<Value> {
    let object = payload
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("connection share body must be a JSON object"))?;
    if let Some(existing) = object.get("connectionId") {
        let matches = match existing {
            Value::String(value) => value == connection_id,
            Value::Number(value) => value.to_string() == connection_id,
            _ => false,
        };
        if !matches {
            bail!(
                "connection share body connectionId does not match positional connection id '{}', refusing to send",
                connection_id
            );
        }
    } else {
        object.insert(
            "connectionId".to_string(),
            Value::String(connection_id.to_string()),
        );
    }
    Ok(payload)
}

/// Query string for `DELETE /v4/connections/share`.
///
/// Percent-encodes every value: ids are server-supplied strings, and an id
/// containing `&` or `=` would otherwise forge extra query parameters.
pub(crate) fn build_connection_unshare_query(
    connection_id: &str,
    subject_id: &str,
    subject_type: ShareSubjectType,
) -> String {
    fn encode(value: &str) -> String {
        value
            .bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                other => format!("%{other:02X}"),
            })
            .collect()
    }
    format!(
        "/v4/connections/share?connectionId={}&subjectId={}&subjectType={}",
        encode(connection_id),
        encode(subject_id),
        encode(subject_type.as_api_str()),
    )
}

/// Find one subject in a `sharedSubjects` response.
///
/// `GET /v4/connections/{id}/permissions/{subjectId}` does not exist, so
/// `permissions detail` reads the whole shared-subject list and filters here.
/// Returns the matching entry from either the `people` or `groups` array.
pub(crate) fn find_shared_subject(
    shared_subjects: &serde_json::Value,
    subject_id: &str,
) -> Option<serde_json::Value> {
    for bucket in ["people", "groups"] {
        let Some(entries) = shared_subjects.get(bucket).and_then(|v| v.as_array()) else {
            continue;
        };
        for entry in entries {
            let matches = ["subjectId", "id"].iter().any(|key| match entry.get(key) {
                Some(serde_json::Value::String(s)) => s == subject_id,
                Some(serde_json::Value::Number(n)) => n.to_string() == subject_id,
                _ => false,
            });
            if matches {
                let mut found = entry.clone();
                if let Some(obj) = found.as_object_mut() {
                    obj.insert("subjectBucket".to_string(), json!(bucket));
                }
                return Some(found);
            }
        }
    }
    None
}

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
    command: OneConnectionsCommand,
) -> Result<Envelope> {
    Ok(match command {
        OneConnectionsCommand::List {
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
                "connection",
                "list",
                "/v4/connections",
                &[],
                &params,
            )?
        }
        OneConnectionsCommand::Count { profile } => {
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
        OneConnectionsCommand::Create { profile, body } => {
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
        OneConnectionsCommand::DryRun { profile, body } => {
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
        OneConnectionsCommand::Detail { profile, id } => {
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
        OneConnectionsCommand::Status { profile, id } => {
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
        OneConnectionsCommand::Update { profile, id, body } => {
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
        OneConnectionsCommand::Delete { profile, id } => {
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
        OneConnectionsCommand::ConnectorMetadata { command } => match command {
            OneConnectorMetadataCommand::Defaults { profile, connector } => {
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
            OneConnectorMetadataCommand::Detail { profile, connector } => {
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
            OneConnectorMetadataCommand::PublishInfo { profile, connector } => {
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
            OneConnectorMetadataCommand::Template { profile, connector } => {
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
            OneConnectorMetadataCommand::Overrides { command } => match command {
                OneConnectorMetadataOverridesCommand::List { profile, connector } => {
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
                OneConnectorMetadataOverridesCommand::Create {
                    profile,
                    connector,
                    body,
                } => {
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
                OneConnectorMetadataOverridesCommand::Delete { profile, connector } => {
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
        OneConnectionsCommand::Permissions { command } => match command {
            OneConnectionPermissionCommand::List { profile, id } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                one_api_live_request(
                    &config,
                    "connection",
                    "permissions",
                    "GET",
                    "/v4/connections/{id}/permissions/sharedSubjects",
                    false,
                    &[("id", id.as_str())],
                )?
            }
            OneConnectionPermissionCommand::Create {
                profile,
                id,
                policy,
                to_person,
                to_group,
                body,
            } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let payload = match body {
                    Some(path) => validate_connection_share_body(&id, load_payload(&path)?)?,
                    None => {
                        let Some(policy) = policy else {
                            bail!(
                                "missing --policy: pass --policy <editor|viewer> \
                                 (or supply a raw --body)"
                            );
                        };
                        build_connection_share_body(&id, policy, &to_person, &to_group)?
                    }
                };
                if apply {
                    cmd::confirm::require_tty_confirmation(
                        yes,
                        &cmd::confirm::access_change_message(
                            "share",
                            &format!("connection id='{id}'"),
                            &config.profile_name,
                        ),
                    )?;
                }
                // The id is in the body, not the path: POST /v4/connections/share.
                one_api_live_request_with_body(
                    &config,
                    "connection",
                    "permissions-create",
                    "POST",
                    "/v4/connections/share",
                    true,
                    &[],
                    Some(payload),
                )?
            }
            OneConnectionPermissionCommand::Detail {
                profile,
                connection_id,
                subject_id,
            } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                // No per-subject route exists; read the shared-subject list and
                // filter locally. Say so in the envelope so a caller can tell this
                // apart from a server-side lookup.
                let envelope = one_api_live_request(
                    &config,
                    "connection",
                    "permissions-detail",
                    "GET",
                    "/v4/connections/{id}/permissions/sharedSubjects",
                    false,
                    &[("id", connection_id.as_str())],
                )?;
                if !envelope.ok {
                    return Ok(envelope);
                }
                let detail_source = "synthesized client-side by filtering \
                     GET /v4/connections/{id}/permissions/sharedSubjects; the API has no \
                     per-subject permission route";
                match find_shared_subject(&envelope.data["response"], &subject_id) {
                    Some(subject) => Envelope::ok_with_data(
                        format!("connection permissions-detail ok (subject {subject_id})"),
                        json!({
                            "surface": "connection",
                            "operation": "permissions-detail",
                            "connection_id": connection_id,
                            "subject_id": subject_id,
                            "detail_source": detail_source,
                            "response": subject,
                        }),
                    ),
                    None => Envelope::err_coded(
                        ErrorCode::NotFound,
                        format!(
                            "connection permissions-detail failed: subject {subject_id} \
                             is not shared on connection {connection_id}"
                        ),
                        json!({
                            "surface": "connection",
                            "operation": "permissions-detail",
                            "connection_id": connection_id,
                            "subject_id": subject_id,
                            "detail_source": detail_source,
                            "response": serde_json::Value::Null,
                            "error_code": "not_found",
                        }),
                    ),
                }
            }
            OneConnectionPermissionCommand::Delete {
                profile,
                connection_id,
                subject_id,
                subject_type,
            } => {
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
                // Ids travel as query parameters on a shared /share route.
                let endpoint =
                    build_connection_unshare_query(&connection_id, &subject_id, subject_type);
                one_api_live_request(
                    &config,
                    "connection",
                    "permissions-delete",
                    "DELETE",
                    &endpoint,
                    true,
                    &[],
                )?
            }
        },
    })
}

#[cfg(test)]
mod share_tests {
    use super::{
        build_connection_share_body, build_connection_unshare_query, find_shared_subject,
        validate_connection_share_body,
    };
    use crate::{ConnectionSharePolicy, ShareSubjectType};
    use serde_json::json;

    #[test]
    fn share_body_moves_the_id_into_the_body_and_omits_empty_subject_buckets() {
        let body = build_connection_share_body(
            "44865",
            ConnectionSharePolicy::Editor,
            &["113168".to_string()],
            &[],
        )
        .expect("valid share");

        assert_eq!(body["connectionId"], "44865");
        assert_eq!(body["policy"], "EDITOR");
        assert_eq!(body["subjects"]["person"], json!(["113168"]));
        assert!(body["subjects"].get("group").is_none());
    }

    #[test]
    fn share_body_includes_both_non_empty_subject_buckets() {
        let body = build_connection_share_body(
            "44865",
            ConnectionSharePolicy::Viewer,
            &["113168".to_string()],
            &["900".to_string()],
        )
        .expect("valid share");

        assert_eq!(body["subjects"]["person"], json!(["113168"]));
        assert_eq!(body["subjects"]["group"], json!(["900"]));
    }

    #[test]
    fn share_body_policy_renders_the_upper_case_api_enum() {
        let body = build_connection_share_body(
            "1",
            ConnectionSharePolicy::Viewer,
            &["2".to_string()],
            &[],
        )
        .expect("valid share");
        assert_eq!(body["policy"], "VIEWER");
    }

    #[test]
    fn share_body_requires_at_least_one_recipient() {
        let err = build_connection_share_body("1", ConnectionSharePolicy::Editor, &[], &[])
            .expect_err("no recipients must fail before any network call");
        let msg = err.to_string();
        assert!(msg.contains("--to-person"), "actionable message: {msg}");
        assert!(msg.contains("--to-group"), "actionable message: {msg}");
    }

    #[test]
    fn raw_share_body_injects_missing_connection_id() {
        let body = validate_connection_share_body(
            "44865",
            json!({"policy": "VIEWER", "subjects": {"person": ["4477"]}}),
        )
        .expect("missing id should be bound");
        assert_eq!(body["connectionId"], "44865");
    }

    #[test]
    fn raw_share_body_rejects_conflicting_connection_id() {
        let err = validate_connection_share_body(
            "44865",
            json!({"connectionId": "99999", "policy": "VIEWER"}),
        )
        .expect_err("conflicting id must be rejected");
        assert!(err.to_string().contains("does not match"));
    }

    #[test]
    fn unshare_query_percent_encodes_every_value() {
        let q = build_connection_unshare_query("44865", "113168", ShareSubjectType::Person);
        assert_eq!(
            q,
            "/v4/connections/share?connectionId=44865&subjectId=113168&subjectType=person"
        );

        // An id containing & or = must not be able to forge extra parameters.
        let hostile = build_connection_unshare_query("1&x=2", "3=4", ShareSubjectType::Group);
        assert_eq!(
            hostile,
            "/v4/connections/share?connectionId=1%26x%3D2&subjectId=3%3D4&subjectType=group"
        );
    }

    #[test]
    fn shared_subject_lookup_matches_people_and_groups_by_either_id_key() {
        let subjects = json!({
            "people": [
                { "subjectId": 646, "id": 646, "policyTag": "connection_author" },
                { "subjectId": 113168, "id": 113168, "policyTag": "connection_author" }
            ],
            "groups": [ { "id": 900, "name": "analysts" } ]
        });

        let person = find_shared_subject(&subjects, "113168").expect("person found");
        assert_eq!(person["subjectId"], 113168);
        assert_eq!(person["subjectBucket"], "people");

        let group = find_shared_subject(&subjects, "900").expect("group found");
        assert_eq!(group["subjectBucket"], "groups");

        assert!(find_shared_subject(&subjects, "999").is_none());
    }

    #[test]
    fn shared_subject_lookup_tolerates_missing_buckets() {
        assert!(find_shared_subject(&json!({}), "1").is_none());
        assert!(find_shared_subject(&serde_json::Value::Null, "1").is_none());
        assert!(find_shared_subject(&json!({ "people": "not-an-array" }), "1").is_none());
    }
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
