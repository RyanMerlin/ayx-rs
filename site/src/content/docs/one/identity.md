---
title: Identity & auth
description: Sign in, sign out, check who you are, and verify Alteryx One auth posture — ayx one login, logout, whoami, auth status/diagnose, doctor identity, and inventory.
sidebar:
  order: 1
---

`ayx one` handles authentication and identity through a small set of commands: `login` and `logout` manage credentials, `whoami` shows who you're signed in as, `auth status` / `auth diagnose` check token posture, `doctor identity` runs a deeper identity health check, and `inventory` summarizes the current One API surface registry. OAuth2.0 API access/refresh credentials are the recommended method for automation, CI, and agents; email OTP remains the default interactive method. Both are selected per workspace and secure persistence uses the operating-system keyring.

All mutating commands (anything that creates, updates, suspends, removes, or deletes) are dry-run by default. Add `--apply` to commit the change. Add `--yes` to skip the TTY confirmation in scripts.

## What's here

| Area | Command | What you do |
|---|---|---|
| Sign in | `ayx one login` | Authenticate and store credentials (OAuth API access/refresh for automation; email OTP by default for interactive use) |
| Sign out | `ayx one logout` | Clear stored credentials from the active profile |
| Identity | `ayx one whoami` | Show the current One user profile |
| Auth status | `ayx one auth status` | Summarize One API token posture for managed IAM |
| Auth diagnose | `ayx one auth diagnose` | Validate One API token reachability and workspace scope |
| Identity doctor | `ayx one doctor identity` | Run the One identity doctor workflow |
| Inventory | `ayx one inventory` | Summarize the current One API surface registry |

Workspace, user, token, and role administration all build on the identity established here — see [Workspace](/one/workspace/), [Person](/one/person/), [API tokens](/one/token/), and [Roles](/one/role/).

## Signing in

Choose the credential method deliberately. For unattended use, prefer the
OAuth API access/refresh method below. It is a one-time import of a pair issued
by Alteryx One; subsequent access-token renewal is automatic while the refresh
credential remains valid. Use email OTP when a human is available for the
passcode and workspace-password prompts.

```bash
# Email OTP flow
ayx one login

# One-time OAuth API-token setup, not email OTP: paste the refresh token at the hidden prompt.
# It is verified, then stored in the operating-system keyring.
ayx one login --oauth-api-token

# Non-interactive automation only: import from stdin (POSIX shell)
printf '%s' "$AYX_ONE_API_REFRESH_TOKEN" |
  ayx one login --auth-method oauth-refresh --refresh-token-stdin

# PowerShell equivalent
$env:AYX_ONE_API_REFRESH_TOKEN |
  ayx one login --auth-method oauth-refresh --refresh-token-stdin

# Device-code grant
ayx one login --device

# Browser PKCE flow
ayx one login --browser

# Compatibility path; prefer --refresh-token-env or --refresh-token-stdin
ayx one login --refresh-token <t>
ayx one login --access-token <t>
ayx one login --access-token-env AYX_ONE_API_ACCESS_TOKEN

# Bind credentials to a specific workspace
ayx one login --workspace-id <id> --workspace-gid <gid>
```

`--client-id` overrides the profile's `oauth_client_id` for the `--browser` / `--device` flows. See [Connecting](/connecting/) for the full sign-in walkthrough.

### Choosing the credential method

`--auth-method email-otp` and `--auth-method oauth-refresh` set the user credential policy on the selected workspace credential. For a person, `--oauth-api-token` is the clear setup command: it is not email OTP and asks for the visible Client ID followed by one hidden Refresh Token paste, then verifies and stores the pair securely. It never falls back to OTP or a service principal. A workspace with no `credential_kind` keeps legacy behavior; existing workspaces with a refresh credential are treated as OAuth for compatibility.

