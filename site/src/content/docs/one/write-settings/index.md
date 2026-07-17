---
title: Write settings
description: Create, inspect, update, and delete write settings — the per-flow configuration that controls where and how flow output data is written.
sidebar:
  order: 2
---

`ayx one write-settings` manages write settings for your Alteryx One workspace. Write settings are named configurations that control where a flow writes its output — the destination connection, format, and related options. Flows reference a write setting by ID, letting you change the output target without editing the flow itself.

Mutating commands are dry-run by default. Add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one write-settings list` | List all write settings; supports pagination |
| `ayx one write-settings count` | Return the total count |
| `ayx one write-settings create --body <json>` | Create a write setting |
| `ayx one write-settings detail <id>` | Get details for a single write setting |
| `ayx one write-settings update <id> --body <json>` | Update a write setting |
| `ayx one write-settings delete <id>` | Delete a write setting |

## Listing and counting

```bash
# List all write settings
ayx one write-settings list

# Follow all pages automatically
ayx one write-settings list --all

# Limit results
ayx one write-settings list --limit 50

# Count only
ayx one write-settings count
```

`--page-token <token>` resumes from a known pagination position. `--max-pages <n>` caps automatic pagination.

## Getting details

```bash
ayx one write-settings detail <id>
```

Add `--profile <name>` to target a non-default workspace.

## Creating a write setting

Pass the full write setting definition as a JSON body:

```bash
# Dry-run — shows the request, sends nothing
ayx one write-settings create --body '{"name": "prod-output", ...}'

# Commit
ayx one write-settings create --body '{"name": "prod-output", ...}' --apply
```

## Updating a write setting

```bash
ayx one write-settings update <id> --body '{"name": "renamed"}' --apply
```

## Deleting a write setting

```bash
# Dry-run
ayx one write-settings delete <id>

# Commit (skip TTY prompt in scripts)
ayx one write-settings delete <id> --apply --yes
```

## JSON output

```bash
ayx one write-settings list --output json
```

The envelope is `{ ok, message, timestamp_utc, data }` on success; failures also include `error_code`.

## Related

- [Output objects](/one/output-objects/) — the output endpoints that flows write to
- [Alteryx One overview](/one/) — all `ayx one` areas
- [Safety model](/safety-model/) — dry-run and `--apply`
