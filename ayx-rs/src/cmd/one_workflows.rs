//! Alteryx One cloud-native workflows.
//!
//! These are NOT `one flows`. `one flows` is the Designer Cloud (Trifacta-derived)
//! `/v4/flows` family, keyed by integer ids. Cloud-native workflows are the
//! Alteryx One canvas product (the `cloud-native/workflows/{id}` web path), keyed by
//! ULIDs, and live on a separate service: `/svc-workflow/api/vN`. A workspace can
//! hold 85 cloud-native workflows while `GET /v4/flows` returns zero items.
//!
//! Two quirks of that service shape this module, both live-verified 2026-07-26:
//!
//!   - There is no `GET /v4/workflows/{id}` and no `/v4/workflows/count`; both answer
//!     `RouteNotFoundException`. `detail` and `count` are therefore synthesized from
//!     the list endpoints, and say so via `detail_source` so a caller can tell
//!     client-side assembly from a server lookup.
//!   - The service answers unknown routes with an Express HTML page rather than the
//!     `/v4` gateway's JSON `RouteNotFoundException`. The transport classifies that as
//!     `response_kind: "html"` and attaches a hint.

use anyhow::{Result, bail};
use ayx_core::envelope::{Envelope, ErrorCode};
use ayx_one_api::{
    OneListParams, one_api_list_request, one_api_live_request, one_api_live_request_with_body,
};
use serde_json::{Value, json};

use crate::{
    OneWorkflowsCommand, WorkflowPrivilege,
    cmd::{self, RuntimeCtx},
    load_payload,
};

/// `GET /v4/workflows` is the only route that lists cloud-native workflows; the
/// richer `assets` view lives on the workflow service itself.
const WORKFLOWS_LIST_ENDPOINT: &str = "/v4/workflows";
const ASSETS_LIST_ENDPOINT: &str = "/svc-workflow/api/v1/assets";

/// Why a synthesized leaf is synthesized. Emitted as `detail_source` so an agent
/// parsing the envelope can distinguish this from a real server-side route.
const DETAIL_SOURCE: &str = "synthesized client-side from GET /svc-workflow/api/v1/assets; \
                             the API has no GET /v4/workflows/{id} route";
const COUNT_SOURCE: &str = "synthesized client-side from the GET /v4/workflows envelope; \
                            the API has no GET /v4/workflows/count route";

/// Locate one workflow in a list response by exact id.
///
/// ULIDs are case-sensitive and fixed-length, so this is an exact match — a
/// case-insensitive or prefix match would risk returning the wrong workflow.
pub(crate) fn find_workflow_asset(items: &[Value], id: &str) -> Option<Value> {
    items
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(id))
        .cloned()
}

/// Total workflow count from a raw `GET /v4/workflows` response body.
///
/// The response carries a top-level `count` covering the whole collection, not just
/// the returned page — so a `?limit=1` request is enough to answer. Fall back to the
/// length of the returned `data` array if that field ever disappears, and label which
/// source was used rather than silently reporting a page size as a total.
pub(crate) fn synthesize_workflow_count(response: &Value) -> (u64, &'static str) {
    if let Some(count) = response.get("count").and_then(Value::as_u64) {
        return (count, "server");
    }
    let fetched = response
        .get("data")
        .and_then(Value::as_array)
        .map(|items| items.len() as u64)
        .unwrap_or(0);
    (fetched, "returned-items")
}

/// Fetch every workflow asset, following pages up to the standard cap.
fn fetch_all_assets(config: &ayx_core::profile::Config) -> Result<Envelope> {
    let params = OneListParams::new()
        .with_limit(Some(200))
        .with_all(true, Some(50));
    one_api_list_request(
        config,
        "workflow",
        "assets",
        ASSETS_LIST_ENDPOINT,
        &[],
        &params,
    )
}