The OAuth pair normally consists of a client ID, token endpoint, access token,
and refresh token from Alteryx One's OAuth2.0 API-token administration flow.
Configure the client ID and endpoint in the profile (or use the documented
environment overrides), or run `ayx one login --oauth-api-token` and paste the
Client ID shown on the OAuth2.0 API Tokens page followed by the Refresh Token
at the hidden prompt. The CLI verifies the workspace before persisting the
pair. It stores only secure references in the profile when secure persistence
is selected, preserves provider-issued refresh-token replacements, and does
not print token values. The
`--refresh-token-env NAME` and `--refresh-token-stdin` forms are for
non-interactive automation.

`--access-token-env NAME` and `--access-token-stdin` avoid placing an access
token in process arguments or shell history. An access-token-only import cannot
be used with `oauth-refresh`, because it cannot support refresh-token rotation.

`auth_rollout` is separate: it selects the Wizard or Legacy implementation of the email-OTP flow and has no meaning for OAuth credentials. `auth_mode: service-principal` is also separate and selects client-credentials authentication for machine identities; it cannot be combined with a workspace user-credential policy.

OAuth access tokens are short-lived. The CLI stores the rotating refresh token and refreshes before an applied mutation when its access token is expired or within the safety window. It never replays a mutation after an uncertain response.

After OAuth API-token setup, ordinary `ayx one ...` commands renew access
automatically. A bare `ayx one login` reports that OAuth is already configured
instead of silently consuming a refresh grant. Use `ayx one auth diagnose` to
validate the connection, or `ayx one login --oauth-api-token` only when you
intend to replace the saved Refresh Token.

Refresh-token exchange and provider-side rotation cannot be one atomic
transaction with local keyring storage. If the process or keyring fails after
the provider accepts an exchange, local state may be temporarily in doubt. The
CLI reports that condition and does not blindly retry the exchange; obtain or
re-enter a fresh provider-issued pair if recovery is required.

### Saving the workspace password

The default email-OTP login prompts for the workspace password when it is not already available. After the remote login succeeds, the first interactive login asks:

```text
Save this workspace password securely for future logins? [Y/n]
```

Press Enter to save it in the operating system's secure keyring, or answer `n` to keep it for this login only. Later logins for the selected profile reuse the saved password. On Windows, this uses Windows Credential Manager; it is not written into the profile YAML or an environment file.

`--save-workspace-password` remains an optional automation shorthand for the default email-OTP flow. If secure storage is unavailable, `--secret-policy plaintext` is an explicit fallback that requires affirmative consent. The standalone login command rejects `--secret-policy session` because it cannot preserve a session after the process exits.

## Signing out

```bash
ayx one logout
```

Clears stored Alteryx One credentials from the active profile. Local keyring entries are deleted only when no other profile references them; shared entries are retained. Remote token revocation is not attempted. Add `--apply` to commit and `--yes` to skip the TTY confirmation.

## Who am I?

```bash
ayx one whoami
```

Shows the current One user profile — the account and workspace your active token resolves to. (The root-level `ayx whoami` gives a broader summary across profile, One, and Server; `ayx one whoami` is the One-specific view.)

## Checking auth posture

```bash
# Summarize token posture for managed IAM
ayx one auth status

# Validate reachability and workspace scope
ayx one auth diagnose

# Deeper identity health check
ayx one doctor identity
```

Run these first when a One command fails with an auth error. If `auth diagnose` passes but a later command still fails, your token has likely expired — run `ayx one login` again.

## Asset inventory

```bash
ayx one inventory
```

Summarizes the current One API surface registry — a quick read on what's reachable from the active profile.

## JSON output

Pass `--output json` to get a structured envelope on stdout. `--output` is a global flag, so it can appear before or after the subcommand:

```bash
ayx --output json one whoami
```

The envelope shape is `{ ok, message, timestamp_utc, data }` on success; failures also include `error_code`. Combine with `--verbose` to see progress on stderr without polluting stdout.

## Related

- [Workspace](/one/workspace/) — workspace CRUD, membership, configuration, and transfers
- [Person](/one/person/) — user lifecycle and password management
- [API tokens](/one/token/) — token issuance and revocation
- [Roles](/one/role/) — role assignment and unassignment
- [Diagnostics](/one/diagnostics/) — the broader `ayx one doctor` / `ayx one inventory` picture
- [Safety model](/safety-model/) — how dry-run and `--apply` work
