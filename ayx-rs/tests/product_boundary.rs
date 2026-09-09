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

/// One-only email OTP whose access token has already expired:
/// `access_token_expires_at` is 2001-09-09T01:46:40Z, comfortably in the past.
/// The credential is well formed — it is simply out of date, and the remedy is
/// to sign in again, not to repair the profile.
fn one_only_expired_otp_home() -> TempDir {
    home_with_profile(
        "one-otp-expired",
        r#"profile_name: one-otp-expired
alteryx_one:
  account_email: operator@example.com
  base_url: https://us1.alteryxcloud.com
  active_workspace_id: '91946'
  workspace_credentials:
    '91946':
      workspace_id: '91946'
      credential_kind: email_otp
      access_token: test-access-token
      access_token_expires_at: 1000000000
"#,
    )
}

/// One-only, a full OAuth triple (access token, refresh token, client id)
/// under `workspace_credentials`, but with no `credential_kind` at all. This
/// is the shape a profile written before `credential_kind` existed still has:
/// it predates the label, not the capability.
fn one_only_legacy_oauth_home_without_credential_kind() -> TempDir {
    home_with_profile(
        "one-legacy-oauth",
        r#"profile_name: one-legacy-oauth
alteryx_one:
  account_email: operator@example.com
  base_url: https://us1.alteryxcloud.com
  active_workspace_id: '91946'
  workspace_credentials:
    '91946':
      workspace_id: '91946'
      access_token: test-access-token
      refresh_token: test-refresh-token
      oauth_client_id: test-client-id
"#,
    )
}

/// One-only, onboarded but never logged in: an `alteryx_one` section exists
/// (account email, base url, active workspace id) but there is no
/// `workspace_credentials` entry at all. This is a valid, realistic profile
/// shape — a profile that was set up but never authenticated — and it must
/// not be confused with a credential missing required fields (which fails
/// validation before `doctor` ever runs).
fn one_only_never_logged_in_home() -> TempDir {
    home_with_profile(
        "one-nologin",
        r#"profile_name: one-nologin
alteryx_one:
  account_email: operator@example.com
  base_url: https://us1.alteryxcloud.com
  active_workspace_id: '91946'
"#,
    )
}

/// Neither product configured: no `alteryx_one:` and no `server:` section.
fn neither_configured_home() -> TempDir {
    home_with_profile(
        "neither-configured",
        r#"profile_name: neither-configured
"#,
    )
}

/// Alteryx Server only. No `alteryx_one:` section at all.
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

#[test]
fn doctor_calls_email_otp_time_limited_not_incomplete() {
    // An email-OTP credential intentionally has no refresh token and no client
    // id.  That is the flow working as designed, not a malformed profile.  It
    // is time-limited and non-rotating, and doctor must say exactly that.
    let home = one_only_otp_home();
    let auth = doctor_check(&home, "auth");

    assert_eq!(
        auth["one_status"], "configured_time_limited",
        "OTP is a valid, time-limited credential kind:\n{auth:#}"
    );
    assert_eq!(
        auth["one"]["credential_kind"], "email_otp",
        "the credential kind must be reported:\n{auth:#}"
    );
    assert_eq!(
        auth["one"]["renews_automatically"], false,
        "OTP cannot refresh silently and must not imply it can:\n{auth:#}"
    );
    assert_eq!(
        auth["one"]["access_token_expires_at"], 4_102_444_800u64,
        "expiry is safe operational metadata and must not be redacted:\n{auth:#}"
    );

    let summary = auth["summary"].as_str().expect("summary string");
    assert!(
        !summary.contains("incomplete"),
        "a valid OTP profile must not be called incomplete: {summary}"
    );
}

#[test]
fn doctor_reports_renewal_capability_not_label_for_legacy_oauth_profile() {
    // A profile written before `credential_kind` existed still has a full
    // OAuth triple: it predates the label, not the renewal capability. The
    // refresh machinery keys off the tokens, not the label, so
    // `renews_automatically` must agree with the durable `one_status` this
    // profile already gets.
    let home = one_only_legacy_oauth_home_without_credential_kind();
    let auth = doctor_check(&home, "auth");

    assert_eq!(
        auth["one_status"], "configured",
        "a full OAuth triple is durable even without a credential_kind label:\n{auth:#}"
    );
    assert_eq!(
        auth["one"]["renews_automatically"], true,
        "a stored refresh token and client id are what renewal actually uses:\n{auth:#}"
    );
}

