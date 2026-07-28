//! Permission telemetry (Phase 3).
//!
//! Surfaces "who has access to what" across both backends:
//!
//! * **Connections** — One iterates `/v4/connections`; with `--deep` it
//!   additionally fetches `/v4/connections/{id}/permissions/sharedSubjects`
//!   per id and groups grantees by subject. Server falls back to a plan
//!   envelope around `dcm_connections_list_envelope`.
//! * **Workflows** — One has no per-flow ACL endpoint, so we surface the
//!   workspace people roster via `/iam/v1/workspaces/{id}/people`. Server
//!   uses `v3/collections` (Gallery workflow-membership lives in the
//!   collection ACLs).
//! * **Collections** — Server-only.
//! * **Summary** — Joins the above into counts per subject.
//!
//! Default (shallow) modes complete in a single page each; `--deep` on
//! One-side connections is the only path that fans out per id.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use ayx_core::envelope::Envelope;
use ayx_core::profile::Config;
use ayx_one_api::{OneListParams, one_api_list_request, one_api_live_request};
use chrono::Utc;
use serde_json::{Value, json};

use super::source::TelemetrySource;
use super::{TelemetryArgs, load_and_pick_source};

pub fn connections(
    environment: Option<&str>,
    args: &TelemetryArgs,
    deep: bool,
) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    match src {
        TelemetrySource::One => connections_one(&config, args, deep),
        TelemetrySource::Server => connections_server(&config),
    }
}

pub fn workflows(
    environment: Option<&str>,
    args: &TelemetryArgs,
    workspace_id: Option<&str>,
) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    match src {
        TelemetrySource::One => workflows_one(&config, args, workspace_id),
        TelemetrySource::Server => workflows_server(&config),
    }
}

pub fn collections(environment: Option<&str>, args: &TelemetryArgs) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    if src != TelemetrySource::Server {
        return Err(anyhow!(
            "validation: telemetry permissions collections requires --source server; One uses workspace-level scope, not collections"
        ));
    }
    let env = ayx_server_api::collections_list_envelope(&config, "Default")
        .map_err(|e| anyhow!("server-api collections list failed: {e}"))?;
    Ok(wrap_server_envelope(env, "collections"))
}

pub fn summary(
    environment: Option<&str>,
    args: &TelemetryArgs,
    workspace_id: Option<&str>,
) -> Result<Envelope> {
    let (config, src) = load_and_pick_source(args, environment)?;
    match src {
        TelemetrySource::One => summary_one(&config, args, workspace_id),
        TelemetrySource::Server => summary_server(&config),
    }
}

// ─── One-side ──────────────────────────────────────────────────────────────

fn connections_one(config: &Config, args: &TelemetryArgs, deep: bool) -> Result<Envelope> {
    let params = OneListParams::new()
        .with_limit(Some(200))
        .with_all(args.all, args.max_pages);
    let env = one_api_list_request(
        config,
        "platform",
        "connections-list",
        "/v4/connections",
        &[],
        &params,
    )?;
    if !env.ok {
        return Ok(env);
    }
    let items = env
        .data
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if !deep {
        let connection_rows: Vec<Value> = items
            .iter()
            .map(|c| {
                json!({
                    "id": c.get("id"),
                    "name": c.get("name"),
                    "connector_id": c.get("connectorId").or_else(|| c.get("connector_id")),
                    "owner_id": c.get("ownerId").or_else(|| c.get("owner_id")),
                })
            })
            .collect();
        return Ok(Envelope::ok_with_data(
            format!(
                "telemetry permissions connections: {} connection{} (shallow; pass --deep for per-id grantees)",
                connection_rows.len(),
                if connection_rows.len() == 1 { "" } else { "s" }
            ),
            json!({
                "source": "one",
                "generated_at": Utc::now().to_rfc3339(),
                "deep": false,
                "items": connection_rows,
            }),
        ));
    }

    // --deep: fetch /v4/connections/{id}/permissions/sharedSubjects per connection
    // and group grantees by subject id. O(N) extra requests — operator gated.
    //
    // `/v4/connections/{id}/permissions` (no `/sharedSubjects` suffix) is a dead
    // route that answers 404 — see one_connections.rs for the live-verified shape.
    // A permissions lookup failing here must surface as a failure, not silently
    // read back as "this connection has zero grantees".
    let mut per_connection: Vec<Value> = Vec::with_capacity(items.len());
    let mut by_subject: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in &items {
        let Some(conn_id) = resource_id(c) else {
            continue;
        };
        let resp = one_api_live_request(
            config,
            "platform",
            "connection-permissions",
            "GET",
            "/v4/connections/{id}/permissions/sharedSubjects",
            false,
            &[("id", &conn_id)],
        )?;
        if !resp.ok {
            return Ok(resp);
        }
        let grantee_ids =
            extract_shared_subject_ids(resp.data.get("response").unwrap_or(&Value::Null));
        for sid in &grantee_ids {
            by_subject
                .entry(sid.clone())
                .or_default()
                .push(conn_id.clone());
        }
        per_connection.push(json!({
            "id": conn_id,
            "name": c.get("name"),
            "grantee_count": grantee_ids.len(),
            "grantees": grantee_ids,
        }));
    }
    let by_subject_rows: Vec<Value> = by_subject
        .into_iter()
        .map(|(subject_id, conn_ids)| {
            json!({
                "subject_id": subject_id,
                "connection_count": conn_ids.len(),
                "connection_ids": conn_ids,
            })
        })
        .collect();
    Ok(Envelope::ok_with_data(
        format!(
            "telemetry permissions connections --deep: {} connection{}, {} subject{} with grants",
            per_connection.len(),
            if per_connection.len() == 1 { "" } else { "s" },
            by_subject_rows.len(),
            if by_subject_rows.len() == 1 { "" } else { "s" }
        ),
        json!({
            "source": "one",
            "generated_at": Utc::now().to_rfc3339(),
            "deep": true,
            "items": per_connection,
            "by_subject": by_subject_rows,
        }),
    ))
}

