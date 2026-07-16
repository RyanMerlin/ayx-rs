# Alteryx One Email-OTP Login Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `ayx one platform auth login`'s default email-OTP flow tolerate transient network failures and wrong OTP/workspace-password entries without forcing a full restart (and a wasted OTP email) on every hiccup.

**Architecture:** Two independent, narrowly-scoped mechanisms added entirely inside `ayx-one-api/src/email_otp.rs` (plus a `use` change): a generic `retry_transient<T, E>` loop reused across every HTTP call in the flow with per-call-site classification of what's safe to retry, and two outer re-prompt loops (OTP, workspace password) that re-ask for input on rejection instead of bailing immediately. No new crate dependency, no changes outside this one file and the plan's CHANGELOG entry.

**Tech Stack:** Rust, `reqwest` 0.13 (blocking client), `anyhow`, existing `ayx-one-api` crate conventions (reuses `crate::should_retry_status` / `crate::retry_delay`, already defined and tested in `ayx-one-api/src/lib.rs`).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-15-ayx-one-auth-hardening-design.md` — every task below implements a specific decision from that document; read it if a "why" isn't obvious from a task's inline comments.
- No new Cargo dependency (spec decision #1: reject generic HTTP middleware).
- `sendPasscode` and the `apiAccessTokens` mint must **never** auto-retry on an ambiguous transport failure (timeout, 5xx) — only on a failure confirmed to have happened before the request reached the server (spec decision #3). Getting this wrong reintroduces the exact "duplicate OTP email / orphaned PAT" risk the spec exists to prevent.
- Existing tests in `ayx-one-api/src/email_otp.rs` (`host_allowed_*`, `is_valid_interaction_id`, `extract_interaction_id*`, `resolve_workspace_password_*`) must keep passing unmodified — they pin down security-relevant behavior (redirect-host allowlisting, terminal-corruption avoidance) that this work must not touch.
- `cargo fmt --all` before every commit (repo convention — an unformatted commit breaks CI's `--check` gate).
- Verify with `cargo clippy --workspace --all-targets -- -D warnings` and `cargo nextest run -p ayx-one-api` (this crate specifically; full-workspace `nextest run` on the final task).
- Add a `### Fixed` entry to `CHANGELOG.md`'s `## Unreleased` section on the final task, matching the existing style (bold one-line summary + explanatory paragraph) — see the "workspace-password prompt no longer echoes" entry under `0.13.1` for the template; this work lives in the same command.

---

### Task 1: Generic retry loop + reqwest-specific classification predicates

**Files:**
- Modify: `ayx-one-api/src/email_otp.rs:1-20` (imports and constants)
- Modify: `ayx-one-api/src/email_otp.rs:443-448` (test module imports)

**Interfaces:**
- Produces:
  - `fn retry_transient<T, E>(max_attempts: u32, should_retry_err: impl Fn(&E) -> bool, should_retry_ok: impl Fn(&T) -> bool, attempt_once: impl FnMut() -> Result<T, E>) -> Result<T, E>`
  - `fn is_pre_send_failure(err: &reqwest::Error) -> bool`
  - `fn is_transient_transport_error(err: &reqwest::Error) -> bool`
  - `fn retryable_status_response(response: &Response) -> bool`
  - `const TRANSIENT_RETRY_ATTEMPTS: u32 = 3`

This task adds code with no call sites yet — it's a pure addition, safe to land standalone.

- [ ] **Step 1: Update the top-of-file imports and add constants**

Change line 14 from:
```rust
use reqwest::blocking::Client;
```
to:
```rust
use reqwest::blocking::{Client, Response};
```

Immediately after the existing `const BROWSER_UA` block (after line 20), add:

```rust
/// Attempts for HTTP calls where repeating the same request has no
/// duplication risk (either it's read-only, or a retried POST like
/// validatePasscode/session has no side effect beyond the first success).
/// Calls where a retry COULD duplicate a side effect (sendPasscode,
/// apiAccessTokens mint) use a narrower retry predicate instead of a
/// separate constant — see `is_pre_send_failure`.
const TRANSIENT_RETRY_ATTEMPTS: u32 = 3;
```

- [ ] **Step 2: Write the failing tests for `retry_transient`**

Add to the `#[cfg(test)] mod tests` block (after the existing `use super::{...}` line, before the `host_allowed` tests):

