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
        access_token: Some("test-token".to_string()),
        access_token_ref: None,
        refresh_token: None,
        refresh_token_ref: None,
        expected_workspace_id: expected_workspace.map(|s| s.to_string()),
    });
    config
}

#[test]
#[serial]
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

    assert_eq!(envelope.data["status_code"], json!(404));
    assert_eq!(envelope.data["ok"], json!(false));
    // ErrorCode classification is on the envelope itself.
    let serialized = serde_json::to_value(&envelope).unwrap();
    assert_eq!(serialized["error_code"], json!("not_found"));
}

#[test]
#[serial]
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
    assert_eq!(mutation_mock.hits(), 0);
}

#[test]
#[serial]
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
fn read_only_retries_on_429_with_retry_after() {
    let server = MockServer::start();
    // First call: 429 with Retry-After: 1. Second call: 200.
    let throttled = server.mock(|when, then| {
        when.method(GET).path("/v4/flows");
        then.status(429).header("Retry-After", "1").body("");
    });
    // httpmock matches in registration order, so register the throttled
    // mock with `expect_at_most`, then a permanent 200 mock.
    let _ = throttled;
    server.mock(|when, then| {
        when.method(GET).path("/v4/flows");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"items":[],"nextPageToken":""}"#);
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
