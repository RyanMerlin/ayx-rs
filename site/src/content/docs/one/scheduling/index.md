---
title: Scheduling
description: List, inspect, enable, and disable Alteryx One schedules from the CLI.
sidebar:
  order: 1
---

Schedules define when job groups run automatically in Alteryx One. You can list, inspect, enable, and disable them from the CLI. Mutating commands are dry-run by default — add `--apply` to commit.

> **Enterprise tier required.** Scheduling endpoints return 404 on some workspace tiers. Commands are present in all builds but will only succeed on enterprise-tier accounts.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one scheduling list` | List all schedules |
| `ayx one scheduling count` | Count schedules |
| `ayx one scheduling detail` | Inspect a single schedule |
| `ayx one scheduling enable` | Enable a schedule |
| `ayx one scheduling disable` | Disable a schedule |

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
ayx one scheduling list --all --output json
```

## Counting schedules

```bash
ayx one scheduling count

ayx one scheduling count --output json
```

Useful for a quick health check — verify the number of active schedules hasn't changed unexpectedly.

## Inspecting a schedule

```bash
ayx one scheduling detail <id>

ayx one scheduling detail <id> --output json
```

`detail` returns the full schedule record including the cron expression, target job group, enabled state, and last/next run times.

## Enabling a schedule

```bash
# Dry-run — shows the request, changes nothing
ayx one scheduling enable <id>

# Commit
ayx one scheduling enable <id> --apply
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

## Automation patterns

Audit all enabled schedules:

```bash
ayx one scheduling list --all --output json \
  | jq -r '.data[] | select(.enabled == true) | [.id, .name, .nextRunAt] | @tsv'
```

Disable every schedule in a profile before a maintenance window:

```bash
ayx one scheduling list --all --profile <profile-id> --output json \
  | jq -r '.data[] | select(.enabled == true) | .id' \
  | xargs -I{} ayx one scheduling disable {} --apply --yes
```

Re-enable them after maintenance:

```bash
ayx one scheduling list --all --profile <profile-id> --output json \
  | jq -r '.data[] | select(.enabled == false) | .id' \
  | xargs -I{} ayx one scheduling enable {} --apply
```

Count active vs inactive schedules for a status report:

```bash
ayx one scheduling list --all --output json | jq '
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