```rust
    use super::retry_transient;

    #[test]
    fn retry_transient_returns_ok_on_first_success() {
        let mut calls = 0u32;
        let result: Result<u32, &'static str> = retry_transient(
            3,
            |_: &&'static str| true,
            |_: &u32| false,
            || {
                calls += 1;
                Ok(7)
            },
        );
        assert_eq!(result, Ok(7));
        assert_eq!(calls, 1);
    }

    #[test]
    fn retry_transient_retries_on_retryable_err_then_succeeds() {
        let mut calls = 0u32;
        let result: Result<u32, &'static str> = retry_transient(
            3,
            |_: &&'static str| true,
            |_: &u32| false,
            || {
                calls += 1;
                if calls < 3 { Err("transient") } else { Ok(42) }
            },
        );
        assert_eq!(result, Ok(42));
        assert_eq!(calls, 3);
    }

    #[test]
    fn retry_transient_stops_at_max_attempts_on_persistent_err() {
        let mut calls = 0u32;
        let result: Result<u32, &'static str> = retry_transient(
            3,
            |_: &&'static str| true,
            |_: &u32| false,
            || {
                calls += 1;
                Err("still broken")
            },
        );
        assert_eq!(result, Err("still broken"));
        assert_eq!(calls, 3);
    }

    #[test]
    fn retry_transient_does_not_retry_non_retryable_err() {
        let mut calls = 0u32;
        let result: Result<u32, &'static str> = retry_transient(
            3,
            |_: &&'static str| false,
            |_: &u32| false,
            || {
                calls += 1;
                Err("terminal")
            },
        );
        assert_eq!(result, Err("terminal"));
        assert_eq!(calls, 1);
    }

    #[test]
    fn retry_transient_retries_retryable_ok_value_then_stops() {
        let mut calls = 0u32;
        let result: Result<u32, &'static str> = retry_transient(
            3,
            |_: &&'static str| true,
            |v: &u32| *v == 429,
            || {
                calls += 1;
                if calls < 2 { Ok(429) } else { Ok(200) }
            },
        );
        assert_eq!(result, Ok(200));
        assert_eq!(calls, 2);
    }

    fn connect_refused_error() -> reqwest::Error {
        // Port 1 on loopback is always unbound — connecting to it fails
        // immediately with ECONNREFUSED, deterministically and without any
        // real network access (loopback-only, no DNS involved).
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .expect("client build");
        client
            .get("http://127.0.0.1:1/")
            .send()
            .expect_err("connecting to a closed loopback port must fail")
    }

    #[test]
    fn is_pre_send_failure_true_for_connection_refused() {
        assert!(super::is_pre_send_failure(&connect_refused_error()));
    }

    #[test]
    fn is_transient_transport_error_true_for_connection_refused() {
        assert!(super::is_transient_transport_error(&connect_refused_error()));
    }
```

- [ ] **Step 3: Run the tests to verify they fail to compile**

Run: `cargo nextest run -p ayx-one-api retry_transient --no-run`
Expected: FAIL — `error[E0433]: failed to resolve: use of undeclared function 'retry_transient'` (and similarly for `is_pre_send_failure` / `is_transient_transport_error`), since none of these exist yet.

- [ ] **Step 4: Implement `retry_transient` and the classification predicates**

Add after the `TRANSIENT_RETRY_ATTEMPTS` constant from Step 1, before `email_otp_login`:

```rust
/// Retries `attempt_once` up to `max_attempts` times. `should_retry_err`
/// decides whether a returned error is worth retrying; `should_retry_ok`
/// does the same for a value that came back successfully but still
/// warrants another try (e.g. an HTTP 429/5xx that round-tripped fine at
/// the transport level). Sleeps between attempts using the crate's
/// existing jittered backoff (`crate::retry_delay`), the same pacing
/// already used by the rest of this crate's One API request loop.
///
/// Generic over `T`/`E` so the retry mechanics can be unit tested without
/// constructing real `reqwest` types (which have no public test
/// constructors) — callers plug in `reqwest::blocking::Response` /
/// `reqwest::Error` at the call site.
fn retry_transient<T, E>(
    max_attempts: u32,
    should_retry_err: impl Fn(&E) -> bool,
    should_retry_ok: impl Fn(&T) -> bool,
    mut attempt_once: impl FnMut() -> Result<T, E>,
) -> Result<T, E> {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match attempt_once() {
            Ok(value) => {
                if attempt >= max_attempts || !should_retry_ok(&value) {
                    return Ok(value);
                }
            }
            Err(err) => {
                if attempt >= max_attempts || !should_retry_err(&err) {
                    return Err(err);
                }
            }
        }
        std::thread::sleep(crate::retry_delay(attempt, None));
    }
}

/// A transport failure where we're confident the request never reached the
/// server — the failure occurred while establishing the connection (DNS,
/// TCP connect, TLS handshake), not while waiting on a response. Safe to
/// retry even for calls with a side effect (an OTP email send, a PAT mint)
/// because there is no risk the server already processed the request.
///
/// Deliberately checks connection-phase only (`reqwest::Error::is_connect`),
/// not `is_timeout` in isolation: a connect attempt that itself times out
/// (e.g. TCP SYN never ACKed) still reports `is_connect() == true` — the
/// request body was never sent — so it belongs in this safe-to-retry set
/// too. A timeout *after* the connection was established (waiting on the
/// response) does not set `is_connect()` and is excluded here on purpose.
fn is_pre_send_failure(err: &reqwest::Error) -> bool {
    err.is_connect()
}

/// Any transport-level failure — connect or timeout — treated as retryable
/// for calls with no duplication risk (repeating the request has no side
/// effect beyond the first successful attempt).
fn is_transient_transport_error(err: &reqwest::Error) -> bool {
    err.is_connect() || err.is_timeout()
}

/// Whether a *successfully received* response (any status) is worth
/// retrying. Reuses this crate's existing status-based retry policy
/// (`crate::should_retry_status`, already covered by
/// `retry_policy_retries_gets_but_not_mutations` in `lib.rs`) with
/// `mutating = false` — every call site that uses this predicate has
/// already been classified as duplication-safe to retry.
fn retryable_status_response(response: &Response) -> bool {
    crate::should_retry_status(response.status(), false)
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo nextest run -p ayx-one-api retry_transient is_pre_send_failure is_transient_transport_error -v`
Expected: 7 tests pass (`retry_transient_returns_ok_on_first_success`, `retry_transient_retries_on_retryable_err_then_succeeds`, `retry_transient_stops_at_max_attempts_on_persistent_err`, `retry_transient_does_not_retry_non_retryable_err`, `retry_transient_retries_retryable_ok_value_then_stops`, `is_pre_send_failure_true_for_connection_refused`, `is_transient_transport_error_true_for_connection_refused`). Note: the middle three tests each sleep ~250ms–1s for their retries (real jittered backoff, not mocked) — expect the run to take a few seconds, not milliseconds; that's expected, not a hang.

- [ ] **Step 6: Full crate check and commit**

