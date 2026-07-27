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

use anyhow::Result;
use ayx_core::envelope::{Envelope, ErrorCode};
use ayx_one_api::{OneListParams, one_api_list_request, one_api_live_request_with_body};
use serde_json::{Value, json};

use crate::{
    OneWorkflowsCommand,
    cmd::{self, RuntimeCtx},
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
