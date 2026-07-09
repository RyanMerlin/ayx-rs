---
title: Webhooks
description: Create, inspect, delete, and test webhook-triggered flow tasks in Alteryx One.
sidebar:
  order: 3
---

`ayx one webhook-flow-tasks` manages webhook-triggered flow tasks. A webhook flow task links an inbound HTTP call to a flow execution — when the webhook endpoint receives a request, Alteryx One triggers the associated flow. This lets external systems kick off flows without a schedule.

Mutating commands are dry-run by default. Add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one webhook-flow-tasks create --body <json>` | Create a webhook flow task |
| `ayx one webhook-flow-tasks detail --webhook-flow-task-id <id>` | Get details for a webhook flow task |
| `ayx one webhook-flow-tasks delete --webhook-flow-task-id <id>` | Delete a webhook flow task |
| `ayx one webhook-flow-tasks test --body <json>` | Send a test trigger to a webhook flow task |

## Creating a webhook flow task

Pass the task definition as a JSON body. The body specifies the flow to trigger and any task configuration.

```bash
# Dry-run — shows the request, sends nothing
ayx one webhook-flow-tasks create --body '{"flow_id": "<id>", ...}'

# Commit
ayx one webhook-flow-tasks create --body '{"flow_id": "<id>", ...}' --apply
```

Add `--profile <name>` to target a non-default workspace.

## Getting task details

```bash
ayx one webhook-flow-tasks detail --webhook-flow-task-id <id>
```

Use this to retrieve the webhook URL and current task configuration.

## Testing a webhook

Send a test payload to verify the webhook is wired up correctly before production use:

```bash
ayx one webhook-flow-tasks test --body '{"test_payload": {}}'
```

`test` is a mutating command. It sends a real trigger to the platform — include `--apply` to actually fire it:

```bash
ayx one webhook-flow-tasks test --body '{"test_payload": {}}' --apply
```

## Deleting a webhook flow task

```bash
# Dry-run
ayx one webhook-flow-tasks delete --webhook-flow-task-id <id>

# Commit
ayx one webhook-flow-tasks delete --webhook-flow-task-id <id> --apply --yes
```

## JSON output

```bash
ayx --output json one webhook-flow-tasks detail --webhook-flow-task-id <id>
```

The envelope is `{ ok, message, timestamp_utc, data }` on success; failures also include `error_code`.

## Related

- [Alteryx One overview](/one/) — all `ayx one` areas
- [Safety model](/safety-model/) — dry-run and `--apply`
