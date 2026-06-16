use std::collections::HashMap;
use std::fs;
use std::process::Command;

use ayx_core::profile::{
    AlteryxOneProfile, ApiAuth, ApiAuthMode, ApiProfile, AyxState, Config, MongoDatabases,
    MongoEmbedded, MongoMode, MongoProfile,
};
use serde_json::Value;
use tempfile::TempDir;

fn live_smoke_enabled() -> bool {
    matches!(
        std::env::var("AYX_ONE_LIVE_SMOKE").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

struct LiveSmokeContext {
    config_home: TempDir,
}

impl LiveSmokeContext {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_home = temp.path();
        let env = repo_env_values();
        fs::create_dir_all(config_home.join("profiles")).expect("profiles dir");
        fs::create_dir_all(config_home.join("workspaces")).expect("workspaces dir");

        let state = AyxState {
            active_profile: Some("live".to_string()),
            active_workspace: None,
        };
        fs::write(
            config_home.join("state.yaml"),
            serde_yaml::to_string(&state).expect("state yaml"),
        )
        .expect("write state");

        let profile = Config {
            profile_name: "live".to_string(),
            mongo: MongoProfile {
                mode: MongoMode::Embedded,
                databases: MongoDatabases {
                    gallery_name: "AlteryxGallery".to_string(),
                    service_name: "AlteryxService".to_string(),
                },
                embedded: Some(MongoEmbedded {
                    runtime_settings_path: Some("RuntimeSettings.xml".to_string()),
                    alteryx_service_path: None,
                    restore_target_path: None,
                }),
                managed: None,
            },
            alteryx_one: Some(AlteryxOneProfile {
                account_email: repo_env(&env, "AYX_ACCOUNT_EMAIL", "user@example.com"),
                base_url: Some("https://us1.alteryxcloud.com".to_string()),
                oauth_client_id: Some(repo_env(&env, "AYX_ONE_OAUTH_CLIENT_ID", "client-id")),
                client_secret: None,
                client_secret_ref: None,
                token_endpoint_url: Some(repo_env(
                    &env,
                    "AYX_ONE_TOKEN_ENDPOINT_URL",
                    "https://pingauth.alteryxcloud.com/as",
                )),
                access_token: Some(repo_env(&env, "AYX_ONE_API_ACCESS_TOKEN", "topsecret")),
                access_token_ref: None,
                refresh_token: Some(repo_env(&env, "AYX_ONE_API_REFRESH_TOKEN", "topsecret")),
                refresh_token_ref: None,
                workspace_credentials: Default::default(),
                expected_workspace_id: None,
            }),
            observability: None,
            server_api: None,
            api: Some(ApiProfile {
                base_url: "https://us1.alteryxcloud.com".to_string(),
                auth: ApiAuth {
                    mode: ApiAuthMode::Oauth2ClientCredentials,
                    pat: None,
                    client_id: Some("client-id".to_string()),
                    client_secret: Some("client-secret".to_string()),
                    client_secret_ref: None,
                    scope: None,
                },
                timeout_ms: Some(60_000),
            }),
            server: None,
            sqlserver: None,
            upgrade: None,
        };
        fs::write(
            config_home.join("profiles").join("live.yaml"),
            serde_yaml::to_string(&profile).expect("profile yaml"),
        )
        .expect("write profile");

        Self { config_home: temp }
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ayx"));
        command.args(args);
        command.env("AYX_CONFIG_HOME", self.config_home.path());
        command.env("AYX_PROFILE", "live");
        command.output().expect("ayx binary should run")
    }
}

fn run_ayx_result(args: &[&str], context: &LiveSmokeContext) -> (bool, String, String) {
    let output = context.run(args);
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn write_json_payload(payload: &str) -> tempfile::NamedTempFile {
    let file = tempfile::NamedTempFile::new().expect("temp payload file");
    fs::write(file.path(), payload).expect("write payload");
    file
}

fn repo_env_values() -> HashMap<String, String> {
    let mut values = HashMap::new();
    let Ok(content) = fs::read_to_string(".env") else {
        return values;
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let mut parts = trimmed.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        if key.is_empty() {
            continue;
        }
        values.insert(
            key.to_string(),
            value.trim_matches('"').trim_matches('\'').to_string(),
        );
    }

    values
}

fn repo_env(values: &HashMap<String, String>, key: &str, fallback: &str) -> String {
    values
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_owned())
        .unwrap_or_else(|| fallback.to_string())
}

fn assert_contains(stdout: &str, needle: &str) {
    assert!(
        stdout.contains(needle),
        "expected stdout to contain {needle:?}\nstdout:\n{stdout}"
    );
}

fn assert_live_ok(stdout: &str) {
    assert_contains(stdout, "\"ok\": true");
}

fn assert_known_live_failure(stderr: &str, allowed_needles: &[&str]) {
    assert!(
        allowed_needles.iter().any(|needle| stderr.contains(needle)),
        "unexpected live failure\nstderr:\n{stderr}"
    );
}

fn live_auth_unavailable(stderr: &str) -> bool {
    stderr.contains("\"error_code\": \"auth_failed\"")
        || stderr.contains("refresh token request returned error status")
}

fn json_value(stdout: &str) -> Option<Value> {
    serde_json::from_str(stdout).ok()
}

fn first_list_item_id(stdout: &str, id_keys: &[&str]) -> Option<String> {
    let value = json_value(stdout)?;
    let candidates = [
        &value["data"]["response"]["items"],
        &value["data"]["items"],
        &value["response"]["items"],
        &value["items"],
        &value["data"]["response"],
        &value["data"],
    ];

    for candidate in candidates {
        if let Some(items) = candidate.as_array() {
            if let Some(first) = items.first() {
                for key in id_keys {
                    if let Some(id) = first.get(*key).and_then(|value| value.as_str()) {
                        return Some(id.to_string());
                    }
                }
            }
        }
    }

    None
}

fn require_live_flow_id(live: &LiveSmokeContext) -> Option<String> {
    require_live_list_item_id(
        live,
        &["--output", "json", "one", "flows", "list"],
        &["id", "flowId", "flow_id"],
        "flow",
    )
}

fn require_live_list_item_id(
    live: &LiveSmokeContext,
    args: &[&str],
    id_keys: &[&str],
    label: &str,
) -> Option<String> {
    let (success, stdout, stderr) = run_ayx_result(args, live);
    if !success {
        if live_auth_unavailable(&stderr) {
            return None;
        }
        panic!(
            "command failed: {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
            args.join(" ")
        );
    }
    assert_live_ok(&stdout);
    let id = first_list_item_id(&stdout, id_keys)
        .unwrap_or_else(|| panic!("expected at least one live {label} object\nstdout:\n{stdout}"));
    Some(id)
}

fn error_code_from_stderr(stderr: &str) -> Option<String> {
    json_value(stderr)
        .and_then(|value| value.get("error_code").cloned())
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
}

fn assert_live_error_code(stderr: &str, allowed: &[&str]) {
    let code = error_code_from_stderr(stderr);
    assert!(
        code.as_deref()
            .is_some_and(|code| allowed.iter().any(|allowed| *allowed == code))
            || allowed.iter().any(|needle| stderr.contains(needle)),
        "unexpected live failure\nstderr:\n{stderr}"
    );
}

fn assert_page_boundary(stdout: &str) {
    assert_contains(stdout, "\"pages_fetched\": 1");
}

macro_rules! live_case {
    ($name:ident, args = [$($arg:expr),+], ok = [$($ok:expr),+], fail = [$($fail:expr),+]) => {
        #[test]
        fn $name() {
            if !live_smoke_enabled() {
                return;
            }

            let live = LiveSmokeContext::new();
            let (success, stdout, stderr) = run_ayx_result(&[$($arg),+], &live);
            if !success {
                if live_auth_unavailable(&stderr) {
                    return;
                }
                assert_known_live_failure(&stderr, &[$($fail),+]);
                return;
            }
            assert_live_ok(&stdout);
            $(assert_contains(&stdout, $ok);)+
        }
    };
}

macro_rules! live_unexpected_case {
    ($name:ident, args = [$($arg:expr),+], ok = [$($ok:expr),+]) => {
        #[test]
        fn $name() {
            if !live_smoke_enabled() {
                return;
            }

            let live = LiveSmokeContext::new();
            let (success, stdout, stderr) = run_ayx_result(&[$($arg),+], &live);
            if !success {
                if live_auth_unavailable(&stderr) {
                    return;
                }
                panic!(
                    "command failed: {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                    [$($arg),+].join(" ")
                );
            }
            assert_live_ok(&stdout);
            $(assert_contains(&stdout, $ok);)+
        }
    };
}

macro_rules! live_page_boundary_case {
    ($name:ident, args = [$($arg:expr),+], ok = [$($ok:expr),+]) => {
        #[test]
        fn $name() {
            if !live_smoke_enabled() {
                return;
            }

            let live = LiveSmokeContext::new();
            let (success, stdout, stderr) = run_ayx_result(&[$($arg),+], &live);
            if !success {
                if live_auth_unavailable(&stderr) {
                    return;
                }
                panic!(
                    "command failed: {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                    [$($arg),+].join(" ")
                );
            }
            assert_live_ok(&stdout);
            $(assert_contains(&stdout, $ok);)+
            assert_page_boundary(&stdout);
        }
    };
}

live_case!(
    one_platform_workspace_current_live,
    args = [
        "--output",
        "json",
        "one",
        "platform",
        "workspace",
        "current"
    ],
    ok = [
        "\"surface\": \"platform\"",
        "\"operation\": \"workspace-current\""
    ],
    fail = [
        "\"error_code\": \"not_found\"",
        "\"operation\": \"workspace-current\""
    ]
);

live_case!(
    one_plans_count_live,
    args = ["--output", "json", "one", "plans", "count"],
    ok = ["\"surface\": \"plans\"", "\"operation\": \"plans-count\""],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "refresh token request returned error status"
    ]
);

