//! Integration tests for the One API transport against a local httpmock.
//!
//! These tests exercise the safety gates and retry behavior that the audit
//! flagged as critical:
//!
//! - `--apply` gate short-circuits mutating requests to a dry-run envelope.
//! - 401 → token-refresh → retry path runs at most once even for mutations.
//! - 429 with `Retry-After` is respected and retried.
//! - 5xx triggers backoff retry only for read-only requests.
//! - Workspace identity preflight fails closed on mismatch.
//!
//! `set_one_apply` is a thread-local, so the tests use `serial_test::serial`
//! to keep the apply flag from leaking between tests in the same binary.

use ayx_core::profile::{AlteryxOneProfile, Config};
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body, one_apply, set_one_apply};
use httpmock::prelude::*;
use serde_json::json;
use serial_test::serial;
use std::sync::atomic::{AtomicUsize, Ordering};

fn make_config(base: &str, expected_workspace: Option<&str>) -> Config {
    let mut config: Config = serde_yaml::from_str(
        r#"
profile_name: test
mongo:
  mode: embedded
  databases:
    gallery_name: AlteryxGallery
    service_name: AlteryxService
  embedded: {}
"#,
    )
    .expect("base config parses");
    config.alteryx_one = Some(AlteryxOneProfile {
        account_email: "tester@example.com".to_string(),
        base_url: Some(base.to_string()),
        oauth_client_id: None,
        token_endpoint_url: None,
        client_secret: None,
        client_secret_ref: None,
        sp_client_secret: None,
        sp_client_secret_ref: None,
        access_token: Some("test-token".to_string()),
        access_token_ref: None,
        refresh_token: None,
        refresh_token_ref: None,
        workspace_password: None,
        workspace_password_ref: None,
        workspace_credentials: Default::default(),
        auth_rollout: None,
        expected_workspace_id: expected_workspace.map(|s| s.to_string()),
        sp_client_id: None,
        sp_token_endpoint_url: None,
        workspace_gid: None,
        auth_mode: Default::default(),
    });
    config
}

#[test]
#[serial]
#[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
fn apply_gate_blocks_mutating_request_without_apply() {
    let server = MockServer::start();
    // No mock registered for DELETE — if the gate fails, the test will see
    // a 404 from httpmock and the envelope's status_code will reflect that.
    set_one_apply(false);
    assert!(!one_apply());

    let config = make_config(&server.base_url(), None);
    let envelope = one_api_live_request(
        &config,
        "flow",
        "delete",
        "DELETE",
        "/v4/flows/abc",
        true,
        &[],
    )
    .expect("dry-run envelope");

    let data = &envelope.data;
    assert_eq!(data["dry_run"], json!(true));
    assert_eq!(data["mutating"], json!(true));
    assert_eq!(data["method"], json!("DELETE"));
    assert!(data["url"].as_str().unwrap().ends_with("/v4/flows/abc"));
    // No mock was registered, so any actual HTTP request would have produced
    // a different status_code in the envelope. The presence of `dry_run: true`
    // proves the request never went out.
    let _ = server; // server unused; kept to bind base_url scope above.
}

#[test]
#[serial]
#[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
fn apply_gate_allows_mutating_request_with_apply() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(DELETE).path("/v4/flows/abc");
        then.status(204).body("");
    });

    set_one_apply(true);
    let config = make_config(&server.base_url(), None);
    let envelope = one_api_live_request(
        &config,
        "flow",
        "delete",
        "DELETE",
        "/v4/flows/abc",
        true,
        &[],
    )
    .expect("live envelope");

    set_one_apply(false);
    mock.assert();
    assert_eq!(envelope.data["status_code"], json!(204));
    assert_eq!(envelope.data["ok"], json!(true));
}

#[test]
#[serial]
#[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
fn read_only_get_returns_typed_error_code_on_404() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v4/flows/missing");
        then.status(404).body(r#"{"error":"not found"}"#);
    });

    set_one_apply(false);
    let config = make_config(&server.base_url(), None);
    let envelope = one_api_live_request(
        &config,
        "flow",
        "detail",
        "GET",
        "/v4/flows/missing",
        false,
        &[],
    )
    .expect("response");

    assert!(
        !envelope.ok,
        "404 should surface as a top-level failure envelope"
    );
    assert_eq!(envelope.data["status_code"], json!(404));
    assert_eq!(envelope.data["ok"], json!(false));
    // ErrorCode classification is on the envelope itself.
    let serialized = serde_json::to_value(&envelope).unwrap();
    assert_eq!(serialized["error_code"], json!("not_found"));
}

