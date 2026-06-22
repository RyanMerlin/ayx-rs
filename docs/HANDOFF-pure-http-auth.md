# Pure-HTTP first-login for Alteryx One

**Completed:** 2026-06-22
**Branch:** `fix/keyring-no-default-store`
**Status:** ✅ End-to-end verified. Committed as `9d96b50`.
**Files touched:** `ayx-one-api/src/email_otp.rs`, `ayx-one-api/src/lib.rs`, `ayx-core/src/profile.rs`.

---

## Goal

Replace the `python3 + playwright` headless-Chromium subprocess used for Alteryx One
first-login (email OTP → workspace OIDC → mint 30-day PAT) with a dependency-free pure
`reqwest` flow, **keeping Playwright as an automatic fallback**.

## TL;DR of what was done

`ayx-one-api/src/email_otp.rs`:
- `email_otp_login` (public, **signature unchanged** — call site in
  `ayx-rs/src/cmd/one_platform/auth.rs:249` untouched) is now a wrapper:
  tries pure-HTTP first, falls back to Playwright on any error.
  Escape hatch: `AYX_ONE_AUTH_FORCE_BROWSER=1` skips pure-HTTP entirely.
- `email_otp_login_pure_http` (new) — the reqwest implementation.
- `email_otp_login_playwright` (the original function, renamed verbatim) — the fallback.
- New helpers: `follow_redirects`, `extract_interaction_id`, `resolve_workspace_name`,
  `resolve_workspace_password`, `decode_local_auth_workspace`. `cookie_value_from_jar`
  un-dead-coded (now used).

## The verified flow (reverse-engineered via instrumented Playwright network traces)

The **entire OIDC dance is server-side on the us1 BFF** — confirmed by dumping
localStorage/sessionStorage (no PKCE verifier/code/token client-side). So the pure-HTTP
path needs **no PKCE, no client secret, no token-exchange** — just a cookie jar spanning
the `us1` + `pingauth` domains, manual redirect-following, and one form POST:

```
1. POST  /v4/auth/sendPasscode            {email}                       -> passcodeReferenceId
2. (prompt OTP)
   POST  /v4/auth/validatePasscode        {email,passcode,referenceId}  -> account session cookies
3. GET   /v4/auth/accounts                                              -> map workspaceGid -> name
   GET   /?workspace=<name>&workspaceGid=<gid>   (follow redirects)
         -> pingauth /as/authorize (BFF builds it: code_challenge, redirect_uri
            =/token/auth/code/<uuid>/callback, login_hint — all server-side)
         -> pingauth /rp/authenticate -> us1 /oidc/auth -> /token/<interactionId>
         -> /sign-in?redirect_to=/token/<id>/resume&interaction_id=<id>&workspaceGid=<gid>
         -> 200 /auth-portal/workspaces/<gid>?redirect_to=/token/<id>/resume   (password page)
      [capture <interactionId> from the chain — prefer interaction_id= query param]
4. POST  /session                          form: email & password (workspace pw)
5. GET   /token/<interactionId>/resume     (follow redirects)
         -> /oidc/auth/<id> -> pingauth /rp/callback?code -> /as/resume?flowId
         -> us1 /token/auth/code/<uuid>/callback?code  (BFF does code->token exchange)
         -> sets cookie  local-auth-workspace  (base64url JSON {accessToken, refreshToken})
6. decode local-auth-workspace -> accessToken (~1095 chars, JWT, exp ~5 min)
7. POST  /v4/apiAccessTokens   Bearer <accessToken>
         + headers x-csrf-token (cookie), x-alteryx-workspace-gid
         {name:"ayx-rs-cli", lifetimeSeconds:2592000}  -> 30-day PAT (tokenValue)
```

### Key constants / facts
- Auth-portal base: `https://us1.alteryxcloud.com`
- Workspace-bearer OIDC client_id: `574c67d1-cc32-4d5e-bd82-3f392e7e4717`
  (DIFFERENT from the user-token client `af1b5321-...`), scope
  `local-auth-workspace openid profile`, `acr_values=AlteryxAuthIDPPolicy`.
