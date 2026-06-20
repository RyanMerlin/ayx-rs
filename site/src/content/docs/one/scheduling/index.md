---
title: Scheduling
description: List, inspect, enable, and disable Alteryx One schedules from the CLI.
sidebar:
  order: 1
---

Schedules define when job groups run automatically in Alteryx One. You can list, inspect, enable, and disable them from the CLI. Mutating commands are dry-run by default — add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one scheduling list` | List all schedules |
| `ayx one scheduling count` | Count schedules |
| `ayx one scheduling detail` | Inspect a single schedule |
| `ayx one scheduling enable` | Enable a schedule |
| `ayx one scheduling disable` | Disable a schedule |

To view the schedules attached to a specific plan, use `ayx one plans schedules --plan-id <id>`.

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
ayx one scheduling detail --schedule-id <id>

ayx --output json one scheduling detail --schedule-id <id>
```

`detail` returns the full schedule record including the cron expression, target job group, enabled state, and last/next run times.

## Enabling a schedule

```bash
# Dry-run — shows the request, changes nothing
ayx one scheduling enable --schedule-id <id>

# Commit
ayx one scheduling enable --schedule-id <id> --apply
```

## Disabling a schedule

```bash
# Dry-run
ayx one scheduling disable --schedule-id <id>

# Commit
ayx one scheduling disable --schedule-id <id> --apply

# Non-interactive (CI / scripts)
ayx one scheduling disable --schedule-id <id> --apply --yes
```

Disabling a schedule stops future runs but does not cancel any run that is already in progress.

## Automation patterns

Audit all enabled schedules:

```bash
ayx --output json one scheduling list --all \
  | jq -r '.data[] | select(.enabled == true) | [.id, .name, .nextRunAt] | @tsv'
```

Disable every schedule in a profile before a maintenance window:

```bash
ayx --output json one scheduling list --all --profile <profile-id> \
  | jq -r '.data[] | select(.enabled == true) | .id' \
  | xargs -I{} ayx one scheduling disable --schedule-id {} --apply --yes
```

Re-enable them after maintenance:

```bash
ayx --output json one scheduling list --all --profile <profile-id> \
  | jq -r '.data[] | select(.enabled == false) | .id' \
  | xargs -I{} ayx one scheduling enable --schedule-id {} --apply
```

Count active vs inactive schedules for a status report:

```bash
ayx --output json one scheduling list --all | jq '
  .data | {
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
