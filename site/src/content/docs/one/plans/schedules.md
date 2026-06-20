---
title: Plan schedules
description: Read the schedules attached to an Alteryx One plan.
sidebar:
  order: 2
---

The `ayx one plans schedules` command returns the schedules configured for a plan. It is a read-only command — schedule creation and modification are handled through the Alteryx One platform.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one plans schedules` | List schedules for a plan |

## List schedules for a plan

```bash
ayx one plans schedules --plan-id <plan-id>
ayx one plans schedules --plan-id <plan-id> --profile <name>
```

Returns all schedules attached to the plan. Each schedule entry includes the recurrence definition, enabled state, and next run time.

## JSON output

```bash
ayx --output json one plans schedules --plan-id <plan-id>
```

The response follows the standard envelope:

```json
{
  "ok": true,
  "message": "...",
  "timestamp_utc": "...",
  "data": [...]
}
```

`data` contains the array of schedule records.

## Automation patterns

### Check whether a plan has any active schedules

```bash
ayx --output json one plans schedules --plan-id <plan-id> \
  | jq '[.data[] | select(.enabled == true)] | length'
```

### List all plans with their next run times

```bash
ayx --output json one plans list --all | jq -r '.data[].id' \
  | while IFS= read -r id; do
      ayx --output json one plans schedules --plan-id "$id" \
        | jq -r --arg id "$id" '.data[] | [$id, .nextRun] | @tsv'
    done
```

## Related

- [Plans](/one/plans/)
- [Plans import & export](/one/plans/import-export/)
- [Output & automation](/output-automation/)
