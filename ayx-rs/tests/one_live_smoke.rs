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
    // The default CI lane creates a disposable profile from `.env`. An explicit
    // local profile lane intentionally keeps the user's OS-keyring-backed
    // profile in place so a real interactive login can be validated without
    // exporting its credentials into `.env`.
    config_home: Option<TempDir>,
    profile: String,
}

impl LiveSmokeContext {
    fn new() -> Self {
        if let Ok(profile) = std::env::var("AYX_ONE_LIVE_PROFILE")
            && !profile.trim().is_empty()
        {
            return Self {
                config_home: None,
                profile,
            };
        }

        let temp = tempfile::tempdir().expect("tempdir");
        let config_home = temp.path();
        let env = repo_env_values();
        // This constructor is only reached when the guard is enabled. Refuse to
        // run against a dummy/partial profile: a missing live secret must fail
        // loudly here rather than silently pass later via placeholder creds.
        // (AYX_ONE_BASE_URL is intentionally optional — it defaults to us1.)
        for key in [
            "AYX_ONE_API_ACCESS_TOKEN",
            "AYX_ONE_API_REFRESH_TOKEN",
            "AYX_ONE_OAUTH_CLIENT_ID",
            "AYX_ONE_TOKEN_ENDPOINT_URL",
            "AYX_ACCOUNT_EMAIL",
        ] {
            assert!(
                env.get(key).is_some_and(|value| !value.trim().is_empty()),
                "AYX_ONE_LIVE_SMOKE is enabled but required live secret {key} is \
                 missing/empty in .env — refusing to run against placeholder creds"
            );
        }
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
                base_url: Some(repo_env(
                    &env,
                    "AYX_ONE_BASE_URL",
                    "https://us1.alteryxcloud.com",
                )),
                oauth_client_id: Some(repo_env(&env, "AYX_ONE_OAUTH_CLIENT_ID", "client-id")),
                client_secret: None,
                client_secret_ref: None,
                sp_client_secret: None,
                sp_client_secret_ref: None,
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
                ..Default::default()
            }),
            observability: None,
            server_api: None,
            api: Some(ApiProfile {
                base_url: repo_env(&env, "AYX_ONE_BASE_URL", "https://us1.alteryxcloud.com"),
                auth: ApiAuth {
                    mode: ApiAuthMode::Oauth2ClientCredentials,
                    pat: None,
                    client_id: Some("client-id".to_string()),
                    client_secret: Some("client-secret".to_string()),
                    client_secret_ref: None,
                    scope: None,
                },
                timeout_ms: Some(60_000),
                derived: false,
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

        Self {
            config_home: Some(temp),
            profile: "live".to_string(),
        }
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ayx"));
        command.args(args);
        if let Some(config_home) = &self.config_home {
            command.env("AYX_CONFIG_HOME", config_home.path());
        }
        command.env("AYX_PROFILE", &self.profile);
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

/// Read `.env` from the current dir or any ancestor.
///
/// nextest runs integration tests with the working directory set to the
/// *package* dir (`ayx-rs/`), but the canonical `.env` lives at the workspace
/// root. Walking up bounded levels lets a single root `.env` serve local runs
/// from anywhere and lets CI materialize `.env` at the root it checks out into.
fn read_dotenv_from_cwd_or_ancestors() -> Option<String> {
    let mut dir = std::env::current_dir().ok()?;
    for _ in 0..6 {
        if let Ok(content) = fs::read_to_string(dir.join(".env")) {
            return Some(content);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn repo_env_values() -> HashMap<String, String> {
    let mut values = HashMap::new();
    let Some(content) = read_dotenv_from_cwd_or_ancestors() else {
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
    // An expired/revoked token surfaces as an explicit auth_failed envelope OR
    // as a failed OAuth refresh exchange. The live wording is
    // `token request failed: refresh token request to '<url>' returned <status>`
    // (error_code "internal"), so match the message prefixes rather than a
    // single brittle substring — this is the canonical "rotate the PAT" signal.
    stderr.contains("\"error_code\": \"auth_failed\"")
        || stderr.contains("refresh token request returned error status")
        || stderr.contains("refresh token request to")
        || stderr.contains("token request failed")
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
        if let Some(items) = candidate.as_array()
            && let Some(first) = items.first()
        {
            for key in id_keys {
                // Ids come back in BOTH shapes: cloud-native workflows use ULID
                // strings, while connections, flows, folders, job groups, output
                // objects and write settings use JSON numbers. Accepting only
                // strings silently starved every numeric-id `*_real_object` case —
                // they panicked with "expected at least one live <thing> object"
                // even though the listing had returned items.
                match first.get(*key) {
                    Some(serde_json::Value::String(id)) => return Some(id.clone()),
                    Some(serde_json::Value::Number(id)) => return Some(id.to_string()),
                    _ => {}
                }
            }
        }
    }

    None
}

fn require_live_workflow_id(live: &LiveSmokeContext) -> Option<String> {
    require_live_list_item_id(
        live,
        &["--output", "json", "one", "workflows", "list"],
        &["id", "workflowId", "workflow_id"],
        "workflow",
    )
}

fn require_live_write_setting_id(live: &LiveSmokeContext) -> Option<String> {
    require_live_list_item_id(
        live,
        &["--output", "json", "one", "write-settings", "list"],
        &["id", "writeSettingId", "write_setting_id"],
        "write setting",
    )
}

/// A real, resolvable email in this workspace, for `workflows share` email
/// resolution tests. Reads `GET /v4/people` (via `one person list`) rather than
/// hardcoding an address — a hardcoded email is only ever valid for the one
/// workspace it was captured from and silently starves this test on any other.
///
/// Live-verified 2026-07-27: this tenant's people directory contains at least
/// one entry with a corrupted `email` field (two addresses concatenated with
/// `". "`). Picking blindly picked that entry and made resolution fail for a
/// perfectly valid address — so this scans for the first entry whose `email`
/// looks like a single, well-formed address rather than taking the first item
/// unconditionally.
fn require_live_person_email(live: &LiveSmokeContext) -> Option<String> {
    let (success, stdout, stderr) =
        run_ayx_result(&["--output", "json", "one", "person", "list"], live);
    if !success {
        if live_auth_unavailable(&stderr) {
            return None;
        }
        panic!("command failed: one person list\nstdout:\n{stdout}\nstderr:\n{stderr}");
    }
    assert_live_ok(&stdout);

    let Some(value) = json_value(&stdout) else {
        panic!("one person list did not return valid JSON:\n{stdout}");
    };
    let items = value["data"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let clean_email = items.iter().find_map(|item| {
        let email = item.get("email")?.as_str()?.trim();
        let well_formed = email.matches('@').count() == 1 && !email.contains(char::is_whitespace);
        well_formed.then(|| email.to_string())
    });
    match clean_email {
        Some(email) => Some(email),
        None => {
            eprintln!(
                "live-smoke: skipping — no person with a clean, resolvable email exists in \
                 this workspace"
            );
            None
        }
    }
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
    // A listing that SUCCEEDS but is empty means the tenant simply has no fixture of
    // this kind — there is nothing to exercise, so skip (the `None` every caller
    // already handles) rather than fail. Panicking here made every `*_real_object`
    // case red on any workspace that happens not to use that resource, which is how
    // the nightly live-smoke run ended up mostly red for reasons unrelated to the CLI.
    //
    // A listing that FAILS still panics above — "the endpoint is broken" and "this
    // tenant has none of these" are different findings and must not be conflated.
    match first_list_item_id(&stdout, id_keys) {
        Some(id) => Some(id),
        None => {
            eprintln!(
                "live-smoke: skipping — no {label} objects exist in this workspace, \
                 so there is no fixture to exercise ({})",
                args.join(" ")
            );
            None
        }
    }
}

fn error_code_from_stderr(stderr: &str) -> Option<String> {
    json_value(stderr)
        .and_then(|value| value.get("error_code").cloned())
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
}

fn assert_live_error_code(stderr: &str, allowed: &[&str]) {
    let code = error_code_from_stderr(stderr);
    assert!(
        code.as_deref().is_some_and(|code| allowed.contains(&code))
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

/// Hard liveness gate for the whole live-smoke suite.
///
/// Every other live case *tolerates* an unavailable token by returning green
/// (see `live_auth_unavailable`) so one transient auth blip mid-run does not
/// spam dozens of failures. That tolerance is only safe because THIS gate
/// fails loud: when the suite is enabled (`AYX_ONE_LIVE_SMOKE=1`) but the
/// configured token cannot authenticate, the run goes RED here instead of
/// silently passing. A red gate is the signal to rotate the PAT
/// (`ayx one login`) and refresh the CI secret.
///
/// The probe mirrors `one auth diagnose`, which hits
/// `GET /v4/apiAccessTokens` to prove the token authenticates against the
/// tenant. A dead/expired token surfaces as `auth_failed` or a refresh-token
/// exchange error; any other failure (permission_denied / not_found) still
/// proves the request reached the API authenticated, so the token is live.
#[test]
fn live_smoke_requires_a_live_token() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let (success, stdout, stderr) = run_ayx_result(&["--output", "json", "one", "token"], &live);

    if success {
        assert_live_ok(&stdout);
        return;
    }

    // The command failed. A dead/expired token is the one failure this gate
    // refuses to tolerate — surface it loudly with rotation instructions.
    if live_auth_unavailable(&stderr) {
        panic!(
            "AYX_ONE_LIVE_SMOKE is enabled but the Alteryx One token is not live \
             (auth_failed / refresh-token exchange failed). Rotate the PAT with \
             `ayx one login` and refresh the CI secret.\nstderr:\n{stderr}"
        );
    }

    // Positive allowlist: only an authorization / not-found error still proves
    // the request reached the tenant *authenticated*, i.e. the token is live.
    // Any other failure (network, TLS, 5xx, misconfig) cannot confirm liveness,
    // so the gate must NOT pass on it — otherwise a transient error is a
    // false green, defeating the whole point of the gate.
    assert!(
        stderr.contains("\"error_code\": \"permission_denied\"")
            || stderr.contains("\"error_code\": \"not_found\""),
        "live-token gate: could not confirm the token is live — the probe failed \
         with an error that is neither a recognized auth failure nor an \
         authenticated authz/not-found response. Investigate before trusting a \
         green run.\nstderr:\n{stderr}"
    );
}

live_case!(
    one_workspace_current_live,
    args = ["--output", "json", "one", "workspace", "current"],
    ok = [
        "\"surface\": \"workspace\"",
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
    ok = ["\"surface\": \"plans\"", "\"operation\": \"count\""],
    fail = ["\"error_code\": \"permission_denied\""]
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
        "\"surface\": \"auth\"",
        "\"diagnosis\":",
        "\"access_token_present\": true",
        "\"workspace_probe\":"
    ]
);

live_unexpected_case!(
    one_api_status_live,
    args = ["--output", "json", "one", "api", "status"],
    ok = [
        "\"product\": \"Alteryx One\"",
        "\"base_url\":",
        "\"has_credentials\":"
    ]
);

live_unexpected_case!(
    one_workspace_list_live,
    args = ["--output", "json", "one", "workspace", "list"],
    ok = [
        "\"surface\": \"workspace\"",
        "\"operation\": \"workspace-list\"",
        "\"pages_fetched\":",
        "\"items\":"
    ]
);

live_page_boundary_case!(
    one_workspace_list_page_boundary_live,
    args = [
        "--output",
        "json",
        "one",
        "workspace",
        "list",
        "--limit",
        "1",
        "--all",
        "--max-pages",
        "1"
    ],
    ok = [
        "\"surface\": \"workspace\"",
        "\"operation\": \"workspace-list\""
    ]
);

live_case!(
    one_person_current_live,
    args = ["--output", "json", "one", "person", "current"],
    ok = [
        "\"surface\": \"person\"",
        "\"operation\": \"person-current\""
    ],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "\"error_code\": \"not_found\""
    ]
);

live_page_boundary_case!(
    one_person_list_page_boundary_live,
    args = [
        "--output",
        "json",
        "one",
        "person",
        "list",
        "--limit",
        "1",
        "--all",
        "--max-pages",
        "1"
    ],
    ok = ["\"surface\": \"person\"", "\"operation\": \"person-list\""]
);

live_case!(
    one_token_list_live,
    args = ["--output", "json", "one", "token"],
    ok = [
        "\"surface\": \"token\"",
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
fn one_token_detail_not_found_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(token_id) = require_live_list_item_id(
        &live,
        &["--output", "json", "one", "token"],
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
            "token",
            "detail",
            &invalid_token_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        assert_contains(&stderr, "\"surface\": \"token\"");
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

// ---------------------------------------------------------------------------
// Alteryx One cloud-native workflows (/svc-workflow). Distinct from `one workflows`.
// ---------------------------------------------------------------------------

live_case!(
    one_workflows_list_live,
    args = ["--output", "json", "one", "workflows", "list"],
    ok = [
        "\"surface\": \"workflow\"",
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
    one_workflows_list_page_boundary_live,
    args = [
        "--output",
        "json",
        "one",
        "workflows",
        "list",
        "--limit",
        "1",
        "--all",
        "--max-pages",
        "1"
    ],
    ok = ["\"surface\": \"workflow\"", "\"operation\": \"list\""]
);

live_case!(
    one_workflows_tools_live,
    args = ["--output", "json", "one", "workflows", "tools"],
    ok = ["\"surface\": \"workflow\"", "\"operation\": \"tools\""],
    fail = [
        "\"error_code\": \"permission_denied\"",
        "\"error_code\": \"not_found\""
    ]
);

/// `count` must report the collection total, not the size of the page it fetched.
/// It requests ?limit=1, so a naive implementation would answer 1.
#[test]
fn one_workflows_count_reports_collection_total_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let (success, stdout, stderr) =
        run_ayx_result(&["--output", "json", "one", "workflows", "count"], &live);
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        panic!("command failed: one workflows count\nstdout:\n{stdout}\nstderr:\n{stderr}");
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"workflow\"");
    assert_contains(&stdout, "\"operation\": \"count\"");
    // Proves the total came from the server envelope rather than the returned page.
    assert_contains(&stdout, "\"count_source\": \"server\"");
}

/// `detail` is synthesized client-side (no GET /v4/workflows/{id} exists), so it
/// must both resolve a real id and advertise that synthesis.
#[test]
fn one_workflows_detail_live_real_object() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(workflow_id) = require_live_workflow_id(&live) else {
        return;
    };

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "workflows",
            "detail",
            &workflow_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        panic!(
            "command failed: one workflows detail {workflow_id}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"workflow\"");
    assert_contains(&stdout, "\"operation\": \"detail\"");
    assert_contains(&stdout, "\"detail_source\":");
    assert_contains(&stdout, &workflow_id);
}

#[test]
fn one_workflows_dependencies_live_real_object() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(workflow_id) = require_live_workflow_id(&live) else {
        return;
    };

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "workflows",
            "dependencies",
            &workflow_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        assert_contains(&stderr, "\"surface\": \"workflow\"");
        assert_live_error_code(&stderr, &["not_found", "permission_denied", "validation"]);
        return;
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"operation\": \"dependencies\"");
    assert_contains(&stdout, "\"connections\":");
    assert_contains(&stdout, "\"datasets\":");
}

/// A ULID that parses but names nothing must be a clean not_found, not an HTML
/// parse failure or an internal error.
#[test]
fn one_workflows_detail_not_found_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "workflows",
            "detail",
            "01AAAAAAAAAAAAAAAAAAAAAAAA",
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        assert_contains(&stderr, "\"surface\": \"workflow\"");
        assert_contains(&stderr, "\"operation\": \"detail\"");
        assert_live_error_code(&stderr, &["not_found"]);
        return;
    }
    panic!("expected an unknown workflow id to fail\nstdout:\n{stdout}\nstderr:\n{stderr}");
}

/// Workflow execution is mutating, so the default command must stop at the
/// CLI apply gate and show the real cloud-native run route without contacting
/// the provider.
#[test]
fn one_workflows_run_dry_run_shape_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(workflow_id) = require_live_workflow_id(&live) else {
        return;
    };

    let (success, stdout, stderr) = run_ayx_result(
        &["--output", "json", "one", "workflows", "run", &workflow_id],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        panic!(
            "command failed: one workflows run {workflow_id}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"workflow\"");
    assert_contains(&stdout, "\"operation\": \"run\"");
    assert_contains(
        &stdout,
        "\"endpoint_template\": \"/svc-workflow/api/v1/workflows/{id}/run\"",
    );
    assert_contains(&stdout, "\"mutating\": true");
    assert_contains(&stdout, "\"dry_run\": true");
}

/// Cancellation accepts the provider job id returned by `run`, not a workflow
/// definition ULID, and is dry-run by default.
#[test]
fn one_workflows_cancel_dry_run_shape_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "workflows",
            "cancel",
            "123456789",
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        panic!("command failed: one workflows cancel\nstdout:\n{stdout}\nstderr:\n{stderr}");
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"workflow\"");
    assert_contains(&stdout, "\"operation\": \"cancel\"");
    assert_contains(
        &stdout,
        "\"endpoint_template\": \"/svc-workflow/api/v1/jobs/{id}/cancel\"",
    );
    assert_contains(&stdout, "\"mutating\": true");
    assert_contains(&stdout, "\"dry_run\": true");
}

/// `copy` is mutating, so without --apply it must dry-run. Also proves --version
/// is resolved to the workflow's current version before the gate, so the body
/// shown in would_send is exactly what --apply would send.
#[test]
fn one_workflows_copy_dry_run_shape_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(workflow_id) = require_live_workflow_id(&live) else {
        return;
    };

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "workflows",
            "copy",
            &workflow_id,
            "--name",
            "ayx-live-smoke-copy",
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        panic!(
            "command failed: one workflows copy {workflow_id}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"workflow\"");
    assert_contains(&stdout, "\"operation\": \"copy\"");
    assert_contains(&stdout, "\"mutating\": true");
    assert_contains(&stdout, "\"dry_run\": true");
    assert_contains(&stdout, "\"ayx-live-smoke-copy\"");
    // The version must already be resolved in the dry-run body.
    assert_contains(&stdout, "\"version\":");
}

