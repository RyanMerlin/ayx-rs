//! Hermetic tests for `onboard`'s opt-in "log in now" step (1c).
//!
//! These drive the interactive wizard with piped stdin in an isolated config
//! home and assert only on the *decision* it makes — whether it offers the
//! immediate login or points at the next command. They never answer "yes" to
//! the login prompt, so no network call or OTP flow is ever triggered.
//!
//! Spawn-based, so gated off Windows like `cli_smoke.rs`.
#![cfg(not(windows))]

use std::process::{Command, Stdio};

use tempfile::TempDir;

const REAL_GID: &str = "01KMGF85WTTEJZ397MW1RBD9ZB";

/// Run `ayx onboard` with the given stdin script in a fresh config home.
fn run_onboard(stdin: &str) -> String {
    let home: TempDir = tempfile::tempdir().expect("tempdir");
    let mut child = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .arg("onboard")
        .env("AYX_CONFIG_HOME", home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
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
