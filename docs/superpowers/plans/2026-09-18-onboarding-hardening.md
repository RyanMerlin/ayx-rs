# Onboarding Hardening (post-v0.22.3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the remaining confirmed ways a first-time Windows user gets stuck or confused after `ayx onboard`, and lock the sign-in failure modes behind end-to-end tests.

**Architecture:** Three independent, test-first changes:
1. Terminal rendering: only emit ANSI color when the console will interpret it.
2. `ayx doctor auth` guidance: tell a user with no credentials exactly what to run.
3. A wire-level scenario suite: drive the full email-OTP login against the existing in-process recording server for every token-mint outcome.

None of them changes a public JSON field name or a command.

**Tech Stack:** Rust (edition 2024, let-chains allowed), `anyhow`, `clap` styling (`anstyle`), `crossterm` 0.29, `cargo nextest`, in-crate `RecordingServer` test harness in `ayx-one-api/src/email_otp.rs`.

**Spec:** The sources of truth are the findings of the 2026-09-18 onboarding incident and the v0.22.3 fix:
- PR #197 description (https://github.com/RyanMerlin/ayx-rs/pull/197)
- `docs/releases/v0.22.3.md`

Incident evidence:
- A user's `ayx onboard` output showed raw escape sequences (`←[1m←[38;2;0;103;185m onboarding completed←[0m`).
- `ayx doctor auth` reported `one_status: incomplete` with `guidance: -` for a profile that had no credentials.
- A token-mint failure was reported as "PAT-mint outcome is unknown".

## Global Constraints

- **Base:** branch from tag `v0.22.3` (commit `7a668f8`) in a new worktree, for example `git worktree add ../ayx-rs-onboarding-hardening -b fix/onboarding-hardening v0.22.3`. Do not base on local `main`, which carries unreleased, unpushed work.
- **Line endings:** files in the worktree are checked out with CRLF (`core.autocrlf=true`). Make multi-line edits with an editor tool, not scripts that match `\n`.
- **Gates** (must pass before every commit, same as CI):
  - `cargo fmt --all --check`
  - `cargo run -q -p xtask -- refresh-command-surface --check`
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`
  - `AYX_FORCE_INLINE_SECRETS=1 cargo nextest run --workspace --locked`
- **Commits:** every commit message ends with `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`.
- **Changelog:** each task adds its line under `## Unreleased` in `CHANGELOG.md` in the same commit.
- **Compatibility:** do not change the keyring credential-binding issuer (`AlteryxOneProfile::binding_issuer_url_for_workspace`); a regression test pins it.
- **Safety:** never replay `POST /v4/apiAccessTokens` after it was sent. `retry_transient` must keep `is_pre_send_failure` as its retry predicate for the mint.

---

### Task 1: Do not print raw escape codes on consoles that cannot render them

On Windows PowerShell 5.1 in the classic console host, virtual-terminal (VT) processing is off, so the CLI's ANSI color sequences print literally. `crossterm::ansi_support::supports_ansi()` (public, Windows-only, no feature flag) tries to turn VT on and reports whether it worked. Color must require it.

**Files:**
- Modify: `ayx-rs/src/render.rs` (`color_enabled` ~line 196, `color_for` ~line 207, tests ~lines 850-873)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces: `fn color_for(ok: bool, is_terminal: impl Fn(Stream) -> bool, no_color: bool, ansi_supported: bool) -> bool` and `fn console_accepts_ansi() -> bool` (private to `render.rs`).

- [ ] **Step 1: Write the failing test.** In the `#[cfg(test)]` module of `ayx-rs/src/render.rs`:
  - Update every existing `color_for(...)` call in the tests to pass a fourth argument `true`. They are the assertions around lines 855-871: `color_for(true, stdout_only, false)` becomes `color_for(true, stdout_only, false, true)`, and so on.
  - Then add:

```rust
    #[test]
    fn no_color_when_the_console_cannot_interpret_escape_codes() {
        let stdout_only = |stream: Stream| stream == Stream::Stdout;
        assert!(
            !color_for(true, stdout_only, false, false),
            "a console without VT processing prints escape codes literally"
        );
        assert!(color_for(true, stdout_only, false, true));
    }
```

- [ ] **Step 2: Run the test to verify it fails.**
Run: `cargo test -p ayx-rs --bin ayx render::tests::no_color_when_the_console_cannot_interpret_escape_codes`
Expected: compile error `this function takes 3 arguments but 4 arguments were supplied`.