live_case!(
    one_doctor_discover_live,
    args = ["--output", "json", "one", "doctor", "discover"],
    ok = ["\"checks\""],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "refresh token request returned error status"
    ]
);

live_unexpected_case!(
    one_doctor_auth_live,
    args = ["--output", "json", "one", "doctor", "auth"],
    ok = [
        "\"surface\": \"platform\"",
        "\"diagnosis\":",
        "\"access_token_present\": true",
        "\"workspace_probe\":"
    ]
);

live_unexpected_case!(
    one_platform_api_status_live,
    args = ["--output", "json", "one", "platform", "api", "status"],
    ok = [
        "\"product\": \"one platform\"",
        "\"base_url\":",
        "\"has_credentials\":"
    ]
);

live_unexpected_case!(
    one_platform_workspace_list_live,
    args = ["--output", "json", "one", "platform", "workspace", "list"],
    ok = [
        "\"surface\": \"platform\"",
        "\"operation\": \"workspace-list\"",
        "\"pages_fetched\":",
        "\"items\":"
    ]
);

live_page_boundary_case!(
    one_platform_workspace_list_page_boundary_live,
    args = [
        "--output",
        "json",
        "one",
        "platform",
        "workspace",
        "list",
        "--limit",
        "1",
        "--all",
        "--max-pages",
        "1"
    ],
    ok = [
        "\"surface\": \"platform\"",
        "\"operation\": \"workspace-list\""
    ]
);

