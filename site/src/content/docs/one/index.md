---
title: Alteryx One overview
description: What ayx one covers — flows, plans, connections, job groups, scheduling, platform administration, and more.
sidebar:
  order: 0
---

`ayx one` is the primary command surface for Alteryx One. It gives you programmatic access to every major area of the platform: running and managing flows, orchestrating plans, managing data connections, inspecting job groups, administering users and workspaces, and more.

All mutating commands are dry-run by default. Nothing changes on the server until you add `--apply`. Add `--yes` to skip the confirmation prompt in scripts. See the [Safety model](/safety-model/) for the full rules.

## Major areas

| Area | Command prefix | What you do |
|---|---|---|
| Flows | `ayx one flows` | List, create, run, validate, import/export, manage permissions |
| Plans | `ayx one plans` | Orchestrate multi-flow plans, manage schedules, share, import/export |
| Connections | `ayx one connections` | Manage data connections and connector metadata |
| Job groups | `ayx one job-groups` | Run, cancel, inspect, and retrieve results for job groups |
| Output objects | `ayx one output-objects` | CRUD for output objects; inspect inputs; convert to Python |
| Write settings | `ayx one write-settings` | Configure where flows write their output data |
| Webhooks | `ayx one webhook-flow-tasks` | Create, inspect, delete, and test webhook-triggered flow tasks |
| Scheduling | `ayx one scheduling` | List schedules, enable and disable them |
| Identity & users | `ayx one login` / `logout` / `whoami` / `auth` / `workspace` / `person` / `token` / `role` | Sign in/out, inspect identity, and administer workspaces, users, tokens, and roles |
| Billing | `ayx one billing` | Account information and usage exports |
| Diagnostics | `ayx one doctor` / `ayx one inventory` | Health checks across auth, identity, plans, scheduling, and billing |

## How `--apply` keeps you safe

Every command that modifies remote state prints a structured dry-run by default — it shows you exactly what request would be sent, then exits. Add `--apply` to commit.

```bash
# Shows what would be deleted; changes nothing
ayx one flows delete <id>

# Actually deletes
ayx one flows delete <id> --apply
```

For non-interactive automation (CI pipelines, scripts), add `--yes` to suppress the TTY confirmation prompt.

## JSON output

Pass `--output json` for machine-readable output on stdout. `--output` is a global flag, so it can appear before or after the subcommand:

```bash
ayx --output json one flows list
```

The envelope is `{ ok, message, timestamp_utc, data }` on success; failures also include `error_code`. Combine with `--verbose` to get human-readable progress on stderr without polluting stdout.

## Targeting a specific environment

Most commands accept `--profile <name>` to target a non-default workspace profile (each command's page notes any exceptions).

```bash
ayx one flows list --profile staging
```

## Related

- [Connecting](/connecting/) — set up profiles and credentials
- [Output objects](/one/output-objects/) — CRUD and Python conversion for output objects
- [Write settings](/one/write-settings/) — configure flow output destinations
- [Webhooks](/one/webhooks/) — webhook-triggered flow tasks
- [Billing](/one/billing/) — account and usage data
- [Diagnostics](/one/diagnostics/) — health checks and status
- [Identity & auth](/one/identity/) — sign in/out, whoami, auth status, workspaces, users, roles, tokens
- [Safety model](/safety-model/) — dry-run and `--apply` in detail
