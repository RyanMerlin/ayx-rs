---
title: Profiles & configuration
description: How ayx stores connections, switches environments, and resolves settings.
sidebar:
  order: 3
---

A **profile** is your saved connection to an Alteryx One workspace (and, optionally, Alteryx Server). ayx keeps profiles in a central config home, so you can switch environments without editing files by hand.

## Where config lives

| Platform | Config home |
|----------|-------------|
| macOS / Linux | `~/.config/ayx` |
| Windows | `%AppData%\ayx` |

Override it with the `AYX_CONFIG_HOME` environment variable.

## Working with profiles

```bash
ayx profile list        # every stored profile
ayx profile current     # the active one
ayx profile use <name>  # change the default
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

```yaml
profile_name: my-profile
alteryx_one:
  base_url: https://us1.alteryxcloud.com
  account_email: admin@example.com
  oauth_client_id: <client-id>
  token_endpoint_url: https://us1.alteryxcloud.com/oauth/token
  access_token: <token>
server:
  api:
    base_url: https://your-server.example.com
    client_id: <id>
    client_secret: <secret>
```

[Connecting to Alteryx One](/connecting/) covers where each value comes from.

## Multiple environments

`environments.yaml` holds several named environments in one file — `workspace_name`, `active_environment`, and an `environments` map. Switch for a single run:

```bash
ayx --environment prod one flows list
```

Like `--output`, `--environment` is a top-level flag, so it goes before the command.

## Settings from the environment

Every One credential can come from an environment variable instead of the profile — handy for CI:

| Variable | Sets |
|----------|------|
| `AYX_ONE_OAUTH_CLIENT_ID` | OAuth client ID (alias: `AYX_ONE_CLIENT_ID`) |
| `AYX_ONE_CLIENT_SECRET` | OAuth client secret |
| `AYX_ONE_TOKEN_ENDPOINT_URL` | Token endpoint |
| `AYX_ONE_API_ACCESS_TOKEN` | Access token |
| `AYX_ONE_API_REFRESH_TOKEN` | Refresh token |

The full resolution order — flags, then environment, then profile, then defaults — is in the [runtime config contract](/reference/runtime-config-contract/).