/// Body for `POST /svc-workflow/api/v2/workflows/{id}/share`.
///
/// Recovered live from the service's own schema-validation errors — this shape
/// is not in any published spec. `includeDependencies`, `privileges`, and
/// `sendEmail` are all REQUIRED, even when their value is `false`/empty-looking;
/// `toPersonIds`/`toGroupIds` need at least one entry between them;
/// `additionalInfoMsg` must be omitted (not `null`) when the caller supplies no
/// message.
///
/// Pure data transformation, no I/O, so it is unit-tested without a live call.
/// Person/group ids must already be resolved to integers by the caller (see
/// `resolve_person_ids`) — this function does no email resolution, which is what
/// lets it run identically for both a dry run and a `--apply` call.
pub(crate) fn build_workflow_share_body(
    include_dependencies: bool,
    privileges: &[WorkflowPrivilege],
    send_email: bool,
    to_person_ids: &[u64],
    to_group_ids: &[u64],
    additional_info_msg: Option<&str>,
) -> Result<Value> {
    if privileges.is_empty() {
        bail!(
            "validation: --privilege is required (at least one of \
             create|delete|execute|read|share|update)"
        );
    }
    if to_person_ids.is_empty() && to_group_ids.is_empty() {
        bail!("validation: no share recipients: pass at least one --to-person or --to-group");
    }

    let mut privilege_strs: Vec<&'static str> = privileges.iter().map(|p| p.as_api_str()).collect();
    privilege_strs.sort_unstable();
    privilege_strs.dedup();

    let mut body = json!({
        // Present even when false/empty: the service's schema validator
        // rejects the request outright if any of these three keys is absent.
        "includeDependencies": include_dependencies,
        "privileges": privilege_strs,
        "sendEmail": send_email,
        "toPersonIds": to_person_ids,
        "toGroupIds": to_group_ids,
    });
    if let Some(msg) = additional_info_msg {
        body["additionalInfoMsg"] = json!(msg);
    }
    Ok(body)
}

/// One `--to-person` value, classified before resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShareRecipient {
    /// Already a numeric person id — passes straight into `toPersonIds`.
    PersonId(u64),
    /// An email address that must be resolved against `GET /v4/people`.
    Email(String),
}

/// Classify a raw `--to-person` value. A trimmed value that is entirely ASCII
/// digits is a numeric person id; anything else is treated as an email address
/// for `resolve_person_ids` to look up.
pub(crate) fn classify_share_recipient(raw: &str) -> ShareRecipient {
    let trimmed = raw.trim();
    match trimmed.parse::<u64>() {
        Ok(id) => ShareRecipient::PersonId(id),
        Err(_) => ShareRecipient::Email(trimmed.to_string()),
    }
}

/// A `GET /v4/people` item's `id` field, whichever JSON shape the server sends.
fn person_id_as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
}

/// Resolve every `--to-person` value to a numeric person id, using a single
/// already-fetched `GET /v4/people` listing (`people`) rather than one lookup
/// per recipient — N recipients cost one network call, not N. Numeric ids pass
/// through unchanged.
///
/// Matching is case-insensitive and tolerant of surrounding whitespace on both
/// sides (the input, via `classify_share_recipient`, and the server's `email`
/// field, here). An email matching more than one person counts as unresolved —
/// silently picking one would be a security-relevant guess about who gets
/// access — and every unresolved address is named in the returned error so the
/// caller knows exactly which one(s) to fix.
pub(crate) fn resolve_person_ids(recipients: &[String], people: &[Value]) -> Result<Vec<u64>> {
    let mut resolved = Vec::with_capacity(recipients.len());
    let mut unresolved: Vec<String> = Vec::new();

    for raw in recipients {
        match classify_share_recipient(raw) {
            ShareRecipient::PersonId(id) => resolved.push(id),
            ShareRecipient::Email(email) => {
                let needle = email.to_ascii_lowercase();
                let matches: Vec<u64> = people
                    .iter()
                    .filter(|person| {
                        person
                            .get("email")
                            .and_then(Value::as_str)
                            .map(|e| e.trim().to_ascii_lowercase() == needle)
                            .unwrap_or(false)
                    })
                    .filter_map(|person| person.get("id").and_then(person_id_as_u64))
                    .collect();
                match matches.as_slice() {
                    [id] => resolved.push(*id),
                    [] => unresolved.push(email),
                    _ => unresolved.push(format!("{email} (ambiguous: {} matches)", matches.len())),
                }
            }
        }
    }

    if !unresolved.is_empty() {
        bail!(
            "validation: could not resolve --to-person address(es) to a person id: {}",
            unresolved.join(", ")
        );
    }
    Ok(resolved)
}