/// `share` is mutating, so without --apply it must dry-run. Also proves the
/// `--to-person` email was resolved to a numeric person id via `GET /v4/people`
/// BEFORE the --apply gate: `would_send.toPersonIds` must already carry the
/// resolved id — as a JSON *string* (live-verified 2026-08-31: the API
/// rejects `toPersonIds`/`toGroupIds` sent as numbers with HTTP 400
/// SchemaValidationError, "Invalid input: expected string, received
/// number") — not the raw email, so a later --apply sends byte-identical
/// content. `--include-dependencies` must additionally attach a
/// `dependency_preview` so the blast radius is visible before commit.
#[test]
fn one_workflows_share_dry_run_shape_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(workflow_id) = require_live_workflow_id(&live) else {
        return;
    };
    let Some(person_email) = require_live_person_email(&live) else {
        return;
    };

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "workflows",
            "share",
            &workflow_id,
            "--to-person",
            &person_email,
            "--privilege",
            "read",
            "--privilege",
            "execute",
            "--include-dependencies",
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        panic!(
            "command failed: one workflows share {workflow_id} --to-person ... --privilege \
             read --privilege execute --include-dependencies\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"workflow\"");
    assert_contains(&stdout, "\"operation\": \"share\"");
    assert_contains(&stdout, "\"mutating\": true");
    assert_contains(&stdout, "\"dry_run\": true");

    let value = json_value(&stdout).expect("stdout is valid JSON");
    let would_send = &value["data"]["would_send"];
    let to_person_ids = would_send["toPersonIds"]
        .as_array()
        .expect("would_send.toPersonIds must be an array");
    assert!(
        !to_person_ids.is_empty(),
        "expected the email to resolve to at least one person id: {would_send}"
    );
    for id in to_person_ids {
        assert!(
            id.is_string(),
            "toPersonIds entries must be JSON strings of already-resolved \
             person ids, not {id:?} — live-verified 2026-08-31, the API \
             rejects numbers with HTTP 400 SchemaValidationError"
        );
        assert!(
            id.as_str().unwrap().parse::<u64>().is_ok(),
            "toPersonIds entries must be resolved to a numeric id (as a \
             string), not {id:?} — resolution must happen before the \
             --apply gate"
        );
    }
    assert_eq!(
        would_send["privileges"],
        serde_json::json!(["execute", "read"])
    );
    assert_eq!(would_send["includeDependencies"], serde_json::json!(true));

    assert!(
        value["data"].get("dependency_preview").is_some(),
        "--include-dependencies on a dry run must attach a dependency_preview: {}",
        value["data"]
    );
    assert_eq!(
        value["data"]["dependency_preview_ok"],
        serde_json::json!(true)
    );
}