#[test]
fn one_only_profile_does_not_report_server_state_in_the_one_row() {
    // Alteryx Server being unconfigured is a Server fact. It must never appear
    // in the One auth row, and it must never make a valid One profile fail.
    let home = one_only_oauth_home();
    let auth = doctor_check(&home, "auth");

    let summary = auth["summary"].as_str().expect("summary string");
    assert!(
        !summary.contains("Server"),
        "the auth summary for a One-only profile must not mention Server: {summary}"
    );
    assert_eq!(
        auth["status"], "ok",
        "a complete One credential with no Server configured is not a warning:\n{auth:#}"
    );
    assert_eq!(
        doctor_check(&home, "server")["status"],
        "skip",
        "an unconfigured Server is a Server-only skip"
    );
}

#[test]
fn server_only_profile_does_not_warn_about_one() {
    // The mirror case: configuring Server must not make One look broken.
    let home = server_only_home();
    let auth = doctor_check(&home, "auth");

    assert_eq!(auth["one_status"], "not_configured", "{auth:#}");
    assert_eq!(auth["server_status"], "configured", "{auth:#}");
    assert_eq!(
        auth["status"], "ok",
        "a complete Server credential with no One configured is not a warning:\n{auth:#}"
    );
    let summary = auth["summary"].as_str().expect("summary string");
    assert!(
        !summary.contains("incomplete"),
        "nothing here is incomplete: {summary}"
    );
}

#[test]
fn combined_profile_reports_both_products_configured() {
    let home = combined_home();
    let auth = doctor_check(&home, "auth");
    assert_eq!(auth["one_status"], "configured", "{auth:#}");
    assert_eq!(auth["server_status"], "configured", "{auth:#}");
    assert_eq!(auth["status"], "ok", "{auth:#}");
}

#[test]
fn one_incomplete_profile_does_not_report_server_state_in_the_one_row() {
    // Defect 3a: a One profile that was onboarded but never logged in has no
    // `workspace_credentials` at all. It is configured-but-incomplete, and
    // Server is unconfigured here too — but the warn branch must still speak
    // only about One. Splicing in "Server not configured" blames a product
    // the operator never touched for a warning that belongs entirely to One.
    let home = one_only_never_logged_in_home();
    let auth = doctor_check(&home, "auth");

    assert_eq!(auth["one_status"], "incomplete", "{auth:#}");
    assert_eq!(auth["server_status"], "not_configured", "{auth:#}");
    assert_eq!(auth["status"], "warn", "{auth:#}");

    let summary = auth["summary"].as_str().expect("summary string");
    assert!(
        !summary.contains("Server"),
        "a One-only warning must not mention Server, which was never configured: {summary}"
    );
    assert!(
        summary.contains("One"),
        "the warning should still say something about One: {summary}"
    );
}

#[test]
fn otp_only_profile_ok_summary_keeps_the_time_limited_nuance() {
    // Defect 3b: after the OTP classification fix, an OTP-only profile with no
    // Server reaches the ok branch — but plain "One auth configured" loses the
    // fact that this is a time-limited login, not a durable credential. The
    // clause builder must render the same nuance in the ok branch that the
    // warn branch already carried.
    let home = one_only_otp_home();
    let auth = doctor_check(&home, "auth");

    assert_eq!(auth["one_status"], "configured_time_limited", "{auth:#}");
    assert_eq!(auth["status"], "ok", "{auth:#}");
    assert_eq!(
        auth["summary"], "One auth configured (time-limited login)",
        "{auth:#}"
    );
}

#[test]
fn neither_product_configured_reports_a_single_skip_with_one_message() {
    // There must be exactly one skip path for "neither product configured",
    // reached through the clause builder itself (an empty `clauses` vec)
    // rather than a separate, earlier-returning condition with different
    // wording. This exercises that path end-to-end through the real profile
    // loader and `ayx doctor auth`, not just the unit-level function.
    let home = neither_configured_home();
    let auth = doctor_check(&home, "auth");

    assert_eq!(auth["status"], "skip", "{auth:#}");
    assert_eq!(
        auth["summary"], "No Alteryx One or Server auth configured",
        "{auth:#}"
    );
}