Run: `cargo fmt --all && cargo clippy -p ayx-one-api --all-targets -- -D warnings && cargo nextest run -p ayx-one-api`
Expected: clean clippy, all existing + new tests pass.

```bash
git add ayx-one-api/src/email_otp.rs
git commit -m "feat(one-api): add generic transient-retry loop for the OTP login flow

Adds retry_transient<T, E> plus reqwest-specific classification
predicates (is_pre_send_failure, is_transient_transport_error,
retryable_status_response). No call sites wired up yet — this is
pure infrastructure, landed standalone per
docs/superpowers/specs/2026-07-15-ayx-one-auth-hardening-design.md."
```

---

### Task 2: Wrap the duplication-risk calls (sendPasscode, apiAccessTokens mint)

**Files:**
- Modify: `ayx-one-api/src/email_otp.rs:95-108` (sendPasscode, step 1 of `email_otp_login_pure_http`)
- Modify: `ayx-one-api/src/email_otp.rs:177-203` (apiAccessTokens mint, step 7)

**Interfaces:**
- Consumes: `retry_transient`, `is_pre_send_failure` (Task 1)
- Produces: `fn send_passcode(client: &Client, base: &str, email: &str) -> Result<String>` (returns the `passcodeReferenceId`) — Task 4 calls this.

These two calls have a real side effect (an email sent, a token minted) if the request reaches the server, so per spec decision #3 they retry **only** on `is_pre_send_failure` — never on a timeout-awaiting-response or any received status, ambiguous or not.

- [ ] **Step 1: Extract `send_passcode` as a named function with retry**

Replace lines 95-108 (currently inline in `email_otp_login_pure_http`):
```rust
    // 1. Send the passcode.
    let send: Value = client
        .post(format!("{base}/v4/auth/sendPasscode"))
        .json(&serde_json::json!({ "email": email }))
        .send()
        .context("sendPasscode request failed")?
        .error_for_status()
        .context("sendPasscode returned an error status")?
        .json()
        .context("sendPasscode response was not JSON")?;
    let reference_id = send["passcodeReferenceId"]
        .as_str()
        .context("sendPasscode response missing passcodeReferenceId")?
        .to_string();
```

with a call to the new helper:
```rust
    // 1. Send the passcode.
    let reference_id = send_passcode(&client, base, email)?;
```

Add the new function above `email_otp_login_pure_http` (after the `OtpAuthResult` struct, before `pub fn email_otp_login`):

```rust
/// `POST /v4/auth/sendPasscode`. Retries only on a pre-send failure — see
/// `is_pre_send_failure` — because a retry after the request reached the
/// server risks sending a second passcode email for the same login attempt.
fn send_passcode(client: &Client, base: &str, email: &str) -> Result<String> {
    let response = retry_transient(
        TRANSIENT_RETRY_ATTEMPTS,
        is_pre_send_failure,
        |_: &Response| false,
        || {
            client
                .post(format!("{base}/v4/auth/sendPasscode"))
                .json(&serde_json::json!({ "email": email }))
                .send()
        },
    )
    .context("sendPasscode request failed")?
    .error_for_status()
    .context("sendPasscode returned an error status")?;
    let send: Value = response
        .json()
        .context("sendPasscode response was not JSON")?;
    let reference_id = send["passcodeReferenceId"]
        .as_str()
        .context("sendPasscode response missing passcodeReferenceId")?
        .to_string();
    Ok(reference_id)
}
```

- [ ] **Step 2: Wrap the apiAccessTokens mint**

Replace lines 177-203 (the PAT mint block, from `let csrf = ...` through the closing `?;` of the `pat` binding):
```rust
    // 7. Mint a 30-day PAT.
    let csrf = cookie_value_from_jar(&jar, &base_for_cookies, "x-csrf-token").unwrap_or_default();
    let pat: Value = client
        .post(format!("{base}/v4/apiAccessTokens"))
        .header("x-csrf-token", csrf)
        .header("x-alteryx-workspace-gid", workspace_gid)
        .bearer_auth(&bearer)
        .json(&serde_json::json!({
            "name": "ayx-rs-cli",
            "lifetimeSeconds": 2_592_000,
        }))
        .send()
        .context("apiAccessTokens request failed")?
        .error_for_status()
        .context("apiAccessTokens returned an error status")?
        .json()
        .context("apiAccessTokens response was not JSON")?;
```

with:
```rust
    // 7. Mint a 30-day PAT. Retries only on a pre-send failure — a retry
    //    after the request reached the server risks minting a second,
    //    orphaned PAT the caller never sees (see is_pre_send_failure).
    let csrf = cookie_value_from_jar(&jar, &base_for_cookies, "x-csrf-token").unwrap_or_default();
    let pat_payload = serde_json::json!({
        "name": "ayx-rs-cli",
        "lifetimeSeconds": 2_592_000,
    });
    let pat: Value = retry_transient(
        TRANSIENT_RETRY_ATTEMPTS,
        is_pre_send_failure,
        |_: &Response| false,
        || {
            client
                .post(format!("{base}/v4/apiAccessTokens"))
                .header("x-csrf-token", csrf.as_str())
                .header("x-alteryx-workspace-gid", workspace_gid)
                .bearer_auth(&bearer)
                .json(&pat_payload)
                .send()
        },
    )
    .context("apiAccessTokens request failed")?
    .error_for_status()
    .context("apiAccessTokens returned an error status")?
    .json()
    .context("apiAccessTokens response was not JSON")?;
```

- [ ] **Step 3: Verify existing tests still pass and the crate compiles**

