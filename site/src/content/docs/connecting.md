---
title: Connecting to Alteryx One
description: Point ayx at your Alteryx One workspace, and optionally Alteryx Server.
sidebar:
  order: 2
---

ayx talks to Alteryx One over its `/v4` REST API using an OAuth bearer token. The fastest way to set this up is the onboarding wizard — this page explains what it's asking for and how to confirm it worked.

## The quick path

```bash
ayx onboard
```

The wizard prompts for your workspace and credentials, validates them as you go, and writes a profile. For unattended setup (CI), pass `--non-interactive` with the values supplied through the environment.

When you log in (`auth login`) on a machine where no OS keyring backend is available, ayx warns that credentials will be stored inline in the config file (plaintext at rest). Configuring a keyring backend — such as the system keychain on macOS, `libsecret` on Linux, or Windows Credential Manager — eliminates plaintext storage and suppresses the warning.

## What you'll need

| Field | Where it comes from |
|-------|---------------------|
| `base_url` | Your Alteryx One region host, e.g. `https://us1.alteryxcloud.com` |
| `account_email` | The account the credentials belong to |
| `oauth_client_id` | An OAuth client created in Alteryx One |
| `token_endpoint_url` | Usually `<base_url>/oauth/token` |
| `access_token` | A bearer token — or let ayx mint one from the client credentials |

These map directly to the `alteryx_one:` block of your [profile](/configuration/). Any of them can also come from an environment variable, which is the usual approach for pipelines.

## Confirm it worked

```bash
ayx doctor auth     # checks the token path end to end
ayx whoami          # shows the workspace you're connected to
```

If `doctor auth` passes but a command later fails, it's almost always an expired token — refresh it and retry.

## Connecting to Alteryx Server (optional)

If you also administer Alteryx Server, add a `server:` block to your profile with the Server API host and credentials:

```yaml
server:
  api:
    base_url: https://your-server.example.com
    client_id: <id>
    client_secret: <secret>
```

The `ayx server` commands — status, diagnostics, upgrade planning, and more — then run against it.
