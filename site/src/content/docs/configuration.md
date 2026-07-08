---
title: Profiles & configuration
description: How ayx stores connections, switches environments, and resolves settings.
sidebar:
  order: 3
---

A **profile** is your saved connection to an Alteryx One workspace (and, optionally, Alteryx Server). `ayx` keeps profiles in a central config home, so you can switch environments without editing files by hand.

## Where config lives

| Platform | Config home |
|----------|-------------|
| macOS / Linux | `~/.config/ayx` |
| Windows | `%AppData%\ayx` |

Override it with the `AYX_CONFIG_HOME` environment variable.

## Working with profiles

`ayx onboard` creates a profile, saves it under the name you give it, and makes it active. Run it again with a different name to keep several — one per workspace or environment:

```bash
ayx profile list        # every stored profile
ayx profile current     # the active one
ayx profile use <name>  # change the active profile
```

Run a single command against a different profile without changing your default:

```bash
ayx whoami --profile staging
```

Import a legacy YAML file into the central store and give it a name:

```bash
ayx profile migrate --profile /path/to/old.yaml --name my-profile
```

## What a profile looks like

After onboarding and signing in, a One profile is small — the connection details plus a reference to the stored token:

```yaml
profile_name: my-profile
alteryx_one:
  account_email: admin@example.com
  base_url: https://us1.alteryxcloud.com
  workspace_gid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
  access_token_ref: keyring:my-profile/alteryx_one.access_token
```

You don't write the token in by hand — `ayx one platform auth login` obtains it and stores it for you (in your OS keyring where available; see [Connecting](/connecting/)). `base_url` and `workspace_gid` come from the workspace URL you paste during onboarding.

### Secret references

Any secret can be a **reference** instead of a literal, so nothing sensitive sits in plaintext:

- `keyring:<account>` — resolved from the OS keyring (the default once a keyring backend is available).
- `env:VARNAME` — read from an environment variable at run time, the usual choice for CI.

```yaml
alteryx_one:
  account_email: admin@example.com
  base_url: https://us1.alteryxcloud.com
  access_token_ref: env:AYX_ONE_API_ACCESS_TOKEN
```

## Multiple workspaces in one profile

A single profile can carry a separate token per workspace under `workspace_credentials`, keyed by workspace id. Bind a login to a workspace, then switch which one is active:

```bash
ayx one platform auth login --workspace-id <id>          # store that workspace's token
ayx one platform workspace switch --workspace-id <id>    # make it active
```

The active workspace's token is used for every One command until you switch again. `expected_workspace_id` guards mutating commands against running on the wrong workspace.

## Multiple environments

`environments.yaml` holds several named environments in one file — `workspace_name`, `active_environment`, and an `environments` map. Switch for a single run:

```bash
ayx --environment prod one flows list
```

Like `--output`, `--environment` is a top-level flag, so it goes before the command.

## Settings from the environment

Credentials can come from environment variables instead of the profile — handy for CI:

| Variable | Sets |
|----------|------|
| `AYX_ONE_API_ACCESS_TOKEN` | Access token |
| `AYX_ONE_API_REFRESH_TOKEN` | Refresh token |
| `AYX_ONE_OAUTH_CLIENT_ID` | OAuth client ID for the `--browser`/`--device` flows (alias: `AYX_ONE_CLIENT_ID`) |
| `AYX_ONE_CLIENT_SECRET` | OAuth client secret (advanced flows) |

The full resolution order — flags, then environment, then profile, then defaults — is in the [runtime config contract](/reference/runtime-config-contract/).