Run: `cargo nextest run -p ayx-one-api`
Expected: all tests pass (the redirect/host-allowlist/interaction-id tests are untouched by this task; this step exists to catch any typo/signature mismatch before moving on).

- [ ] **Step 4: Clippy, fmt, commit**

Run: `cargo fmt --all && cargo clippy -p ayx-one-api --all-targets -- -D warnings`
Expected: clean.

```bash
git add ayx-one-api/src/email_otp.rs
git commit -m "feat(one-api): retry sendPasscode/apiAccessTokens on pre-send failure only

Both calls have a side effect if the request reaches the server (an
OTP email, a minted PAT), so unlike the rest of the flow they retry
only when we're confident the request never left the client
(is_pre_send_failure), never on a timeout or 5xx that could mean the
server already processed it."
```

---

### Task 3: Wrap the duplication-safe calls (validatePasscode, accounts lookup, redirect chain, session POST)

**Files:**
- Modify: `ayx-one-api/src/email_otp.rs:110-128` (validatePasscode, step 2)
- Modify: `ayx-one-api/src/email_otp.rs:151-163` (workspace-password POST /session, step 4)
- Modify: `ayx-one-api/src/email_otp.rs:215-259` (`resolve_workspace_name`, the accounts GET)
- Modify: `ayx-one-api/src/email_otp.rs:317-364` (`follow_redirects`, per-hop GET)

**Interfaces:**
- Consumes: `retry_transient`, `is_transient_transport_error`, `retryable_status_response` (Task 1)
- Produces:
  - `fn validate_passcode(client: &Client, base: &str, email: &str, reference_id: &str, code: &str) -> Result<()>` — Task 4 calls this.
  - `fn submit_workspace_password(client: &Client, base: &str, email: &str, password: &str) -> Result<()>` — Task 5 calls this.

These four calls have no duplication risk (resubmitting the same code/password/GET has no side effect beyond the first success), so they retry on any transient transport error or a retryable status (429/5xx).

- [ ] **Step 1: Extract `validate_passcode` as a named function with retry**

Replace lines 110-128 (currently inline):
```rust
    // 2. Prompt for the OTP and validate it.
    let otp = get_otp()?;
    let validate = client
        .post(format!("{base}/v4/auth/validatePasscode"))
        .json(&serde_json::json!({
            "email": email,
            "passcode": otp.trim(),
            "passcodeReferenceId": reference_id,
        }))
        .send()
        .context("validatePasscode request failed")?;
    let validate_status = validate.status();
    if !validate_status.is_success() {
        let body = validate.text().unwrap_or_default();
        bail!(
            "validatePasscode failed: HTTP {validate_status}: {}",
            redact_text(&body.chars().take(200).collect::<String>())
        );
    }
```

with a call to the new function (this becomes part of `otp_login_with_reprompt` in Task 4 — for this task, call it once inline, matching current behavior exactly):
```rust
    // 2. Prompt for the OTP and validate it.
    let otp = get_otp()?;
    validate_passcode(&client, base, email, &reference_id, &otp)?;
```

Add the new function next to `send_passcode`:
```rust
/// `POST /v4/auth/validatePasscode`. Retries on any transient transport
/// failure or retryable status — resubmitting the same code has no
/// duplication risk, unlike sendPasscode.
fn validate_passcode(
    client: &Client,
    base: &str,
    email: &str,
    reference_id: &str,
    code: &str,
) -> Result<()> {
    let response = retry_transient(
        TRANSIENT_RETRY_ATTEMPTS,
        is_transient_transport_error,
        retryable_status_response,
        || {
            client
                .post(format!("{base}/v4/auth/validatePasscode"))
                .json(&serde_json::json!({
                    "email": email,
                    "passcode": code.trim(),
                    "passcodeReferenceId": reference_id,
                }))
                .send()
        },
    )
    .context("validatePasscode request failed")?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        bail!(
            "validatePasscode failed: HTTP {status}: {}",
            redact_text(&body.chars().take(200).collect::<String>())
        );
    }
    Ok(())
}
```

- [ ] **Step 2: Extract `submit_workspace_password` as a named function with retry**

Replace lines 151-163 (currently inline):
```rust
    // 4. Submit the workspace password.
    let ws_password = resolve_workspace_password()?;
    let session = client
        .post(format!("{base}/session"))
        .form(&[("email", email), ("password", ws_password.as_str())])
        .send()
        .context("POST /session (workspace password) failed")?;
    if !session.status().is_success() {
        bail!(
            "workspace password rejected: POST /session returned HTTP {}",
            session.status()
        );
    }
```

with (for this task, still called once inline — the re-prompt loop wiring is Task 5):
```rust
    // 4. Submit the workspace password.
    let ws_password = resolve_workspace_password()?;
    submit_workspace_password(&client, base, email, &ws_password)?;
```

Add the new function next to `validate_passcode`:
```rust
/// `POST /session` (workspace password). Retries on any transient
/// transport failure or retryable status — resubmitting the same password
/// has no duplication risk.
fn submit_workspace_password(
    client: &Client,
    base: &str,
    email: &str,
    password: &str,
) -> Result<()> {
    let response = retry_transient(
        TRANSIENT_RETRY_ATTEMPTS,
        is_transient_transport_error,
        retryable_status_response,
        || {
            client
                .post(format!("{base}/session"))
                .form(&[("email", email), ("password", password)])
                .send()
        },
    )
    .context("POST /session (workspace password) failed")?;
    if !response.status().is_success() {
        bail!(
            "workspace password rejected: POST /session returned HTTP {}",
            response.status()
        );
    }
    Ok(())
}
```

- [ ] **Step 3: Wrap the accounts GET inside `resolve_workspace_name`**