live_case!(
    one_platform_person_current_live,
    args = ["--output", "json", "one", "platform", "person", "current"],
    ok = [
        "\"surface\": \"platform\"",
        "\"operation\": \"person-current\""
    ],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "\"error_code\": \"not_found\""
    ]
);

live_page_boundary_case!(
    one_platform_person_list_page_boundary_live,
    args = [
        "--output",
        "json",
        "one",
        "platform",
        "person",
        "list",
        "--limit",
        "1",
        "--all",
        "--max-pages",
        "1"
    ],
    ok = [
        "\"surface\": \"platform\"",
        "\"operation\": \"person-list\""
    ]
);

live_case!(
    one_platform_token_list_live,
    args = ["--output", "json", "one", "platform", "token"],
    ok = [
        "\"surface\": \"platform\"",
        "\"operation\": \"api-access-tokens-list\""
    ],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "\"error_code\": \"not_found\""
    ]
);

live_case!(
    one_plans_list_live,
    args = ["--output", "json", "one", "plans", "list"],
    ok = [
        "\"surface\": \"plans\"",
        "\"operation\": \"list\"",
        "\"pages_fetched\":",
        "\"items\":"
    ],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "\"error_code\": \"not_found\""
    ]
);

#[test]
fn one_platform_token_detail_not_found_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(token_id) = require_live_list_item_id(
        &live,
        &["--output", "json", "one", "platform", "token"],
        &["id", "tokenId", "token_id"],
        "token",
    ) else {
        return;
    };
    let invalid_token_id = format!("{token_id}-missing");

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "platform",
            "token",
            "detail",
            "--token-id",
            &invalid_token_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        assert_contains(&stderr, "\"surface\": \"platform\"");
        assert_contains(&stderr, "\"operation\": \"api-access-tokens-detail\"");
        assert_live_error_code(&stderr, &["not_found", "validation"]);
        return;
    }
    panic!("expected invalid token id to fail\nstdout:\n{stdout}\nstderr:\n{stderr}");
}