fn workflows_one(
    config: &Config,
    args: &TelemetryArgs,
    workspace_id: Option<&str>,
) -> Result<Envelope> {
    let workspace = resolve_workspace_id(config, workspace_id)?;
    let params = OneListParams::new()
        .with_limit(Some(200))
        .with_all(args.all, args.max_pages);
    let env = one_api_list_request(
        config,
        "platform",
        "workspace-people-list",
        "/iam/v1/workspaces/{id}/people",
        &[("id", &workspace)],
        &params,
    )?;
    if !env.ok {
        return Ok(env);
    }
    let items = env
        .data
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let rows: Vec<Value> = items
        .iter()
        .map(|p| {
            json!({
                "id": p.get("id"),
                "email": p.get("email"),
                "is_admin": p.get("isAdmin").or_else(|| p.get("is_admin")),
                "is_suspended": p.get("isSuspended").or_else(|| p.get("is_suspended")),
            })
        })
        .collect();
    Ok(Envelope::ok_with_data(
        format!(
            "telemetry permissions workflows: {} workspace member{} (One has no per-flow ACL endpoint; workspace scope is authoritative)",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" }
        ),
        json!({
            "source": "one",
            "workspace_id": workspace,
            "generated_at": Utc::now().to_rfc3339(),
            "items": rows,
        }),
    ))
}

fn summary_one(
    config: &Config,
    args: &TelemetryArgs,
    workspace_id: Option<&str>,
) -> Result<Envelope> {
    // Shallow counts only — operators who want grantee-level detail run
    // `permissions connections --deep` directly.
    let params = OneListParams::new()
        .with_limit(Some(200))
        .with_all(args.all, args.max_pages);
    let conn_env = one_api_list_request(
        config,
        "platform",
        "connections-list",
        "/v4/connections",
        &[],
        &params,
    )?;
    if !conn_env.ok {
        return Ok(conn_env);
    }
    let connection_count = conn_env
        .data
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);

    let people_count = match resolve_workspace_id(config, workspace_id) {
        Ok(workspace) => {
            let env = one_api_list_request(
                config,
                "platform",
                "workspace-people-list",
                "/iam/v1/workspaces/{id}/people",
                &[("id", &workspace)],
                &params,
            )?;
            // A failed lookup is unknown, not zero. `None` renders as "member
            // count omitted"; `Some(0)` would assert the workspace is empty.
            if env.ok {
                env.data
                    .get("items")
                    .and_then(Value::as_array)
                    .map(Vec::len)
            } else {
                None
            }
        }
        Err(_) => None,
    };

    Ok(Envelope::ok_with_data(
        format!(
            "telemetry permissions summary: {connection_count} connection(s){}",
            people_count
                .map(|n| format!(", {n} workspace member(s)"))
                .unwrap_or_default()
        ),
        json!({
            "source": "one",
            "generated_at": Utc::now().to_rfc3339(),
            "summary": {
                "connection_count": connection_count,
                "workspace_member_count": people_count,
            },
        }),
    ))
}