In `resolve_workspace_name` (currently lines 215-259), replace:
```rust
    let accounts_resp = client
        .get(accounts_url)
        // The accounts endpoint identifies the caller via this header, not session cookies.
        .header("x-alteryx-auth-email", email)
        .send()
        .context("failed to fetch /v4/auth/accounts")?;
```
with:
```rust
    let accounts_resp = retry_transient(
        TRANSIENT_RETRY_ATTEMPTS,
        is_transient_transport_error,
        retryable_status_response,
        || {
            client
                .get(accounts_url.clone())
                // The accounts endpoint identifies the caller via this header, not session cookies.
                .header("x-alteryx-auth-email", email)
                .send()
        },
    )
    .context("failed to fetch /v4/auth/accounts")?;
```

(`accounts_url` is a `reqwest::Url`, which implements `Clone` — the closure needs to be callable more than once, hence `.clone()` at each call site rather than moving the original.)

- [ ] **Step 4: Wrap the per-hop GET inside `follow_redirects`**

In `follow_redirects` (currently lines 317-364), replace:
```rust
        let resp = client
            .get(current.clone())
            .header(
                reqwest::header::ACCEPT,
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .send()
            .with_context(|| format!("request to {current} failed"))?;
```
with:
```rust
        let resp = retry_transient(
            TRANSIENT_RETRY_ATTEMPTS,
            is_transient_transport_error,
            retryable_status_response,
            || {
                client
                    .get(current.clone())
                    .header(
                        reqwest::header::ACCEPT,
                        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                    )
                    .send()
            },
        )
        .with_context(|| format!("request to {current} failed"))?;
```

(3xx redirect responses are neither `is_success()` nor in the 429/5xx set `retryable_status_response` checks, so redirects pass straight through unaffected — only a genuine 429/5xx or transport blip triggers a retry here.)

- [ ] **Step 5: Verify existing tests still pass**

Run: `cargo nextest run -p ayx-one-api`
Expected: all tests pass, including the untouched `host_allowed_*`, `extract_interaction_id*`, `resolve_workspace_password_*` tests — this task only adds retry around calls, it doesn't change `follow_redirects`'/`resolve_workspace_name`'s external behavior on success or terminal failure.

- [ ] **Step 6: Clippy, fmt, commit**

Run: `cargo fmt --all && cargo clippy -p ayx-one-api --all-targets -- -D warnings`
Expected: clean.

```bash
git add ayx-one-api/src/email_otp.rs
git commit -m "feat(one-api): retry validatePasscode/session/accounts/redirects on transient failure

These four calls have no duplication risk, so they retry on any
transient transport error or a 429/5xx response — unlike
sendPasscode/apiAccessTokens (previous commit), which only retry on
a pre-send failure."
```

---

### Task 4: Wrong-OTP re-prompt loop with capped auto-resend

**Files:**
- Modify: `ayx-one-api/src/email_otp.rs:1-20` (constants)
- Modify: `ayx-one-api/src/email_otp.rs:95-128` (replace the inline sendPasscode+validate call sequence)

**Interfaces:**
- Consumes: `send_passcode`, `validate_passcode` (Task 2, Task 3)
- Produces: `fn otp_login_with_reprompt<F>(client: &Client, base: &str, email: &str, get_otp: &F) -> Result<()> where F: Fn() -> Result<String>` — called once from `email_otp_login_pure_http`.

Design note (also in the spec): rather than parsing Alteryx's response body to distinguish "wrong code, reference still valid" from "reference expired/dead" — an API detail nobody has verified without burning a real OTP — this retries the same `passcodeReferenceId` a fixed number of times regardless of the specific rejection reason, then falls back to a fresh send. A live typo gets corrected within the local retry budget without ever needing a second email; a genuinely dead reference fails identically on every local attempt and is caught by the same budget. Same operational outcome, no assumption about the server's exact error taxonomy required.

- [ ] **Step 1: Add the two new constants**

Add next to `TRANSIENT_RETRY_ATTEMPTS` (from Task 1):
```rust
/// Local re-prompt attempts against a single passcodeReferenceId before
/// falling back to sending a fresh passcode.
const OTP_ATTEMPTS_PER_REFERENCE: u32 = 3;
/// Total passcode emails sent per login() call before giving up entirely.
const MAX_OTP_SENDS: u32 = 2;
```

- [ ] **Step 2: Write the failing test for the re-prompt/resend behavior**

This function drives real HTTP calls (`send_passcode`/`validate_passcode`), so it isn't unit-testable without a live server or a mock — same constraint the existing `email_otp_login` flow already has (there is no existing test that exercises it end-to-end; live behavior is verified manually, per the spec's testing section). What *is* unit-testable in isolation is the **counting/looping logic** — attempt budget, resend cap, and the boundary between "re-prompt" and "resend" — so this step extracts that into a small pure helper and tests it directly instead of asserting on real network behavior.

Add to the `tests` module:
```rust
    use super::next_otp_action;

    #[test]
    fn next_otp_action_reprompts_when_attempts_remain() {
        assert_eq!(next_otp_action(1, 1), OtpAction::Reprompt);
        assert_eq!(next_otp_action(2, 1), OtpAction::Reprompt);
    }

    #[test]
    fn next_otp_action_resends_when_attempts_and_sends_exhausted_but_sends_remain() {
        assert_eq!(next_otp_action(3, 1), OtpAction::Resend);
    }

    #[test]
    fn next_otp_action_gives_up_when_attempts_and_sends_both_exhausted() {
        assert_eq!(next_otp_action(3, 2), OtpAction::GiveUp);
    }
```

- [ ] **Step 3: Run the tests to verify they fail to compile**