/// Every `--to-group` (or a `--no-resolve-emails` `--to-person`) value must
/// already be a numeric id — neither has an email-style lookup.
fn parse_numeric_ids(flag: &str, raw: &[String]) -> Result<Vec<u64>> {
    raw.iter()
        .map(|value| {
            value.trim().parse::<u64>().map_err(|_| {
                anyhow::anyhow!("validation: {flag} values must be numeric ids; '{value}' is not")
            })
        })
        .collect()
}

/// Pull the item array out of a `GET /v4/people` response body, tolerating the
/// shapes seen elsewhere in this API: a bare array, or an object wrapper — the
/// `/v4` gateway uses `data` (confirmed live: `/v4/people` wraps its array as
/// `{"data": [...]}`), `svc-workflow` uses `items`/`assets` (see the module
/// doc comment above).
fn extract_people(response: &Value) -> Vec<Value> {
    if let Some(arr) = response.as_array() {
        return arr.clone();
    }
    for key in ["data", "items", "results", "records", "value", "assets"] {
        if let Some(arr) = response.get(key).and_then(Value::as_array) {
            return arr.clone();
        }
    }
    Vec::new()
}

/// `GET /v4/people`, unpaginated — the one call `share` uses to resolve every
/// `--to-person` email in a single round trip, however many were passed.
fn fetch_people(config: &ayx_core::profile::Config) -> Result<Envelope> {
    one_api_live_request(
        config,
        "workflow",
        "share-people-lookup",
        "GET",
        "/v4/people",
        false,
        &[],
    )
}

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: OneWorkflowsCommand,
) -> Result<Envelope> {
    Ok(match command {
        OneWorkflowsCommand::List {
            profile,
            limit,
            page_token,
            all,
            max_pages,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let params = OneListParams::new()
                .with_limit(limit)
                .with_page_token(page_token)
                .with_all(all, max_pages);
            one_api_list_request(
                &config,
                "workflow",
                "list",
                WORKFLOWS_LIST_ENDPOINT,
                &[],
                &params,
            )?
        }
        OneWorkflowsCommand::Assets {
            profile,
            limit,
            page_token,
            all,
            max_pages,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let params = OneListParams::new()
                .with_limit(limit)
                .with_page_token(page_token)
                .with_all(all, max_pages);
            one_api_list_request(
                &config,
                "workflow",
                "assets",
                ASSETS_LIST_ENDPOINT,
                &[],
                &params,
            )?
        }
        OneWorkflowsCommand::Count { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            // `?limit=1` because the total lives in the envelope, not in the page —
            // no reason to transfer 85 records to count them.
            let envelope = one_api_live_request_with_body(
                &config,
                "workflow",
                "count",
                "GET",
                "/v4/workflows?limit=1",
                false,
                &[],
                None,
            )?;
            if !envelope.ok {
                return Ok(envelope);
            }
            let (count, count_source) =
                synthesize_workflow_count(envelope.data.get("response").unwrap_or(&Value::Null));
            Envelope::ok_with_data(
                format!("workflow count ok ({count})"),
                json!({
                    "surface": "workflow",
                    "operation": "count",
                    "count": count,
                    "count_source": count_source,
                    "detail_source": COUNT_SOURCE,
                }),
            )
        }
        OneWorkflowsCommand::Detail {
            profile,
            id,
            include_dependencies,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let envelope = fetch_all_assets(&config)?;
            if !envelope.ok {
                return Ok(envelope);
            }
            let items = envelope
                .data
                .get("items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let Some(workflow) = find_workflow_asset(&items, &id) else {
                return Ok(Envelope::err_coded(
                    ErrorCode::NotFound,
                    format!("workflow detail failed: no workflow with id {id}"),
                    json!({
                        "surface": "workflow",
                        "operation": "detail",
                        "workflow_id": id,
                        "detail_source": DETAIL_SOURCE,
                        "searched_items": items.len(),
                        "response": Value::Null,
                        "error_code": "not_found",
                    }),
                ));
            };

            let mut data = json!({
                "surface": "workflow",
                "operation": "detail",
                "workflow_id": id,
                "detail_source": DETAIL_SOURCE,
                "searched_items": items.len(),
                "response": workflow,
            });
            if include_dependencies {
                let deps = fetch_dependencies(&config, &id)?;
                if !deps.ok {
                    return Ok(deps);
                }
                data["dependencies"] = deps.data.get("response").cloned().unwrap_or(Value::Null);
            }
            Envelope::ok_with_data(format!("workflow detail ok ({id})"), data)
        }
        OneWorkflowsCommand::Dependencies { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            fetch_dependencies(&config, &id)?
        }
        OneWorkflowsCommand::Engines { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request_with_body(
                &config,
                "workflow",
                "engines",
                "GET",
                "/svc-workflow/api/v0/workflows/{id}/availableEngines",
                false,
                &[("id", id.as_str())],
                None,
            )?
        }
        OneWorkflowsCommand::Tools { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request_with_body(
                &config,
                "workflow",
                "tools",
                "GET",
                "/svc-workflow/api/v1/tools",
                false,
                &[],
                None,
            )?
        }
        OneWorkflowsCommand::Copy {
            profile,
            id,
            name,
            version,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            // `version` selects which stored revision to copy. Resolve the current
            // one when the caller does not pin it, so `copy` needs only an id.
            let version = match version {
                Some(version) => version,
                None => resolve_workflow_version(&config, &id)?,
            };
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::destructive_action_message(
                        "create a copy of",
                        &format!("workflow id='{id}' as '{name}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request_with_body(
                &config,
                "workflow",
                "copy",
                "POST",
                "/svc-workflow/api/v2/workflows/{id}/duplicate",
                true,
                &[("id", id.as_str())],
                Some(json!({ "name": name, "version": version })),
            )?
        }
        OneWorkflowsCommand::Share {
            profile,
            id,
            to_person,
            to_group,
            privilege,
            include_dependencies,
            send_email,
            message,
            no_resolve_emails,
            body,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = match body {
                Some(path) => load_payload(&path)?,
                None => {
                    // Resolution runs here, BEFORE the transport's --apply gate
                    // (ayx_one_api::one_api_live_request_with_body). That ordering is
                    // the whole point: a dry run's `would_send` and a later --apply
                    // call end up carrying byte-identical, already-resolved ids.
                    let people = if no_resolve_emails {
                        Vec::new()
                    } else if to_person.iter().any(|raw| {
                        matches!(classify_share_recipient(raw), ShareRecipient::Email(_))
                    }) {
                        // One GET /v4/people call resolves every email at once,
                        // however many recipients were passed.
                        let people_envelope = fetch_people(&config)?;
                        if !people_envelope.ok {
                            return Ok(people_envelope);
                        }
                        extract_people(people_envelope.data.get("response").unwrap_or(&Value::Null))
                    } else {
                        Vec::new()
                    };
                    let to_person_ids = resolve_person_ids(&to_person, &people)?;
                    let to_group_ids = parse_numeric_ids("--to-group", &to_group)?;

                    build_workflow_share_body(
                        include_dependencies,
                        &privilege,
                        send_email,
                        &to_person_ids,
                        &to_group_ids,
                        message.as_deref(),
                    )?
                }
            };

            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "share",
                        &format!("workflow id='{id}'"),
                        &config.profile_name,
                    ),
                )?;
            }

            let mut envelope = one_api_live_request_with_body(
                &config,
                "workflow",
                "share",
                "POST",
                "/svc-workflow/api/v2/workflows/{id}/share",
                true,
                &[("id", id.as_str())],
                Some(payload),
            )?;

            // On a dry run, preview the dependency blast radius so an
            // authorized:false connection or dataset is visible before --apply,
            // not after. A failed preview fetch is surfaced as an explicit
            // failure marker (`dependency_preview_ok: false`), never silently
            // reported as "no dependencies" — the whole point of the preview is
            // to catch exactly that kind of blind spot.
            if include_dependencies
                && envelope.ok
                && envelope.data.get("dry_run").and_then(Value::as_bool) == Some(true)
            {
                let deps = fetch_dependencies(&config, &id)?;
                envelope.data["dependency_preview_ok"] = json!(deps.ok);
                envelope.data["dependency_preview"] = if deps.ok {
                    deps.data.get("response").cloned().unwrap_or(Value::Null)
                } else {
                    deps.data.clone()
                };
            }

            envelope
        }
    })
}

