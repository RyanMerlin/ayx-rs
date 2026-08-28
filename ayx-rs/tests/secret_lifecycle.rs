//! Black-box coverage for the `ayx secret` lifecycle.
//!
//! These exercise the compiled binary through its public commands and JSON
//! output rather than calling internals, so they pin the contract an operator
//! or automation actually sees.
//!
//! Isolation rules, all of which matter for correctness of the assertions:
//!
//!   * Every run pins `AYX_CONFIG_HOME`, `HOME`, and `XDG_CONFIG_HOME` at a
//!     temp dir, so the host's real profile store is never read or written.
//!   * `AYX_CONFIG_HOME` also suppresses the working-directory `.env`. Without
//!     that, a suite run from a checkout containing a `.env` would inherit real
//!     credentials and report posture that has nothing to do with the fixture.
//!     `secret_status_is_independent_of_the_working_directory` pins this.
//!   * `AYX_FORCE_INLINE_SECRETS` / `AYX_ALLOW_INLINE_SECRETS` drive the
//!     keyring-unavailable path, so no test touches a real OS keyring.
//!   * Each test uses a unique sentinel and asserts it never appears in stdout,
//!     stderr, or the profile YAML.
//!
//! stdin is closed unless a test supplies it, so nothing can block on a prompt.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;

/// A profile with no credentials at all.
const EMPTY_PROFILE: &str = "profile_name: t\n\
                             alteryx_one:\n\
                            \x20 account_email: user@example.invalid\n\
                            \x20 base_url: https://example.invalid\n";

fn config_home(profiles: &[(&str, &str)]) -> TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().join("profiles");
    fs::create_dir_all(&dir).expect("profiles dir");
    for (name, body) in profiles {
        fs::write(dir.join(format!("{name}.yaml")), body).expect("write profile");
    }
    temp
}

fn profile_path(home: &TempDir, name: &str) -> std::path::PathBuf {
    home.path().join("profiles").join(format!("{name}.yaml"))
}

fn profile_text(home: &TempDir, name: &str) -> String {
    fs::read_to_string(profile_path(home, name)).expect("read profile")
}

struct Run {
    ok: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Run {
    fn combined(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("stdout should be JSON ({e})\nstdout:\n{}", self.stdout))
    }

    /// Assert the sentinel appears in neither stream. Secret *values* must
    /// never reach the operator's terminal or a log.
    fn assert_absent(&self, sentinel: &str, what: &str) {
        assert!(
            !self.combined().contains(sentinel),
            "{what} must not appear in command output\noutput:\n{}",
            self.combined()
        );
    }
}