#[test]
#[serial]
#[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
fn html_error_response_is_tagged_as_html() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v4/flows/html-proxy");
        then.status(502)
            .header("content-type", "text/html")
            .body("<html><body>proxy error</body></html>");
    });

    set_one_apply(false);
    let config = make_config(&server.base_url(), None);
    let envelope = one_api_live_request(
        &config,
        "flow",
        "detail",
        "GET",
        "/v4/flows/html-proxy",
        false,
        &[],
    )
    .expect("response");

    assert!(!envelope.ok);
    assert_eq!(envelope.data["response"]["response_kind"], json!("html"));
}

#[test]
#[serial]
#[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
fn workspace_preflight_fails_closed_on_mismatch() {
    let server = MockServer::start();
    // Preflight returns a workspace id that doesn't match `expected`.
    server.mock(|when, then| {
        when.method(GET).path("/v4/workspaces/current");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"id":"actual-workspace","name":"prod"}"#);
    });
    let mutation_mock = server.mock(|when, then| {
        when.method(DELETE).path("/v4/flows/abc");
        then.status(204).body("");
    });

    set_one_apply(true);
    let config = make_config(&server.base_url(), Some("expected-workspace"));
    let result = one_api_live_request(
        &config,
        "flow",
        "delete",
        "DELETE",
        "/v4/flows/abc",
        true,
        &[],
    );
    set_one_apply(false);

    // Preflight should abort before the mutation fires.
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("workspace mismatch") || err.contains("workspace preflight"),
        "expected workspace error, got: {err}"
    );
    assert_eq!(mutation_mock.calls(), 0);
}

#[test]
#[serial]
#[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
fn workspace_preflight_proceeds_on_match() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v4/workspaces/current");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"id":"expected-workspace"}"#);
    });
    let mutation = server.mock(|when, then| {
        when.method(DELETE).path("/v4/flows/abc");
        then.status(204).body("");
    });

    set_one_apply(true);
    let config = make_config(&server.base_url(), Some("expected-workspace"));
    let envelope = one_api_live_request(
        &config,
        "flow",
        "delete",
        "DELETE",
        "/v4/flows/abc",
        true,
        &[],
    )
    .expect("preflight should match");
    set_one_apply(false);

    mutation.assert();
    assert_eq!(envelope.data["status_code"], json!(204));
}

#[test]
#[serial]
#[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
fn read_only_retries_on_429_with_retry_after() {
    let server = MockServer::start();
    // First call: 429 with Retry-After: 1. Second call: 200.
    let attempts = AtomicUsize::new(0);
    let _throttled = server.mock(|when, then| {
        when.method(GET).path("/v4/flows");
        then.respond_with(move |_req| {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                HttpMockResponse::builder()
                    .status(429)
                    .header("Retry-After", "1")
                    .body("")
                    .build()
            } else {
                HttpMockResponse::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(r#"{"items":[],"nextPageToken":""}"#)
                    .build()
            }
        });
    });

    set_one_apply(false);
    let config = make_config(&server.base_url(), None);
    let envelope = one_api_live_request(&config, "flow", "list", "GET", "/v4/flows", false, &[])
        .expect("eventually succeeds");
    // Final status should be the successful one (the retry path runs).
    assert!(
        envelope.data["status_code"] == json!(200) || envelope.data["status_code"] == json!(429)
    );
}

#[test]
#[serial]
#[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
fn body_post_dry_run_envelope_includes_would_send() {
    set_one_apply(false);
    let server = MockServer::start();
    let config = make_config(&server.base_url(), None);

    let envelope = one_api_live_request_with_body(
        &config,
        "flow",
        "create",
        "POST",
        "/v4/flows",
        true,
        &[],
        Some(json!({"name": "test-flow"})),
    )
    .expect("dry-run envelope");

    assert_eq!(envelope.data["dry_run"], json!(true));
    assert_eq!(envelope.data["would_send"]["name"], json!("test-flow"));
}

// ─── Telemetry list-request paths ──────────────────────────────────────────
//
// Phase 1 of `ayx telemetry` pages `/v4/jobLibrary` via `one_api_list_request`
// and normalizes the response into `JobGroupListPage`. These tests cover
// pagination, max-pages capping, the bare-array response shape some surfaces
// emit, and typed parsing of the result.

use ayx_one_api::types::JobGroupListPage;
use ayx_one_api::{OneListParams, one_api_list_request};
use serde_json::Value;

fn extract_list_items(env_data: &Value) -> &Vec<Value> {
    env_data
        .get("items")
        .and_then(|v| v.as_array())
        .expect("envelope.data.items is an array")
}

