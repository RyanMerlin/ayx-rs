---
title: Connecting to Alteryx One
description: Sign in to Alteryx One with OAuth API credentials or a one-time passcode, and optionally connect Alteryx Server.
sidebar:
  order: 2
---

`ayx` talks to Alteryx One over its `/v4` REST API. There are two supported user-credential methods:

- **OAuth API access/refresh credentials** are the preferred method for automation, agents, CI, and users who want a long-lived machine login. Import the pair once; `ayx` stores it in the operating-system keyring, refreshes short-lived access tokens automatically, and does not fall back to OTP.
- **Email one-time passcode (OTP)** is the default interactive method. It asks for a 6-digit code and workspace password, then stores the resulting workspace credential securely.

These are different credential types. An OAuth refresh token is not an OTP, and an API token managed by `ayx one token` is not automatically the same thing as the OAuth access/refresh pair used by `ayx one login --auth-method oauth-refresh`.

## The quick path

```bash
ayx onboard
```

The wizard collects your email and workspace URL and offers to log you in on the spot — see [Getting started](/getting-started/). It uses email OTP for the interactive path. For automation, configure an OAuth API access/refresh pair as described below.

### A beginner's checklist

1. Open PowerShell on Windows, or Terminal on macOS/Linux.
2. Run `ayx onboard`.
3. Paste the workspace URL from your browser when asked.
4. For a normal human login, answer **y**, enter the emailed 6-digit code,
   and enter your workspace password. Press Enter when asked to save it.
5. Run `ayx one workspace current` to confirm the connection.

For a computer, CI job, or agent, use the OAuth checklist below. It uses a
refresh token instead of asking a person for a new email code every time.

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

For an OAuth credential, access-token renewal is automatic. If the provider has
expired or revoked the refresh token, import a newly issued pair using the
OAuth instructions below; the CLI will not silently send an OTP instead.

### OAuth API access/refresh credentials

Use this method when the CLI must run unattended or when you want to avoid
daily interactive authentication. Create or obtain an OAuth2.0 API-token pair
from the Alteryx One administration experience, including its client ID and
token endpoint. Configure the client ID and endpoint in the selected profile,
then import the refresh token through an environment variable or stdin:

```bash
# The environment variable is read for this import only; secure persistence
# stores the resulting credential in the OS keyring.
ayx one login --profile local-dev --workspace-id <workspace-id> \
  --auth-method oauth-refresh \
  --refresh-token-env AYX_ONE_API_REFRESH_TOKEN \
  --secret-policy secure

# Or, on macOS/Linux:
printf '%s' "$AYX_ONE_API_REFRESH_TOKEN" |
  ayx one login --profile local-dev --workspace-id <workspace-id> \
    --auth-method oauth-refresh --refresh-token-stdin --secret-policy secure

# PowerShell:
$env:AYX_ONE_API_REFRESH_TOKEN |
  ayx one login --profile local-dev --workspace-id <workspace-id> \
    --auth-method oauth-refresh --refresh-token-stdin --secret-policy secure
```

The client ID can be configured as `alteryx_one.oauth_client_id` or supplied
through `AYX_ONE_OAUTH_CLIENT_ID`; the token endpoint can be configured as
`alteryx_one.token_endpoint_url` or `AYX_ONE_TOKEN_ENDPOINT_URL`. The refresh
token is bound to the selected workspace and profile. After import, ordinary
commands use the keyring-backed pair; no OTP prompt is expected. Automatic
refresh uses a short safety window and persists any provider-issued replacement
refresh token while preventing replay of an applied mutation after an uncertain
response.

Provider token exchange and local keyring storage are separate systems. A
process or keyring failure immediately after a provider accepts a rotating
refresh token can leave the local state in doubt; `ayx` will not blindly retry
that exchange. Re-import a fresh access/refresh pair if the command reports
that persistence failed.

Do not put token values in command arguments, checked-in YAML, shared logs, or
documentation. `--refresh-token <value>` remains a compatibility option, but
the env/stdin forms are the release-safe choices.

### Credential persistence

Secure operating-system storage is the default. The first interactive workspace-password login offers to save the password in the OS keyring; Enter accepts the save, while `n` keeps the password session-only. `--save-workspace-password` remains an optional automation shorthand for the default email-OTP flow.

If secure storage is unavailable, `--secret-policy plaintext` is an explicit fallback and requires affirmative consent. The standalone login command rejects `--secret-policy session` because its process exits immediately and cannot retain a usable session. OAuth refresh rotation is automatic only when the refresh credential is stored in a supported secure keyring; environment-backed or inline credentials are not rewritten in place.

### Other sign-in flows

You usually won't need these, but they're there:

- `ayx one login --device` — device-code grant: prints a URL and code to complete sign-in on any device.
- `ayx one login --browser` — PKCE authorization-code flow in your browser.
- `ayx one login --refresh-token-env NAME` / `--refresh-token-stdin` — import an OAuth refresh token without exposing its value in process arguments.
- `ayx one login --access-token-env NAME` / `--access-token-stdin` — import an access token without exposing its value; this is a non-rotating compatibility path and cannot select `oauth-refresh` by itself.
- `ayx one login --refresh-token <t>` / `--access-token <t>` — compatibility imports; avoid these forms in shared terminals and automation logs.

The `--browser` and `--device` flows use an OAuth client, so they need an `oauth_client_id` in your profile (or `--client-id`). The default email-OTP flow does not.

## Confirm it worked

```bash
ayx doctor auth     # checks the token path end to end
ayx whoami          # shows the workspace you're connected to
```

If `doctor auth` passes but a command later fails with an auth error, check `ayx one auth status` and `ayx one auth diagnose`. An OTP credential may need a new `ayx one login`; an OAuth credential usually needs no action unless its refresh token has expired or been revoked, in which case import a newly issued pair with `--auth-method oauth-refresh`.

## Multiple workspaces

One profile can hold a separate token per workspace. Bind a login to a specific workspace, then switch which one is active:

```bash
ayx one login --workspace-id <id>   # store this workspace's token
ayx one workspace switch <id>       # make it the active one
```

Each workspace keeps its own credential method and token pair. Switching
workspaces does not copy credentials or change another workspace's OTP/OAuth
policy.

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
