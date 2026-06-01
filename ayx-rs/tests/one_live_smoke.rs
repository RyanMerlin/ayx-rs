use std::collections::HashMap;
use std::fs;
use std::process::Command;

use ayx_core::profile::{
    AlteryxOneProfile, ApiAuth, ApiAuthMode, ApiProfile, AyxState, Config, MongoDatabases,
    MongoEmbedded, MongoMode, MongoProfile,
};
use tempfile::TempDir;

fn live_smoke_enabled() -> bool {
    matches!(
        std::env::var("AYX_ONE_LIVE_SMOKE").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn run_ayx_with_env(args: &[&str], envs: &[(&str, &str)]) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ayx"));
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }

    let output = command.output().expect("ayx binary should run");

    assert!(
        output.status.success(),
        "command failed: {}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).to_string()
}

fn run_ayx_result(args: &[&str], envs: &[(&str, &str)]) -> (bool, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ayx"));
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }

    let output = command.output().expect("ayx binary should run");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
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
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

fn live_config_home() -> TempDir {
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
            base_url: Some("https://api.us1.alteryxcloud.com".to_string()),
            oauth_client_id: Some(repo_env(&env, "AYX_ONE_OAUTH_CLIENT_ID", "client-id")),
            token_endpoint_url: Some(repo_env(
                &env,
                "AYX_ONE_TOKEN_ENDPOINT_URL",
                "https://pingauth.alteryxcloud.com/as",
            )),
            access_token: Some(repo_env(&env, "AYX_ONE_API_ACCESS_TOKEN", "topsecret")),
            access_token_ref: None,
            refresh_token: Some(repo_env(&env, "AYX_ONE_API_REFRESH_TOKEN", "topsecret")),
            refresh_token_ref: None,
            expected_workspace_id: None,
        }),
        observability: None,
        server_api: None,
        api: Some(ApiProfile {
            base_url: "https://api.us1.alteryxcloud.com".to_string(),
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

    temp
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

#[test]
fn one_platform_workspace_current_live() {
    if !live_smoke_enabled() {
        return;
    }

    let config_home = live_config_home();
    let config_home_str = config_home.path().to_string_lossy().to_string();
    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "platform",
            "workspace",
            "current",
        ],
        &[
            ("AYX_CONFIG_HOME", &config_home_str),
            ("AYX_PROFILE", "live"),
        ],
    );
    if !success {
        assert_contains(&stderr, "\"error_code\": \"not_found\"");
        assert_contains(&stderr, "\"operation\": \"workspace-current\"");
        return;
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"platform\"");
    assert_contains(&stdout, "\"operation\": \"workspace-current\"");
}

#[test]
fn one_plans_count_live() {
    if !live_smoke_enabled() {
        return;
    }

    let config_home = live_config_home();
    let config_home_str = config_home.path().to_string_lossy().to_string();
    let (success, stdout, stderr) = run_ayx_result(
        &["--output", "json", "one", "plans", "count"],
        &[
            ("AYX_CONFIG_HOME", &config_home_str),
            ("AYX_PROFILE", "live"),
        ],
    );
    if !success {
        assert_contains(&stderr, "\"error_code\": \"permission_denied\"");
        assert_contains(&stderr, "refresh token request returned error status");
        return;
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"plans\"");
    assert_contains(&stdout, "\"operation\": \"plans-count\"");
}

#[test]
fn one_doctor_discover_live() {
    if !live_smoke_enabled() {
        return;
    }

    let config_home = live_config_home();
    let config_home_str = config_home.path().to_string_lossy().to_string();
    let (success, stdout, stderr) = run_ayx_result(
        &["--output", "json", "one", "doctor", "discover"],
        &[
            ("AYX_CONFIG_HOME", &config_home_str),
            ("AYX_PROFILE", "live"),
        ],
    );
    if !success {
        assert_contains(&stderr, "\"error_code\": \"permission_denied\"");
        assert_contains(&stderr, "refresh token request returned error status");
        return;
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"checks\"");
}

#[test]
fn one_doctor_auth_live() {
    if !live_smoke_enabled() {
        return;
    }

    let config_home = live_config_home();
    let config_home_str = config_home.path().to_string_lossy().to_string();
    let stdout = run_ayx_with_env(
        &["--output", "json", "one", "doctor", "auth"],
        &[
            ("AYX_CONFIG_HOME", &config_home_str),
            ("AYX_PROFILE", "live"),
        ],
    );
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"platform\"");
    assert_contains(&stdout, "\"diagnosis\":");
    assert_contains(&stdout, "\"access_token_present\": true");
    assert_contains(&stdout, "\"workspace_probe\":");
}

#[test]
fn one_platform_api_status_live() {
    if !live_smoke_enabled() {
        return;
    }

    let config_home = live_config_home();
    let config_home_str = config_home.path().to_string_lossy().to_string();
    let stdout = run_ayx_with_env(
        &["--output", "json", "one", "platform", "api", "status"],
        &[
            ("AYX_CONFIG_HOME", &config_home_str),
            ("AYX_PROFILE", "live"),
        ],
    );
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"product\": \"one platform\"");
    assert_contains(&stdout, "\"base_url\":");
    assert_contains(&stdout, "\"has_credentials\":");
}

#[test]
fn one_platform_workspace_list_live() {
    if !live_smoke_enabled() {
        return;
    }

    let config_home = live_config_home();
    let config_home_str = config_home.path().to_string_lossy().to_string();
    let stdout = run_ayx_with_env(
        &["--output", "json", "one", "platform", "workspace", "list"],
        &[
            ("AYX_CONFIG_HOME", &config_home_str),
            ("AYX_PROFILE", "live"),
        ],
    );
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"platform\"");
    assert_contains(&stdout, "\"operation\": \"workspace-list\"");
    assert_contains(&stdout, "\"pages_fetched\":");
    assert_contains(&stdout, "\"items\":");
}