- [ ] **Step 3: Implement.** Replace `color_enabled` and `color_for` in `ayx-rs/src/render.rs` with:

```rust
/// Color only when the stream this envelope goes to is a terminal that will
/// interpret escape codes. Checking stdout for a failure wrote ANSI escapes
/// into a redirected stderr log.
fn color_enabled(ok: bool) -> bool {
    color_for(
        ok,
        |stream| match stream {
            Stream::Stdout => std::io::stdout().is_terminal(),
            Stream::Stderr => std::io::stderr().is_terminal(),
        },
        env::var_os("NO_COLOR").is_some(),
        console_accepts_ansi(),
    )
}

fn color_for(
    ok: bool,
    is_terminal: impl Fn(Stream) -> bool,
    no_color: bool,
    ansi_supported: bool,
) -> bool {
    let stream = if ok { Stream::Stdout } else { Stream::Stderr };
    !no_color && ansi_supported && is_terminal(stream)
}

/// A Windows console prints ANSI escape codes literally unless virtual-terminal
/// processing is on (Windows PowerShell 5.1 in the classic console host leaves
/// it off). Try to turn it on; if that fails, render without color.
#[cfg(windows)]
fn console_accepts_ansi() -> bool {
    crossterm::ansi_support::supports_ansi()
}

#[cfg(not(windows))]
fn console_accepts_ansi() -> bool {
    true
}
```

- [ ] **Step 4: Run the tests to verify they pass.**
Run: `cargo test -p ayx-rs --bin ayx render::tests`
Expected: all `render::tests` pass, including the new test and the updated `color_for` assertions.

- [ ] **Step 5: Manual check on Windows.** Build with `cargo build -p ayx-rs`. Open **Windows PowerShell 5.1** (`powershell.exe`, not `pwsh`) in the classic console host and run `target\debug\ayx.exe doctor`.
Expected: `✔`/`⚠` rows render, with either real color or plain text. No `←[` sequences appear.

- [ ] **Step 6: Add the changelog line and commit.** Under `## Unreleased` in `CHANGELOG.md` add:

```markdown
### Fixed

- Terminal output no longer shows raw escape codes (`←[1m…`) in Windows
  consoles that do not interpret them, such as Windows PowerShell 5.1 in the
  classic console host. The CLI turns on virtual-terminal processing where it
  can and otherwise prints without color.
```

```bash
git add ayx-rs/src/render.rs CHANGELOG.md
git commit -m "fix(render): print without color when the Windows console cannot interpret ANSI" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `ayx doctor auth` says "not signed in" and what to run

For a profile with an `alteryx_one` section but no access or refresh token, `doctor auth` reports `one_status: incomplete` and `guidance: -`. Move the guidance selection into a pure function and add that case first.

**Files:**
- Modify: `ayx-rs/src/main.rs` (`doctor_auth_envelope`, the `let one_guidance: Option<&str> = ...` block ~lines 6751-6776; tests module near the existing `auth_product_status` tests ~line 927)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces: `fn one_auth_guidance(configured: bool, kind: Option<ayx_core::profile::OneCredentialKind>, access_token_present: bool, refresh_token_present: bool, access_token_expired: bool, renews_automatically: bool) -> Option<&'static str>` (private, `main.rs`).

- [ ] **Step 1: Write the failing test.** Add to the `#[cfg(test)]` module in `ayx-rs/src/main.rs`:

```rust
    #[test]
    fn one_auth_guidance_tells_a_profile_without_credentials_to_sign_in() {
        let guidance = one_auth_guidance(true, None, false, false, false, false)
            .expect("a configured profile with no credentials needs guidance");
        assert!(guidance.contains("Not signed in"), "{guidance}");
        assert!(guidance.contains("ayx one login"), "{guidance}");
        assert!(guidance.contains("--oauth-api-token"), "{guidance}");
    }

    #[test]
    fn one_auth_guidance_keeps_the_existing_credential_messages() {
        use ayx_core::profile::OneCredentialKind;
        let otp = one_auth_guidance(true, Some(OneCredentialKind::EmailOtp), true, false, false, false)
            .expect("OTP guidance");
        assert!(otp.contains("30 days"), "{otp}");
        let expired_otp =
            one_auth_guidance(true, Some(OneCredentialKind::EmailOtp), true, false, true, false)
                .expect("expired OTP guidance");
        assert!(expired_otp.contains("passed its"), "{expired_otp}");
        let renewing = one_auth_guidance(true, Some(OneCredentialKind::OAuthRefresh), true, true, false, true)
            .expect("renewing guidance");
        assert!(renewing.contains("renews access tokens automatically"), "{renewing}");
        assert_eq!(one_auth_guidance(false, None, false, false, false, false), None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail.**
Run: `cargo test -p ayx-rs --bin ayx one_auth_guidance`
Expected: compile error `cannot find function one_auth_guidance`.

- [ ] **Step 3: Implement.**
  - Add this function next to `doctor_auth_envelope` in `ayx-rs/src/main.rs`. The existing message strings are moved verbatim.

```rust
/// What `ayx doctor auth` tells the user to do about their Alteryx One
/// credential. A profile with no credential at all comes first: it is the
/// state a failed first sign-in leaves behind, and it needs a next step.
fn one_auth_guidance(
    configured: bool,
    kind: Option<ayx_core::profile::OneCredentialKind>,
    access_token_present: bool,
    refresh_token_present: bool,
    access_token_expired: bool,
    renews_automatically: bool,
) -> Option<&'static str> {
    if configured && !access_token_present && !refresh_token_present {
        return Some(
            "Not signed in to Alteryx One for this profile. Run `ayx one login`, or set up the \
             durable credential with `ayx one login --oauth-api-token` (Client ID and refresh \
             token from Alteryx One: profile menu > API Tokens).",
        );
    }
    if kind == Some(ayx_core::profile::OneCredentialKind::EmailOtp) {
        return if access_token_expired {
            Some(
                "Email OTP is a time-limited login and this access token has passed its \
                 expiry. Sign in again with `ayx one login`, or set up the durable \
                 credential once with `ayx one login --oauth-api-token` so access renews \
                 on its own.",
            )
        } else {
            Some(
                "Email OTP is a time-limited login: this access token lasts 30 days and will \
                 not renew on its own. To stop signing in again on that cycle, upgrade to the \
                 durable credential with `ayx one login --oauth-api-token`.",
            )
        };
    }
    if configured && renews_automatically {
        Some("This credential renews access tokens automatically; no periodic sign-in.")
    } else if configured && access_token_expired {
        Some(
            "This access token has passed its expiry and this credential cannot renew it. \
             Sign in again with `ayx one login`.",
        )
    } else {
        None
    }
}
```

  - Then replace the whole `let one_guidance: Option<&str> = if ... else { None };` block in `doctor_auth_envelope` with:

```rust
    let one_guidance = one_auth_guidance(
        one_configured,
        one_credential_kind,
        one_access_token_present,
        one_refresh_token_present,
        one_access_token_expired,
        one_renews_automatically,
    );
```

  - Keep the explanatory comment that sat above the old block; move it onto `one_auth_guidance`.

- [ ] **Step 4: Run the tests to verify they pass.**
Run: `cargo test -p ayx-rs --bin ayx one_auth_guidance` then `cargo test -p ayx-rs --bin ayx doctor`
Expected: PASS. The existing doctor tests are unchanged and still pass.

- [ ] **Step 5: Add the changelog line and commit.** Under `## Unreleased` / `### Fixed` in `CHANGELOG.md` add:

```markdown
- `ayx doctor auth` now tells a profile with no Alteryx One credential that it
  is not signed in and to run `ayx one login` (or `--oauth-api-token`),
  instead of reporting `guidance: -`.
```

```bash
git add ayx-rs/src/main.rs CHANGELOG.md
git commit -m "fix(doctor): guide a profile without One credentials to sign in" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Wire-level scenarios for every token-mint outcome

The v0.22.3 unit tests cover `pat_response_value` and `pat_mint_failure` in isolation. This task drives the **whole** email-OTP login (session, passcode, workspace, password, OIDC resume, mint) against the existing `RecordingServer` in `ayx-one-api/src/email_otp.rs`. It checks what the user sees for each mint outcome, and that the mint is sent exactly once.

**Files:**
- Modify: `ayx-one-api/src/email_otp.rs`, test module only:
  - `RecordingServer` constructors, ~lines 1084-1110;
  - its request-dispatch branch, ~lines 1130-1150;
  - new tests after `wizard_otp_flow_preserves_legacy_http_sequence`, ~line 2229.
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes (from v0.22.3):
  - `crate::email_otp::PatMintRejected { status: u16, server_message: Option<String> }`
  - `PatMintRejected::suggests_oauth_api_token(&self) -> bool`
  - `crate::WizardOtpAdapter.login(base_url, email, workspace_gid, workspace_password: Option<String>, get_otp) -> Result<OtpAuthResult>`
  - `crate::email_otp_login(base_url, email, workspace_gid, workspace_password: Option<String>, get_otp) -> Result<OtpAuthResult>` (legacy lane; `get_otp` must be `Send + 'static`)