/// An email address that cannot be resolved against `GET /v4/people` must fail
/// as a validation error naming the exact address — never silently dropped,
/// never sent to the share endpoint as-is (the endpoint requires integers).
#[test]
fn one_workflows_share_email_resolution_failure_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(workflow_id) = require_live_workflow_id(&live) else {
        return;
    };
    let bogus_email = "definitely-not-a-real-person-ayxrs-test@example.invalid";

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "workflows",
            "share",
            &workflow_id,
            "--to-person",
            bogus_email,
            "--privilege",
            "read",
        ],
        &live,
    );
    if success {
        panic!("expected an unresolvable email to fail\nstdout:\n{stdout}");
    }
    if live_auth_unavailable(&stderr) {
        return;
    }
    assert_contains(&stderr, "\"error_code\": \"validation\"");
    assert_contains(&stderr, bogus_email);
}

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

#[test]
fn one_write_settings_detail_not_found_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(write_setting_id) = require_live_write_setting_id(&live) else {
        return;
    };
    let invalid_write_setting_id = format!("{write_setting_id}-missing");

    let (success, stdout, stderr) = run_ayx_result(
        &[
            "--output",
            "json",
            "one",
            "write-settings",
            "detail",
            &invalid_write_setting_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        assert_contains(&stderr, "\"surface\": \"writeSetting\"");
        assert_contains(&stderr, "\"operation\": \"detail\"");
        assert_live_error_code(&stderr, &["not_found", "validation"]);
        return;
    }
    panic!("expected invalid write setting id to fail\nstdout:\n{stdout}\nstderr:\n{stderr}");
}

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
fn one_person_detail_not_found_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let Some(person_id) = require_live_list_item_id(
        &live,
        &["--output", "json", "one", "person", "list"],
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
            "person",
            "detail",
            &invalid_person_id,
        ],
        &live,
    );
    if !success {
        if live_auth_unavailable(&stderr) {
            return;
        }
        assert_contains(&stderr, "\"surface\": \"person\"");
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
        assert_contains(&stderr, "\"surface\": \"connection\"");
        assert_contains(&stderr, "\"operation\": \"dry-run\"");
        assert_contains(&stderr, "\"error_code\": \"validation\"");
        return;
    }
    assert_live_ok(&stdout);
    assert_contains(&stdout, "\"surface\": \"connection\"");
    assert_contains(&stdout, "\"operation\": \"dry-run\"");
    assert_contains(&stdout, "\"dry_run\": true");
    assert_contains(&stdout, "\"mutating\": false");
    assert_contains(&stdout, "\"would_send\":");
}

