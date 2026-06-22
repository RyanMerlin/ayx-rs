---
title: Configuration
sidebar_position: 3
---

# Configuration

## Config home

The central config home stores profiles, environment files, and sensitive runtime artifacts.

| Platform | Default path |
|----------|-------------|
| Linux / macOS | `~/.config/ayx` |
| Windows | `%AppData%\\ayx` |

## Profiles

A profile is a named YAML file inside the config home. Use `ayx profile list` to inspect stored profiles and `ayx profile use <name>` to switch the active default.

`--profile <name>` selects a central profile for a single run without changing the default.

Use `ayx profile migrate --profile <path>` to import a legacy YAML file into the central store.

## Environment files

`environments.yaml` is the canonical multi-environment file shape. It should contain `workspace_name`, `active_environment`, and an `environments` map of named `Config` entries.

Use `--environment <name>` to override the active environment for a single run.

## Minimum profile fields

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

## Environment variable overrides

| Variable | Purpose |
|----------|---------|
| `AYX_ONE_CLIENT_ID` | OAuth client ID |
| `AYX_ONE_CLIENT_SECRET` | OAuth client secret |
| `AYX_ONE_TOKEN_ENDPOINT_URL` | Token endpoint |
| `AYX_ONE_API_ACCESS_TOKEN` | Access token |
| `AYX_ONE_API_REFRESH_TOKEN` | Refresh token |

See the [runtime config contract](./reference/runtime-config-contract) for the detailed resolution order.
