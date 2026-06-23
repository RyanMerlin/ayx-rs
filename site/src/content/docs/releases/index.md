---
title: Releases
description: Versioned release notes for ayx.
sidebar:
  order: 0
---

Release notes for each tagged version of `ayx`. For current behavior, use the live docs above; for a specific binary, read the notes for that version.

## v0.10.3

**Dependency security bump.** `quinn-proto` was bumped from 0.11.14 to 0.11.15 to clear **RUSTSEC-2026-0185**, a remote memory-exhaustion / DoS advisory in a transitive HTTP/3 QUIC dependency. `quinn-proto` is not on the CLI's HTTP/1.1 request path, so this is a `Cargo.lock`-only change with no behavior impact, but it restores a green `cargo audit` gate.

## v0.10.2

**Auth-transport security hardening.**

- **Redirect-host allowlist.** The OIDC redirect follower now refuses off-domain redirects. Only the configured base host, its parent domain, and sibling subdomains are accepted (e.g. `us1.alteryxcloud.com` permits `pingauth.alteryxcloud.com`; a redirect to `evil.com` is rejected with an error).
- **Interaction-id shape validation.** The OIDC interaction id is validated at parse time: 6–128 characters, restricted charset. Malformed values are rejected before any network request is made.
- **Broader response-body redaction.** Two additional error paths (`validatePasscode` and `/v4/auth/accounts`) now redact response bodies in error output. Combined with prior redaction, all major auth-flow error paths suppress raw server responses.
- **Latent unwrap removed in `auth diagnose`.** A panic path reachable under certain error conditions in the diagnose command has been removed.

Known limitation (tracked for a follow-up): loading a profile that contains an `env:`-backed secret ref and then saving it can materialize the resolved value as a concrete secret, dropping the `env:` indirection. Preserving `env:` refs through a load→save round-trip is tracked but not yet fixed.

## v0.10.1

**Playwright fallback removed.**  The email-OTP first-login flow is now pure-HTTP only (reqwest). The headless-Chromium fallback path that was present as a last resort has been removed. There are no longer any `python3`, `playwright`, or `chromium` dependencies for authentication. The `AYX_ONE_AUTH_FORCE_BROWSER` and `AYX_ONE_AUTH_NO_FALLBACK` environment variables have been removed.

The separate `--browser` PKCE auth-code flow on `auth login` is unaffected.

## v0.10.0

**Workspace model clarified — the token determines the workspace.**  The `x-alteryx-workspace-gid` header is ignored server-side; switching workspaces requires `workspace switch` (re-points to an already-authenticated credential) or `auth login` (authenticates a new one).

**`workspace switch --workspace-id <id>`** — new command that instantly makes an already-authenticated workspace credential active.  Errors with guidance to run `auth login` if the credential doesn't exist yet.

**`workspace people` and `workspace admins` are now argless.**  `--workspace-id` has been removed; both commands are scoped to the active workspace via the token.

**Membership mutations reject a mismatched `--workspace-id`.**  `invite-users`, `remove-user`, `suspend-users`, `unsuspend-users`, `transfer`, and `transfer-assets` now error if an explicit `--workspace-id` doesn't match the active workspace.  Omit the flag and use `workspace switch` to change workspaces first.

**`auth login` warns on inline secret storage.**  When no OS keyring backend is available, the command prints a warning that credentials will be stored in the config file as plaintext.  Configuring a keyring backend (macOS Keychain, `libsecret`, Windows Credential Manager) eliminates the plaintext-at-rest risk.

**`connections connector-metadata template` placeholder output.**  When the connection type cannot be confidently inferred, `type` now emits a `<jdbc|remotefile|…>` placeholder and a `_note` field explaining the ambiguity, instead of always defaulting to `remotefile`.

- [v0.9.14](/releases/v0914/)
- [v0.9.13](/releases/v0913/)
- [v0.9.12](/releases/v0912/)
- [v0.9.10](/releases/v0910/)
- [v0.9.9](/releases/v099/)