- us1 IS its own IdP that pingauth federates to (`GET /oidc/auth?client_id=alteryx_oidc_client_id`).
- Flow crosses TWO cookie domains (us1 + pingauth: `_interaction`,`_session`,`RPSID`,`ST`).
- Test account: `ryan.merlin@alteryx.com`, workspace `alteryx-fde` gid
  `01KMGF85WTTEJZ397MW1RBD9ZB`. Workspace password is in `.env` as `AYX_ONE_WS_PASSWORD`.

## Token persistence (answers the "OTP should be rare" question)

OTP is a **first-login-only** event. Output is a **30-day PAT** stored in the profile
(`access_token_ref` via `ayx-core/src/profile.rs`) and reused for every subsequent API
call for 30 days. The `local-auth-workspace.refreshToken` (~1 year) re-mints access tokens
silently via the existing `refresh_token` grant (`POST pingauth/as/token`, no browser, no
OTP). Pure-HTTP changes **nothing** about persistence — only how the one first-login leg is
performed. NOTE: the committed flow currently discards the refresh token; consider also
storing it (`refresh_token_ref`) to extend silent re-auth.

## Key discovery: Accept header

The root cause of the initial failure (workspace-entry URL returning `401` JSON instead of
the OIDC redirect chain) was a missing `Accept` header in `follow_redirects`. The BFF
uses `vary: Accept` to decide response type:

- `Accept: application/json` (reqwest default) → `401` JSON ("MissingPersonException")
- `Accept: text/html,...` (browser default) → `302` → full OIDC chain

Fix: one line in `follow_redirects` — add `.header(ACCEPT, "text/html,...")` to each hop.

## Verified redirect chain (live trace, OTP 675566, 2026-06-22)

```
GET /?workspace=alteryx-fde&workspaceGid=01KMGF85WTTEJZ397MW1RBD9ZB
 → /sign-in
 → pingauth.alteryxcloud.com/as/authorize?client_id=574c67d1-...
 → pingauth.alteryxcloud.com/rp/authenticate?...
 → us1.alteryxcloud.com/oidc/auth?client_id=alteryx_oidc_client_id&...
 → /token/glqI9FpDHQkirawE3nYD5
 → /sign-in?redirect_to=/token/glqI9FpDHQkirawE3nYD5/resume&interaction_id=...
 → /auth-portal/workspaces/01KMGF85WTTEJZ397MW1RBD9ZB?redirect_to=...
```
Then: POST /session (workspace password) → GET /token/<id>/resume → local-auth-workspace
cookie set → POST /v4/apiAccessTokens → 30-day PAT stored.

## Running the flow

```
ayx one platform auth login
```
- Prompts for one 6-digit OTP (emailed to the account).
- Runs pure-HTTP first; on any failure auto-falls-back to Playwright and prints the reason.
- `AYX_ONE_AUTH_FORCE_BROWSER=1` skips pure-HTTP entirely.
- On success the 30-day PAT lands in the profile under `access_token_ref`.

## Operational notes
- `follow_redirects` cap is 25 hops (entry chain ~8, resume chain ~6).
- `extract_interaction_id` prefers the `interaction_id=` query param, falls back to the
  first `/token/<seg>` path segment that isn't `auth`. If Alteryx changes URL shape, this
  is the most likely break point.
- Redirect policy is `Policy::none()` + manual following so the cookie jar
  (`Arc<Jar>` via `.cookie_provider`) accumulates across us1↔pingauth.
- Workspace password: `AYX_ONE_WS_PASSWORD` env, else stdin prompt.

## Follow-ups (not blocking)
- Store the `local-auth-workspace.refreshToken` (~1 year) in `refresh_token_ref` for
  silent PAT re-mint without OTP. Currently discarded.
- Investigate whether workspace password alone (no OTP) can bootstrap the OIDC flow.

## Background / full detail
See agent memory `auth_model.md` (section "Interactive first-login (OTP) surface" and
"PKCE custody RESOLVED") for the complete trace-derived notes, dead-ends already ruled out
(refresh token in `.env` is server-revoked; user-token client + guessed redirect_uri →
"Redirect URI mismatch"; calling validatePasscode before initiating authorize leaves no
interaction to resume → no password form).
