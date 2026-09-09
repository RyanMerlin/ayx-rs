//! Product-boundary regression tests.
//!
//! Alteryx One, Alteryx Server, and Mongo are independent products with no
//! configuration, compatibility, or health dependency between them. A profile
//! that configures one must never make another report a warning, and no command
//! under `ayx one` may require Alteryx Server configuration.
//!
//! Every test builds an isolated `AYX_CONFIG_HOME` from a `tempfile::tempdir()`,
//! so nothing here reads the developer's real profile or touches the network.

use std::fs;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

/// Build a config home containing one profile file and an active-profile pointer.
fn home_with_profile(name: &str, yaml: &str) -> TempDir {
    let home = tempfile::tempdir().expect("tempdir");
    let profiles = home.path().join("profiles");
    fs::create_dir_all(&profiles).expect("create profiles dir");
    fs::write(profiles.join(format!("{name}.yaml")), yaml).expect("write profile");
    fs::write(
        home.path().join("state.yaml"),
        format!("active_profile: {name}\n"),
    )
    .expect("write state");
    home
}

/// One-only, authenticated with a durable OAuth refresh credential. The
/// credential is workspace-scoped, which is the shape `ayx one login` writes.
fn one_only_oauth_home() -> TempDir {
    home_with_profile(
        "one-oauth",
        r#"profile_name: one-oauth
alteryx_one:
  account_email: operator@example.com
  base_url: https://us1.alteryxcloud.com
  active_workspace_id: '91946'
  workspace_credentials:
    '91946':
      workspace_id: '91946'
      credential_kind: oauth_refresh
      access_token: test-access-token
      refresh_token: test-refresh-token
      oauth_client_id: test-client-id
"#,
    )
}

/// One-only, authenticated with email OTP: an access token and an expiry, and
/// deliberately no refresh token and no client id. That is what the OTP flow
/// returns; it is a valid credential, not an incomplete one.
///
/// Not yet used by this file's own test; a later task in this plan consumes
/// it. Kept here (rather than added later) because this file is the shared
/// harness every later task builds on.
#[allow(dead_code)]
fn one_only_otp_home() -> TempDir {
    home_with_profile(
        "one-otp",
        r#"profile_name: one-otp
alteryx_one:
  account_email: operator@example.com
  base_url: https://us1.alteryxcloud.com
  active_workspace_id: '91946'
  workspace_credentials:
    '91946':
      workspace_id: '91946'
      credential_kind: email_otp
      access_token: test-access-token
      access_token_expires_at: 4102444800
"#,
    )
}

/// Alteryx Server only. No `alteryx_one:` section at all.
///
/// Not yet used by this file's own test; a later task in this plan consumes
/// it. Kept here (rather than added later) because this file is the shared
/// harness every later task builds on.
#[allow(dead_code)]
fn server_only_home() -> TempDir {
    home_with_profile(
        "server-only",
        r#"profile_name: server-only
server:
  webapi_url: https://server.example.com/webapi/
  curator_api_key: test-key
  curator_api_secret: test-secret
"#,
    )
}

/// Both products configured in one profile.
///
/// Not yet used by this file's own test; a later task in this plan consumes
/// it. Kept here (rather than added later) because this file is the shared
/// harness every later task builds on.
#[allow(dead_code)]
fn combined_home() -> TempDir {
    home_with_profile(
        "combined",
        r#"profile_name: combined
alteryx_one:
  account_email: operator@example.com
  base_url: https://us1.alteryxcloud.com
  active_workspace_id: '91946'
  workspace_credentials:
    '91946':
      workspace_id: '91946'
      credential_kind: oauth_refresh
      access_token: test-access-token
      refresh_token: test-refresh-token
      oauth_client_id: test-client-id
server:
  webapi_url: https://server.example.com/webapi/
  curator_api_key: test-key
  curator_api_secret: test-secret
"#,
    )
}

/// Run any `ayx` command against an isolated home and parse its JSON envelope.
fn run_ayx(home: &TempDir, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(args)
        .args(["--output", "json-full"])
        .env("AYX_CONFIG_HOME", home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .output()
        .expect("ayx binary should run");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "expected a JSON envelope from `ayx {}`: {err}\nstdout: {}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// One `ayx doctor` sub-check, run without the live workspace probe that bare
/// `ayx doctor` performs. `name` is "auth", "network", or "server".
///
/// RULING P2: the brief's original `doctor_checks(home)` helper shelled out to
/// bare `ayx doctor`, which performs a live One workspace probe (a real
/// network request to the fixture's `base_url`). That violates this plan's
/// Global Constraint that a test must never make a network call. The scoped
/// subcommands `ayx doctor auth`, `ayx doctor network`, and `ayx doctor
/// server` are local-only and return the check payload at the `data` root
/// rather than nested under `checks`.
fn doctor_check(home: &TempDir, name: &str) -> Value {
    run_ayx(home, &["doctor", name])["data"].clone()
}

#[test]
fn doctor_reads_workspace_scoped_one_credentials() {
    // `ayx one login` writes credentials under
    // `alteryx_one.workspace_credentials[<workspace id>]`, not the legacy
    // top-level fields.  Doctor must read what login writes, or it reports
    // every modern profile as having no credentials at all.
    let home = one_only_oauth_home();
    let auth = doctor_check(&home, "auth");

    assert_eq!(
        auth["one"]["access_token_present"], true,
        "workspace-scoped access token should be seen:\n{auth:#}"
    );
    assert_eq!(
        auth["one"]["refresh_token_present"], true,
        "workspace-scoped refresh token should be seen:\n{auth:#}"
    );
    assert_eq!(
        auth["one"]["oauth_client_id_present"], true,
        "workspace-scoped client id should be seen:\n{auth:#}"
    );
    assert_eq!(
        auth["one_status"], "configured",
        "a complete OAuth refresh credential is configured, not incomplete:\n{auth:#}"
    );
}