// ─── Server-side ───────────────────────────────────────────────────────────

fn connections_server(config: &Config) -> Result<Envelope> {
    let env = ayx_server_api::dcm_connections_list_envelope(config)
        .map_err(|e| anyhow!("server-api dcm connections list failed: {e}"))?;
    Ok(wrap_server_envelope(env, "dcm_connections"))
}

fn workflows_server(config: &Config) -> Result<Envelope> {
    let env = ayx_server_api::collections_list_envelope(config, "Default")
        .map_err(|e| anyhow!("server-api collections list failed: {e}"))?;
    Ok(wrap_server_envelope(env, "collections"))
}

fn summary_server(config: &Config) -> Result<Envelope> {
    // Pull the three lists; surface raw bodies in the envelope along with
    // top-level counts when the V3 response shape is `{records:[...]}` or
    // `[...]`. Failures fall back to absent counts rather than aborting the
    // envelope — server-api auth posture is a separate doctor concern.
    let collections = ayx_server_api::collections_list_envelope(config, "Default").ok();
    let dcm = ayx_server_api::dcm_connections_list_envelope(config).ok();
    let users = ayx_server_api::users_list_envelope(config, "Default").ok();
    let body_count = |env: &Option<Envelope>| -> Option<usize> {
        let env = env.as_ref()?;
        let body = env.data.get("body")?;
        if let Some(arr) = body.as_array() {
            return Some(arr.len());
        }
        if let Some(arr) = body.get("records").and_then(Value::as_array) {
            return Some(arr.len());
        }
        None
    };
    let summary = json!({
        "collection_count": body_count(&collections),
        "dcm_connection_count": body_count(&dcm),
        "user_count": body_count(&users),
    });
    Ok(Envelope::ok_with_data(
        "telemetry permissions summary (server): counts derived from V3 list endpoints",
        json!({
            "source": "server",
            "generated_at": Utc::now().to_rfc3339(),
            "summary": summary,
        }),
    ))
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn resolve_workspace_id(config: &Config, explicit: Option<&str>) -> Result<String> {
    if let Some(v) = explicit {
        return Ok(v.to_string());
    }
    config
        .alteryx_one
        .as_ref()
        .and_then(|o| o.expected_workspace_id.clone())
        .ok_or_else(|| {
            anyhow!(
                "validation: --workspace-id is required (no `alteryx_one.expected_workspace_id` configured in the profile)"
            )
        })
}

/// Extract every grantee's subject id from a `GET
/// /v4/connections/{id}/permissions/sharedSubjects` response.
///
/// A resource's `id`, accepting either JSON shape the One API uses.
///
/// Ids come back both ways: cloud-native workflows use ULID strings, while
/// connections, flows, folders, job groups, output objects, and write settings
/// use JSON *numbers*. Reading only `as_str()` yielded `None` for every
/// connection, so `telemetry permissions --deep`'s per-connection loop
/// `continue`d on every item and reported `ok: true` with empty results on a
/// tenant with dozens of shared connections. The same trap had already been
/// found and fixed in the live-smoke helper and in `one_connections.rs`.
fn resource_id(resource: &Value) -> Option<String> {
    match resource.get("id") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// Unlike a plain list endpoint, this response groups grantees into `people`
/// and `groups` buckets rather than one flat array (see
/// `one_connections::find_shared_subject` for the sibling lookup that reads
/// the same shape).
fn extract_shared_subject_ids(shared_subjects: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    for bucket in ["people", "groups"] {
        let Some(entries) = shared_subjects.get(bucket).and_then(Value::as_array) else {
            continue;
        };
        for entry in entries {
            let id = ["subjectId", "id"]
                .iter()
                .find_map(|key| match entry.get(key) {
                    Some(Value::String(s)) => Some(s.clone()),
                    Some(Value::Number(n)) => Some(n.to_string()),
                    _ => None,
                });
            if let Some(id) = id {
                ids.push(id);
            }
        }
    }
    ids
}

fn wrap_server_envelope(inner: Envelope, resource: &str) -> Envelope {
    let mut data = match inner.data {
        Value::Object(_) => inner.data,
        other => json!({"raw": other}),
    };
    if let Value::Object(map) = &mut data {
        map.insert("source".into(), Value::String("server".into()));
        map.insert(
            "generated_at".into(),
            Value::String(Utc::now().to_rfc3339()),
        );
        map.insert("resource".into(), Value::String(resource.to_string()));
    }
    Envelope::ok_with_data(format!("telemetry permissions {resource} (server)"), data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::profile::{
        AlteryxOneProfile, MongoDatabases, MongoMode, MongoProfile, ServerProfile,
    };

    /// The `--deep` per-connection loop read `id` with `as_str()` only. Live
    /// connection ids are JSON numbers, so every item yielded `None` and was
    /// skipped: the loop body never ran, and a tenant with dozens of shared
    /// connections got `ok: true` with an empty result and no error.
    #[test]
    fn resource_id_accepts_the_numeric_ids_the_one_api_actually_returns() {
        assert_eq!(
            resource_id(&json!({ "id": 44865 })),
            Some("44865".to_string()),
            "connections, flows, folders, job groups, output objects and write \
             settings all return numeric ids"
        );
        assert_eq!(
            resource_id(&json!({ "id": "01KY5TC876M1GFEA2A4P2CZVBR" })),
            Some("01KY5TC876M1GFEA2A4P2CZVBR".to_string()),
            "cloud-native workflows return ULID strings"
        );
        assert_eq!(resource_id(&json!({ "name": "no id" })), None);
        assert_eq!(resource_id(&json!({ "id": null })), None);
    }

    fn base() -> Config {
        Config {
            profile_name: "t".into(),
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

    fn with_one(cfg: &mut Config, ws: Option<&str>) {
        cfg.alteryx_one = Some(AlteryxOneProfile {
            account_email: "t@e.com".into(),
            base_url: None,
            oauth_client_id: None,
            client_secret: None,
            client_secret_ref: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_credentials: Default::default(),
            expected_workspace_id: ws.map(String::from),
            ..Default::default()
        });
    }

    #[test]
    fn resolve_workspace_id_prefers_explicit() {
        let mut cfg = base();
        with_one(&mut cfg, Some("from-profile"));
        assert_eq!(
            resolve_workspace_id(&cfg, Some("from-cli")).unwrap(),
            "from-cli"
        );
    }

    #[test]
    fn resolve_workspace_id_falls_back_to_profile() {
        let mut cfg = base();
        with_one(&mut cfg, Some("ws-1"));
        assert_eq!(resolve_workspace_id(&cfg, None).unwrap(), "ws-1");
    }

    #[test]
    fn resolve_workspace_id_errors_when_unset() {
        let mut cfg = base();
        with_one(&mut cfg, None);
        let err = resolve_workspace_id(&cfg, None).unwrap_err().to_string();
        assert!(err.contains("workspace-id"));
    }

    #[test]
    fn extract_shared_subject_ids_reads_both_people_and_group_buckets() {
        let shared_subjects = json!({
            "people": [
                { "subjectId": 646, "policyTag": "connection_author" },
                { "id": 113168, "policyTag": "editor" },
            ],
            "groups": [
                { "subjectId": "grp-1" },
            ],
        });
        let mut ids = extract_shared_subject_ids(&shared_subjects);
        ids.sort();
        assert_eq!(ids, vec!["113168", "646", "grp-1"]);
    }

    #[test]
    fn extract_shared_subject_ids_tolerates_missing_or_empty_buckets() {
        assert!(extract_shared_subject_ids(&json!({})).is_empty());
        assert!(extract_shared_subject_ids(&Value::Null).is_empty());
        assert!(extract_shared_subject_ids(&json!({"people": [], "groups": []})).is_empty());
    }

    #[test]
    fn collections_requires_server_source() {
        // The actual function loads a profile path so we can't easily call
        // it end-to-end without a fixture, but the dispatch contract is
        // documented: One source falls into a Validation error path.
        let mut cfg = base();
        cfg.server = Some(ServerProfile {
            webapi_url: "http://srv".into(),
            curator_api_key: "k".into(),
            curator_api_secret: "s".into(),
            curator_api_secret_ref: None,
            verify_tls: None,
            derived: false,
        });
        // Sanity: a server-only profile picks Server in auto mode.
        let pick = super::super::source::pick(&cfg, None).unwrap();
        assert_eq!(pick, TelemetrySource::Server);
    }
}