- Produces: test-only `RecordingServer::start_with_mint_response(status: u16, body: &'static str) -> RecordingServer`.

- [ ] **Step 1: Give the recorder a configurable mint response.**
  - In the test module of `ayx-one-api/src/email_otp.rs`, add a fifth parameter `mint_response: Option<(u16, &'static str)>` to `RecordingServer::start_with_failures`.
  - Pass `None` from every existing caller. Find them with `rg -n "start_with_failures\(" ayx-one-api/src/email_otp.rs`; the four wrappers `start_with_otp_rejections_and_status` / `start_with_password_rejections` and any direct calls.
  - Add the constructor:

```rust
        fn start_with_mint_response(status: u16, body: &'static str) -> Self {
            Self::start_with_failures(0, 401, 0, 401, Some((status, body)))
        }
```

  - In the request-dispatch chain inside the spawned thread, add this branch immediately before the final `} else { response_for_request(&request) };`:

```rust
                            } else if request.method == "POST"
                                && request.target.starts_with("/v4/apiAccessTokens")
                                && let Some((status, body)) = mint_response
                            {
                                (
                                    status,
                                    vec![("Content-Type", "application/json".to_string())],
                                    body.to_string(),
                                )
```

- [ ] **Step 2: Write the scenario tests.** Add after `wizard_otp_flow_preserves_legacy_http_sequence`:

```rust
    fn mint_requests(server: &RecordingServer) -> usize {
        server
            .requests()
            .iter()
            .filter(|request| {
                request.method == "POST" && request.target.starts_with("/v4/apiAccessTokens")
            })
            .count()
    }

    fn wizard_login_error(server: &RecordingServer) -> anyhow::Error {
        match crate::WizardOtpAdapter.login(
            &server.base_url,
            "person@example.com",
            "gid-1",
            Some("workspace-secret".to_string()),
            || Ok("123456".to_string()),
        ) {
            Ok(_) => panic!("a failed token mint must not produce a login"),
            Err(err) => err,
        }
    }

    #[test]
    fn wizard_reports_a_refused_mint_as_a_typed_rejection_and_sends_it_once() {
        let server = RecordingServer::start_with_mint_response(
            403,
            r#"{"exception":{"name":"AccessControlException","message":"Permission denied","details":"API access tokens are disabled for this workspace."}}"#,
        );
        let err = wizard_login_error(&server);
        let rejected = err
            .downcast_ref::<PatMintRejected>()
            .expect("a refused mint is a typed rejection at the top of the chain");
        assert_eq!(rejected.status, 403);
        assert!(rejected.suggests_oauth_api_token());
        let message = err.to_string();
        assert!(message.contains("API access tokens are disabled"), "{message}");
        assert!(message.contains("ayx one login --oauth-api-token"), "{message}");
        assert!(!message.contains("unknown"), "{message}");
        assert_eq!(mint_requests(&server), 1, "a sent mint must never be replayed");
    }

    #[test]
    fn wizard_advises_retrying_login_when_the_session_expired_before_the_mint() {
        let server = RecordingServer::start_with_mint_response(
            401,
            r#"{"exception":{"message":"Unauthorized"}}"#,
        );
        let err = wizard_login_error(&server);
        let rejected = err
            .downcast_ref::<PatMintRejected>()
            .expect("typed rejection");
        assert_eq!(rejected.status, 401);
        assert!(!rejected.suggests_oauth_api_token());
        assert!(err.to_string().contains("run `ayx one login` again"), "{err}");
        assert_eq!(mint_requests(&server), 1);
    }

    #[test]
    fn wizard_keeps_a_server_error_mint_unknown_with_its_cause_and_sends_it_once() {
        let server =
            RecordingServer::start_with_mint_response(503, r#"{"message":"upstream unavailable"}"#);
        let err = wizard_login_error(&server);
        assert!(err.downcast_ref::<PatMintRejected>().is_none());
        let message = err.to_string();
        assert!(message.contains("unknown"), "{message}");
        assert!(message.contains("503"), "{message}");
        assert_eq!(mint_requests(&server), 1, "a sent mint must never be replayed");
    }

    #[test]
    fn legacy_lane_reports_a_refused_mint_as_a_typed_rejection() {
        let server = RecordingServer::start_with_mint_response(
            403,
            r#"{"exception":{"details":"API access tokens are disabled for this workspace."}}"#,
        );
        let err = match crate::email_otp_login(
            &server.base_url,
            "person@example.com",
            "gid-1",
            Some("workspace-secret".to_string()),
            || Ok("123456".to_string()),
        ) {
            Ok(_) => panic!("a refused mint must not produce a login"),
            Err(err) => err,
        };
        assert!(
            err.chain().any(|cause| cause.is::<PatMintRejected>()),
            "legacy lane must surface the typed rejection: {err:#}"
        );
        assert_eq!(mint_requests(&server), 1);
    }
```