fn fetch_dependencies(config: &ayx_core::profile::Config, id: &str) -> Result<Envelope> {
    one_api_live_request_with_body(
        config,
        "workflow",
        "dependencies",
        "GET",
        "/svc-workflow/api/v1/assets/{id}/dependencies",
        false,
        &[("id", id)],
        None,
    )
}

/// Current stored version of a workflow, for `copy` without an explicit `--version`.
fn resolve_workflow_version(config: &ayx_core::profile::Config, id: &str) -> Result<u64> {
    let envelope = fetch_all_assets(config)?;
    if !envelope.ok {
        anyhow::bail!(
            "could not resolve the current version of workflow {id}: listing workflows failed. \
             Pass --version <N> to skip the lookup."
        );
    }
    let items = envelope
        .data
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(workflow) = find_workflow_asset(&items, id) else {
        anyhow::bail!("no workflow with id {id}");
    };
    workflow
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "workflow {id} has no numeric `version` field; pass --version <N> explicitly"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{find_workflow_asset, synthesize_workflow_count};
    use serde_json::json;

    #[test]
    fn workflow_lookup_is_an_exact_case_sensitive_match() {
        let items = vec![
            json!({ "id": "01KY5TC876M1GFEA2A4P2CZVBR", "name": "land-lease-intel-LQ" }),
            json!({ "id": "01KVWJA412RB8PJ9CTA4BF67SH", "name": "AP_Intel_part1" }),
        ];

        let found = find_workflow_asset(&items, "01KY5TC876M1GFEA2A4P2CZVBR").expect("found");
        assert_eq!(found["name"], "land-lease-intel-LQ");

        assert!(find_workflow_asset(&items, "01ky5tc876m1gfea2a4p2czvbr").is_none());
        assert!(find_workflow_asset(&items, "01KY5TC876").is_none());
        assert!(find_workflow_asset(&items, "nope").is_none());
        assert!(find_workflow_asset(&[], "01KY5TC876M1GFEA2A4P2CZVBR").is_none());
    }

    #[test]
    fn workflow_lookup_tolerates_items_without_an_id() {
        let items = vec![json!({ "name": "no id here" }), json!("not even an object")];
        assert!(find_workflow_asset(&items, "x").is_none());
    }

    #[test]
    fn count_prefers_the_server_total_over_the_returned_page() {
        // The whole point: a ?limit=1 page must still report the collection total,
        // never the page size.
        let response = json!({ "data": [ { "id": "a" } ], "count": 85 });
        assert_eq!(synthesize_workflow_count(&response), (85, "server"));
    }

    #[test]
    fn count_falls_back_to_returned_items_when_the_server_total_is_absent() {
        let response = json!({ "data": [ { "id": "a" }, { "id": "b" } ] });
        assert_eq!(synthesize_workflow_count(&response), (2, "returned-items"));

        assert_eq!(
            synthesize_workflow_count(&json!({ "data": [] })),
            (0, "returned-items")
        );
        assert_eq!(synthesize_workflow_count(&json!({})), (0, "returned-items"));
        assert_eq!(
            synthesize_workflow_count(&serde_json::Value::Null),
            (0, "returned-items")
        );
    }
}