live_page_boundary_case!(
    one_plans_list_page_boundary_live,
    args = [
        "--output",
        "json",
        "one",
        "plans",
        "list",
        "--limit",
        "1",
        "--all",
        "--max-pages",
        "1"
    ],
    ok = ["\"surface\": \"plans\"", "\"operation\": \"list\""]
);

live_case!(
    one_flows_list_live,
    args = ["--output", "json", "one", "flows", "list"],
    ok = [
        "\"surface\": \"flow\"",
        "\"operation\": \"list\"",
        "\"pages_fetched\":",
        "\"items\":"
    ],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "\"error_code\": \"not_found\""
    ]
);

live_page_boundary_case!(
    one_flows_list_page_boundary_live,
    args = [
        "--output",
        "json",
        "one",
        "flows",
        "list",
        "--limit",
        "1",
        "--all",
        "--max-pages",
        "1"
    ],
    ok = ["\"surface\": \"flow\"", "\"operation\": \"list\""]
);

#[test]
fn one_flows_detail_live_real_object() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(flow_id) = require_live_flow_id(&live) else {
        return;
    };

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "flows",
            "detail",
            "--flow-id",
            &flow_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        panic!(
            "command failed: --output json one flows detail --flow-id {flow_id}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"flow\"");
    assert_contains(&stdout, "\"operation\": \"detail\"");
    assert_contains(&stdout, &flow_id);
}

#[test]
fn one_flows_detail_not_found_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(flow_id) = require_live_flow_id(&live) else {
        return;
    };
    let invalid_flow_id = format!("{flow_id}-missing");

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "flows",
            "detail",
            "--flow-id",
            &invalid_flow_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        assert_contains(&stderr, "\"surface\": \"flow\"");
        assert_contains(&stderr, "\"operation\": \"detail\"");
        assert_live_error_code(&stderr, &["not_found", "validation"]);
        return;
    }
    panic!("expected invalid flow id to fail\nstdout:\n{stdout}\nstderr:\n{stderr}");
}

#[test]
fn one_flows_validate_live_real_object() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(flow_id) = require_live_flow_id(&live) else {
        return;
    };

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "flows",
            "validate",
            "--flow-id",
            &flow_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        panic!(
            "command failed: --output json one flows validate --flow-id {flow_id}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"flow\"");
    assert_contains(&stdout, "\"operation\": \"validate\"");
    assert_contains(&stdout, &flow_id);
}

