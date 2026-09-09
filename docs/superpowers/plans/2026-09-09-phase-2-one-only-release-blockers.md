# Phase 2: One-Only Windows Release Blockers — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make an Alteryx One-only profile a fully supported, honestly reported configuration on Windows — `doctor` reads the credentials login actually writes, classifies email OTP as time-limited rather than broken, keeps One and Server as independent products, and no command under `ayx one` requires Alteryx Server configuration.

**Architecture:** Four defects share one root shape: code reads or reports the *wrong product's* state. `doctor`'s auth check reads legacy top-level credential fields while `ayx one login` writes workspace-scoped ones; its summaries splice Server state into the One row; `ayx one api status|diagnose` calls a Server-API helper. Each fix is small and local. The durable protection is a new hermetic test binary that builds isolated `AYX_CONFIG_HOME` profiles — One-only OTP, One-only OAuth, Server-only, and combined — and asserts product independence. Every fix in this plan is written test-first against that harness.

**Tech Stack:** Rust 2024 edition, rustc 1.97.1, clap 4 (`derive` + `wrap_help`), serde/serde_yaml/serde_json, tempfile, cargo-nextest.

**Spec:** `docs/roadmap/operator-followups.md` — sections "Root `doctor` must preserve product boundaries", "`ayx one api` must be a One surface, not a Server API dependency", "Authentication lifecycle and onboarding contract", and "Product boundary and documentation architecture". Phase 2 of that document's "Fresh-session execution sequence".

## Global Constraints

- Rust edition 2024, `rust-version = "1.97.1"`. Do not raise either.
- `Cargo.lock` is committed. CI builds with `--locked`. Never gitignore it.
- Every task ends green on: `cargo fmt --all`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo nextest run --workspace --locked`.
- Baseline at plan start is **1054 tests passing, 22 skipped** on the integration base plus the merged onboarding fix. Test count only ever goes up.
- Product naming is load-bearing. `ayx one` is **Alteryx One**; `ayx server` is **Alteryx Server**; Mongo is its own local data-store domain. Never let one product's state appear in another's row, summary, or remediation text.
- No credential value may appear in any output format. Expiry, credential kind, token source, workspace identity, and request ids are safe operational metadata and **must not** be redacted.
- Windows is the release-validation platform. All tests must pass on Windows; do not use Unix-only path or permission assumptions in test code.
- Tests must be hermetic: always set `AYX_CONFIG_HOME`, `HOME`, and `XDG_CONFIG_HOME` to a `tempfile::tempdir()`. A test must never read the developer's real profile or make a network call.

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `ayx-rs/tests/product_boundary.rs` | Hermetic product-independence regression suite. Owns the profile fixture builders and every One-only / Server-only / combined assertion. | **Create** |
| `ayx-rs/src/main.rs:5551-5623` | `doctor_auth_envelope` — reads One and Server credential presence. | Modify |
| `ayx-rs/src/main.rs:5625-5658` | `doctor_network_envelope` — reports configured endpoints. | Modify |
| `ayx-rs/src/main.rs:5811-5880` | `doctor_auth_status_summary`, `auth_product_status`, `doctor_network_status_summary` — the conflated rollup text. | Modify |
| `ayx-one-api/src/lib.rs:807-880` | `api_status_envelope` / `api_diagnose_envelope` — currently Server-API-only, shared by One and license surfaces. | Modify |
| `ayx-rs/src/cmd/one_api/mod.rs:11-38` | `ayx one api` dispatch. | Modify |

Why one test file rather than a shared `tests/common/` module: this repo has no shared test-module convention today — `ayx-rs/tests/fixtures/` holds JSON data only, and each existing test binary carries its own local helpers (see `run_onboard_in` in `ayx-rs/tests/onboard_login_offer.rs:24`). Follow the established pattern.

---

## Evidence: all four defects reproduced

Reproduced on 2026-09-09 against the Phase 1 integration base binary, using a hermetic `AYX_CONFIG_HOME`. Quote these in commit messages.

**A. `doctor` ignores workspace-scoped credentials entirely.** A profile with a complete OAuth-refresh credential under `workspace_credentials['91946']` — access token, refresh token, and client id all present — reports:

```text
one_status : incomplete
summary    : One incomplete; Server not configured
one fields : {"access_token_present": false, "refresh_token_present": false,
              "oauth_client_id_present": false, "configured": true}
```

Every field is `false` because `doctor_auth_envelope` reads the legacy top-level `alteryx_one.access_token` / `.refresh_token` / `.oauth_client_id`, while `ayx one login` writes workspace-scoped credentials. **This is not recorded in the tracker and is the most severe of the four**: `doctor` misreports *every* modern profile regardless of credential kind, so fixing only the OTP classification would leave the real bug in place.

**B. Email OTP is classified as a broken configuration.** A One-only OTP profile yields `one_status: "incomplete"` because `doctor_auth_status_summary` (`main.rs:5824`) requires refresh token *and* client id for readiness. An OTP credential intentionally has neither.

**C. Product conflation.** The same One-only profile produces `summary: "One incomplete; Server not configured"` — one row, two products — and `overall: "fail"` for a valid configuration.

**D. `ayx one api` requires Server config.** Under the same One-only profile:

```text
ayx one api status    -> ok: false, error_code: config_missing
ayx one api diagnose  -> ok: false, error_code: config_missing
```

`ayx-rs/src/cmd/one_api/mod.rs:15` calls `api_status_envelope(&config, "one")`, which reads `config.api` (the Server API section) at `ayx-one-api/src/lib.rs:811`. The file's own doc comment says "One only". Note that the sibling subcommands `open-api-spec` and `coverage` *are* correctly One-native, so this group is a genuine hybrid.

---

### Task 1: Product-boundary harness + `doctor` reads the credentials login writes

Fixes defect A. Creates the test file every later task builds on.

**Files:**
- Create: `ayx-rs/tests/product_boundary.rs`
- Modify: `ayx-rs/src/main.rs:5551-5578` (the seven `one_*_present` / `secret_source` reads in `doctor_auth_envelope`)

**Interfaces:**
- Consumes: nothing.
- Produces, for every later task in this plan:
  - `fn one_only_oauth_home() -> TempDir` — One configured, workspace-scoped `credential_kind: oauth_refresh`, complete credentials.
  - `fn one_only_otp_home() -> TempDir` — One configured, workspace-scoped `credential_kind: email_otp`, access token + `access_token_expires_at`, deliberately no refresh token and no client id.
  - `fn server_only_home() -> TempDir` — `server:` section only, no `alteryx_one:`.
  - `fn combined_home() -> TempDir` — both One (oauth_refresh) and Server.
  - `fn doctor_checks(home: &TempDir) -> Value` — runs `ayx doctor --output json-full` and returns `data.checks`.
  - `fn run_ayx(home: &TempDir, args: &[&str]) -> Value` — runs any command with `--output json-full`, returns the parsed envelope.

- [ ] **Step 1: Write the failing test**

Create `ayx-rs/tests/product_boundary.rs`:

```rust
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

