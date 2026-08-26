---
title: Connecting to Alteryx One
description: Sign in to Alteryx One with a one-time passcode, and optionally connect Alteryx Server.
sidebar:
  order: 2
---

`ayx` talks to Alteryx One over its `/v4` REST API. First-time sign-in is an **email one-time passcode (OTP)** flow: you enter a 6-digit code emailed to you plus your workspace password, and `ayx` stores a **30-day token** in your profile and reuses it for every command after. The Wizard email-OTP flow and secure persistence are the defaults. There's no OAuth client to create and no token to paste by hand.

## The quick path

```bash
ayx onboard
```

The wizard collects your email and workspace URL and offers to log you in on the spot — see [Getting started](/getting-started/). Everything below is what it's doing under the hood, and how to sign in again when the token expires.

## Signing in

```bash
ayx one login
```

With no flags this runs the **email-OTP flow**:

1. A 6-digit passcode is emailed to your account address.
2. `ayx` prompts you for the passcode, then for your **workspace password**.
3. On success it stores a 30-day token in the active profile.
4. On the first interactive login, it asks whether to save the workspace password securely for future logins. Press Enter for the default **Yes**, or answer `n` to decline.

The successful login prints an `Authentication Successful!` confirmation only after the credentials and profile state have been persisted. It also reports token expiry and, when available, the authenticated workspace id and name.

Profile selection is `--profile <name>`, then `AYX_PROFILE`, then the active profile pointer, then the central `default` profile.

It reads three fields from your profile — your email (from the onboarding prompt) and your workspace id + region (parsed from the workspace URL you paste during onboarding):

| Field | Where it comes from |
|-------|---------------------|
| `account_email` | The address you sign in to Alteryx One with |
| `workspace_gid` | The workspace id (a ULID) in your workspace URL — required by the sign-in handshake |
| `base_url` | Your Alteryx One region host, e.g. `https://us1.alteryxcloud.com` (also read from the URL) |

If the token later expires, just run `ayx one login` again.

### Credential persistence

Secure operating-system storage is the default. The first interactive workspace-password login offers to save the password in the OS keyring; Enter accepts the save, while `n` keeps the password session-only. `--save-workspace-password` remains an optional automation shorthand for the default email-OTP flow.

If secure storage is unavailable, `--secret-policy plaintext` is an explicit fallback and requires affirmative consent. The standalone login command rejects `--secret-policy session` because its process exits immediately and cannot retain a usable session.

### Other sign-in flows

You usually won't need these, but they're there:

- `ayx one login --device` — device-code grant: prints a URL and code to complete sign-in on any device.
- `ayx one login --browser` — PKCE authorization-code flow in your browser.
- `ayx one login --refresh-token <t>` / `--access-token <t>` — store tokens you already have, for CI.

The `--browser` and `--device` flows use an OAuth client, so they need an `oauth_client_id` in your profile (or `--client-id`). The default email-OTP flow does not.

## Confirm it worked

```bash
ayx doctor auth     # checks the token path end to end
ayx whoami          # shows the workspace you're connected to
```

If `doctor auth` passes but a command later fails with an auth error, your token has expired — run `ayx one login` again.

## Multiple workspaces

One profile can hold a separate token per workspace. Bind a login to a specific workspace, then switch which one is active:

```bash
ayx one login --workspace-id <id>   # store this workspace's token
ayx one workspace switch <id>       # make it the active one
```

See [Profiles & configuration](/configuration/) for the full model.

## Auth-transport safety

The email-OTP first-login flow is pure-HTTP (reqwest). There is no browser, Python, or Playwright dependency.

During the OIDC flow, `ayx` applies two transport-level guards:

- **Redirect-host allowlist.** The redirect follower only accepts the configured Alteryx domain and its subdomains. An off-domain redirect (e.g. to an unrelated host) is rejected with an error before any credential is sent.
- **Interaction-id validation.** The OIDC interaction id is validated for shape (6–128 characters, restricted charset) before use. A malformed value from the server is rejected rather than forwarded.

Response bodies are redacted in auth-flow error output so credential material does not appear in logs or terminal output.

When you sign in on a machine where no OS keyring backend is available, `ayx` asks for explicit consent before storing credentials inline in the config file (plaintext at rest). Configuring a keyring backend — the system keychain on macOS, `libsecret` on Linux, or Windows Credential Manager — keeps credentials out of the profile and suppresses the warning.

## Connecting to Alteryx Server (optional)

If you also administer Alteryx Server, add a `server:` block to your profile with the Server API host and credentials:

```yaml
server:
  api:
    base_url: https://your-server.example.com
    client_id: <id>
    client_secret: <secret>
```

The `ayx server` commands — status, diagnostics, upgrade planning, and more — then run against it. `ayx onboard` can set this up interactively too.
