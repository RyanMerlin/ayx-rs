---
title: Identity & auth
description: Sign in, sign out, check who you are, and verify Alteryx One auth posture — ayx one login, logout, whoami, auth status/diagnose, doctor identity, and inventory.
sidebar:
  order: 1
---

`ayx one` handles authentication and identity through a small set of commands: `login` and `logout` manage credentials, `whoami` shows who you're signed in as, `auth status` / `auth diagnose` check token posture, `doctor identity` runs a deeper identity health check, and `inventory` summarizes the current One API surface registry.

All mutating commands (anything that creates, updates, suspends, removes, or deletes) are dry-run by default. Add `--apply` to commit the change. Add `--yes` to skip the TTY confirmation in scripts.

## What's here

| Area | Command | What you do |
|---|---|---|
| Sign in | `ayx one login` | Authenticate and store credentials (email OTP by default; `--device` / `--browser` / token flags also available) |
| Sign out | `ayx one logout` | Clear stored credentials from the active profile |
| Identity | `ayx one whoami` | Show the current One user profile |
| Auth status | `ayx one auth status` | Summarize One API token posture for managed IAM |
| Auth diagnose | `ayx one auth diagnose` | Validate One API token reachability and workspace scope |
| Identity doctor | `ayx one doctor identity` | Run the One identity doctor workflow |
| Inventory | `ayx one inventory` | Summarize the current One API surface registry |

Workspace, user, token, and role administration all build on the identity established here — see [Workspace](/one/workspace/), [Person](/one/person/), [API tokens](/one/token/), and [Roles](/one/role/).

## Signing in

```bash
# Default: email OTP flow
ayx one login

# Device-code grant
ayx one login --device

# Browser PKCE flow
ayx one login --browser

# Store a token you already have (CI)
ayx one login --refresh-token <t>
ayx one login --access-token <t>

# Bind credentials to a specific workspace
ayx one login --workspace-id <id> --workspace-gid <gid>
```

`--client-id` overrides the profile's `oauth_client_id` for the `--browser` / `--device` flows. See [Connecting](/connecting/) for the full sign-in walkthrough.

## Signing out

```bash
ayx one logout
```

Clears stored Alteryx One credentials from the active profile. Add `--apply` to commit and `--yes` to skip the TTY confirmation.

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
