---
title: Diagnostics
description: Check Alteryx One health with ayx one doctor, ayx one status, and ayx one inventory.
sidebar:
  order: 5
---

Three commands give you a quick read on the health of your Alteryx One environment: `ayx one doctor`, `ayx one status`, and `ayx one inventory`. Run these first when something seems off, or wire them into a monitoring script.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one doctor auth` | Verify authentication credentials are valid |
| `ayx one doctor discover` | Probe API discoverability |
| `ayx one doctor platform` | Check platform surface health |
| `ayx one doctor plans` | Check plans surface health |
| `ayx one doctor scheduling` | Check scheduling surface health |
| `ayx one doctor billing` | Check billing surface health |
| `ayx one status` | Top-level platform status |
| `ayx one inventory` | Asset inventory across the workspace |

## Running a full health check

Run all doctor subcommands in sequence to get a broad picture:

```bash
ayx one doctor auth
ayx one doctor discover
ayx one doctor platform
ayx one doctor plans
ayx one doctor scheduling
ayx one doctor billing
```

Each check is independent. A failure in one does not block the others.

## Authentication check

Start here when commands return 401 or credential errors:

```bash
ayx one doctor auth
```

Add `--profile <name>` to test a specific profile:

```bash
ayx one doctor auth --profile staging
```

## Discovery check

`discover` probes the API to confirm the Alteryx One endpoint is reachable and returning expected metadata:

```bash
ayx one doctor discover
```

## Platform, plans, scheduling, and billing checks

These targeted checks verify that specific API surfaces are healthy:

```bash
ayx one doctor platform
ayx one doctor plans
ayx one doctor scheduling
ayx one doctor billing
```

Run `ayx one doctor billing` before investigating billing data issues — it confirms the billing API is reachable before you try an export.

## Status and inventory

`status` and `inventory` are standalone commands (not under `doctor`) that provide a broader view:

```bash
# Top-level platform status
ayx one status

# Asset inventory across the workspace
ayx one inventory
```

Both accept `--profile <name>`.

## JSON output

All diagnostic commands support `--output json`:

```bash
ayx --output json one doctor auth
ayx --output json one status
ayx --output json one inventory
```

The envelope is `{ ok, message, timestamp_utc, data }` on success; failures also include `error_code`. Pipe to `jq` for scripting:

```bash
ayx --output json one doctor auth | jq '.ok'
```

## Related

- [Billing](/one/billing/) — account and usage data
- [Connecting](/connecting/) — how to configure profiles and credentials
- [Alteryx One overview](/one/) — all `ayx one` areas
- [Troubleshooting](/troubleshooting/) — common errors and fixes
