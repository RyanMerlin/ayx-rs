---
title: Diagnostics
description: Check Alteryx One health with ayx one doctor and ayx one inventory.
sidebar:
  order: 5
---

Two commands give you a quick read on the health of your Alteryx One environment: `ayx one doctor` and `ayx one inventory`. Run these first when something seems off, or wire them into a monitoring script.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one doctor auth` | Verify authentication credentials are valid |
| `ayx one doctor discover` | Probe API discoverability |
| `ayx one doctor identity` | Check identity surface health |
| `ayx one doctor plans` | Check plans surface health |
| `ayx one doctor scheduling` | Check scheduling surface health |
| `ayx one inventory` | Asset inventory across the workspace |

## Running a full health check

Run all doctor subcommands in sequence to get a broad picture:

```bash
ayx one doctor auth
ayx one doctor discover
ayx one doctor identity
ayx one doctor plans
ayx one doctor scheduling
```

Each check is independent. A failure in one does not block the others.

## Authentication check

Start here when commands return 401 or credential errors:

```bash
ayx one doctor auth
```

The result reports the selected workspace credential method without printing
token values. OAuth credentials are normally renewed automatically; email-OTP
credentials require an interactive login when their stored token expires. If
an OAuth refresh token is expired or revoked, re-import a newly issued pair
with `ayx one login --auth-method oauth-refresh` using the env/stdin input
options described in [Identity & auth](/one/identity/).

Add `--profile <name>` to test a specific profile:

```bash
ayx one doctor auth --profile staging
```

## Discovery check

`discover` probes the API to confirm the Alteryx One endpoint is reachable and returning expected metadata:

```bash
ayx one doctor discover
```

## Identity, plans, and scheduling checks

These targeted checks verify that specific API surfaces are healthy:

```bash
ayx one doctor identity
ayx one doctor plans
ayx one doctor scheduling
```

## Inventory

`inventory` is a standalone command (not under `doctor`) that provides a broader view:

```bash
# Asset inventory across the workspace
ayx one inventory
```

Accepts `--profile <name>`.

The old top-level `status` command under `one` — and its nested equivalent from the former platform group — have both been removed entirely, with no direct successor. Looking for Alteryx Server health instead? That's `ayx server api status` — a separate command outside the `one` surface.

## JSON output

All diagnostic commands support `--output json`:

```bash
ayx --output json one doctor auth
ayx --output json one inventory
```

The envelope is `{ ok, message, timestamp_utc, data }` on success; failures also include `error_code`. Pipe to `jq` for scripting:

```bash
ayx --output json one doctor auth | jq '.ok'
```

## Related

- [API introspection](/one/diagnostics/api/) — spec fetch and coverage diff, separate from the health checks on this page
- [Connecting](/connecting/) — how to configure profiles and credentials
- [Alteryx One overview](/one/) — all `ayx one` areas
- [Troubleshooting](/troubleshooting/) — common errors and fixes
