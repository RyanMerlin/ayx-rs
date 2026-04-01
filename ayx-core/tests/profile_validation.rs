use std::fs;

use ayx_core::profile::Config;

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
fn loads_minimal_valid_config() {
    let dir = unique_temp_dir("ayx-core-profile");
    let path = dir.join("config.yaml");
    fs::write(
        &path,
        r#"
profile_name: local
mongo:
  mode: embedded
  databases:
    gallery_name: AlteryxGallery
    service_name: AlteryxService
  embedded:
    runtime_settings_path: C:\ProgramData\Alteryx\RuntimeSettings.xml
api:
  base_url: http://localhost/
  auth:
    mode: pat
    pat: token
"#,
    )
    .expect("config should be writable");

    let config = Config::load_from_path(&path).expect("config should load");
    assert_eq!(config.profile_name, "local");
}

#[test]
fn rejects_missing_required_fields() {
    let dir = unique_temp_dir("ayx-core-profile-invalid");
    let path = dir.join("config.yaml");
    fs::write(
        &path,
        r#"
profile_name: local
mongo:
  mode: embedded
  databases:
    gallery_name: ""
    service_name: AlteryxService
  embedded:
    runtime_settings_path: C:\ProgramData\Alteryx\RuntimeSettings.xml
"#,
    )
    .expect("config should be writable");

    let err = Config::load_from_path(&path).expect_err("config should fail");
    assert!(err.to_string().contains("gallery_name"));
}

#[test]
fn loads_observability_block() {
    let dir = unique_temp_dir("ayx-core-profile-observability");
    let path = dir.join("config.yaml");
    fs::write(
        &path,
        r#"
profile_name: local
mongo:
  mode: embedded
  databases:
    gallery_name: AlteryxGallery
    service_name: AlteryxService
  embedded:
    runtime_settings_path: C:\ProgramData\Alteryx\RuntimeSettings.xml
observability:
  api_logging:
    enabled: true
    path: logs/api-events.jsonl
    redact_bodies: true
    log_requests: false
    log_responses: false
"#,
    )
    .expect("config should be writable");

    let config = Config::load_from_path(&path).expect("config should load");
    let obs = config.observability.expect("observability should exist");
    let api_logging = obs.api_logging.expect("api logging should exist");
    assert!(api_logging.enabled);
    assert_eq!(api_logging.path.as_deref(), Some("logs/api-events.jsonl"));
    assert_eq!(api_logging.redact_bodies, Some(true));
}

#[test]
fn validates_enabled_observability_path() {
    let dir = unique_temp_dir("ayx-core-profile-observability-invalid");
    let path = dir.join("config.yaml");
    fs::write(
        &path,
        r#"
profile_name: local
mongo:
  mode: embedded
  databases:
    gallery_name: AlteryxGallery
    service_name: AlteryxService
  embedded:
    runtime_settings_path: C:\ProgramData\Alteryx\RuntimeSettings.xml
observability:
  api_logging:
    enabled: true
    path: ""
"#,
    )
    .expect("config should be writable");

    let err = Config::load_from_path(&path).expect_err("config should fail");
    assert!(err.to_string().contains("observability.api_logging.path"));
}