#[cfg(test)]
mod share_tests {
    use super::{
        ShareRecipient, build_workflow_share_body, classify_share_recipient, extract_people,
        resolve_person_ids,
    };
    use crate::WorkflowPrivilege;
    use serde_json::json;

    #[test]
    fn share_body_carries_the_required_keys_even_when_false() {
        let body = build_workflow_share_body(
            false,
            &[WorkflowPrivilege::Read],
            false,
            &[113168],
            &[],
            None,
        )
        .expect("valid share");

        // Present with their real (false) value, not omitted.
        assert_eq!(body["includeDependencies"], json!(false));
        assert_eq!(body["sendEmail"], json!(false));
        assert_eq!(body["privileges"], json!(["read"]));
        assert_eq!(body["toPersonIds"], json!([113168]));
        assert_eq!(body["toGroupIds"], json!([]));
    }

    #[test]
    fn share_body_omits_additional_info_msg_when_absent_rather_than_nulling_it() {
        let body =
            build_workflow_share_body(true, &[WorkflowPrivilege::Read], true, &[1], &[], None)
                .expect("valid share");

        assert!(
            body.get("additionalInfoMsg").is_none(),
            "additionalInfoMsg must be omitted, not present as null: {body}"
        );
    }

    #[test]
    fn share_body_includes_additional_info_msg_when_present() {
        let body = build_workflow_share_body(
            true,
            &[WorkflowPrivilege::Read],
            true,
            &[1],
            &[],
            Some("please review"),
        )
        .expect("valid share");

        assert_eq!(body["additionalInfoMsg"], json!("please review"));
    }