#[test]
fn network_check_does_not_warn_merely_because_no_probe_ran() {
    // Endpoints being configured with no live probe run is the normal, healthy
    // state — `doctor` deliberately does not make invasive network calls.
    // Reporting it as a warning trains operators to ignore warnings.
    let home = one_only_oauth_home();
    let network = doctor_check(&home, "network");

    assert_eq!(
        network["status"], "ok",
        "configured endpoints with no probe are healthy, not a warning:\n{network:#}"
    );
    let summary = network["summary"].as_str().expect("summary string");
    assert!(
        !summary.contains("Server"),
        "a One-only profile's network row must not mention Server: {summary}"
    );
    assert_eq!(
        network["probes_run"], false,
        "say plainly that no probe ran rather than implying a fault:\n{network:#}"
    );
}

#[test]
fn server_check_does_not_warn_merely_because_no_probe_ran() {
    // Mirror of the network fix: a fully configured Alteryx Server with no
    // live validation run is the normal, healthy state, not a warning.
    let home = server_only_home();

    assert_eq!(
        doctor_check(&home, "server")["status"],
        "ok",
        "a fully configured Server with no live validation run is healthy, not a warning"
    );
}

#[test]
fn one_api_commands_never_require_server_configuration() {
    // Every command under `ayx one` belongs to Alteryx One. Requiring the
    // Server `api:` section under a One namespace is a product-boundary
    // defect, not an incomplete One profile.
    let home = one_only_oauth_home();

    for command in [vec!["one", "api", "status"], vec!["one", "api", "diagnose"]] {
        let envelope = run_ayx(&home, &command);
        let label = command.join(" ");

        if envelope["ok"] == false {
            let code = envelope["error_code"].as_str().unwrap_or_default();
            assert_ne!(
                code, "config_missing",
                "`ayx {label}` must not fail for missing Server config:\n{envelope:#}"
            );
            let text = serde_json::to_string(&envelope).unwrap_or_default();
            assert!(
                !text.contains("server_api"),
                "`ayx {label}` must not mention the Server API section:\n{envelope:#}"
            );
        }
    }
}