#[test]
fn one_flows_parameters_live_real_object() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(flow_id) = require_live_flow_id(&live) else {
        return;
    };

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "flows",
            "parameters",
            "--flow-id",
            &flow_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        panic!(
            "command failed: --output json one flows parameters --flow-id {flow_id}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"flow\"");
    assert_contains(&stdout, "\"operation\": \"parameters\"");
    assert_contains(&stdout, &flow_id);
}

#[test]
fn one_flows_inputs_live_real_object() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(flow_id) = require_live_flow_id(&live) else {
        return;
    };

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "flows",
            "inputs",
            "--flow-id",
            &flow_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        panic!(
            "command failed: --output json one flows inputs --flow-id {flow_id}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"flow\"");
    assert_contains(&stdout, "\"operation\": \"inputs\"");
    assert_contains(&stdout, &flow_id);
}

#[test]
fn one_flows_outputs_live_real_object() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(flow_id) = require_live_flow_id(&live) else {
        return;
    };

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "flows",
            "outputs",
            "--flow-id",
            &flow_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        panic!(
            "command failed: --output json one flows outputs --flow-id {flow_id}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"flow\"");
    assert_contains(&stdout, "\"operation\": \"outputs\"");
    assert_contains(&stdout, &flow_id);
}

#[test]
fn one_flows_export_dry_run_live_real_object() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(flow_id) = require_live_flow_id(&live) else {
        return;
    };

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "flows",
            "export-dry-run",
            "--flow-id",
            &flow_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        panic!(
            "command failed: --output json one flows export-dry-run --flow-id {flow_id}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"flow\"");
    assert_contains(&stdout, "\"operation\": \"export-dry-run\"");
    assert_contains(&stdout, "\"dry_run\": true");
    assert_contains(&stdout, &flow_id);
}

#[test]
fn one_connections_detail_not_found_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(connection_id) = require_live_list_item_id(
        &live,
        &["--output", "json", "one", "connections", "list"],
        &["id", "connectionId", "connection_id"],
        "connection",
    ) else {
        return;
    };
    let invalid_connection_id = format!("{connection_id}-missing");

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "connections",
            "detail",
            "--connection-id",
            &invalid_connection_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        assert_contains(&stderr, "\"surface\": \"connection\"");
        assert_contains(&stderr, "\"operation\": \"detail\"");
        assert_live_error_code(&stderr, &["not_found", "validation"]);
        return;
    }
    panic!("expected invalid connection id to fail\nstdout:\n{stdout}\nstderr:\n{stderr}");
}

live_case!(
    one_connections_list_live,
    args = ["--output", "json", "one", "connections", "list"],
    ok = [
        "\"surface\": \"connection\"",
        "\"operation\": \"list\"",
        "\"pages_fetched\":",
        "\"items\":"
    ],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "\"error_code\": \"not_found\""
    ]
);

live_page_boundary_case!(
    one_connections_list_page_boundary_live,
    args = [
        "--output",
        "json",
        "one",
        "connections",
        "list",
        "--limit",
        "1",
        "--all",
        "--max-pages",
        "1"
    ],
    ok = ["\"surface\": \"connection\"", "\"operation\": \"list\""]
);

live_case!(
    one_job_groups_list_live,
    args = ["--output", "json", "one", "job-groups", "list"],
    ok = [
        "\"surface\": \"jobGroup\"",
        "\"operation\": \"list\"",
        "\"pages_fetched\":",
        "\"items\":"
    ],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "\"error_code\": \"not_found\""
    ]
);

live_page_boundary_case!(
    one_job_groups_list_page_boundary_live,
    args = [
        "--output",
        "json",
        "one",
        "job-groups",
        "list",
        "--limit",
        "1",
        "--all",
        "--max-pages",
        "1"
    ],
    ok = ["\"surface\": \"jobGroup\"", "\"operation\": \"list\""]
);

live_case!(
    one_output_objects_list_live,
    args = ["--output", "json", "one", "output-objects", "list"],
    ok = [
        "\"surface\": \"outputObject\"",
        "\"operation\": \"list\"",
        "\"pages_fetched\":",
        "\"items\":"
    ],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "\"error_code\": \"not_found\""
    ]
);