Run: `cargo nextest run -p ayx-one-api next_otp_action --no-run`
Expected: FAIL — `error[E0433]: failed to resolve: use of undeclared type 'OtpAction'` / undeclared function `next_otp_action`.

- [ ] **Step 4: Implement `next_otp_action` and `otp_login_with_reprompt`**

Add above `email_otp_login_pure_http`, next to `send_passcode`/`validate_passcode`:

```rust
/// What to do after a `validate_passcode` rejection, given how many local
/// attempts have been made against the current reference (`attempt`, 1-
/// indexed, already includes the failing one) and how many passcodes have
/// been sent so far (`sends`, 1-indexed).
#[derive(Debug, PartialEq, Eq)]
enum OtpAction {
    /// Attempts remain against the current reference — ask for the code again.
    Reprompt,
    /// The local attempt budget for this reference is exhausted, but
    /// there's sends budget left — send a fresh passcode and reset.
    Resend,
    /// Both budgets are exhausted — bail.
    GiveUp,
}

fn next_otp_action(attempt: u32, sends: u32) -> OtpAction {
    if attempt < OTP_ATTEMPTS_PER_REFERENCE {
        OtpAction::Reprompt
    } else if sends < MAX_OTP_SENDS {
        OtpAction::Resend
    } else {
        OtpAction::GiveUp
    }
}

/// Sends a passcode and validates it, re-prompting on a wrong/expired code
/// (up to `OTP_ATTEMPTS_PER_REFERENCE` times against the same reference)
/// and automatically sending a fresh passcode if that budget is exhausted
/// (up to `MAX_OTP_SENDS` sends total). See the module-level design note
/// on why this doesn't need to parse the API's exact rejection reason.
fn otp_login_with_reprompt<F>(
    client: &Client,
    base: &str,
    email: &str,
    get_otp: &F,
) -> Result<()>
where
    F: Fn() -> Result<String>,
{
    let mut sends = 0u32;
    loop {
        sends += 1;
        let reference_id = send_passcode(client, base, email)?;
        if sends > 1 {
            eprintln!("Sent a new passcode to {email}.");
        }
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let otp = get_otp()?;
            match validate_passcode(client, base, email, &reference_id, &otp) {
                Ok(()) => return Ok(()),
                Err(err) => match next_otp_action(attempt, sends) {
                    OtpAction::Reprompt => {
                        eprintln!(
                            "Incorrect or expired passcode ({attempt}/{OTP_ATTEMPTS_PER_REFERENCE}) — try again."
                        );
                    }
                    OtpAction::Resend => {
                        eprintln!(
                            "Still not accepted after {OTP_ATTEMPTS_PER_REFERENCE} tries — sending a new passcode..."
                        );
                        break;
                    }
                    OtpAction::GiveUp => {
                        return Err(err.context(format!(
                            "passcode rejected {OTP_ATTEMPTS_PER_REFERENCE} times across {sends} passcode(s) sent"
                        )));
                    }
                },
            }
        }
    }
}
```