/// `ayx doctor`'s per-check map.
fn doctor_checks(home: &TempDir) -> Value {
    run_ayx(home, &["doctor"])["data"]["checks"].clone()
}

#[test]
fn doctor_reads_workspace_scoped_one_credentials() {
    // `ayx one login` writes credentials under
    // `alteryx_one.workspace_credentials[<workspace id>]`, not the legacy
    // top-level fields.  Doctor must read what login writes, or it reports
    // every modern profile as having no credentials at all.
    let home = one_only_oauth_home();
    let auth = doctor_checks(&home)["auth"].clone();

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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo nextest run -p ayx-rs --test product_boundary doctor_reads_workspace_scoped_one_credentials`

Expected: FAIL. All three `_present` fields are `false` and `one_status` is `"incomplete"`, matching Evidence A.

- [ ] **Step 3: Read the resolved credential once, then use it**

In `ayx-rs/src/main.rs`, inside `doctor_auth_envelope`, replace the legacy field reads. The `resolved_*` accessors already exist on `AlteryxOneProfile` (`ayx-core/src/profile.rs:936-1000`) and transparently prefer the active workspace credential, falling back to the legacy top-level field.

Replace:

```rust
    let one_access_token_present = one
        .and_then(|v| v.access_token.as_ref())
        .is_some_and(|v| !v.trim().is_empty());
    let one_refresh_token_present = one
        .and_then(|v| v.refresh_token.as_ref())
        .is_some_and(|v| !v.trim().is_empty());
    let one_oauth_client_id_present = one
        .and_then(|v| v.oauth_client_id.as_ref())
        .is_some_and(|v| !v.trim().is_empty());
```

with:

```rust
    // Read through the workspace-scoped credential that `ayx one login` writes.
    // `resolved_*` falls back to the legacy top-level field for older profiles.
    let one_access_token_present = one
        .and_then(|v| v.resolved_access_token())
        .is_some_and(|v| !v.trim().is_empty());
    let one_refresh_token_present = one
        .and_then(|v| v.resolved_refresh_token())
        .is_some_and(|v| !v.trim().is_empty());
    let one_oauth_client_id_present = one
        .and_then(|v| v.resolved_oauth_client_id())
        .is_some_and(|v| !v.trim().is_empty());
    // The active workspace credential also carries the `*_ref` secure-storage
    // pointers used for source reporting below.
    let one_credential = one.and_then(|v| v.active_workspace_credential());
```

- [ ] **Step 4: Report the credential source from the same place**

Still in `doctor_auth_envelope`, in the `"one"` JSON object, replace:

```rust
                "access_token_source": secret_source(
                    one.and_then(|v| v.access_token_ref.as_ref()),
                    one.and_then(|v| v.access_token.as_deref()),
                ),
                "refresh_token_source": secret_source(
                    one.and_then(|v| v.refresh_token_ref.as_ref()),
                    one.and_then(|v| v.refresh_token.as_deref()),
                ),
```

with:

```rust
                "access_token_source": secret_source(
                    one_credential
                        .and_then(|c| c.access_token_ref.as_ref())
                        .or_else(|| one.and_then(|v| v.access_token_ref.as_ref())),
                    one.and_then(|v| v.resolved_access_token()),
                ),
                "refresh_token_source": secret_source(
                    one_credential
                        .and_then(|c| c.refresh_token_ref.as_ref())
                        .or_else(|| one.and_then(|v| v.refresh_token_ref.as_ref())),
                    one.and_then(|v| v.resolved_refresh_token()),
                ),
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo nextest run -p ayx-rs --test product_boundary doctor_reads_workspace_scoped_one_credentials`
Expected: PASS

- [ ] **Step 6: Run the full suite and lints**

Run:
```bash
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
```
Expected: all green, test count 1055 or higher. If an existing `cli_smoke` or doctor test asserted the *old* wrong values, that test encoded the bug — fix the assertion and say so in the commit message.

- [ ] **Step 7: Commit**

```bash
git add ayx-rs/tests/product_boundary.rs ayx-rs/src/main.rs
git commit -m "fix(doctor): read the One credentials that login actually writes

doctor_auth_envelope read the legacy top-level alteryx_one.access_token,
.refresh_token, and .oauth_client_id fields, but \`ayx one login\` writes
workspace-scoped credentials under workspace_credentials[<id>]. Every
modern profile therefore reported access_token_present, refresh_token_present,
and oauth_client_id_present as false and one_status as incomplete, no matter
which credential kind it held.

Read through the existing resolved_* accessors, which prefer the active
workspace credential and fall back to the legacy field for older profiles,
and report the secure-storage source from the same credential.

Adds ayx-rs/tests/product_boundary.rs, a hermetic suite that builds isolated
AYX_CONFIG_HOME profiles for the One-only, Server-only, and combined cases."
```

---

### Task 2: `doctor` describes email OTP as time-limited, not incomplete

Fixes defect B. Depends on Task 1.

**Files:**
- Modify: `ayx-rs/src/main.rs:5551-5623` (`doctor_auth_envelope`), `5811-5860` (`doctor_auth_status_summary`, `auth_product_status`)
- Test: `ayx-rs/tests/product_boundary.rs`

**Interfaces:**
- Consumes: Task 1's fixture builders and `doctor_checks`.
- Produces: the One auth status vocabulary used by Task 3 — `"configured"`, `"configured_time_limited"`, `"incomplete"`, `"not_configured"`.

- [ ] **Step 1: Write the failing test**

Append to `ayx-rs/tests/product_boundary.rs`:

```rust
#[test]
fn doctor_calls_email_otp_time_limited_not_incomplete() {
    // An email-OTP credential intentionally has no refresh token and no client
    // id.  That is the flow working as designed, not a malformed profile.  It
    // is time-limited and non-rotating, and doctor must say exactly that.
    let home = one_only_otp_home();
    let auth = doctor_checks(&home)["auth"].clone();

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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo nextest run -p ayx-rs --test product_boundary doctor_calls_email_otp_time_limited_not_incomplete`
Expected: FAIL — `one_status` is `"incomplete"`, and `credential_kind`, `renews_automatically`, and `access_token_expires_at` are absent.

- [ ] **Step 3: Make One readiness credential-kind aware**

In `ayx-rs/src/main.rs`, add above `auth_product_status`:

```rust
/// One's auth readiness, judged against what the profile's credential kind can
/// actually provide. An email-OTP credential has no refresh token and no client
/// id by design: it is valid but time-limited, and reporting it as incomplete
/// sends operators looking for a configuration error that does not exist.
fn one_auth_product_status(
    configured: bool,
    credential_kind: Option<ayx_core::profile::OneCredentialKind>,
    access_token_present: bool,
    refresh_token_present: bool,
    oauth_client_id_present: bool,
) -> &'static str {
    use ayx_core::profile::OneCredentialKind;
    if !configured {
        return "not_configured";
    }
    match credential_kind {
        Some(OneCredentialKind::EmailOtp) => {
            if access_token_present {
                "configured_time_limited"
            } else {
                "incomplete"
            }
        }
        Some(OneCredentialKind::OAuthRefresh) => {
            if access_token_present && refresh_token_present && oauth_client_id_present {
                "configured"
            } else {
                "incomplete"
            }
        }
        // An unlabelled profile predates credential_kind. Judge it by what it
        // holds: a complete OAuth triple is durable, a lone access token is not.
        None => {
            if access_token_present && refresh_token_present && oauth_client_id_present {
                "configured"
            } else if access_token_present {
                "configured_time_limited"
            } else {
                "incomplete"
            }
        }
    }
}
```

- [ ] **Step 4: Use it and report the lifecycle facts**

In `doctor_auth_envelope`, after the `one_credential` binding from Task 1, add:

```rust
    let one_credential_kind = one.and_then(|v| v.resolved_credential_kind());
    let one_access_token_expires_at = one.and_then(|v| v.resolved_access_token_expires_at());
    let one_renews_automatically =
        one_credential_kind == Some(ayx_core::profile::OneCredentialKind::OAuthRefresh)
            && one_refresh_token_present
            && one_oauth_client_id_present;
```

Replace the existing `one_status` binding:

```rust
    let one_status = auth_product_status(
        one_configured,
        one_access_token_present && one_refresh_token_present && one_oauth_client_id_present,
    );
```

with:

```rust
    let one_status = one_auth_product_status(
        one_configured,
        one_credential_kind,
        one_access_token_present,
        one_refresh_token_present,
        one_oauth_client_id_present,
    );
```

Add to the `"one"` JSON object:

```rust
                "credential_kind": one_credential_kind.map(|kind| match kind {
                    ayx_core::profile::OneCredentialKind::EmailOtp => "email_otp",
                    ayx_core::profile::OneCredentialKind::OAuthRefresh => "oauth_refresh",
                }),
                "renews_automatically": one_renews_automatically,
                "access_token_expires_at": one_access_token_expires_at,
```

- [ ] **Step 5: Stop the rollup calling OTP incomplete**

In `doctor_auth_status_summary`, the `one_ready` computation hardcodes the OAuth triple. Change the signature's One arguments to take the already-computed status. Replace:

```rust
    let one_ready = !one_configured
        || (one_access_token_present && one_refresh_token_present && one_oauth_client_id_present);
```

with:

```rust
    // `one_status` already accounts for credential kind; do not re-derive
    // readiness here or OTP will be called incomplete again.
    let one_ready = !one_configured || one_status != "incomplete";
```

and add `one_status: &str` as a parameter, replacing the three `one_*_present` bools. Update the `one_label` arm to match:

```rust
        let one_label = match one_status {
            "not_configured" => "One not configured",
            "configured" => "One configured",
            "configured_time_limited" => "One configured (time-limited login)",
            _ => "One incomplete",
        };
```

Update the call site in `doctor_auth_envelope` to pass `one_status`, and move the `let one_status = ...` binding above the `doctor_auth_status_summary` call.

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo nextest run -p ayx-rs --test product_boundary doctor_calls_email_otp_time_limited_not_incomplete`
Expected: PASS

- [ ] **Step 7: Run the full suite and lints**

Run:
```bash
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
```
Expected: all green.

- [ ] **Step 8: Commit**

```bash
git add ayx-rs/tests/product_boundary.rs ayx-rs/src/main.rs
git commit -m "fix(doctor): report email OTP as time-limited, not incomplete

An email-OTP credential has no refresh token and no OAuth client id by
design. doctor required all three for readiness, so a working OTP login was
reported incomplete and operators were sent looking for a configuration
error that does not exist.

Judge One readiness against the credential kind the profile actually holds,
and report credential_kind, renews_automatically, and access_token_expires_at
as the operational metadata they are. Expiry is safe to display: it is not a
credential value."
```

---

### Task 3: `doctor` reports One and Server as independent domains

Fixes defect C. Depends on Task 2.

**Files:**
- Modify: `ayx-rs/src/main.rs:5811-5852` (`doctor_auth_status_summary`)
- Test: `ayx-rs/tests/product_boundary.rs`

**Interfaces:**
- Consumes: Task 2's `one_status` vocabulary.
- Produces: the guarantee later tasks rely on — a product's row mentions only that product.

- [ ] **Step 1: Write the failing test**

Append to `ayx-rs/tests/product_boundary.rs`:

```rust
#[test]
fn one_only_profile_does_not_report_server_state_in_the_one_row() {
    // Alteryx Server being unconfigured is a Server fact. It must never appear
    // in the One auth row, and it must never make a valid One profile fail.
    let home = one_only_oauth_home();
    let checks = doctor_checks(&home);
    let auth = checks["auth"].clone();

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
        checks["server"]["status"], "skip",
        "an unconfigured Server is a Server-only skip:\n{checks:#}"
    );
}

