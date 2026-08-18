---
title: Scheduling
description: Create, inspect, update, enable, disable, and delete Alteryx One schedules from the CLI.
sidebar:
  order: 1
---

Schedules define when workflows, flows, plans, or Auto Insights tasks run automatically in Alteryx One. You can manage their full lifecycle from the CLI. Mutating commands are dry-run by default — add `--apply` to commit; applied schedule mutations also require confirmation or `--yes`.

> **Enterprise tier required.** Scheduling endpoints return 404 on some workspace tiers. Commands are present in all builds but will only succeed on enterprise-tier accounts.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one scheduling list` | List all schedules |
| `ayx one scheduling count` | Count schedules |
| `ayx one scheduling detail` | Inspect a single schedule |
| `ayx one scheduling create --body <file>` | Create a schedule from JSON |
| `ayx one scheduling update <id> --body <file>` | Replace a schedule definition |
| `ayx one scheduling enable` | Enable a schedule |
| `ayx one scheduling disable` | Disable a schedule |
| `ayx one scheduling delete` | Delete a schedule |

To view the schedules attached to a specific plan, use `ayx one plans schedules <id>`.

## Listing schedules

```bash
# All schedules (paginated — first page)
ayx one scheduling list

# All schedules, all pages
ayx one scheduling list --all

# Scoped to a profile
ayx one scheduling list --profile <profile-id>

# Limit results per page
ayx one scheduling list --limit 50

# Machine-readable
ayx --output json one scheduling list --all
```

## Counting schedules

```bash
ayx one scheduling count

ayx --output json one scheduling count
```

Useful for a quick health check — verify the number of active schedules hasn't changed unexpectedly.

## Inspecting a schedule

```bash
ayx one scheduling detail <id>

ayx --output json one scheduling detail <id>
```

`detail` returns the full schedule record including the cron expression, target job group, enabled state, and last/next run times.

## Enabling a schedule

```bash
# Dry-run — shows the request, changes nothing
ayx one scheduling enable <id>

# Commit (interactive confirmation)
ayx one scheduling enable <id> --apply

# Non-interactive
ayx one scheduling enable <id> --apply --yes
```

## Disabling a schedule

```bash
# Dry-run
ayx one scheduling disable <id>

# Commit
ayx one scheduling disable <id> --apply

# Non-interactive (CI / scripts)
ayx one scheduling disable <id> --apply --yes
```

Disabling a schedule stops future runs but does not cancel any run that is already in progress.

## Creating, updating, and deleting

The create and update payloads require a schedule `name`, one task, and one trigger. For a cloud
workflow task, the shape is:

```json
{
  "name": "Daily workflow",
  "tasks": [{"runWorkflow": {"workflowId": "<workflow-ulid>"}}],
  "triggers": [{
    "timeBased": {
      "daily": {"hourOfDay": 6, "minuteOfHour": 0},
      "timezone": "America/Denver"
    }
  }]
}
```

```bash
ayx one scheduling create --body schedule.json
ayx one scheduling create --body schedule.json --apply --yes
ayx one scheduling update <id> --body schedule.json --apply --yes
ayx one scheduling delete <id> --apply --yes
```

Use a future validity window for disposable tests so the schedule cannot run during validation.

## Automation patterns

Audit all enabled schedules:

```bash
ayx --output json one scheduling list --all \
  | jq -r '.data.items[] | select(.enabled == true) | [.id, .name, .nextFireDate] | @tsv'
```

Disable every schedule in a profile before a maintenance window:

```bash
ayx --output json one scheduling list --all --profile <profile-id> \
  | jq -r '.data.items[] | select(.enabled == true) | .id' \
  | xargs -I{} ayx one scheduling disable {} --apply --yes
```

Re-enable them after maintenance:

```bash
ayx --output json one scheduling list --all --profile <profile-id> \
  | jq -r '.data.items[] | select(.enabled == false) | .id' \
  | xargs -I{} ayx one scheduling enable {} --apply --yes
```

Count active vs inactive schedules for a status report:

```bash
ayx --output json one scheduling list --all | jq '
  .data.items | {
    total: length,
    enabled: (map(select(.enabled == true)) | length),
    disabled: (map(select(.enabled == false)) | length)
  }'
```

## Related

- [Job groups](/one/job-groups/) — run and inspect the job groups schedules trigger
- [Plan schedules](/one/plans/schedules/) — view schedules attached to a specific plan via `ayx one plans schedules`
- [Safety model](/safety-model/) — how dry-run and `--apply` work
- [Output & automation](/output-automation/) — JSON envelope and scripting patterns