    #[test]
    fn share_body_rejects_empty_privileges_before_any_network_call() {
        let err = build_workflow_share_body(false, &[], false, &[1], &[], None)
            .expect_err("empty privileges must fail");
        let msg = err.to_string();
        assert!(msg.contains("--privilege"), "actionable message: {msg}");
        assert_eq!(
            crate::classify_anyhow_error(&err),
            ayx_core::envelope::ErrorCode::Validation
        );
    }

    #[test]
    fn share_body_rejects_empty_recipients_before_any_network_call() {
        let err =
            build_workflow_share_body(false, &[WorkflowPrivilege::Read], false, &[], &[], None)
                .expect_err("empty recipients must fail");
        let msg = err.to_string();
        assert!(msg.contains("--to-person"), "actionable message: {msg}");
        assert!(msg.contains("--to-group"), "actionable message: {msg}");
        assert_eq!(
            crate::classify_anyhow_error(&err),
            ayx_core::envelope::ErrorCode::Validation
        );
    }

    #[test]
    fn share_body_dedupes_and_sorts_privileges() {
        let body = build_workflow_share_body(
            false,
            &[
                WorkflowPrivilege::Update,
                WorkflowPrivilege::Read,
                WorkflowPrivilege::Read,
                WorkflowPrivilege::Create,
            ],
            false,
            &[1],
            &[],
            None,
        )
        .expect("valid share");

        assert_eq!(body["privileges"], json!(["create", "read", "update"]));
    }

