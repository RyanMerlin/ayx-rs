# Alteryx One email-OTP login: transient-network retry + wrong-credential re-prompt

**Date:** 2026-07-15
**Status:** Approved, ready for implementation planning

## Background

While rotating an expired Alteryx One PAT via `ayx one platform auth login`
(the default email-OTP flow), the operator burned a real OTP email on a
single mistake: `resolve_workspace_password()` already checks
`AYX_ONE_WS_PASSWORD` before falling back to an interactive masked prompt,
but the caller driving the flow didn't pre-supply it, so the process blocked
on a `Workspace password:` prompt that wasn't anticipated. The wrong value
was fed in, `POST /session` rejected it, and the whole flow — including the
already-consumed OTP — had to restart from `sendPasscode`, costing a second
email round-trip.

Reading `ayx-one-api/src/email_otp.rs` and the `login()` call site in
`ayx-rs/src/cmd/one_platform/auth.rs` end to end surfaced two structural
gaps behind that incident, independent of the operator error that triggered
it:

1. **Zero retry logic anywhere in the flow.** `sendPasscode`,
   `validatePasscode`, the OIDC redirect chain, `POST /session`,
   `/token/<id>/resume`, and the `apiAccessTokens` mint are each a single
   attempt with `anyhow` context wrapping and no retry. A transient network
   blip at any of these ~7 steps kills the whole multi-hop flow.
2. **Wrong OTP code or wrong workspace password is a hard, unrecoverable
   failure.** `validatePasscode` and `POST /session` both `bail!()`
   immediately on rejection. There is no re-prompt — any typo forces a
   restart from `sendPasscode`, burning a new OTP email every time.

This spec hardens both, scoped entirely to the OTP login flow. It does not
touch credential storage (the plaintext-YAML-when-keyring-unavailable
fallback is a separate, deliberately deferred concern) and does not touch
any other command's HTTP client.

## Decisions

### 1. Scope: targeted, not generic middleware

Rejected: a generic `reqwest-middleware`/`reqwest-retry` layer applied to
every HTTP client in ayx-rs. That's a new dependency and a blast radius
covering commands that were never part of this problem. This spec adds a
small local retry helper inside `ayx-one-api` and applies it only to the
calls inside `email_otp_login_pure_http`, plus a re-prompt loop in the
`get_otp` closure's call site and around the workspace-password submission.

### 2. Two independent mechanisms

**Transient-network retry** (`retry_transient<T>()`, ~3 attempts, short
fixed backoff — 250ms / 750ms / 1.5s) is about the *transport*: did the
request round-trip at all. **Wrong-credential re-prompt** is about the
*content*: the request round-tripped fine and the server said no. These are
different failure classes with different safe responses and must not be
conflated into one retry loop — retrying a rejected password blindly (as a
"transient failure") would silently hammer the auth endpoint; treating a
network timeout as "wrong credential" would burn an OTP resend for no
reason.

### 3. Retry eligibility is per-call, based on duplication risk

| Call | Retry on transient failure? | Why |
|---|---|---|
| `GET /v4/auth/accounts` | Yes, any transient/5xx | Read-only, idempotent |
| Redirect-chain GETs | Yes, any transient/5xx | Read-only, idempotent |
| `POST /v4/auth/sendPasscode` | Only on pre-send failure (DNS/connect-refused) | A timeout-awaiting-response is ambiguous — the email may already be in flight. Retrying then risks a duplicate OTP email. |
| `POST /v4/auth/validatePasscode` | Yes, any transient/5xx | Resubmitting the same code has no duplication side effect |
| `POST /session` | Yes, any transient/5xx | Resubmitting the same password has no duplication side effect |
| `/token/<id>/resume` redirect chain | Yes, any transient/5xx | Read-only-ish resumption, idempotent |
| `POST /v4/apiAccessTokens` | Only on pre-send failure | A timeout-awaiting-response is ambiguous — the PAT may already be minted server-side. Retrying then risks minting a second, orphaned PAT the user never sees. |

"Pre-send failure" means the error is a connect/DNS-level `reqwest::Error`
(`.is_connect()`), not a timeout waiting on a response
(`.is_timeout()` after the request was sent) and not any HTTP status —
those are treated as "reached the server, outcome unknown, do not repeat."

### 4. Wrong-OTP re-prompt

Up to 3 attempts reusing the same `passcodeReferenceId` from the original
`sendPasscode` call. On each `validatePasscode` rejection:

- If the response indicates "wrong code" (the reference is still valid) —
  re-prompt for a fresh 6-digit code, same reference, decrement the
  attempt budget.
- If the response indicates the reference itself is dead (expired /
  too-many-attempts) — automatically call `sendPasscode` again, tell the
  user plainly why ("Code expired or too many attempts — sending a new
  passcode..."), and reset the attempt counter against the new reference.
  Capped at **2 total sends** for the whole login call so a persistent
  problem degrades to a clear final error instead of silently spamming the
  inbox.

This does not require the API's exact multi-attempt semantics to be known
in advance — it tries reuse first and only falls back to a fresh send when
the server itself signals the reference is unusable, so it degrades
correctly either way.

### 5. Wrong-workspace-password re-prompt

Up to 3 attempts against the same already-established session/interaction
(the cookies and `interaction_id` captured before the password step are
reused as-is; nothing upstream of `POST /session` is repeated). Exhausting
3 attempts **bails with an explicit "run `ayx one platform auth login`
again" message** — it does not automatically loop back to `sendPasscode`.
A fresh OTP send must stay a deliberate user action; auto-restarting the
outer flow after a password typo streak would silently consume another OTP
email without the user asking for it.

### 6. Testing

The classification logic — is this error retryable, is this response
"wrong code" vs. "dead reference," is this a pre-send vs. post-send
failure — is written as pure functions and unit tested the same way
`host_allowed`/`is_valid_interaction_id` already are in this file: no live
HTTP, deterministic inputs. The retry loop's attempt-counting is tested
with a fake closure counting invocations, not real network calls.

Live end-to-end behavior (actual transient network flakiness, an actual
wrong-OTP-then-right-OTP round trip against the real API) is **not
covered by automated tests** — exercising it for real would consume real
OTP emails against the live tenant, which is unacceptable to run in CI or
on every local `cargo nextest run`. This gap is accepted and stays
manually verified at implementation time, not silently claimed as tested.

## Out of scope (explicitly deferred)

- OS keyring storage vs. the current plaintext-YAML-with-warning fallback.
- The `--browser` (PKCE) and `--device` login flows — this spec only
  covers the default email-OTP path.
- Any change to credential storage format, `access_token_ref`/
  `refresh_token_ref` scheme handling, or the `.env`-based live-smoke test
  harness.