/// Re-probes one representative read-only command per service from
/// `docs/one-endpoint-matrix.md`, so the matrix's live evidence has an automated
/// tripwire instead of going stale silently between hand re-verification passes.
///
/// Tolerant by design: the known error allowance covers application not-found,
/// permission, and body-validation responses. The old `/plans/v1` and
/// `/scheduling/v1` paths were not entitlement gaps; the spec-documented `/v4`
/// replacements were re-verified live on 2026-08-18. A differently-scoped PAT
/// can legitimately 403 on some rows (for example, role assignment reads). This case only fails loud on a
/// genuinely *unexpected* shape: neither success, nor one of the error codes
/// this doc's live sweep already saw for these rows, nor an auth-unavailable
/// signal. `plan` (no safe read-only row — every endpoint needs a
/// live plan id or mutates) and `webhookFlowTask` (no `list`; `detail`/`delete`/`test`
/// all need an id this suite has no way to resolve without creating one) are excluded
/// for the same reason they're `unverified` in the doc itself.
#[test]
fn one_endpoint_matrix_spot_check_live() {
    if !live_smoke_enabled() {
        return;
    }

    let live = LiveSmokeContext::new();
    let known_tenant_gaps = ["permission_denied", "not_found", "validation"];

    let cases: &[(&str, &[&str], &str)] = &[
        (
            "platform.iam",
            &["--output", "json", "one", "workspace", "current"],
            "\"surface\": \"workspace\"",
        ),
        (
            "misc",
            &["--output", "json", "one", "api", "coverage"],
            "\"coverage_pct\"",
        ),
        (
            "plans",
            &["--output", "json", "one", "plans", "list"],
            "\"surface\": \"plans\"",
        ),
        (
            "workflow",
            &["--output", "json", "one", "workflows", "tools"],
            "\"surface\": \"workflow\"",
        ),
        (
            "dataset",
            &["--output", "json", "one", "datasets", "count"],
            "\"surface\": \"datasets\"",
        ),
        (
            "connection",
            &["--output", "json", "one", "connections", "count"],
            "\"surface\": \"connection\"",
        ),
        (
            "jobs",
            &["--output", "json", "one", "jobs", "count"],
            "\"surface\": \"jobs\"",
        ),
        (
            "outputObject",
            &["--output", "json", "one", "output-objects", "count"],
            "\"surface\": \"outputObject\"",
        ),
        (
            "writeSetting",
            &["--output", "json", "one", "write-settings", "count"],
            "\"surface\": \"writeSetting\"",
        ),
        (
            "scheduling",
            &["--output", "json", "one", "scheduling", "count"],
            "\"surface\": \"scheduling\"",
        ),
        (
            "apiAccessTokens",
            &["--output", "json", "one", "token"],
            "\"surface\": \"token\"",
        ),
        (
            "person",
            &["--output", "json", "one", "person", "current"],
            "\"surface\": \"person\"",
        ),
        (
            "workspace",
            &["--output", "json", "one", "workspace", "list"],
            "\"surface\": \"workspace\"",
        ),
    ];

    for (service, args, ok_needle) in cases {
        let (success, stdout, stderr) = run_ayx_result(args, &live);
        if !success {
            if live_auth_unavailable(&stderr) {
                continue;
            }
            assert_live_error_code(&stderr, &known_tenant_gaps);
            continue;
        }
        assert_live_ok(&stdout);
        assert_contains(&stdout, ok_needle);
        let _ = service; // named for failure-message clarity only
    }
}
