---
title: Configuration
description: Profiles, environment files, and the order ayx resolves configuration from.
sidebar:
  order: 2
---

## Config home

ayx stores profiles, environment files, and runtime artifacts in a central config home:

| Platform | Default path |
|----------|--------------|
| Linux / macOS | `~/.config/ayx` |
| Windows | `%AppData%\ayx` |

Override the location with the `AYX_CONFIG_HOME` environment variable.

## Profiles

A profile is a named YAML file in the config home. Inspect and switch profiles:

```bash
ayx profile list
ayx profile current
ayx profile use <name>
```

`--profile <name>` selects a profile for a single run without changing the default. Import a legacy YAML file into the central store with:

```bash
ayx profile migrate --profile <path>
```

## Minimum profile

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

## Environment files

`environments.yaml` is the multi-environment file shape. It carries `workspace_name`, `active_environment`, and an `environments` map of named config entries. Use `--environment <name>` to override the active environment for a single run.

## Environment-variable overrides

| Variable | Purpose |
|----------|---------|
| `AYX_ONE_OAUTH_CLIENT_ID` | OAuth client ID (alias: `AYX_ONE_CLIENT_ID`) |
| `AYX_ONE_CLIENT_SECRET` | OAuth client secret |
| `AYX_ONE_TOKEN_ENDPOINT_URL` | Token endpoint |
| `AYX_ONE_API_ACCESS_TOKEN` | Access token |
| `AYX_ONE_API_REFRESH_TOKEN` | Refresh token |

See the [runtime config contract](/reference/runtime-config-contract/) for the full resolution order.