    #[test]
    fn classify_recognizes_numeric_ids_and_emails() {
        assert_eq!(
            classify_share_recipient("113168"),
            ShareRecipient::PersonId(113168)
        );
        assert_eq!(
            classify_share_recipient("  113168  "),
            ShareRecipient::PersonId(113168)
        );
        assert_eq!(
            classify_share_recipient("person.name@example.com"),
            ShareRecipient::Email("person.name@example.com".to_string())
        );
        // Not purely digits (leading zero-x, punctuation) -> email bucket, so a
        // malformed id still gets a resolution attempt (and a clear "unresolved"
        // error) rather than silently truncating to a wrong numeric parse.
        assert_eq!(
            classify_share_recipient("113168x"),
            ShareRecipient::Email("113168x".to_string())
        );
    }

    #[test]
    fn resolve_person_ids_passes_through_numeric_ids_without_a_people_list() {
        let resolved =
            resolve_person_ids(&["113168".to_string(), " 646 ".to_string()], &[]).expect("ok");
        assert_eq!(resolved, vec![113168, 646]);
    }

    #[test]
    fn resolve_person_ids_matches_email_case_insensitively_and_trims_whitespace() {
        let people = vec![json!({ "id": 500, "email": "Person.Name@Example.com" })];
        let resolved =
            resolve_person_ids(&["  person.name@example.com  ".to_string()], &people).expect("ok");
        assert_eq!(resolved, vec![500]);
    }

    #[test]
    fn resolve_person_ids_accepts_numeric_or_string_ids_from_the_server() {
        let people = vec![json!({ "id": "500", "email": "a@b.com" })];
        let resolved = resolve_person_ids(&["a@b.com".to_string()], &people).expect("ok");
        assert_eq!(resolved, vec![500]);
    }

    #[test]
    fn resolve_person_ids_reports_an_ambiguous_match_as_unresolved() {
        let people = vec![
            json!({ "id": 1, "email": "dup@example.com" }),
            json!({ "id": 2, "email": "dup@example.com" }),
        ];
        let err = resolve_person_ids(&["dup@example.com".to_string()], &people)
            .expect_err("ambiguous match must fail, not silently pick one");
        assert!(err.to_string().contains("dup@example.com"));
    }

    #[test]
    fn resolve_person_ids_names_every_unresolved_address() {
        let people = vec![json!({ "id": 1, "email": "known@example.com" })];
        let err = resolve_person_ids(
            &[
                "known@example.com".to_string(),
                "ghost1@example.com".to_string(),
                "ghost2@example.com".to_string(),
            ],
            &people,
        )
        .expect_err("unresolved addresses must fail");
        let msg = err.to_string();
        assert!(msg.contains("ghost1@example.com"), "names ghost1: {msg}");
        assert!(msg.contains("ghost2@example.com"), "names ghost2: {msg}");
        assert!(
            !msg.contains("known@example.com"),
            "must not blame the address that DID resolve: {msg}"
        );
        assert_eq!(
            crate::classify_anyhow_error(&err),
            ayx_core::envelope::ErrorCode::Validation
        );
    }

    /// Live-verified 2026-07-27: `GET /v4/people`'s raw body wraps its array
    /// under `data`, matching this module's own doc comment ("the `/v4`
    /// gateway uses `data`") — NOT `items`. A first cut of this function only
    /// checked `items` and silently resolved zero people against a real tenant.
    #[test]
    fn extract_people_reads_the_v4_gateway_data_wrapper() {
        let response = json!({ "data": [ { "id": 1, "email": "a@b.com" } ] });
        let people = extract_people(&response);
        assert_eq!(people.len(), 1);
        assert_eq!(people[0]["email"], json!("a@b.com"));
    }

    #[test]
    fn extract_people_also_tolerates_an_items_wrapper_or_bare_array() {
        assert_eq!(
            extract_people(&json!({ "items": [ { "id": 1 } ] })).len(),
            1
        );
        assert_eq!(extract_people(&json!([ { "id": 1 } ])).len(), 1);
        assert_eq!(extract_people(&json!({})).len(), 0);
        assert_eq!(extract_people(&serde_json::Value::Null).len(), 0);
    }
}
