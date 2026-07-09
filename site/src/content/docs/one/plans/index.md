---
title: Plans
description: Create, run, and manage Alteryx One plans — scheduled or on-demand collections of flows.
sidebar:
  order: 1
---

Plans in Alteryx One group flows into a single orchestrated unit that can be run on demand or on a schedule. The `ayx one plans` branch covers the full lifecycle: browsing, creating, running, updating, sharing, and managing access.

Mutating commands are dry-run by default — add `--apply` to commit.

> **Enterprise tier required.** Plans endpoints return 404 on some workspace tiers. Commands are present in all builds but will only succeed on enterprise-tier accounts.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one plans list` | List plans in the workspace |
| `ayx one plans count` | Return total plan count |
| `ayx one plans detail` | Fetch a plan's summary metadata |
| `ayx one plans full` | Fetch a plan's full metadata including flows |
| `ayx one plans run` | Trigger an on-demand plan run |
| `ayx one plans run-parameters` | List the run parameters a plan accepts |
| `ayx one plans create` | Create a new plan |
| `ayx one plans update` | Update a plan |
| `ayx one plans delete` | Delete a plan |
| `ayx one plans share` | Share a plan with users or groups |
| `ayx one plans permissions` | Read permissions for a plan |
| `ayx one plans schedules` | List schedules attached to a plan |
| `ayx one plans export` | Export a plan to a portable format |
| `ayx one plans import` | Import a plan |

## List and inspect

### List plans

```bash
ayx one plans list
ayx one plans list --profile <name>
ayx one plans list --limit 50
ayx one plans list --all
ayx one plans list --all --max-pages 10
```

`--all` follows pagination automatically. Pass `--page-token` to resume from a specific page.

### Count plans

```bash
ayx one plans count
ayx one plans count --profile <name>
```

### Plan detail

```bash
# Summary record
ayx one plans detail --plan-id <plan-id>

# Full record including flows
ayx one plans full --plan-id <plan-id>
```

Use `detail` for fast lookups. Use `full` when you need to inspect which flows are in the plan or their configuration.

### Run parameters

```bash
ayx one plans run-parameters --plan-id <plan-id>
```

Lists the parameters that can be passed when triggering a run.

### Schedules

```bash
ayx one plans schedules --plan-id <plan-id>
```

Returns the schedules configured for this plan. See [Plan schedules](/one/plans/schedules/) for detail.

### Permissions

```bash
ayx one plans permissions --plan-id <plan-id>
ayx one plans permissions --plan-id <plan-id> --subject-id <subject-id>
```

`--subject-id` filters the response to a specific user or group. Omit it to return all permission entries for the plan.

## Run

```bash
# Dry-run
ayx one plans run --plan-id <plan-id>

# Trigger an on-demand run
ayx one plans run --plan-id <plan-id> --apply

# Non-interactive
ayx one plans run --plan-id <plan-id> --apply --yes
```

## Create and update

```bash
# Preview
ayx one plans create --body '<json>'

# Create
ayx one plans create --body '<json>' --apply

# Update
ayx one plans update --plan-id <plan-id> --body '<json>' --apply
```

Both commands require `--body` with the plan definition or patch as a JSON string.

## Share

```bash
# Preview
ayx one plans share --plan-id <plan-id> --body '<json>'

# Commit
ayx one plans share --plan-id <plan-id> --body '<json>' --apply
```

## Delete

```bash
# Dry-run
ayx one plans delete --plan-id <plan-id>

# Commit
ayx one plans delete --plan-id <plan-id> --apply

# Non-interactive
ayx one plans delete --plan-id <plan-id> --apply --yes
```

## Automation patterns

### List all plans as JSON

```bash
ayx --output json one plans list --all \
  | jq '.data[]'
```

### Find a plan by name

```bash
ayx --output json one plans list --all \
  | jq -r '.data[] | select(.name == "Daily ETL") | .id'
```

### Run a plan and check success

```bash
result=$(ayx --output json one plans run --plan-id <plan-id> --apply)
echo "$result" | jq -e '.ok'
```

### Run plans in a specific environment

```bash
ayx --output json --environment prod one plans list --all
```

## Related

- [Plan schedules](/one/plans/schedules/)
- [Plans import & export](/one/plans/import-export/)
- [Flows](/one/flows/)
- [Safety model](/safety-model/)
- [Output & automation](/output-automation/)
