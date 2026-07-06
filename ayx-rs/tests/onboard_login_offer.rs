//! Hermetic tests for `onboard`'s opt-in "log in now" step (1c).
//!
//! These drive the interactive wizard with piped stdin in an isolated config
//! home and assert only on the *decision* it makes — whether it offers the
//! immediate login or points at the next command. They never answer "yes" to
//! the login prompt, so no network call or OTP flow is ever triggered.
//!
//! Spawn-based; runs on all platforms now that the `ayx-rs` build script
//! reserves a 16 MiB main-thread stack on Windows (issue #59 Part 2).

use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::TempDir;

const REAL_GID: &str = "01KMGF85WTTEJZ397MW1RBD9ZB";

/// Run `ayx onboard` with the given stdin script against a specific config home.
fn run_onboard_in(home: &Path, stdin: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .arg("onboard")
        .env("AYX_CONFIG_HOME", home)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ayx onboard");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    combined
}

/// Run `ayx onboard` in a fresh throwaway config home.
fn run_onboard(stdin: &str) -> String {
    let home: TempDir = tempfile::tempdir().expect("tempdir");
    run_onboard_in(home.path(), stdin)
}

#[test]
fn offers_login_when_workspace_gid_is_present() {
    // profile name (default), email, workspace URL (with gid), no server, no login.
    let script = format!(
        "\nuser@example.com\nhttps://us1.alteryxcloud.com/auth-portal/workspaces/{REAL_GID}\nn\nn\n"
    );
    let out = run_onboard(&script);
    assert!(
        out.contains("Log in now"),
        "expected the login offer when a workspace_gid was parsed; output:\n{out}"
    );
    // Declined cleanly, and it must not have attempted a real login.
    assert!(
        out.contains("Skipped. Connect any time"),
        "expected a clean skip on declining; output:\n{out}"
    );
}

#[test]
fn points_at_next_step_when_workspace_gid_absent() {
    // profile name (default), email, blank workspace URL, no server.
    let script = "\nuser@example.com\n\nn\n";
    let out = run_onboard(script);
    assert!(
        out.contains("add your workspace URL/id"),
        "expected next-step guidance when no workspace_gid; output:\n{out}"
    );
    assert!(
        !out.contains("Log in now"),
        "must not offer immediate login without a workspace_gid; output:\n{out}"
    );
}

/// Regression guard for the profile-split bug: onboard must save the profile
/// under its *name* and make it active, so a later `auth login` (which writes
/// the token to `profile_storage_path(profile_name)`) targets the very same
/// file. If onboard instead wrote to `default.yaml` while naming the profile
/// `prod`, the token would land in a different file and split the profile.
#[test]
fn onboard_saves_under_profile_name_and_sets_active() {
    let home = tempfile::tempdir().expect("tempdir");
    // profile name "prod", email, workspace URL, no server, decline login.
    let script = format!(
        "prod\nuser@example.com\nhttps://us1.alteryxcloud.com/auth-portal/workspaces/{REAL_GID}\nn\nn\n"
    );
    let out = run_onboard_in(home.path(), &script);

    // Saved under the profile name, and announced as active.
    assert!(
        home.path().join("profiles/prod.yaml").exists(),
        "profile must be saved as profiles/prod.yaml; output:\n{out}"
    );
    assert!(
        out.contains("set as active"),
        "onboard must set the new profile active; output:\n{out}"
    );

    // Active profile in state matches the saved file — the login write target.
    let state = std::fs::read_to_string(home.path().join("state.yaml")).expect("state.yaml");
    assert!(
        state.contains("active_profile: prod"),
        "active_profile must be the onboarded profile; state:\n{state}"
    );

    // No split: onboard must not leave a stray `default.yaml` holding the config
    // while the token would go elsewhere.
    assert!(
        !home.path().join("profiles/default.yaml").exists(),
        "must not leave a split default.yaml"
    );
}

/// `AYX_PROFILE` set in the environment must not divert onboarding's save target
/// or the profile the login is run against. Onboard saves/activates the named
/// profile, and the login is dispatched with that name explicitly (not via the
/// active/`AYX_PROFILE` resolution), so save-target and login-target can't split.
#[test]
fn ayx_profile_env_does_not_divert_onboard_or_login_target() {
    let home = tempfile::tempdir().expect("tempdir");
    let script = format!(
        "prod\nuser@example.com\nhttps://us1.alteryxcloud.com/auth-portal/workspaces/{REAL_GID}\nn\nn\n"
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .arg("onboard")
        .env("AYX_CONFIG_HOME", home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("AYX_PROFILE", "some-other-profile")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ayx onboard");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(script.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        home.path().join("profiles/prod.yaml").exists(),
        "onboard must save the named profile regardless of AYX_PROFILE; output:\n{combined}"
    );
    let state = std::fs::read_to_string(home.path().join("state.yaml")).expect("state.yaml");
    assert!(
        state.contains("active_profile: prod"),
        "named profile must be activated regardless of AYX_PROFILE; state:\n{state}"
    );
    assert!(
        !home
            .path()
            .join("profiles/some-other-profile.yaml")
            .exists(),
        "AYX_PROFILE must not cause a write to a different profile file"
    );
    assert!(
        combined.contains("Log in now"),
        "login should still be offered for the onboarded profile; output:\n{combined}"
    );
}