- [ ] **Step 3: Run the scenarios.**
Run: `cargo test -p ayx-one-api --lib -- wizard_ legacy_lane_`
Expected: the four new tests pass on v0.22.3 code. **They are characterization tests: they lock in the v0.22.3 behavior.**
  - If a test fails, the failure is a real bug in the v0.22.3 wiring, not in the test. Stop and report it with the failing assertion.
  - To prove the suite has teeth, temporarily change `pat_response_value` so the `status.is_client_error()` branch uses `bail!` instead of returning `PatMintRejected`. Re-run and confirm the 403/401/legacy tests fail, then revert.

- [ ] **Step 4: Run the whole crate.**
Run: `cargo test -p ayx-one-api --lib`
Expected: PASS. The existing recorder tests are unchanged because they pass `mint_response: None`.

- [ ] **Step 5: Add the changelog line and commit.** Under `## Unreleased` add:

```markdown
### Changed

- Added end-to-end email-OTP login scenarios for refused (403), expired-session
  (401) and server-error (503) token mints, in both the Wizard and legacy
  flows. They verify what the user is told and that the mint request is never
  replayed.
```

```bash
git add ayx-one-api/src/email_otp.rs CHANGELOG.md
git commit -m "test(one): cover every token-mint outcome over the full OTP wire flow" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Gate, PR, and patch release

- [ ] **Step 1:** Run all four gates from Global Constraints. Expected: all pass.
- [ ] **Step 2:** Push `fix/onboarding-hardening` and open a PR into `main`. The PR body lists the three fixes and the incident evidence above.
- [ ] **Step 3:** After the PR is approved and merged, follow the `v0.22.3` release pattern for `v0.22.4`:
  - bump `version` in `Cargo.toml` (refresh `Cargo.lock` with `cargo build -p ayx-rs --offline`);
  - move the `## Unreleased` entries under `## 0.22.4 — <date>`;
  - add `docs/releases/v0.22.4.md`. `build-release.yml` fails without it.
  - The maintainer pushes an annotated tag `v0.22.4`.

---

## Not in this plan: decisions needed first

These were found in the same investigation but need a product or design decision before they can be planned:

1. **Lead onboarding with the OAuth API-token login.** The email-OTP path depends on `feature.apiAccessTokens.enabled`, which defaults to off. The OAuth API-token feature appears to be on by default, and its tokens renew themselves. Choose: OAuth first, an equal choice up front, or keep OTP first with the v0.22.3 fallback.
2. **The credential fallback between profiles.** `merge_one_profiles` deliberately gives a non-active profile the active profile's tokens and `workspace_credentials`. A `--profile X` check can then pass on another profile's login, and `login --access-token --profile <new>` fails when the active profile is OAuth. Draft issue: `C:\code\ayx-rs-token-store\ISSUE-DRAFT-profile-credential-overlay.md`.
3. **`ayx onboard` exit status when sign-in failed.** Currently `ok: true`, exit 0, with an honest message. Returning non-zero is a CLI contract change for scripts and agents.
4. **Token clutter.** Every email-OTP login mints a new 30-day `ayx-rs-cli` token; one account had 25. Choose: reuse, revoke the previous one, or leave it.

## Not in this plan: investigate first

- `ayx one logout --profile <name>` returned `ok: false` and left keyring entries behind (seen once, on a profile created by `--store-profile`).
- Whether `pingauth.alteryxcloud.com` is the OAuth issuer for regions other than us1. eu1 and au1 regional hosts behave like us1 at `/as/token`, but the issuer itself is unconfirmed.
- The browser and device-code login flows build `/as/authorize` and `/as/device_authorization` from the token endpoint. A reviewer flagged the path as possibly wrong for Ping; unverified.
- **Root cause of the two colleagues' failures.** Confirm from `v0.22.3` output, or by checking the workspace's API-access-token setting.