Now replace lines 95-128 (the original inline "1. Send the passcode" + "2. Prompt for the OTP and validate it" blocks, already touched by Task 2/Task 3's extraction) with a single call:
```rust
    // 1-2. Send the passcode and validate it, retrying wrong entries and
    //      automatically requesting a fresh passcode if the reference dies.
    otp_login_with_reprompt(&client, base, email, get_otp)?;
```

Delete the now-unused `reference_id` binding and the standalone `let otp = get_otp()?; validate_passcode(...)?;` lines that Task 2/3 left in place — `otp_login_with_reprompt` owns that whole exchange now.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo nextest run -p ayx-one-api next_otp_action -v`
Expected: 3 tests pass (`next_otp_action_reprompts_when_attempts_remain`, `next_otp_action_resends_when_attempts_and_sends_exhausted_but_sends_remain`, `next_otp_action_gives_up_when_attempts_and_sends_both_exhausted`).

- [ ] **Step 6: Full crate verification**

Run: `cargo nextest run -p ayx-one-api`
Expected: all tests pass — this confirms the rewiring in `email_otp_login_pure_http` didn't break compilation or any existing test.

- [ ] **Step 7: Clippy, fmt, commit**

Run: `cargo fmt --all && cargo clippy -p ayx-one-api --all-targets -- -D warnings`
Expected: clean.

```bash
git add ayx-one-api/src/email_otp.rs
git commit -m "feat(one-api): re-prompt on wrong OTP, auto-resend when the reference dies

Wrong or expired codes get up to 3 local re-prompts against the same
passcodeReferenceId before automatically sending one fresh passcode
(capped at 2 sends total) — a typo no longer forces a full restart
and a new email every time."
```

---

### Task 5: Wrong-workspace-password re-prompt loop

**Files:**
- Modify: `ayx-one-api/src/email_otp.rs:1-20` (constant)
- Modify: `ayx-one-api/src/email_otp.rs:151-163` (replace the inline session-submit call from Task 3)
- Modify: `ayx-one-api/src/email_otp.rs:263-280` (`resolve_workspace_password`, split into two pieces)

**Interfaces:**
- Consumes: `submit_workspace_password` (Task 3)
- Produces:
  - `fn workspace_password_from_env() -> Option<String>`
  - `fn prompt_workspace_password() -> Result<String>`
  - `fn workspace_login_with_reprompt(client: &Client, base: &str, email: &str) -> Result<()>` — called once from `email_otp_login_pure_http`.
  - `resolve_workspace_password()` keeps its exact current signature and behavior (implemented in terms of the two new functions) — the existing `resolve_workspace_password_env_var_short_circuit` and `resolve_workspace_password_no_tty_fails_cleanly` tests must keep passing unmodified.

Design note: unlike the OTP loop, this does **not** treat every source of the password the same way. If the password came from `AYX_ONE_WS_PASSWORD`, retrying with the same env value is pointless — it'll fail identically every time — so that path fails fast on the first rejection instead of burning the retry budget against a value that can't change. The interactive-prompt path gets the full re-prompt budget, since a human might have mistyped.

- [ ] **Step 1: Add the constant**

Add next to the OTP constants from Task 4:
```rust
/// Workspace-password re-prompt attempts before bailing (interactive path
/// only — see workspace_login_with_reprompt).
const WORKSPACE_PASSWORD_ATTEMPTS: u32 = 3;
```

- [ ] **Step 2: Write the failing test for the env-vs-interactive retry decision**

As with Task 4, the HTTP-driving function isn't unit-testable without a live server; what's testable is the pure decision of whether to keep retrying. Add to the `tests` module:

```rust
    use super::should_retry_workspace_password;

    #[test]
    fn should_retry_workspace_password_false_when_from_env() {
        assert!(!should_retry_workspace_password(1, true));
        assert!(!should_retry_workspace_password(3, true));
    }

    #[test]
    fn should_retry_workspace_password_true_while_attempts_remain_interactively() {
        assert!(should_retry_workspace_password(1, false));
        assert!(should_retry_workspace_password(2, false));
    }

    #[test]
    fn should_retry_workspace_password_false_when_interactive_attempts_exhausted() {
        assert!(!should_retry_workspace_password(3, false));
    }
```

- [ ] **Step 3: Run the tests to verify they fail to compile**

Run: `cargo nextest run -p ayx-one-api should_retry_workspace_password --no-run`
Expected: FAIL — `error[E0433]: failed to resolve: use of undeclared function 'should_retry_workspace_password'`.

- [ ] **Step 4: Split `resolve_workspace_password` and implement the re-prompt loop**

Replace the current `resolve_workspace_password` (lines 263-280):
```rust
/// Read the workspace password from `AYX_ONE_WS_PASSWORD`, prompting on the
/// terminal (masked, no echo) if it is not set.
fn resolve_workspace_password() -> Result<String> {
    if let Ok(pw) = std::env::var("AYX_ONE_WS_PASSWORD")
        && !pw.is_empty()
    {
        return Ok(pw);
    }
    eprint!("Workspace password: ");
    std::io::stderr().flush().ok();
    let pw = rpassword::read_password().context(
        "failed to read workspace password (no interactive terminal available — \
         set AYX_ONE_WS_PASSWORD instead)",
    )?;
    let pw = pw.trim().to_string();
    if pw.is_empty() {
        bail!("workspace password is required (set AYX_ONE_WS_PASSWORD or enter it when prompted)");
    }
    Ok(pw)
}
```

with:
```rust
/// The workspace password from `AYX_ONE_WS_PASSWORD`, if set and non-empty.
fn workspace_password_from_env() -> Option<String> {
    std::env::var("AYX_ONE_WS_PASSWORD")
        .ok()
        .filter(|pw| !pw.is_empty())
}

/// Prompt on the terminal for the workspace password (masked, no echo).
fn prompt_workspace_password() -> Result<String> {
    eprint!("Workspace password: ");
    std::io::stderr().flush().ok();
    let pw = rpassword::read_password().context(
        "failed to read workspace password (no interactive terminal available — \
         set AYX_ONE_WS_PASSWORD instead)",
    )?;
    let pw = pw.trim().to_string();
    if pw.is_empty() {
        bail!("workspace password is required (set AYX_ONE_WS_PASSWORD or enter it when prompted)");
    }
    Ok(pw)
}

/// Read the workspace password from `AYX_ONE_WS_PASSWORD`, prompting on the
/// terminal (masked, no echo) if it is not set.
fn resolve_workspace_password() -> Result<String> {
    match workspace_password_from_env() {
        Some(pw) => Ok(pw),
        None => prompt_workspace_password(),
    }
}

/// Whether to try the workspace password again after a rejection.
/// A password sourced from `AYX_ONE_WS_PASSWORD` never gets retried — a
/// fixed environment value will fail identically every time, so retrying
/// it would just burn attempts (and requests against the live auth
/// endpoint) for nothing. Only an interactively-typed password, which
/// could have been mistyped, gets the retry budget.
fn should_retry_workspace_password(attempt: u32, from_env: bool) -> bool {
    !from_env && attempt < WORKSPACE_PASSWORD_ATTEMPTS
}

/// Submits the workspace password, re-prompting on rejection when the
/// password came from interactive input (see should_retry_workspace_password).
fn workspace_login_with_reprompt(client: &Client, base: &str, email: &str) -> Result<()> {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let password_source = workspace_password_from_env();
        let from_env = password_source.is_some();
        let password = match password_source {
            Some(pw) => pw,
            None => prompt_workspace_password()?,
        };
        match submit_workspace_password(client, base, email, &password) {
            Ok(()) => return Ok(()),
            Err(err) if from_env => {
                return Err(err.context(
                    "AYX_ONE_WS_PASSWORD was rejected — not retrying, since a fixed \
                     environment value won't change between attempts; check the secret",
                ));
            }
            Err(err) if !should_retry_workspace_password(attempt, from_env) => {
                return Err(err.context(format!(
                    "workspace password rejected {WORKSPACE_PASSWORD_ATTEMPTS} times — \
                     run `ayx one platform auth login` again"
                )));
            }
            Err(_) => {
                eprintln!(
                    "Workspace password rejected ({attempt}/{WORKSPACE_PASSWORD_ATTEMPTS}) — try again."
                );
            }
        }
    }
}
```

Now replace the Task 3 call site (lines 151-163, as left after Task 3):
```rust
    // 4. Submit the workspace password.
    let ws_password = resolve_workspace_password()?;
    submit_workspace_password(&client, base, email, &ws_password)?;