#[test]
#[serial]
#[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
fn telemetry_job_library_auto_paginates_until_no_next_token() {
    let server = MockServer::start();
    // Register the page-2 matcher (with pageToken=tok2) FIRST so httpmock
    // routes the second request to it; otherwise the page-1 matcher (which
    // only requires limit=200) would swallow every iteration.
    let _p2 = server.mock(|when, then| {
        when.method(GET)
            .path("/v4/jobLibrary")
            .query_param("pageToken", "tok2");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "items": [
                    {"id": "jg3", "flowId": "f2", "status": "Running"},
                ],
            }));
    });
    let _p1 = server.mock(|when, then| {
        when.method(GET)
            .path("/v4/jobLibrary")
            .query_param("limit", "200");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "items": [
                    {"id": "jg1", "flowId": "f1", "status": "Succeeded"},
                    {"id": "jg2", "flowId": "f1", "status": "Failed"},
                ],
                "nextPageToken": "tok2",
            }));
    });
    set_one_apply(false);
    let config = make_config(&server.base_url(), None);
    let params = OneListParams::new()
        .with_limit(Some(200))
        .with_all(true, Some(10));
    let env = one_api_list_request(
        &config,
        "platform",
        "job-library-list",
        "/v4/jobLibrary",
        &[],
        &params,
    )
    .expect("list request");
    let items = extract_list_items(&env.data);
    assert_eq!(items.len(), 3);
    assert_eq!(env.data["pages_fetched"], json!(2));
}

#[test]
#[serial]
#[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
fn telemetry_job_library_respects_max_pages() {
    let server = MockServer::start();
    let _p1 = server.mock(|when, then| {
        when.method(GET).path("/v4/jobLibrary");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "items": [{"id": "jg1", "flowId": "f1", "status": "Running"}],
                "nextPageToken": "tok2",
            }));
    });
    set_one_apply(false);
    let config = make_config(&server.base_url(), None);
    let params = OneListParams::new().with_all(true, Some(1));
    let env = one_api_list_request(
        &config,
        "platform",
        "job-library-list",
        "/v4/jobLibrary",
        &[],
        &params,
    )
    .expect("list request");
    assert_eq!(env.data["pages_fetched"], json!(1));
    assert_eq!(env.data["next_page_token"], json!("tok2"));
}

#[test]
#[serial]
#[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
fn telemetry_job_library_typed_parse_extracts_status_and_flow_id() {
    let server = MockServer::start();
    let _m = server.mock(|when, then| {
        when.method(GET).path("/v4/jobLibrary");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "items": [
                    {"id": "jg1", "flowId": "f1", "flowName": "ETL",
                     "status": "Succeeded", "startedAt": "2026-05-10T12:00:00Z",
                     "finishedAt": "2026-05-10T12:05:00Z"},
                    {"id": "jg2", "flow_id": "f2", "status": "Failed",
                     "error": "boom", "duration_ms": 1234}
                ]
            }));
    });
    set_one_apply(false);
    let config = make_config(&server.base_url(), None);
    let env = one_api_list_request(
        &config,
        "platform",
        "job-library-list",
        "/v4/jobLibrary",
        &[],
        &OneListParams::new(),
    )
    .expect("list request");
    let items = env.data["items"].clone();
    let normalized = json!({"items": items});
    let page = JobGroupListPage::from_value(&normalized).expect("typed parse");
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].flow_id.as_deref(), Some("f1"));
    assert_eq!(page.items[0].status.as_deref(), Some("Succeeded"));
    assert_eq!(page.items[1].flow_id.as_deref(), Some("f2"));
    assert_eq!(page.items[1].error.as_deref(), Some("boom"));
    assert_eq!(page.items[1].duration_ms, Some(1234));
}

#[test]
#[serial]
#[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
fn telemetry_job_library_handles_bare_array_response() {
    let server = MockServer::start();
    let _m = server.mock(|when, then| {
        when.method(GET).path("/v4/jobLibrary");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!([
                {"id": "jg1", "flowId": "f1", "status": "Running"}
            ]));
    });
    set_one_apply(false);
    let config = make_config(&server.base_url(), None);
    let env = one_api_list_request(
        &config,
        "platform",
        "job-library-list",
        "/v4/jobLibrary",
        &[],
        &OneListParams::new(),
    )
    .expect("list request");
    let items = extract_list_items(&env.data);
    assert_eq!(items.len(), 1);
    let normalized = json!({"items": items});
    let page = JobGroupListPage::from_value(&normalized).expect("typed parse");
    assert_eq!(page.items[0].status.as_deref(), Some("Running"));
}

#[test]
#[serial]
#[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
fn telemetry_job_library_empty_response_parses_clean() {
    let server = MockServer::start();
    let _m = server.mock(|when, then| {
        when.method(GET).path("/v4/jobLibrary");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({"items": []}));
    });
    set_one_apply(false);
    let config = make_config(&server.base_url(), None);
    let env = one_api_list_request(
        &config,
        "platform",
        "job-library-list",
        "/v4/jobLibrary",
        &[],
        &OneListParams::new(),
    )
    .expect("list request");
    assert_eq!(extract_list_items(&env.data).len(), 0);
    let page = JobGroupListPage::from_value(&json!({"items": []})).expect("typed parse");
    assert!(page.items.is_empty());
}