#[test]
fn one_api_status_reports_the_one_surface() {
    let home = one_only_oauth_home();
    let envelope = run_ayx(&home, &["one", "api", "status"]);
    assert_eq!(envelope["ok"], true, "{envelope:#}");

    let data = &envelope["data"];
    assert_eq!(data["product"], "Alteryx One", "{data:#}");
    assert_eq!(
        data["base_url"], "https://us1.alteryxcloud.com",
        "the One base URL, not a Server one:\n{data:#}"
    );
    assert_eq!(data["workspace_id"], "91946", "{data:#}");
    assert_eq!(data["credential_kind"], "oauth_refresh", "{data:#}");

    // `has_credentials` exists to answer present/absent. Redaction recurses
    // into it and matches children on their own names, so keys spelled
    // `access_token` / `refresh_token` were rewritten to the string
    // "[REDACTED]" -- a truthy value that reports an absent credential as
    // present. The `_present` suffix is metadata-exempt and matches what
    // `doctor auth` emits, so the two surfaces agree.
    let has = &data["has_credentials"];
    assert_eq!(
        has["access_token_present"],
        Value::Bool(true),
        "has_credentials must survive redaction as real booleans:
{data:#}"
    );
    assert_eq!(
        has["refresh_token_present"],
        Value::Bool(true),
        "has_credentials must survive redaction as real booleans:
{data:#}"
    );
    assert_eq!(
        has["oauth_client_id_present"],
        Value::Bool(true),
        "has_credentials must survive redaction as real booleans:
{data:#}"
    );

    // No credential value may appear anywhere in the envelope.
    let text = serde_json::to_string(&envelope).expect("serialize");
    assert!(!text.contains("test-access-token"), "access token leaked");
    assert!(!text.contains("test-refresh-token"), "refresh token leaked");
}

#[test]
fn no_one_catalog_entry_declares_a_server_api_prerequisite() {
    // `ayx catalog list --format full` is local, machine-readable metadata --
    // no network call, no live profile requirement. Every `one/...` entry's
    // `prerequisites` list must never name `server_api`: Alteryx One and
    // Alteryx Server are independent products, and no `ayx one` command may
    // require Alteryx Server configuration.
    let home = one_only_oauth_home();
    let envelope = run_ayx(&home, &["catalog", "list", "--format", "full"]);
    assert_eq!(envelope["ok"], true, "{envelope:#}");

    let commands = envelope["data"]["commands"]
        .as_array()
        .expect("commands array");
    assert!(!commands.is_empty(), "expected catalog entries");

    let offenders: Vec<String> = commands
        .iter()
        .filter(|entry| {
            let path = entry["path"].as_str().unwrap_or_default();
            let prereqs = entry["prerequisites"].as_array();
            path.starts_with("one/")
                && prereqs.is_some_and(|list| list.iter().any(|p| p.as_str() == Some("server_api")))
        })
        .map(|entry| entry["path"].as_str().unwrap_or_default().to_string())
        .collect();

    assert!(
        offenders.is_empty(),
        "one/... catalog entries falsely require Alteryx Server config: {offenders:?}"
    );
}

/// Run any command's `--help` against an isolated home and return it with
/// whitespace collapsed, so assertions do not depend on the terminal width
/// clap wrapped at.
fn unwrapped_help(home: &TempDir, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(args)
        .arg("--help")
        .env("AYX_CONFIG_HOME", home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .output()
        .expect("ayx binary should run");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn one_login_help_states_the_otp_lifetime_and_names_the_durable_path() {
    // The OTP flow returns no refresh token and expires after 30 days. Help
    // must say so where the flow is offered, and must point at the path that
    // does renew silently.
    let home = one_only_otp_home();
    let help = unwrapped_help(&home, &["one", "login"]);

    assert!(
        help.contains("30 days") || help.contains("time-limited"),
        "OTP's lifetime must be stated where it is offered:\n{help}"
    );
    assert!(
        help.contains("--oauth-api-token"),
        "the durable path must be named alongside it:\n{help}"
    );
    assert!(
        !help.contains("stay signed in") && !help.contains("stays signed in"),
        "secure storage protects a time-limited credential; it does not make \
         it durable:\n{help}"
    );
}

#[test]
fn doctor_guidance_recommends_the_durable_path_without_calling_otp_invalid() {
    // doctor may recommend upgrading to a renewing credential. It must not
    // describe the configured OTP credential as invalid or malformed.
    //
    // RULING P2 (see `doctor_check`): the brief wrote `doctor_checks(&home)`,
    // which does not exist and would have shelled out to bare `ayx doctor` and
    // its live workspace probe. `doctor auth` is the hermetic equivalent.
    let home = one_only_otp_home();
    let auth = doctor_check(&home, "auth");
    let text = serde_json::to_string(&auth).expect("serialize");

    assert!(
        !text.contains("malformed") && !text.contains("invalid"),
        "a working OTP credential is neither malformed nor invalid:\n{auth:#}"
    );
    assert_eq!(
        auth["one"]["renews_automatically"], false,
        "state plainly that OTP does not renew:\n{auth:#}"
    );
    // The recommendation is an upgrade, not a repair.
    let guidance = auth["one"]["guidance"]
        .as_str()
        .expect("OTP guidance must be present");
    assert!(
        guidance.contains("--oauth-api-token"),
        "doctor should name the durable path: {guidance}"
    );
    assert!(
        !guidance.contains("fix") && !guidance.contains("repair") && !guidance.contains("expired"),
        "the OTP credential is working as designed; this is an upgrade, not a \
         repair: {guidance}"
    );
}

#[test]
fn one_api_status_reports_absent_credentials_as_false_not_a_redaction_marker() {
    // An email-OTP profile has no refresh token and no client id. An agent
    // reading `has_credentials` must see `false`, not the truthy string
    // "[REDACTED]" that recursive key-name redaction used to substitute.
    let home = one_only_otp_home();
    let envelope = run_ayx(&home, &["one", "api", "status"]);
    assert_eq!(envelope["ok"], true, "{envelope:#}");

    let has = &envelope["data"]["has_credentials"];
    assert_eq!(has["access_token_present"], Value::Bool(true), "{has:#}");
    assert_eq!(has["refresh_token_present"], Value::Bool(false), "{has:#}");
    assert_eq!(
        has["oauth_client_id_present"],
        Value::Bool(false),
        "{has:#}"
    );

    let text = serde_json::to_string(has).expect("serialize");
    assert!(
        !text.contains("REDACTED"),
        "presence booleans must not be redacted: {text}"
    );
}

#[test]
fn expired_email_otp_credential_is_reported_as_expired_not_as_healthy() {
    // Before this, `configured_time_limited` was unconditionally `ok`: a
    // 40-day-old OTP profile reported green while its expiry sat in the past.
    // Expiry is operational metadata that doctor already emits; it must also
    // be judged.
    let home = one_only_expired_otp_home();
    let auth = doctor_check(&home, "auth");

    assert_eq!(
        auth["one_status"], "expired",
        "an expired OTP access token is not `configured_time_limited`:
{auth:#}"
    );
    assert_eq!(
        auth["status"], "warn",
        "an expired credential must not report `ok`:
{auth:#}"
    );
    let summary = auth["summary"].as_str().unwrap_or_default();
    assert!(
        summary.contains("expired"),
        "say plainly that it expired: {summary}"
    );
    // The field must answer the question, not be rewritten into a truthy
    // "[REDACTED]" string by output redaction.
    assert_eq!(
        auth["one"]["access_token_expired"],
        Value::Bool(true),
        "access_token_expired must survive redaction as a real boolean:
{auth:#}"
    );
    let text = serde_json::to_string(&auth).expect("serialize");
    assert!(
        !text.contains("malformed") && !text.contains("invalid"),
        "an expired credential is out of date, not malformed:
{auth:#}"
    );
    let guidance = auth["one"]["guidance"]
        .as_str()
        .expect("expired OTP guidance must be present");
    assert!(
        guidance.contains("ayx one login"),
        "the remedy for an expired OTP credential is to sign in again: {guidance}"
    );
}

#[test]
fn unexpired_email_otp_credential_stays_ok() {
    // The other side of the expiry check: a live OTP credential must keep
    // reporting `configured_time_limited` and `ok`.
    let home = one_only_otp_home();
    let auth = doctor_check(&home, "auth");

    assert_eq!(auth["one_status"], "configured_time_limited", "{auth:#}");
    assert_eq!(auth["status"], "ok", "{auth:#}");
}

#[test]
fn a_one_only_profile_does_not_report_a_healthy_mongo_check() {
    // Mongo is an Alteryx Server-side domain. `MongoMode` defaults to
    // `Embedded`, so a profile that never mentions Mongo -- and configures no
    // Server at all -- used to report an active, healthy Mongo check.
    let home = one_only_oauth_home();
    let mongo = doctor_check(&home, "mongo");

    assert_eq!(
        mongo["status"], "skip",
        "nothing Mongo-related is configured here:
{mongo:#}"
    );

    // A Server-only profile still gets a real embedded-Mongo check.
    let server_home = server_only_home();
    let server_mongo = doctor_check(&server_home, "mongo");
    assert_eq!(
        server_mongo["status"], "ok",
        "an Alteryx Server profile has a genuine embedded Mongo:
{server_mongo:#}"
    );
}

#[test]
fn server_api_status_reports_credential_presence_as_booleans() {
    // The same redaction defect fixed for `ayx one api status` lived in the
    // Server sibling: `curator_api_secret` matched the `secret` needle on its
    // own name and became the truthy string "[REDACTED]".
    let home = server_only_home();
    let envelope = run_ayx(&home, &["server", "api", "status"]);
    assert_eq!(envelope["ok"], true, "{envelope:#}");

    let has = &envelope["data"]["has_credentials"];
    assert_eq!(has["curator_api_key_present"], Value::Bool(true), "{has:#}");
    assert_eq!(
        has["curator_api_secret_present"],
        Value::Bool(true),
        "{has:#}"
    );
    let text = serde_json::to_string(has).expect("serialize");
    assert!(
        !text.contains("REDACTED"),
        "presence booleans must not be redacted: {text}"
    );
    assert!(
        !text.contains("test-secret"),
        "the secret value must never appear: {text}"
    );
}
