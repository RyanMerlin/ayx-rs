use std::fs;

use ayx_core::observability::{ApiEvent, record_api_event};
use ayx_core::profile::{ApiLoggingProfile, ObservabilityProfile};
use serde_json::Value;

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    path.push(format!("{}-{}-{}", prefix, std::process::id(), nanos));
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
}

#[test]
fn writes_jsonl_api_event_when_enabled() {
    let dir = unique_temp_dir("ayx-core-observability");
    let log_path = dir.join("api-events.jsonl");
    let observability = ObservabilityProfile {
        api_logging: Some(ApiLoggingProfile {
            enabled: true,
            path: Some(log_path.display().to_string()),
            redact_bodies: Some(true),
            log_requests: Some(false),
            log_responses: Some(false),
        }),
    };

    let written = record_api_event(
        Some(&observability),
        ApiEvent {
            product: "one",
            surface: "platform",
            operation: "workspace-current",
            method: "GET",
            endpoint_template: "/v4/workspaces/current",
            resolved_url: "https://example.test/v4/workspaces/current",
            status_code: Some(200),
            duration_ms: 17,
            attempt: 1,
            retry_after_seconds: None,
            request_id: Some("req-123"),
            ok: true,
            error_class: None,
            response_shape: Some("object"),
            mutating: false,
            dry_run: false,
        },
    )
    .expect("event should write")
    .expect("log path should be returned");

    assert_eq!(written, log_path);
    let content = fs::read_to_string(&log_path).expect("log should exist");
    let line = content.lines().next().expect("jsonl line");
    let json: Value = serde_json::from_str(line).expect("json should parse");
    assert_eq!(json["product"], "one");
    assert_eq!(json["surface"], "platform");
    assert_eq!(json["operation"], "workspace-current");
    assert_eq!(json["status_code"], 200);
    assert_eq!(json["request_id"], "req-123");
    assert_eq!(json["redact_bodies"], true);
}