```
with:
```rust
    // 4. Submit the workspace password, re-prompting on rejection.
    workspace_login_with_reprompt(&client, base, email)?;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo nextest run -p ayx-one-api should_retry_workspace_password resolve_workspace_password -v`
Expected: the 3 new tests pass, and the pre-existing `resolve_workspace_password_env_var_short_circuit` / `resolve_workspace_password_no_tty_fails_cleanly` tests still pass unmodified — confirming `resolve_workspace_password`'s external behavior is unchanged even though it's now implemented in terms of the two new functions.

- [ ] **Step 6: Full crate verification**

Run: `cargo nextest run -p ayx-one-api`
Expected: all tests pass.

- [ ] **Step 7: Clippy, fmt, commit**

Run: `cargo fmt --all && cargo clippy -p ayx-one-api --all-targets -- -D warnings`
Expected: clean.

```bash
git add ayx-one-api/src/email_otp.rs
git commit -m "feat(one-api): re-prompt on wrong workspace password (interactive only)

Up to 3 re-prompts when the password was typed interactively; a
password sourced from AYX_ONE_WS_PASSWORD fails fast on first
rejection instead of retrying a value that can't change. Exhausting
the interactive budget bails with an explicit 'run login again'
message rather than silently sending another OTP."
```

---

### Task 6: Full-workspace verification, CHANGELOG, and manual live check

**Files:**
- Modify: `CHANGELOG.md` (`## Unreleased` section)

**Interfaces:**
- Consumes: nothing new — this task verifies Tasks 1-5 together and documents them.

- [ ] **Step 1: Re-read the fully modified `email_otp_login_pure_http` end to end**

Run: `sed -n '1,220p' ayx-one-api/src/email_otp.rs`
Expected: a coherent read-through — steps 1-2 call `otp_login_with_reprompt`, step 3 unchanged apart from the retry inside `resolve_workspace_name`, step 4 calls `workspace_login_with_reprompt`, steps 5-6 unchanged, step 7 wraps the PAT mint in `retry_transient`. No leftover dead code (e.g. an unused `reference_id` or `ws_password` binding at the top level of `email_otp_login_pure_http` — those now live entirely inside the two re-prompt functions).

- [ ] **Step 2: Full workspace build, lint, and test**

Run: `cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: clean build, zero clippy warnings, no formatting diff.

Run: `cargo nextest run --workspace --locked`
Expected: full suite passes, including the untouched `ayx-rs::one_live_smoke` suite (`AYX_ONE_LIVE_SMOKE` is unset in a normal run, so every case there early-returns — this task doesn't change that gating).

- [ ] **Step 3: Add the CHANGELOG entry**

Add to `CHANGELOG.md` under `## Unreleased` → `### Fixed` (create the `### Fixed` subsection if `## Unreleased` doesn't already have one; if it does, add this as a new bullet):

```markdown
- **`ayx one platform auth login`'s email-OTP flow no longer treats a transient network blip or a typo as a full restart.** Every HTTP call in the flow (sendPasscode, validatePasscode, the OIDC redirect chain, the workspace-password submission, the PAT mint) previously had zero retry logic, and a wrong OTP code or wrong workspace password was an immediate, unrecoverable failure — any of these forced starting over from `sendPasscode`, which means a brand-new OTP email every time. Calls with no duplication risk (validatePasscode, the workspace-password POST, read-only lookups) now retry transient network failures and 429/5xx responses; calls with a real side effect (sendPasscode, the PAT mint) retry only when we're confident the request never reached the server, never on an ambiguous timeout or 5xx, so a retry can't send a second OTP email or mint an orphaned second PAT. A wrong OTP gets up to 3 local re-prompts against the same passcode reference before one fresh passcode is sent automatically (capped at 2 sends total); a wrong interactively-typed workspace password gets up to 3 re-prompts (a password sourced from `AYX_ONE_WS_PASSWORD` fails fast instead, since retrying a fixed value that's wrong just wastes requests).
```

- [ ] **Step 4: Commit the CHANGELOG entry**

```bash
git add CHANGELOG.md
git commit -m "docs(changelog): note the auth login hardening under Unreleased"
```

- [ ] **Step 5: Manual live verification (cannot be automated — do not skip, do not claim done without running it)**

This step exercises real behavior no automated test can safely cover (per the spec's testing section — testing it for real would consume real OTP emails against the live tenant, unacceptable in CI). Run interactively, with a human available to read the OTP email and deliberately mistype once:

1. `ayx one platform auth login --workspace-gid <a real workspace gid> --output json` (use an isolated `AYX_CONFIG_HOME` — do not run against a real working profile without meaning to).
2. When prompted for the passcode, enter an intentionally wrong 6-digit code first.
   - Expected: `Incorrect or expired passcode (1/3) — try again.` printed to stderr, then re-prompted, still using the *same* email (no second email sent).
3. Enter the real code from the email.
   - Expected: login proceeds to the workspace-password step (or completes, if `AYX_ONE_WS_PASSWORD` is set).
4. If testing the workspace-password re-prompt too: temporarily unset `AYX_ONE_WS_PASSWORD` and deliberately mistype the password once.
   - Expected: `Workspace password rejected (1/3) — try again.`, then a successful submission on the second, correct entry completes the login.
5. Confirm the final envelope has `"ok": true` and a `token_expires_at` roughly 30 days out.

Record the outcome (pass/fail, and exact wording of anything unexpected) before considering this plan complete — do not mark this step done from reading the code alone.