#[test]
fn server_only_profile_does_not_warn_about_one() {
    // The mirror case: configuring Server must not make One look broken.
    let home = server_only_home();
    let checks = doctor_checks(&home);
    let auth = checks["auth"].clone();

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
    let auth = doctor_checks(&home)["auth"].clone();
    assert_eq!(auth["one_status"], "configured", "{auth:#}");
    assert_eq!(auth["server_status"], "configured", "{auth:#}");
    assert_eq!(auth["status"], "ok", "{auth:#}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p ayx-rs --test product_boundary`
Expected: the three new tests FAIL. `summary` is `"One configured; Server not configured"` and `status` is `"warn"`.

- [ ] **Step 3: Make an unconfigured product a non-event**

In `doctor_auth_status_summary`, an unconfigured product currently forces the `warn` branch through the `{one_label}; {server_label}` format. Replace the whole `if !one_ready || !server_ready { ... }` block and the trailing `match` with:

```rust
    // Each product contributes its own clause, and only when it is configured.
    // A product that is not configured contributes nothing: it is that
    // product's own `skip` row, not a warning attached to its neighbour.
    let mut clauses: Vec<String> = Vec::new();
    if one_configured {
        clauses.push(
            match one_status {
                "configured" => "One auth configured",
                "configured_time_limited" => "One auth configured (time-limited login)",
                _ => "One auth incomplete",
            }
            .to_string(),
        );
    }
    if server_configured {
        clauses.push(
            if server_ready {
                "Server auth configured"
            } else {
                "Server auth incomplete"
            }
            .to_string(),
        );
    }

    if clauses.is_empty() {
        return ("skip", "No Alteryx One or Server auth configured".to_string());
    }

    let status = if !one_ready || !server_ready {
        "warn"
    } else {
        "ok"
    };
    (status, clauses.join("; "))
```

Delete `auth_product_status` if Task 2 left it with no remaining callers; keep it if the Server path still uses it.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run -p ayx-rs --test product_boundary`
Expected: PASS

- [ ] **Step 5: Run the full suite and lints**

Run:
```bash
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
```
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add ayx-rs/tests/product_boundary.rs ayx-rs/src/main.rs
git commit -m "fix(doctor): report One and Server as independent domains

The auth row spliced both products into one summary, so a One-only profile
read 'One incomplete; Server not configured' — two products, one row, and a
warning caused entirely by the absence of a product the operator never
configured.

A product now contributes a clause only when it is configured. An
unconfigured product is its own skip row, never a warning attached to its
neighbour."
```

---

### Task 4: `doctor network` stops warning about a correctly configured product

**Files:**
- Modify: `ayx-rs/src/main.rs:5863-5880` (`doctor_network_status_summary`)
- Test: `ayx-rs/tests/product_boundary.rs`

**Interfaces:**
- Consumes: Task 1's fixtures.
- Produces: nothing later tasks depend on.

- [ ] **Step 1: Write the failing test**

Append to `ayx-rs/tests/product_boundary.rs`:

```rust
#[test]
fn network_check_does_not_warn_merely_because_no_probe_ran() {
    // Endpoints being configured with no live probe run is the normal, healthy
    // state — `doctor` deliberately does not make invasive network calls.
    // Reporting it as a warning trains operators to ignore warnings.
    let home = one_only_oauth_home();
    let network = doctor_checks(&home)["network"].clone();

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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo nextest run -p ayx-rs --test product_boundary network_check_does_not_warn_merely_because_no_probe_ran`
Expected: FAIL — status is `"warn"`, summary is `"One endpoints configured; no live probes run"`, and `probes_run` is absent.

- [ ] **Step 3: Replace the summary function**

In `ayx-rs/src/main.rs`, replace `doctor_network_status_summary` entirely:

```rust
fn doctor_network_status_summary(
    one_configured: bool,
    server_configured: bool,
) -> (&'static str, String) {
    // `doctor` deliberately validates configured endpoints rather than making
    // invasive probes. That is the designed behaviour, so it is `ok`, not a
    // warning — and each product speaks only for itself.
    let mut clauses: Vec<&str> = Vec::new();
    if one_configured {
        clauses.push("Alteryx One endpoints configured");
    }
    if server_configured {
        clauses.push("Alteryx Server endpoints configured");
    }
    if clauses.is_empty() {
        return (
            "skip",
            "No Alteryx One or Server endpoints configured".to_string(),
        );
    }
    ("ok", clauses.join("; "))
}
```

- [ ] **Step 4: Report probe state as a fact**

In `doctor_network_envelope`, replace the `"notes"` array with an explicit field:

```rust
            "probes_run": false,
            "notes": [
                "Endpoint configuration is validated; no invasive live probe is performed.",
            ],
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo nextest run -p ayx-rs --test product_boundary network_check_does_not_warn_merely_because_no_probe_ran`
Expected: PASS

- [ ] **Step 6: Run the full suite and lints**

Run:
```bash
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
```
Expected: all green. `overall` for a One-only OAuth profile should now be `ok`, not `fail`.

- [ ] **Step 7: Commit**

```bash
git add ayx-rs/tests/product_boundary.rs ayx-rs/src/main.rs
git commit -m "fix(doctor): stop warning that no network probe ran

doctor deliberately validates configured endpoints instead of making
invasive probes, then reported that designed behaviour as a warning for
every profile. A warning that is always present teaches operators to ignore
warnings.

Configured endpoints are now ok, probes_run states the fact plainly, and
each product names itself."
```

---

### Task 5: `ayx one api status` and `diagnose` become Alteryx One surfaces

Fixes defect D.

**Files:**
- Modify: `ayx-one-api/src/lib.rs:807-880`
- Modify: `ayx-rs/src/cmd/one_api/mod.rs:11-38`
- Test: `ayx-rs/tests/product_boundary.rs`

**Interfaces:**
- Consumes: Task 1's fixtures and `run_ayx`.
- Produces: `ayx_one_api::one_api_status_envelope(&Config) -> Result<Envelope>` and `ayx_one_api::one_api_diagnose_envelope(&Config) -> Result<Envelope>`.

Design note: leave `api_status_envelope` / `api_diagnose_envelope` / `api_inventory_envelope` in place unchanged — `ayx-rs/src/main.rs:4734-4747` still uses them for the `license` surface, which genuinely is Server-API-backed. This task adds One-native siblings rather than changing a shared helper's meaning.

- [ ] **Step 1: Write the failing test**

Append to `ayx-rs/tests/product_boundary.rs`:

```rust
#[test]
fn one_api_commands_never_require_server_configuration() {
    // Every command under `ayx one` belongs to Alteryx One. Requiring the
    // Server `api:` section under a One namespace is a product-boundary
    // defect, not an incomplete One profile.
    let home = one_only_oauth_home();

    for command in [
        vec!["one", "api", "status"],
        vec!["one", "api", "diagnose"],
    ] {
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

    // No credential value may appear anywhere in the envelope.
    let text = serde_json::to_string(&envelope).expect("serialize");
    assert!(!text.contains("test-access-token"), "access token leaked");
    assert!(!text.contains("test-refresh-token"), "refresh token leaked");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p ayx-rs --test product_boundary one_api`
Expected: FAIL — both commands return `ok: false` with `error_code: "config_missing"`, matching Evidence D.

- [ ] **Step 3: Add the One-native envelopes**

In `ayx-one-api/src/lib.rs`, after the existing `api_diagnose_envelope`, add:

```rust
/// Alteryx One's API-surface posture, read from the One profile. This is
/// deliberately separate from `api_status_envelope`, which reports the Alteryx
/// Server API section: they are different products and neither implies the
/// other is configured.
pub fn one_api_status_envelope(config: &Config) -> Result<Envelope> {
    let one = config
        .alteryx_one
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no Alteryx One profile configured; run `ayx one login`"))?;

    let workspace_id = one.active_workspace_id();
    Ok(Envelope::ok_with_data(
        "Alteryx One api status",
        json!({
            "product": "Alteryx One",
            "profile": config.profile_name,
            "base_url": one.normalized_base_url(),
            "workspace_id": workspace_id,
            "account_email": one.account_email,
            "credential_kind": one.resolved_credential_kind().map(|kind| match kind {
                crate::OneCredentialKind::EmailOtp => "email_otp",
                crate::OneCredentialKind::OAuthRefresh => "oauth_refresh",
            }),
            "access_token_expires_at": one.resolved_access_token_expires_at(),
            "has_credentials": {
                "access_token": one.resolved_access_token().is_some_and(|v| !v.trim().is_empty()),
                "refresh_token": one.resolved_refresh_token().is_some_and(|v| !v.trim().is_empty()),
                "oauth_client_id": one
                    .resolved_oauth_client_id()
                    .is_some_and(|v| !v.trim().is_empty()),
            },
        }),
    ))
}

/// Alteryx One's API-surface diagnosis: validates that the profile names a
/// usable One endpoint and workspace before any call is attempted.
pub fn one_api_diagnose_envelope(config: &Config) -> Result<Envelope> {
    let one = config
        .alteryx_one
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no Alteryx One profile configured; run `ayx one login`"))?;

    let base_url = one
        .normalized_base_url()
        .ok_or_else(|| anyhow::anyhow!("Alteryx One base_url is not set for this profile"))?;
    let workspace_id = one.active_workspace_id();

    let mut findings: Vec<String> = Vec::new();
    if workspace_id.is_none() {
        findings.push("no active workspace selected; run `ayx one workspace use <id>`".to_string());
    }
    if one.resolved_access_token().is_none_or(|v| v.trim().is_empty()) {
        findings.push("no access token stored; run `ayx one login`".to_string());
    }

    Ok(Envelope::ok_with_data(
        "Alteryx One api diagnose",
        json!({
            "product": "Alteryx One",
            "profile": config.profile_name,
            "base_url": base_url,
            "workspace_id": workspace_id,
            "open_api_spec_path": "/v4/open-api-spec",
            "findings": findings,
        }),
    ))
}
```

If `OneCredentialKind` is not already re-exported from `ayx-one-api`, add `pub use ayx_core::profile::OneCredentialKind;` alongside the crate's other re-exports and adjust the `crate::` paths above to match.

- [ ] **Step 4: Point the One commands at them**

In `ayx-rs/src/cmd/one_api/mod.rs`, change the import and both arms:

```rust
use ayx_one_api::{one_api_diagnose_envelope, one_api_status_envelope};
```

```rust
        OneApiCommand::Status { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_status_envelope(&config)?
        }
        OneApiCommand::Diagnose { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_diagnose_envelope(&config)?
        }
```

Remove the now-unused `use ayx_one::{api_diagnose_envelope, api_status_envelope};`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo nextest run -p ayx-rs --test product_boundary one_api`
Expected: PASS

- [ ] **Step 6: Update the catalog and generated command surface**

Run: `cargo run -p xtask -- <the command-surface regeneration subcommand; check `cargo run -p xtask -- --help`>`

Then check `ayx-rs/src/cmd/catalog.rs` for `one/api/status` and `one/api/diagnose` entries whose `prerequisites` list `server_api`. Change those to the One prerequisites (`central runtime profile`, `alteryx one credential`). Commit the regenerated `docs/command-surface.md` in the same commit.

- [ ] **Step 7: Run the full suite and lints**

Run:
```bash
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
```
Expected: all green.

- [ ] **Step 8: Commit**

```bash
git add ayx-one-api/src/lib.rs ayx-rs/src/cmd/one_api/mod.rs ayx-rs/src/cmd/catalog.rs docs/command-surface.md ayx-rs/tests/product_boundary.rs
git commit -m "fix(one): make \`one api status\` and \`diagnose\` Alteryx One surfaces

Both commands called the shared Server-API helper, which reads config.api,
so under a working Alteryx One profile they failed locally with
'config missing api/server_api section' and never reached One at all. Their
sibling subcommands open-api-spec and coverage were already One-native, so
the group was a genuine hybrid under a namespace documented as 'One only'.

Add one_api_status_envelope and one_api_diagnose_envelope, which read the
One profile and report the One base URL, workspace, credential kind, and
expiry. The Server-API helpers stay unchanged for the license surface, which
is genuinely Server-backed."
```

---

## The auth-path decision: what is actually proven

Read this before Gate V1; it sets what the gate is and is not deciding.

`ayx one login` offers five paths. Their evidence status differs sharply:

| Path | Yields a refresh token | Live evidence |
|---|---|---|
| email OTP (current default) | **No** — 30-day access token only | Verified working on Windows |
| `--oauth-api-token` (paste Client ID + Refresh Token from the One UI) | **Yes** — silent renewal | Verified working on Windows |
| `--refresh-token` / `--access-token` import | Yes, if you supply one | Import path only |
| `--browser` (PKCE) | Yes, if the IdP allows it | **None** |
| `--device` (device-code) | Yes, if the IdP allows it | **None** |

**The durable path already exists and is proven: `--oauth-api-token`.** Durability is therefore *not* blocked on Gate V1. That reframes the whole decision — the gate is about whether to add a *more convenient* durable path, not whether a durable path exists.

There is concrete reason to doubt `--browser` and `--device`. At `ayx-rs/src/cmd/one_platform/auth.rs:765` the device authorization endpoint is derived by string substitution on the token endpoint:

```rust
let device_auth_endpoint = token_endpoint
    .replace("/token", "/device_authorization")
    .replace("/as/token", "/as/device_authorization");
```

No OIDC discovery document is fetched and the endpoint's existence is never verified. The `/as/` form suggests PingFederate, which is consistent with Alteryx's IdP, but it remains a guess. Both grants must be enabled on the Alteryx One OAuth client, and PKCE additionally needs `http://localhost:<port>` registered as a redirect URI — Alteryx's configuration, not ours. Alteryx's documented paths are the API token and email OTP, so the likely outcome is that these two do not work.

**Design decision, independent of the gate's outcome:** email OTP remains the default interactive login. It is the path Alteryx supports and it is genuinely the simplest first run for this audience. What changes is that it is labelled honestly, and `--oauth-api-token` is promoted as the durable alternative for operators who do not want to re-authenticate monthly and for all automation.

## Verification Gate V1 — live auth-flow evidence (human-run)

**Cannot be automated.** Needs a real Alteryx One tenant and a human at a browser. Run it in parallel with Tasks 1-5; it blocks only Task 6.

- [ ] **V1.1** In an isolated config home, run `ayx one login --browser` against the reference tenant. Record: does the browser open, does the local redirect capture succeed, and does `ayx one auth status --output json-full` afterwards show `credential_kind: oauth_refresh` with a stored refresh token? Capture the exact error if it fails, including any IdP error code such as `unauthorized_client` or `invalid_redirect_uri`.
- [ ] **V1.2** Same, with `ayx one login --device`. Record whether the derived `/device_authorization` endpoint exists at all — a 404 here proves the string-substitution guess is wrong — and whether a refresh token is persisted.
- [ ] **V1.3** Confirm the proven durable path still works end to end: `ayx one login --oauth-api-token`, then `ayx one auth status` showing `credential_kind: oauth_refresh`, then a command that forces a silent access-token renewal.
- [ ] **V1.4** Record all outcomes in `docs/roadmap/operator-followups.md` under the authentication section with date, CLI version, and tenant region. Redact every credential value; record credential kind, expiry, and workspace id only.

**Branch:**
- **Either flow works** → Task 6A: keep it, document it, and name it as the recommended durable convenience path.
- **Neither works** → Task 6B: hide or remove the broken flags. **A flow advertised in `--help` that fails at the identity provider is worse than no flow at all** — the operator burns time concluding the failure is theirs. Do not leave them listed as though they work.

Task 6B is the expected outcome. Both branches share the same OTP relabelling and `--oauth-api-token` promotion, which is why that work is written once, in Task 6-common below.

---

### Task 6: Tell the truth about credential lifetime, and name the durable path

**Runs regardless of Gate V1's outcome.** Email OTP stays the default interactive login; what changes is that it is described accurately and `--oauth-api-token` is named as the durable alternative.

**Files:**
- Modify: `ayx-rs/src/main.rs` (`ayx one login` long help and the `--oauth-api-token` / OTP flag help)
- Modify: `ayx-rs/src/onboard.rs` (wizard prompt and completion copy)
- Modify: `README.md`, `site/src/content/docs/one/` login and auth pages
- Test: `ayx-rs/tests/product_boundary.rs`

**Interfaces:**
- Consumes: Task 1's fixtures; Task 2's `credential_kind` / `renews_automatically` doctor fields.
- Produces: the copy contract Task 7A/7B extends.

- [ ] **Step 1: Write the failing test**

Append to `ayx-rs/tests/product_boundary.rs`:

```rust
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
    let home = one_only_otp_home();
    let auth = doctor_checks(&home)["auth"].clone();
    let text = serde_json::to_string(&auth).expect("serialize");

    assert!(
        !text.contains("malformed") && !text.contains("invalid"),
        "a working OTP credential is neither malformed nor invalid:\n{auth:#}"
    );
    assert_eq!(
        auth["one"]["renews_automatically"], false,
        "state plainly that OTP does not renew:\n{auth:#}"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p ayx-rs --test product_boundary one_login_help_states_the_otp_lifetime_and_names_the_durable_path`
Expected: FAIL — the current long help describes the OTP flow without naming its lifetime.

- [ ] **Step 3: Rewrite the copy**

In every location listed under **Files**, make these true:
- Email OTP is described as **a time-limited login: the access token expires after 30 days, does not renew automatically, and you will sign in again.**
- `--oauth-api-token` is described as **the durable path — paste a Client ID and Refresh Token once from the Alteryx One UI, and the CLI renews access tokens silently from then on.** It is also the path for CI and automation.
- No copy anywhere states or implies that secure storage keeps the user signed in. Secure storage protects the credential at rest; it does not extend its lifetime.
- `doctor` guidance may recommend moving to `--oauth-api-token`, phrased as an upgrade, never as a repair.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run -p ayx-rs --test product_boundary`
Expected: PASS

- [ ] **Step 5: Run the full suite and lints**

Run:
```bash
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
```
Expected: all green. `ayx-rs/tests/onboard_login_offer.rs` drives the wizard with piped stdin; if the prompt text changes, update its scripts. Its header warns that running off the end of a script sends a real OTP, so every script must answer the login prompt explicitly.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "docs(one): label email OTP as the time-limited login it is

The OTP flow returns no refresh token and its access token expires after 30
days, but onboarding, login help, and the README implied secure storage kept
the user signed in. Secure storage protects a time-limited credential at
rest; it does not extend its lifetime.

State the lifetime wherever the flow is offered and name --oauth-api-token
as the durable alternative that renews silently. doctor recommends that
upgrade as an upgrade, never as a repair to a credential that is working as
designed."
```

---

### Task 7A: Keep and document a verified convenience flow

**Run only if Gate V1 showed `--browser` or `--device` working.**

- [ ] **Step 1: Write the failing test** — assert `ayx one login --help` names the working flow, states that it persists a renewing credential, and that `--device` (if that is the one that worked) is named the headless/SSH path.
- [ ] **Step 2: Run it to verify it fails.** Run: `cargo nextest run -p ayx-rs --test product_boundary`
- [ ] **Step 3: Document the flow** in `ayx one login` help, `README.md`, and the site auth pages as the recommended durable convenience path — easier than the API-token paste and equally durable. Keep email OTP as the default until a live re-test shows the new flow is reliable enough to promote; note the promotion as a follow-up in `docs/roadmap/operator-followups.md`.
- [ ] **Step 4: Replace the guessed endpoint.** `auth.rs:765` derives the device endpoint by string substitution. Since the flow is now known to work, fetch the OIDC discovery document and read `device_authorization_endpoint` from it, falling back to the substitution only if discovery is unavailable. Add a unit test with a recorded discovery document.
- [ ] **Step 5: Run the test to verify it passes.** Run: `cargo nextest run -p ayx-rs --test product_boundary`
- [ ] **Step 6: Run the full suite and lints.**
```bash
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
```
- [ ] **Step 7: Commit** with a message recording the live evidence: tenant region, CLI version, and that a refresh token was persisted.

---

### Task 7B: Hide the auth flows that do not work

**Run only if Gate V1 showed both `--browser` and `--device` failing.** This is the expected outcome.

A flag advertised in `--help` that fails at the identity provider is worse than no flag: the operator burns time concluding the failure is theirs, and every agent reading the help surface treats it as a supported capability.

**Files:**
- Modify: `ayx-rs/src/main.rs` (the `--browser` and `--device` arg definitions on `ayx one login`)
- Modify: `docs/roadmap/operator-followups.md`
- Test: `ayx-rs/tests/product_boundary.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn unverified_auth_flows_are_not_advertised() {
    // --browser and --device were never validated against a live Alteryx One
    // tenant and fail at the identity provider. Until that changes they must
    // not appear in help as though they are supported.
    let home = one_only_otp_home();
    let help = unwrapped_help(&home, &["one", "login"]);

    assert!(
        !help.contains("--browser"),
        "an unverified flow must not be advertised:\n{help}"
    );
    assert!(
        !help.contains("--device"),
        "an unverified flow must not be advertised:\n{help}"
    );
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo nextest run -p ayx-rs --test product_boundary unverified_auth_flows_are_not_advertised`
Expected: FAIL — both flags are listed in the current help.

- [ ] **Step 3: Hide the flags**

Add `hide = true` to the `--browser` and `--device` `#[arg(...)]` attributes on the `ayx one login` command in `ayx-rs/src/main.rs`. Hide rather than delete: the implementation is correct OAuth and becomes usable the moment Alteryx enables those grants, so keep it reachable for re-testing. Remove both from the command's long help paragraph.

- [ ] **Step 4: Record the negative result**

In `docs/roadmap/operator-followups.md`, under the authentication section, record: which flow failed, the exact error and any IdP error code, whether the derived `/device_authorization` endpoint existed, the CLI version, the tenant region, and the date. State precisely what would have to change on the Alteryx side — the OAuth client enabling the `device_code` and/or `authorization_code` grants, and registering `http://localhost:<port>` as a redirect URI — for the decision to be reopened. Note that the flags are hidden, not removed, and where the implementation lives.

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo nextest run -p ayx-rs --test product_boundary unverified_auth_flows_are_not_advertised`
Expected: PASS

- [ ] **Step 6: Run the full suite and lints**

Run:
```bash
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
```
Expected: all green. Any generated help snapshot or command-surface doc listing these flags must be regenerated in this change.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "fix(one): stop advertising auth flows that do not work

--browser and --device were never validated against a live Alteryx One
tenant. The device authorization endpoint is derived by string substitution
on the token endpoint with no discovery lookup, and both grants must be
enabled on the Alteryx OAuth client for either to succeed. Live testing
confirms they fail.

Hide both flags rather than deleting them: the implementation is correct
OAuth and becomes usable the moment Alteryx enables those grants. A flag
advertised in help that fails at the identity provider is worse than no
flag, because the operator concludes the failure is theirs. The evidence and
the conditions for reopening the decision are recorded in the operator
follow-ups tracker."
```

---

## Verification Gate V2 — Windows release evidence (human-run)

Run after Task 6 and whichever of Task 7A / 7B applies. This is the Phase 2 exit gate; it cannot be automated and it needs an authenticated One profile.

- [ ] **V2.1** Run `scripts/one-read-sweep.ps1` against the built binary. Record platform, binary version, active profile type, and the pass/fail count. The Phase 1 baseline was 71/74 passing.
- [ ] **V2.2** Confirm the two previously failing `ayx one api` invocations now pass, taking the sweep to at least 73/74.
- [ ] **V2.3** The remaining known failure is `ayx one workspace detail 91946` returning a live 403 `AccessControlException` — a real permission boundary. Re-run with an administrator fixture, or teach the sweep to classify an expected unprivileged result rather than counting it as an undifferentiated failure.
- [ ] **V2.4** Run the interactive OTP onboarding scenario by hand into an isolated `AYX_CONFIG_HOME`. Confirm `doctor` describes the resulting credential as time-limited and reports `overall: ok`, not `fail`.
- [ ] **V2.5** Record all results in `docs/roadmap/operator-followups.md` and tick the corresponding Phase 2 items.

---

## Self-Review

**Spec coverage.** Against `docs/roadmap/operator-followups.md`:

| Spec requirement | Task |
|---|---|
| `doctor` reports One, Server, Mongo as independent domains | 3 |
| Unconfigured Server is a Server-only skip | 3 |
| OTP described as time-limited/non-rotating, not incomplete | 2 |
| Rollup is explicit and retains per-product status in structured output | 3 |
| Snapshots/integration tests for One-only, Server-only, combined | 1, 3 |
| `ayx one api` rewired to One config and endpoints | 5 |
| One-only integration test for every `ayx one api ...` command | 5 |
| Help, catalog, README name the owning product | 5, 6 |
| Onboarding/login/doctor/help/README never imply OTP is durable | 6 |
| Explicit product decision on the onboarding auth path | 6, Gate V1 → 7A/7B |
| Test the two auth paths separately | Gate V1 (V1.1-V1.3), V2.4 |
| Do not claim durable OTP without upstream evidence | 6, 7B |
| Expiry metadata not redacted | 2, 5 |
| Windows read sweep as the release gate | Gate V2 |

Two spec items are deliberately **out of scope** for Phase 2 and stay in the tracker for Phase 3: the human-output renderer and redaction-model rework, and the `--output json-full` consolidation. Task 5's test asserts no credential leaks in the current envelope, which is the Phase 2 slice of that concern.

One item is **added beyond the spec**: Task 1's workspace-scoped credential defect, which the tracker does not record and which is more severe than the OTP misclassification it was hiding behind.

**Placeholder scan.** One deliberate gap remains: Task 5 Step 6 does not name the exact `xtask` regeneration subcommand, because the repo's own `CLAUDE.md` says only that `xtask` regenerates `docs/command-surface.md`. The step tells the executor to run `cargo run -p xtask -- --help` to find it. Everything else carries literal code.

**Type consistency.** The One auth status vocabulary is `"configured" | "configured_time_limited" | "incomplete" | "not_configured"`, introduced in Task 2 (`one_auth_product_status`) and consumed unchanged in Task 3's `doctor_auth_status_summary` and its tests. `run_ayx` and `doctor_checks` keep the signatures declared in Task 1 across Tasks 3, 4, 5, and 6. `one_api_status_envelope` / `one_api_diagnose_envelope` take `&Config` and return `Result<Envelope>` consistently between Task 5's Step 3 definition and Step 4 call sites.
