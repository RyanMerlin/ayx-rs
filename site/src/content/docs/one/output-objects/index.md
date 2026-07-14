---
title: Output objects
description: Create, inspect, update, and delete Alteryx One output objects. Inspect their inputs and convert them to Python with wrangle-to-python.
sidebar:
  order: 1
---

`ayx one output-objects` manages the output objects in your Alteryx One workspace. Output objects are named, reusable data targets that flows write to — think of them as addressable output endpoints that downstream flows or external consumers can reference.

Mutating commands are dry-run by default. Add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one output-objects list` | List all output objects; supports pagination |
| `ayx one output-objects count` | Return the total count |
| `ayx one output-objects create --body <json>` | Create an output object |
| `ayx one output-objects detail <id>` | Get details for a single output object |
| `ayx one output-objects update <id> --body <json>` | Update an output object |
| `ayx one output-objects delete <id>` | Delete an output object |
| `ayx one output-objects inputs <id>` | List input references for an output object |
| `ayx one output-objects wrangle-to-python <id>` | Convert an output object to a Python wrangle definition |

## Listing and counting

```bash
# List all output objects
ayx one output-objects list

# Page through large result sets
ayx one output-objects list --all

# Cap the number of results
ayx one output-objects list --limit 25

# Total count only
ayx one output-objects count
```

`--all` follows pagination automatically. `--max-pages <n>` limits how many pages it fetches. `--page-token <token>` lets you resume from a known position.

## Getting details

```bash
ayx one output-objects detail <id>
```

Add `--profile <name>` to target a non-default workspace.

## Creating and updating

Both commands take a `--body` argument containing the JSON payload.

```bash
# Dry-run — shows the request, sends nothing
ayx one output-objects create --body '{"name": "my-output", ...}'

# Commit
ayx one output-objects create --body '{"name": "my-output", ...}' --apply

# Update an existing object
ayx one output-objects update <id> --body '{"name": "renamed"}' --apply
```

## Deleting

```bash
# Dry-run
ayx one output-objects delete <id>

# Commit
ayx one output-objects delete <id> --apply --yes
```

`--yes` skips the TTY confirmation, required in non-interactive scripts.

## Inspecting inputs

List the input references that feed into an output object:

```bash
ayx one output-objects inputs <id>
```

## Converting to Python

`wrangle-to-python` generates a Python definition from an output object. Useful for reproducing the output object's logic in a Python tool or external pipeline.

```bash
ayx one output-objects wrangle-to-python <id>

# Optionally pass a body for conversion parameters
ayx one output-objects wrangle-to-python <id> --body '{"options": {}}'
```

## JSON output

```bash
ayx --output json one output-objects list
```

The envelope is `{ ok, message, timestamp_utc, data }` on success; failures also include `error_code`.

## Related

- [Write settings](/one/write-settings/) — configure where flows write output data
- [Alteryx One overview](/one/) — all `ayx one` areas
- [Safety model](/safety-model/) — dry-run and `--apply`
