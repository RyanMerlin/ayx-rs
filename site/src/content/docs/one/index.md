---
title: Alteryx One overview
description: What ayx one covers — workflows, connections, datasets, jobs, scheduling, platform administration, and the legacy Designer Cloud flows surface.
sidebar:
  order: 0
---

`ayx one` is the primary command surface for Alteryx One. It gives you programmatic access to every major area of the platform: browsing and sharing cloud-native canvas workflows, managing data connections and datasets, running and inspecting jobs, orchestrating schedules and plans, and administering users and workspaces.

## Three workflow surfaces, deliberately kept separate

This CLI reaches three different workflow-like surfaces. They are built on different underlying technologies, they do not share resources, and they are not interchangeable:

| Surface | Command prefix | What it is |
|---|---|---|
| Cloud-native workflows | `ayx one workflows` | Alteryx One's canvas surface, keyed by ULIDs. The execution unit in Alteryx One. See [Workflows](/one/workflows/). |
| On-prem Designer/Server workflows | `ayx designer workflow` | Local `.yxmd` / `.yxmc` / `.yxzp` packages for Alteryx Designer and Server. The execution unit on-prem, and where the large majority of existing customer workflows still live. See [Workflows & packages](/server/workflow/). |
| Designer Cloud flows | `ayx one flows` | The older Designer Cloud (DC) surface at `/v4/flows`, keyed by integer ids. See [Flows (DC Legacy)](/one/flows/). |

Because these are separate technologies rather than separate views of the same data, moving work between them is a **migration**, not a configuration change. A workspace can hold dozens of cloud-native workflows while `ayx one flows list` returns zero items. Commands that cross the boundary say so explicitly — see `ayx designer workflow migrate` and the cloud-conversion notes in [Workflows & packages](/server/workflow/).

Keep the distinction when reading the rest of these docs: a page under **Alteryx One** never describes on-prem Designer/Server behavior, and a page under **Alteryx Server** never describes cloud behavior.

All mutating commands are dry-run by default. Nothing changes on the server until you add `--apply`. Add `--yes` to skip the confirmation prompt in scripts. See the [Safety model](/safety-model/) for the full rules.

## Major areas

| Area | Command prefix | What you do |
|---|---|---|
| Workflows | `ayx one workflows` | List, inspect, copy, share, and delete cloud-native canvas workflows |
| Connections | `ayx one connections` | Manage data connections and connector metadata |
| Datasets | `ayx one datasets` | Browse the One dataset library and inspect dataset details |
| Job groups | `ayx one job-groups` | Run, cancel, inspect, and retrieve results for job groups |
| Output objects | `ayx one output-objects` | CRUD for output objects; inspect inputs; convert to Python |
| Scheduling | `ayx one scheduling` | List schedules, enable and disable them |
| Plans | `ayx one plans` | Orchestrate multi-flow plans, manage schedules, share, import/export |
| Identity & users | `ayx one login` / `logout` / `whoami` / `auth` / `workspace` / `person` / `token` / `role` | Sign in/out, inspect identity, and administer workspaces, users, tokens, and roles |
| Write settings | `ayx one write-settings` | Configure where flows write their output data |
| Webhooks | `ayx one webhook-flow-tasks` | Create, inspect, delete, and test webhook-triggered flow tasks |
| Diagnostics | `ayx one doctor` / `ayx one inventory` / `ayx one api` | Health checks across auth, identity, plans, and scheduling; plus OpenAPI-spec and coverage introspection |
| Flows (DC Legacy) | `ayx one flows` | The older Designer Cloud `/v4/flows` surface: list, create, run, validate, import/export, manage permissions |

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
- [Diagnostics](/one/diagnostics/) — health checks and status
- [Identity & auth](/one/identity/) — sign in/out, whoami, auth status, workspaces, users, roles, tokens
- [Safety model](/safety-model/) — dry-run and `--apply` in detail