fn run_in(
    home: &TempDir,
    cwd: &Path,
    args: &[&str],
    stdin: Option<&str>,
    env: &[(&str, &str)],
) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ayx"));
    command
        .args(args)
        .current_dir(cwd)
        .env("AYX_CONFIG_HOME", home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env_remove("AYX_FORCE_INLINE_SECRETS")
        .env_remove("AYX_ALLOW_INLINE_SECRETS")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    let output: Output = match stdin {
        Some(text) => {
            let mut child = command.stdin(Stdio::piped()).spawn().expect("spawn ayx");
            child
                .stdin
                .as_mut()
                .expect("stdin")
                .write_all(text.as_bytes())
                .expect("write stdin");
            child.wait_with_output().expect("ayx should run")
        }
        None => command
            .stdin(Stdio::null())
            .output()
            .expect("ayx should run"),
    };
    Run {
        ok: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn run(home: &TempDir, args: &[&str]) -> Run {
    run_in(home, home.path(), args, None, &[])
}

fn run_env(home: &TempDir, args: &[&str], env: &[(&str, &str)]) -> Run {
    run_in(home, home.path(), args, None, env)
}

/// Slot reports from `secret status --output json-full`, keyed by slot name.
fn slots(home: &TempDir, profile: &str) -> Vec<Value> {
    let out = run(
        home,
        &[
            "secret",
            "status",
            "--profile",
            profile,
            "--output",
            "json-full",
        ],
    );
    assert!(out.ok, "secret status should succeed\n{}", out.combined());
    out.json()["data"]["slots"]
        .as_array()
        .expect("slots array")
        .clone()
}

fn slot(home: &TempDir, profile: &str, name: &str) -> Value {
    slots(home, profile)
        .into_iter()
        .find(|s| s["slot"] == name)
        .unwrap_or_else(|| panic!("slot {name} should be reported"))
}

/// Whether this host has a usable OS keyring.
///
/// `secret set --from-stdin` and `secret migrate` write through the keyring
/// unconditionally, so on a headless host (no D-Bus / Secret Service, the usual
/// CI and container case) they cannot succeed at all. Tests that need a
/// successful secure write probe first and report a skip rather than failing on
/// an environment gate; the safety properties they share with the failure path
/// are asserted unconditionally elsewhere.
fn keyring_available() -> bool {
    let home = config_home(&[("probe", EMPTY_PROFILE)]);
    let out = run_in(
        &home,
        home.path(),
        &[
            "secret",
            "set",
            "one.client-secret",
            "--profile",
            "probe",
            "--from-stdin",
            "--output",
            "json-full",
        ],
        Some("keyring-probe"),
        &[],
    );
    // `secret set` succeeds either way now — it falls back to plaintext rather
    // than blocking — so success is not the signal. Where the secret landed is.
    out.ok && out.json()["data"]["source"] == "keyring"
}

// ---------------------------------------------------------------------------
// 1. status reports posture without disclosing values or reference targets
// ---------------------------------------------------------------------------

/// Note: this fixture deliberately carries no `keyring:` reference.
///
/// An unresolvable keyring reference is not reported per-slot — it escapes
/// profile *load* and fails the whole command, because `resolve_secret_refs`
/// propagates the error. Platforms differ on which case that is: a host with no
/// store at all yields `Ok(None)` and degrades gracefully, while macOS CI
/// registers a Keychain store with no default keychain and yields `Err`. Keyring
/// posture is covered in the `keyring_available()`-gated tests instead.
#[test]
fn secret_status_reports_posture_without_disclosing_values_or_reference_targets() {
    const PLAINTEXT: &str = "plaintext-sentinel-9f2a";
    const INLINE: &str = "inline-sentinel-7c1b";
    let home = config_home(&[(
        "mixed",
        &format!(
            "profile_name: mixed\n\
             alteryx_one:\n\
            \x20 account_email: user@example.invalid\n\
            \x20 client_secret: {PLAINTEXT}\n\
            \x20 access_token_ref: inline:{INLINE}\n\
            \x20 refresh_token_ref: env:AYX_TEST_MISSING_VARIABLE\n"
        ),
    )]);

    let out = run(
        &home,
        &[
            "secret",
            "status",
            "--profile",
            "mixed",
            "--output",
            "json-full",
        ],
    );
    assert!(out.ok, "status should succeed\n{}", out.combined());

    out.assert_absent(PLAINTEXT, "a plaintext secret value");
    out.assert_absent(INLINE, "an inline secret value");
    out.assert_absent(
        "AYX_TEST_MISSING_VARIABLE",
        "the environment variable a reference names",
    );
    // Posture is still reported for each storage form.
    assert_eq!(
        slot(&home, "mixed", "one.client-secret")["source"],
        "plaintext"
    );
    assert_eq!(slot(&home, "mixed", "one.access-token")["source"], "inline");
    assert_eq!(slot(&home, "mixed", "one.refresh-token")["source"], "env");
}

// ---------------------------------------------------------------------------
// 2. validate exit codes
// ---------------------------------------------------------------------------

#[test]
fn secret_validate_succeeds_when_every_reference_resolves() {
    let home = config_home(&[(
        "good",
        "profile_name: good\n\
         alteryx_one:\n\
        \x20 account_email: user@example.invalid\n\
        \x20 client_secret_ref: env:AYX_TEST_RESOLVABLE\n",
    )]);
    let out = run_env(
        &home,
        &["secret", "validate", "--profile", "good"],
        &[("AYX_TEST_RESOLVABLE", "resolvable-sentinel-1a")],
    );
    assert!(
        out.ok,
        "a resolvable reference must validate\n{}",
        out.combined()
    );
    out.assert_absent("resolvable-sentinel-1a", "the resolved value");
}

#[test]
fn secret_validate_fails_on_an_unresolvable_reference_with_safe_remediation() {
    let home = config_home(&[(
        "missing",
        "profile_name: missing\n\
         alteryx_one:\n\
        \x20 account_email: user@example.invalid\n\
        \x20 client_secret_ref: env:AYX_TEST_DEFINITELY_UNSET\n",
    )]);
    let out = run(&home, &["secret", "validate", "--profile", "missing"]);
    assert!(
        !out.ok,
        "an unresolvable reference must fail\n{}",
        out.combined()
    );
    assert_ne!(out.code, Some(0));
    assert!(
        out.combined().contains("secret validation failed"),
        "the failure should name the problem\n{}",
        out.combined()
    );
    out.assert_absent(
        "AYX_TEST_DEFINITELY_UNSET",
        "the variable name behind a failed reference",
    );
}

#[test]
fn secret_validate_fails_on_a_bare_reference_without_echoing_it() {
    const BARE: &str = "bare-sentinel-4d7e";
    let home = config_home(&[(
        "bare",
        &format!(
            "profile_name: bare\n\
             alteryx_one:\n\
            \x20 account_email: user@example.invalid\n\
            \x20 client_secret_ref: {BARE}\n"
        ),
    )]);
    let out = run(&home, &["secret", "validate", "--profile", "bare"]);
    assert!(!out.ok, "a bare reference must fail\n{}", out.combined());
    out.assert_absent(BARE, "the malformed reference body");
}

#[test]
fn secret_validate_treats_plaintext_as_a_warning_not_a_failure() {
    const PLAINTEXT: &str = "warn-sentinel-8b3f";
    let home = config_home(&[(
        "warn",
        &format!(
            "profile_name: warn\n\
             alteryx_one:\n\
            \x20 account_email: user@example.invalid\n\
            \x20 client_secret: {PLAINTEXT}\n"
        ),
    )]);
    let out = run(&home, &["secret", "validate", "--profile", "warn"]);
    assert!(
        out.ok,
        "plaintext is a posture warning, not a validation failure\n{}",
        out.combined()
    );
    out.assert_absent(PLAINTEXT, "the plaintext value");
    assert_eq!(
        slot(&home, "warn", "one.client-secret")["validation"],
        "warning"
    );
}

// ---------------------------------------------------------------------------
// 3. set automation safety
// ---------------------------------------------------------------------------

#[test]
fn secret_set_from_env_records_only_the_variable_name() {
    let home = config_home(&[("t", EMPTY_PROFILE)]);
    // The variable is deliberately *set* to prove the value is never read.
    let out = run_env(
        &home,
        &[
            "secret",
            "set",
            "one.client-secret",
            "--profile",
            "t",
            "--from-env",
            "AYX_TEST_FROM_ENV",
        ],
        &[("AYX_TEST_FROM_ENV", "from-env-sentinel-2c4a")],
    );
    assert!(out.ok, "--from-env should succeed\n{}", out.combined());
    out.assert_absent("from-env-sentinel-2c4a", "the environment value");

    let yaml = profile_text(&home, "t");
    assert!(
        yaml.contains("client_secret_ref: env:AYX_TEST_FROM_ENV"),
        "the profile should store only the reference\n{yaml}"
    );
    assert!(
        !yaml.contains("from-env-sentinel-2c4a"),
        "the profile must never contain the environment value\n{yaml}"
    );
}
/// The secret arriving on stdin must never be echoed, on the success path or
/// the keyring-unavailable failure path.
#[test]
fn secret_set_from_stdin_never_echoes_the_value() {
    const SECRET: &str = "stdin-sentinel-5e6f";
    let home = config_home(&[("t", EMPTY_PROFILE)]);
    let out = run_in(
        &home,
        home.path(),
        &[
            "secret",
            "set",
            "one.client-secret",
            "--profile",
            "t",
            "--from-stdin",
        ],
        Some(SECRET),
        &[],
    );

    // Holds on every path: the value must never reach the terminal or a log.
    assert!(out.ok, "the write should complete\n{}", out.combined());
    out.assert_absent(SECRET, "the secret read from stdin");

    let yaml = profile_text(&home, "t");
    if yaml.contains("client_secret_ref: keyring:") {
        assert!(
            !yaml.contains(SECRET),
            "a keyring-backed profile must not contain the value\n{yaml}"
        );
    } else {
        // Plaintext fallback: the value is in the file by design, and the
        // command must have said so.
        assert!(
            yaml.contains("client_secret_ref: inline:"),
            "the secret should be stored inline when no keyring exists\n{yaml}"
        );
        assert!(
            out.combined().contains("plaintext"),
            "the downgrade must be announced\n{}",
            out.combined()
        );
    }
}

#[test]
fn secret_set_requires_an_explicit_source_under_no_input() {
    let home = config_home(&[("t", EMPTY_PROFILE)]);
    let out = run(
        &home,
        &[
            "--no-input",
            "secret",
            "set",
            "one.client-secret",
            "--profile",
            "t",
        ],
    );
    assert!(
        !out.ok,
        "--no-input without a source must fail rather than prompt\n{}",
        out.combined()
    );
    assert!(
        out.combined().contains("--from-stdin") && out.combined().contains("--from-env"),
        "the error should name the two automation-safe sources\n{}",
        out.combined()
    );
}

#[test]
fn secret_set_rejects_two_sources_at_once() {
    let home = config_home(&[("t", EMPTY_PROFILE)]);
    let out = run(
        &home,
        &[
            "secret",
            "set",
            "one.client-secret",
            "--profile",
            "t",
            "--from-stdin",
            "--from-env",
            "AYX_TEST_X",
        ],
    );
    assert!(
        !out.ok,
        "conflicting sources must be rejected\n{}",
        out.combined()
    );
}

// ---------------------------------------------------------------------------
// 4. profile isolation
// ---------------------------------------------------------------------------
/// A slot set in two profiles must produce two independent references. Uses
/// `--from-env`, which needs no keyring, so the scoping property is asserted on
/// every host.
#[test]
fn secret_set_scopes_references_per_profile_and_does_not_collide() {
    let home = config_home(&[
        (
            "alpha",
            &EMPTY_PROFILE.replace("profile_name: t", "profile_name: alpha"),
        ),
        (
            "beta",
            &EMPTY_PROFILE.replace("profile_name: t", "profile_name: beta"),
        ),
    ]);
    for (name, var) in [
        ("alpha", "AYX_TEST_ALPHA_VAR"),
        ("beta", "AYX_TEST_BETA_VAR"),
    ] {
        let out = run(
            &home,
            &[
                "secret",
                "set",
                "one.client-secret",
                "--profile",
                name,
                "--from-env",
                var,
            ],
        );
        assert!(out.ok, "set on {name} should succeed\n{}", out.combined());
    }

    let alpha = profile_text(&home, "alpha");
    let beta = profile_text(&home, "beta");
    assert!(
        alpha.contains("env:AYX_TEST_ALPHA_VAR"),
        "alpha keeps its own ref\n{alpha}"
    );
    assert!(
        beta.contains("env:AYX_TEST_BETA_VAR"),
        "beta keeps its own ref\n{beta}"
    );
    assert!(
        !alpha.contains("AYX_TEST_BETA_VAR"),
        "alpha must not carry beta's reference\n{alpha}"
    );
    assert!(
        !beta.contains("AYX_TEST_ALPHA_VAR"),
        "beta must not carry alpha's reference\n{beta}"
    );

    // Keyring accounts are derived from the profile file stem, so the same slot
    // in two profiles must not share an account.
    if keyring_available() {
        for name in ["alpha", "beta"] {
            let out = run_in(
                &home,
                home.path(),
                &[
                    "secret",
                    "set",
                    "one.client-secret",
                    "--profile",
                    name,
                    "--from-stdin",
                ],
                Some(&format!("{name}-secret-sentinel")),
                &[],
            );
            assert!(out.ok, "set on {name} should succeed\n{}", out.combined());
        }
        let alpha = profile_text(&home, "alpha");
        let beta = profile_text(&home, "beta");
        assert!(
            alpha.contains("keyring:alpha/"),
            "alpha account is profile-scoped\n{alpha}"
        );
        assert!(
            beta.contains("keyring:beta/"),
            "beta account is profile-scoped\n{beta}"
        );
    } else {
        eprintln!("skipping keyring account-scoping assertions: no OS keyring on this host");
    }
}

/// Operating on one profile must not pull in the active profile's credentials.
#[test]
fn secret_status_does_not_overlay_the_active_profile() {
    const ACTIVE_SECRET: &str = "active-profile-sentinel-3f9c";
    let home = config_home(&[
        (
            "active",
            &format!(
                "profile_name: active\n\
                 alteryx_one:\n\
                \x20 account_email: active@example.invalid\n\
                \x20 client_secret: {ACTIVE_SECRET}\n"
            ),
        ),
        (
            "other",
            &EMPTY_PROFILE.replace("profile_name: t", "profile_name: other"),
        ),
    ]);
    let used = run(&home, &["profile", "use", "active"]);
    assert!(used.ok, "profile use should succeed\n{}", used.combined());

    let out = run(
        &home,
        &[
            "secret",
            "status",
            "--profile",
            "other",
            "--output",
            "json-full",
        ],
    );
    assert!(out.ok, "status should succeed\n{}", out.combined());
    out.assert_absent(ACTIVE_SECRET, "the active profile's secret");
    assert_eq!(
        slot(&home, "other", "one.client-secret")["source"],
        "missing",
        "the selected profile has no client secret of its own"
    );
}

/// Regression: a `.env` in the working directory must not change reported
/// posture when `AYX_CONFIG_HOME` is set.
///
/// Before this was fixed, running the suite from a checkout containing a `.env`
/// made `secret status` report three phantom `plaintext` One credentials that
/// were nowhere in the fixture, and `secret migrate` would have moved the
/// developer's real tokens into the keyring under the test profile's accounts.
#[test]
fn secret_status_is_independent_of_the_working_directory() {
    let home = config_home(&[("t", EMPTY_PROFILE)]);
    let checkout = tempfile::tempdir().expect("checkout dir");
    fs::write(
        checkout.path().join(".env"),
        "AYX_ONE_API_ACCESS_TOKEN=cwd-must-not-bleed\n\
         AYX_ONE_CLIENT_SECRET=cwd-must-not-bleed-either\n",
    )
    .expect("write .env");

    let args = [
        "secret",
        "status",
        "--profile",
        "t",
        "--output",
        "json-full",
    ];
    let neutral = run_in(&home, home.path(), &args, None, &[]);
    let from_checkout = run_in(&home, checkout.path(), &args, None, &[]);

    assert!(
        neutral.ok && from_checkout.ok,
        "status should succeed in both"
    );
    from_checkout.assert_absent("cwd-must-not-bleed", "a working-directory .env value");
    assert_eq!(
        neutral.json()["data"]["slots"],
        from_checkout.json()["data"]["slots"],
        "posture must not depend on the working directory"
    );
}

// ---------------------------------------------------------------------------
// 5. unset
// ---------------------------------------------------------------------------

#[test]
fn secret_unset_detaches_the_reference() {
    let home = config_home(&[("t", EMPTY_PROFILE)]);
    let set = run_env(
        &home,
        &[
            "secret",
            "set",
            "one.client-secret",
            "--profile",
            "t",
            "--from-env",
            "AYX_TEST_DETACH",
        ],
        &[("AYX_TEST_DETACH", "detach-sentinel-6a7b")],
    );
    assert!(set.ok, "set should succeed\n{}", set.combined());
    assert_eq!(slot(&home, "t", "one.client-secret")["source"], "env");

    let out = run(
        &home,
        &["secret", "unset", "one.client-secret", "--profile", "t"],
    );
    assert!(out.ok, "unset should succeed\n{}", out.combined());
    assert_eq!(
        slot(&home, "t", "one.client-secret")["source"],
        "missing",
        "the reference should be detached"
    );
}

/// An `env:` reference names a variable the CLI does not own. Detaching it must
/// not try to delete anything.
#[test]
fn secret_unset_does_not_disturb_an_env_reference_target() {
    let home = config_home(&[("t", EMPTY_PROFILE)]);
    run_env(
        &home,
        &[
            "secret",
            "set",
            "one.client-secret",
            "--profile",
            "t",
            "--from-env",
            "AYX_TEST_SHARED_VAR",
        ],
        &[("AYX_TEST_SHARED_VAR", "shared-sentinel-1c2d")],
    );
    let out = run_env(
        &home,
        &["secret", "unset", "one.client-secret", "--profile", "t"],
        &[("AYX_TEST_SHARED_VAR", "shared-sentinel-1c2d")],
    );
    assert!(out.ok, "unset should succeed\n{}", out.combined());
    out.assert_absent("shared-sentinel-1c2d", "the environment value");
    assert!(
        !profile_text(&home, "t").contains("env:AYX_TEST_SHARED_VAR"),
        "the reference should be gone from the profile"
    );
}

// ---------------------------------------------------------------------------
// 6. migrate
// ---------------------------------------------------------------------------

#[test]
fn secret_migrate_is_a_noop_when_nothing_is_plaintext() {
    let home = config_home(&[(
        "clean",
        "profile_name: clean\n\
         alteryx_one:\n\
        \x20 account_email: user@example.invalid\n\
        \x20 client_secret_ref: env:AYX_TEST_ALREADY_A_REF\n",
    )]);
    let before = profile_text(&home, "clean");
    let out = run(
        &home,
        &[
            "secret",
            "migrate",
            "--profile",
            "clean",
            "--output",
            "json-full",
        ],
    );
    assert!(out.ok, "migrate should succeed\n{}", out.combined());
    assert_eq!(
        out.json()["data"]["migrated_fields"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0),
        0,
        "nothing was plaintext, so nothing should be reported as migrated\n{}",
        out.combined()
    );
    assert_eq!(
        before,
        profile_text(&home, "clean"),
        "profile should be untouched"
    );
}
/// Migration moves plaintext into secure storage, strips it from the YAML, and
/// reports only fields it actually converted.
///
/// `migrate` writes through the keyring unconditionally. When none is
/// available it must fail without leaking the value and without half-writing
/// the profile, which is asserted on every host.
#[test]
fn secret_migrate_moves_plaintext_out_of_the_profile_and_reports_only_real_conversions() {
    const TOP: &str = "migrate-top-sentinel-3a4b";
    const WORKSPACE: &str = "migrate-ws-sentinel-5c6d";
    let home = config_home(&[(
        "mig",
        &format!(
            "profile_name: mig\n\
             alteryx_one:\n\
            \x20 account_email: user@example.invalid\n\
            \x20 client_secret: {TOP}\n\
            \x20 workspace_credentials:\n\
            \x20   \"12345\":\n\
            \x20     workspace_password: {WORKSPACE}\n"
        ),
    )]);
    let before = profile_text(&home, "mig");

    let out = run(
        &home,
        &[
            "secret",
            "migrate",
            "--profile",
            "mig",
            "--output",
            "json-full",
        ],
    );
    out.assert_absent(TOP, "the top-level plaintext secret");
    out.assert_absent(WORKSPACE, "the workspace plaintext secret");

    assert!(out.ok, "migrate should not fail\n{}", out.combined());
    if out.json()["data"]["warning"].is_string() {
        assert_eq!(
            before,
            profile_text(&home, "mig"),
            "an unfinished migration must leave the profile byte-identical"
        );
        eprintln!("skipping migration success assertions: no OS keyring on this host");
        return;
    }

    let migrated = out.json()["data"]["migrated_fields"]
        .as_array()
        .expect("migrated_fields array")
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>();
    assert!(
        !migrated.is_empty(),
        "two plaintext fields were present, so migration must report them\n{}",
        out.combined()
    );
    assert!(
        migrated
            .iter()
            .all(|field| !field.contains(TOP) && !field.contains(WORKSPACE)),
        "migrated_fields must name fields, never values: {migrated:?}"
    );

    let yaml = profile_text(&home, "mig");
    assert!(
        !yaml.contains(TOP) && !yaml.contains(WORKSPACE),
        "plaintext must be gone from the profile\n{yaml}"
    );
}

// ---------------------------------------------------------------------------
// 7. failure behaviour leaves no residue
// ---------------------------------------------------------------------------

/// Neither the keyring path nor the plaintext fallback may strand a
/// transaction journal or temp file beside the profile.
#[test]
fn a_secret_write_leaves_no_journal_or_temp_file() {
    const SECRET: &str = "atomic-sentinel-7e8f";
    let home = config_home(&[("t", EMPTY_PROFILE)]);
    let out = run_in(
        &home,
        home.path(),
        &[
            "secret",
            "set",
            "one.client-secret",
            "--profile",
            "t",
            "--from-stdin",
        ],
        Some(SECRET),
        &[],
    );
    assert!(out.ok, "the write should complete\n{}", out.combined());
    out.assert_absent(SECRET, "the secret being stored");

    let dir = home.path().join("profiles");
    for entry in fs::read_dir(&dir).expect("read profiles dir") {
        let name = entry
            .expect("dir entry")
            .file_name()
            .to_string_lossy()
            .into_owned();
        assert!(
            !name.ends_with(".auth-txn") && !name.ends_with(".tmp"),
            "the write must not strand {name}"
        );
    }
}

// ---------------------------------------------------------------------------
// keyring-unavailable policy
// ---------------------------------------------------------------------------

/// Without a keyring, `secret set` stores plaintext and warns loudly rather
/// than failing.
///
/// Blocking here strands anyone bootstrapping on a host with no keyring, and
/// the workaround they reach for — hand-editing the same value into the YAML —
/// produces identical plaintext with no warning at all. The warning goes to
/// stderr and into the envelope, and `doctor config` / `secret status` keep
/// reporting the posture until it is resolved.
#[test]
fn secret_set_without_keyring_stores_plaintext_and_warns() {
    const SECRET: &str = "bootstrap-sentinel-9a1c";
    let home = config_home(&[("t", EMPTY_PROFILE)]);
    if keyring_available() {
        eprintln!("skipping: this host has a keyring, so the fallback is not taken");
        return;
    }
    let out = run_in(
        &home,
        home.path(),
        &[
            "secret",
            "set",
            "one.client-secret",
            "--profile",
            "t",
            "--from-stdin",
        ],
        Some(SECRET),
        &[],
    );
    assert!(
        out.ok,
        "a missing keyring must not block bootstrapping\n{}",
        out.combined()
    );
    out.assert_absent(SECRET, "the secret being stored");
    assert!(
        out.combined().contains("plaintext"),
        "the downgrade must be stated plainly\n{}",
        out.combined()
    );
    assert!(
        profile_text(&home, "t").contains("client_secret_ref: inline:"),
        "the secret should be stored inline"
    );

    // And the posture keeps being reported afterwards.
    let entry = slot(&home, "t", "one.client-secret");
    assert_eq!(entry["source"], "inline");
    assert_eq!(entry["validation"], "warning");
}

/// The fallback needs no environment opt-in, and the warning is machine
/// readable so an agent or CI step can act on it.
#[test]
fn secret_set_fallback_warning_is_machine_readable() {
    const SECRET: &str = "optin-sentinel-2b4d";
    let home = config_home(&[("t", EMPTY_PROFILE)]);
    if keyring_available() {
        eprintln!("skipping: this host has a keyring, so the fallback is not taken");
        return;
    }
    let out = run_in(
        &home,
        home.path(),
        &[
            "secret",
            "set",
            "one.client-secret",
            "--profile",
            "t",
            "--from-stdin",
            "--output",
            "json-full",
        ],
        Some(SECRET),
        &[],
    );
    assert!(
        out.ok,
        "should succeed without any opt-in\n{}",
        out.combined()
    );
    out.assert_absent(SECRET, "the secret being stored");
    let json = out.json();
    assert_eq!(json["data"]["source"], "inline");
    assert!(
        json["data"]["warning"].is_string(),
        "the envelope must carry the warning: {}",
        json["data"]
    );
}

/// `secret migrate` moves plaintext *into* secure storage. With no keyring
/// there is nowhere better to put it, so it reports an unfinished no-op rather
/// than failing — a missing keyring is an environment condition, not a user
/// error — and never rewrites the secret as inline plaintext.
#[test]
fn secret_migrate_without_keyring_is_a_warned_no_op() {
    const PLAINTEXT: &str = "nomigrate-sentinel-6c8e";
    let home = config_home(&[(
        "m",
        &format!(
            "profile_name: m\n\
             alteryx_one:\n\
            \x20 account_email: user@example.invalid\n\
            \x20 client_secret: {PLAINTEXT}\n"
        ),
    )]);
    if keyring_available() {
        eprintln!("skipping: this host has a keyring, so migration succeeds");
        return;
    }
    let before = profile_text(&home, "m");
    let out = run(
        &home,
        &[
            "secret",
            "migrate",
            "--profile",
            "m",
            "--output",
            "json-full",
        ],
    );
    assert!(
        out.ok,
        "a missing keyring must not fail the command\n{}",
        out.combined()
    );
    out.assert_absent(PLAINTEXT, "the plaintext being migrated");
    assert_eq!(
        before,
        profile_text(&home, "m"),
        "the profile must be untouched"
    );

    let json = out.json();
    assert_eq!(
        json["data"]["migrated_fields"].as_array().map(Vec::len),
        Some(0),
        "nothing was migrated"
    );
    let warning = json["data"]["warning"].as_str().unwrap_or_default();
    assert!(
        warning.contains("alteryx_one.client_secret"),
        "the warning should name what was left behind: {warning}"
    );
}

// ---------------------------------------------------------------------------
// env-template
// ---------------------------------------------------------------------------

/// The template exists to be pasted into automation. It names variables and
/// carries no values, so it must not be redacted — the trailing `NAME=` on a
/// `_PASSWORD` slot previously tripped the value-based redaction and blanked
/// the whole thing.
#[test]
fn secret_env_template_emits_variable_names_with_empty_values() {
    let home = config_home(&[("t", EMPTY_PROFILE)]);
    let out = run(
        &home,
        &[
            "secret",
            "env-template",
            "--profile",
            "t",
            "--output",
            "json-full",
        ],
    );
    assert!(out.ok, "env-template should succeed\n{}", out.combined());

    let content = out.json()["data"]["content"]
        .as_str()
        .expect("content string")
        .to_string();
    assert_ne!(
        content, "[REDACTED]",
        "the template carries no secret material"
    );
    assert!(
        content.contains("AYX_ONE_CLIENT_SECRET="),
        "the template should name the One client-secret variable\n{content}"
    );
    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        assert!(
            line.ends_with('='),
            "every template line must have an empty value: {line}"
        );
    }
}

/// A write must record only the slot it was asked to record.
///
/// The read path injects an `env:NAME` reference for every credential-shaped
/// variable that happens to be exported, so a write that started from the
/// env-augmented view baked those ambient variables into the profile as durable
/// configuration. From then on the credential is whatever the environment
/// supplies — a CI runner's variables, a container image's `ENV`, or a `.env`
/// dropped beside the profile — and `secret status` calls that healthy.
#[test]
fn setting_one_slot_does_not_bind_ambient_environment_variables() {
    let home = config_home(&[(
        "amb",
        "profile_name: amb\n\
         alteryx_one:\n\
        \x20 account_email: user@example.invalid\n\
        \x20 base_url: https://example.invalid\n",
    )]);
    let out = run_env(
        &home,
        &[
            "secret",
            "set",
            "one.service-principal-client-secret",
            "--profile",
            "amb",
            "--from-env",
            "AYX_TEST_ASKED_FOR",
        ],
        &[
            ("AYX_TEST_ASKED_FOR", "asked-for-value"),
            ("AYX_ONE_API_ACCESS_TOKEN", "ambient-token-must-not-persist"),
            ("AYX_ONE_CLIENT_SECRET", "ambient-secret-must-not-persist"),
        ],
    );
    assert!(out.ok, "the write should succeed\n{}", out.combined());

    let text = profile_text(&home, "amb");
    assert!(
        text.contains("sp_client_secret_ref: env:AYX_TEST_ASKED_FOR"),
        "the requested slot must be recorded\n{text}"
    );
    for ambient in ["AYX_ONE_API_ACCESS_TOKEN", "AYX_ONE_CLIENT_SECRET"] {
        assert!(
            !text.contains(ambient),
            "{ambient} was never requested and must not be persisted as a binding\n{text}"
        );
    }
    // The values themselves must never land on disk either.
    for sentinel in [
        "ambient-token-must-not-persist",
        "ambient-secret-must-not-persist",
        "asked-for-value",
    ] {
        assert!(
            !text.contains(sentinel),
            "a secret value must never be written to the profile\n{text}"
        );
    }
}

/// `secret migrate` must see plaintext held *inside* an `inline:` reference.
///
/// This is the exact state the keyring-unavailable bootstrap path writes, and
/// the state migrate exists to unwind. Inline detection tested "value present
/// AND reference absent", so an `inline:<secret>` reference was invisible:
/// migrate short-circuited before touching the keyring and returned
/// `migrated: [], warning: null` with an affirmative "migration completed" —
/// an all-clear over a cleartext credential that is still on disk. The
/// remediation printed by `secret set`, by every `secret status` row, and by
/// SECURITY.md was a command that did nothing.
///
/// `AYX_FORCE_INLINE_SECRETS` makes the keyring-unavailable branch deterministic
/// so this runs identically on a developer laptop and a headless CI runner.
#[test]
fn secret_migrate_sees_plaintext_held_in_an_inline_reference() {
    const HELD: &str = "held-inside-an-inline-reference";
    let home = config_home(&[(
        "inl",
        &format!(
            "profile_name: inl\n\
             alteryx_one:\n\
            \x20 account_email: user@example.invalid\n\
            \x20 base_url: https://example.invalid\n\
            \x20 client_secret_ref: inline:{HELD}\n"
        ),
    )]);

    let out = run_env(
        &home,
        &[
            "secret",
            "migrate",
            "--profile",
            "inl",
            "--output",
            "json-full",
        ],
        &[("AYX_FORCE_INLINE_SECRETS", "1")],
    );
    assert!(
        out.ok,
        "a keyring-less host is a warned no-op, not a failure\n{}",
        out.combined()
    );

    let data = &out.json()["data"];
    let warning = data["warning"].as_str().unwrap_or("");
    assert!(
        warning.contains("alteryx_one.client_secret"),
        "migrate must name the credential it could not move\nwarning: {warning:?}\n{}",
        out.combined()
    );
    assert_eq!(
        data["migrated_fields"].as_array().map(Vec::len),
        Some(0),
        "nothing can be migrated without secure storage"
    );

    // The headline must not read as an all-clear while plaintext is on disk.
    let message = out.json()["message"].as_str().unwrap_or("").to_string();
    assert!(
        !message.contains("migration completed"),
        "an unfinished migration must not report completion: {message:?}"
    );

    // Unchanged on disk, and the secret never reaches a stream.
    assert!(
        profile_text(&home, "inl").contains("client_secret_ref: inline:"),
        "the inline reference is still there — migrate could not move it"
    );
    out.assert_absent(HELD, "the secret held inside the inline reference");
}
