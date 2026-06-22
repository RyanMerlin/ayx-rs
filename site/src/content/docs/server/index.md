---
title: Alteryx Server overview
description: What the ayx server commands do — discovery, logs, auth, diagnostics, and upgrade — for admins running Alteryx Server.
sidebar:
  order: 0
---

The `ayx server` branch covers everything an admin needs to inspect, diagnose, and upgrade an Alteryx Server installation. All subcommands are read-only unless noted. See the [Connecting](/connecting/) guide to configure a profile before running anything here.

## Quick reference

| Command | What it does |
|---------|-------------|
| `ayx server system-info` | Dump system info to `system_info.json` |
| `ayx server runtime-settings` | Read `RuntimeSettings.xml` (path configurable) |
| `ayx server ayx-paths` | Show resolved Alteryx file paths |
| `ayx server api status` | Check Gallery API reachability |
| `ayx server api diagnose` | Diagnose API connectivity issues |
| `ayx server api call` | Make a raw authenticated API call |
| `ayx server api import-swagger` | Import a Swagger spec for the server API |
| `ayx server auth status` | Show current auth posture |
| `ayx server auth diagnose` | Diagnose auth configuration |
| `ayx server auth simulate` | Simulate an auth flow without committing |
| `ayx server diagnose startup` | Diagnose startup issues |
| `ayx server diagnose logs` | Diagnose log configuration |
| `ayx server diagnose network` | Diagnose network connectivity |
| `ayx server diagnose tls` | Diagnose TLS issues |
| `ayx server diagnose runtime-settings` | Diagnose runtime settings |
| `ayx server doctor startup` | Run doctor checks on startup |
| `ayx server doctor logs` | Run doctor checks on logs |
| `ayx server doctor network` | Run doctor checks on network |
| `ayx server doctor runtime-settings` | Run doctor checks on runtime settings |
| `ayx server server-logs ...` | Log discovery, tailing, and parsing — see [Logs & diagnostics](/server/logs/) |
| `ayx server upgrade ...` | Upgrade planning and execution — see [Upgrade](/server/upgrade/) |
| `ayx server backup` | Standalone backup (separate from upgrade flow) |
| `ayx server backup-plan` | Generate a backup plan for a given backup directory |

## Gathering system info

Capture a system snapshot before opening a support case or starting an upgrade.

```sh
ayx server system-info
```

This writes `system_info.json` to the current directory by default. Supply `--output-file <path>` to redirect.

## Checking runtime settings

Read the active `RuntimeSettings.xml`:

```sh
ayx server runtime-settings
```

The default path is `C:\ProgramData\Alteryx\RuntimeSettings.xml`. Override with `--path <path>`.

## API health check

Verify that the Gallery API is reachable and your credentials work:

```sh
ayx server api status --profile prod
```

For a detailed connectivity diagnosis:

```sh
ayx server api diagnose --profile prod
```

## Auth posture snapshot

Capture auth configuration before an upgrade or for a change record:

```sh
ayx --output json server auth status --profile prod
```

## Doctor vs. diagnose

`ayx server doctor` and `ayx server diagnose` cover overlapping ground but serve different purposes:

- **diagnose** — investigates a specific symptom (startup, logs, network, TLS, runtime-settings). Use when you have a known issue to dig into.
- **doctor** — runs a structured health sweep across the same domains. Use for routine checks or before a change window.

## Standalone backup

Take a backup outside of the upgrade flow:

```sh
# Dry run — see what would be captured
ayx server backup --profile prod --backup-dir backups/pre-change

# Commit
ayx server backup --profile prod --backup-dir backups/pre-change --apply
```

## Related

- [Logs & diagnostics](/server/logs/)
- [Diagnose & auth](/server/diagnose/)
- [Upgrade](/server/upgrade/)
- [MongoDB](/server/mongo/)
- [SQL Server](/server/sqlserver/)
- [Connecting](/connecting/)
