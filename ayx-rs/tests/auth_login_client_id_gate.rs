//! Regression tests pinning the flow-specific `oauth_client_id` gate in
//! `ayx one platform auth login`.
//!
//! Contract:
//!   * The **default email-OTP** flow must NOT require `oauth_client_id` — a
//!     brand-new user has no OAuth client yet, and the OTP/OIDC handshake never
//!     uses one. It should proceed past client-id resolution and stop at the
//!     `workspace_gid` requirement instead.
//!   * The `--browser` (PKCE) and `--device` grants MUST still require
//!     `oauth_client_id`, because they genuinely use it.
//!
//! These pin the fix that moved `oauth_client_id` resolution out of the shared
//! prefix of `login()` into the two branches that consume it. All three cases
//! fail *before any network call*, so the suite is hermetic and never hangs on
//! stdin (stdin is closed).
//!
//! Spawn-based, so gated off Windows like the rest of `cli_smoke.rs`.
#![cfg(not(windows))]

use std::fs;
use std::process::{Command, Stdio};

use tempfile::TempDir;

/// Isolated config home with a single One profile that deliberately has an
/// `account_email` + `base_url` but NO `oauth_client_id` and NO `workspace_gid`.
fn config_home_without_client_id() -> TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    let profiles = temp.path().join("profiles");
    fs::create_dir_all(&profiles).expect("profiles dir");
    fs::write(
        profiles.join("gate.yaml"),
        "profile_name: gate\n\
         alteryx_one:\n\
        \x20 account_email: user@example.com\n\
        \x20 base_url: https://us1.alteryxcloud.com\n",
    )
    .expect("write profile");
    temp
}

/// Run `ayx one platform auth login --profile gate <extra>` against the
/// isolated home. Returns (success, combined stdout+stderr). stdin is closed so
/// the default OTP path can never block waiting for a passcode — it must bail
/// at the `workspace_gid` check first.
fn run_login(home: &TempDir, extra: &[&str]) -> (bool, String) {
    let mut args = vec!["one", "platform", "auth", "login", "--profile", "gate"];
    args.extend_from_slice(extra);
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(&args)
        // AYX_CONFIG_HOME is checked first by config resolution; also pin HOME /
        // XDG so nothing falls back to the host's real profile store.
        .env("AYX_CONFIG_HOME", home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .stdin(Stdio::null())
        .output()
        .expect("ayx binary should run");
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), combined)
}

#[test]
fn default_otp_login_does_not_require_oauth_client_id() {
    let home = config_home_without_client_id();
    let (ok, out) = run_login(&home, &[]);
    assert!(!ok, "login should fail (no workspace_gid)\noutput:\n{out}");
    // Post-fix: the OTP path reaches the workspace_gid check.
    assert!(
        out.contains("workspace_gid is required"),
        "expected the default OTP path to reach the workspace_gid check; output:\n{out}"
    );
    // And it must NOT fail on oauth_client_id — that gate no longer applies here.
    assert!(
        !out.contains("oauth_client_id is required"),
        "the default OTP flow must not require oauth_client_id; output:\n{out}"
    );
}

#[test]
fn browser_login_still_requires_oauth_client_id() {
    let home = config_home_without_client_id();
    let (ok, out) = run_login(&home, &["--browser"]);
    assert!(
        !ok,
        "browser login should fail without client_id\noutput:\n{out}"
    );
    assert!(
        out.contains("oauth_client_id is required for the --browser flow"),
        "the --browser flow must still require oauth_client_id; output:\n{out}"
    );
}

#[test]
fn device_login_still_requires_oauth_client_id() {
    let home = config_home_without_client_id();
    let (ok, out) = run_login(&home, &["--device"]);
    assert!(
        !ok,
        "device login should fail without client_id\noutput:\n{out}"
    );
    assert!(
        out.contains("oauth_client_id is required for the --device flow"),
        "the --device flow must still require oauth_client_id; output:\n{out}"
    );
}