live_page_boundary_case!(
    one_output_objects_list_page_boundary_live,
    args = [
        "--output",
        "json",
        "one",
        "output-objects",
        "list",
        "--limit",
        "1",
        "--all",
        "--max-pages",
        "1"
    ],
    ok = ["\"surface\": \"outputObject\"", "\"operation\": \"list\""]
);

live_case!(
    one_write_settings_list_live,
    args = ["--output", "json", "one", "write-settings", "list"],
    ok = [
        "\"surface\": \"writeSetting\"",
        "\"operation\": \"list\"",
        "\"pages_fetched\":",
        "\"items\":"
    ],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "\"error_code\": \"not_found\""
    ]
);

live_page_boundary_case!(
    one_write_settings_list_page_boundary_live,
    args = [
        "--output",
        "json",
        "one",
        "write-settings",
        "list",
        "--limit",
        "1",
        "--all",
        "--max-pages",
        "1"
    ],
    ok = ["\"surface\": \"writeSetting\"", "\"operation\": \"list\""]
);

live_case!(
    one_scheduling_list_live,
    args = ["--output", "json", "one", "scheduling", "list"],
    ok = [
        "\"surface\": \"scheduling\"",
        "\"operation\": \"list\"",
        "\"pages_fetched\":",
        "\"items\":"
    ],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "\"error_code\": \"not_found\""
    ]
);

live_page_boundary_case!(
    one_scheduling_list_page_boundary_live,
    args = [
        "--output",
        "json",
        "one",
        "scheduling",
        "list",
        "--limit",
        "1",
        "--all",
        "--max-pages",
        "1"
    ],
    ok = ["\"surface\": \"scheduling\"", "\"operation\": \"list\""]
);

live_case!(
    one_billing_current_account_live,
    args = ["--output", "json", "one", "billing", "current-account"],
    ok = [
        "\"surface\": \"billing\"",
        "\"operation\": \"current-account\""
    ],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "\"error_code\": \"not_found\""
    ]
);

#[test]
fn one_plans_detail_not_found_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(plan_id) = require_live_list_item_id(
        &live,
        &["--output", "json", "one", "plans", "list"],
        &["id", "planId", "plan_id"],
        "plan",
    ) else {
        return;
    };
    let invalid_plan_id = format!("{plan_id}-missing");

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "plans",
            "detail",
            "--plan-id",
            &invalid_plan_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        assert_contains(&stderr, "\"surface\": \"plans\"");
        assert_contains(&stderr, "\"operation\": \"detail\"");
        assert_live_error_code(&stderr, &["not_found", "validation"]);
        return;
    }
    panic!("expected invalid plan id to fail\nstdout:\n{stdout}\nstderr:\n{stderr}");
}

#[test]
fn one_platform_person_detail_not_found_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(person_id) = require_live_list_item_id(
        &live,
        &["--output", "json", "one", "platform", "person", "list"],
        &["id", "personId", "person_id"],
        "person",
    ) else {
        return;
    };
    let invalid_person_id = format!("{person_id}-missing");

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "platform",
            "person",
            "detail",
            "--person-id",
            &invalid_person_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        assert_contains(&stderr, "\"surface\": \"platform\"");
        assert_contains(&stderr, "\"operation\": \"person-detail\"");
        assert_live_error_code(&stderr, &["not_found", "validation"]);
        return;
    }
    panic!("expected invalid person id to fail\nstdout:\n{stdout}\nstderr:\n{stderr}");
}

#[test]
fn one_connections_dry_run_shape_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let payload = write_json_payload(r#"{"name":"shape-check"}"#);
    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "connections",
            "dry-run",
            "--body",
            payload.path().to_str().expect("payload path"),
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        panic!(
            "command failed: --output json one connections dry-run\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"connection\"");
    assert_contains(&stdout, "\"operation\": \"dry-run\"");
    assert_contains(&stdout, "\"dry_run\": true");
    assert_contains(&stdout, "\"mutating\": false");
    assert_contains(&stdout, "\"would_send\":");
}
