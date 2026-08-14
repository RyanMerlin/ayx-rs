---
title: Flows
description: List, inspect, run, and manage Alteryx One flows from the CLI.
sidebar:
  order: 1
---

Flows are the core execution unit in Alteryx One. The `ayx one flows` branch covers every lifecycle operation: browsing the catalog, running flows on demand, editing metadata, moving flows between folders, and managing the data connections they use.

> These are **not** the same as Alteryx One's cloud-native workflows documented in [Workflows](/one/workflows/). A workspace can contain many cloud-native workflows while `ayx one flows list` returns no items, because `one flows` reads the separate integer-id-keyed Designer Cloud `/v4/flows` family.

Mutating commands are dry-run by default — add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one flows list` | List flows in the workspace |
| `ayx one flows count` | Return total flow count |
| `ayx one flows detail` | Fetch a single flow's metadata |
| `ayx one flows validate` | Validate a flow |
| `ayx one flows run` | Trigger an on-demand run |
| `ayx one flows create` | Create a new flow |
| `ayx one flows update` | Update flow metadata |
| `ayx one flows delete` | Delete a flow |
| `ayx one flows copy` | Copy a flow |
| `ayx one flows move` | Move a flow to a different folder |
| `ayx one flows parameters` | List flow parameters |
| `ayx one flows inputs` | List flow input connections |
| `ayx one flows outputs` | List flow output connections |
| `ayx one flows replace-dataset` | Replace a dataset reference in a flow |
| `ayx one flows library list` | List library flows |
| `ayx one flows library count` | Count library flows |
| `ayx one flows folders ...` | Manage flow folders — see [Flow folders](/one/flows/folders/) |
| `ayx one flows permissions` | Set permissions on a flow |
| `ayx one flows import` | Import a flow package |
| `ayx one flows import-dry-run` | Preview an import without applying |
| `ayx one flows export` | Export a flow to a file |
| `ayx one flows export-dry-run` | Preview an export without writing |

## List and inspect

### List flows

```bash
ayx one flows list
ayx one flows list --profile <name>
ayx one flows list --limit 50
ayx one flows list --all
ayx one flows list --all --max-pages 10
```

`--all` follows pagination automatically, stopping at `--max-pages` (default 50). Use `--page-token` to start from a specific page returned by a prior call.

### Count flows

```bash
ayx one flows count
ayx one flows count --profile <name>
```

Returns the total number of flows in the workspace.

### Flow detail

```bash
ayx one flows detail <flow-id>
ayx one flows detail <flow-id> --profile <name>
```

Returns the full metadata record for a single flow.

### Validate a flow

```bash
ayx one flows validate <flow-id>
```

Runs server-side validation and returns any errors. Read-only — does not modify the flow.

### Inspect connections and parameters

```bash
# Parameters the flow accepts
ayx one flows parameters <flow-id>
ayx one flows parameters <flow-id> --output-object-type <type>

# Input data connections
ayx one flows inputs <flow-id>

# Output data connections
ayx one flows outputs <flow-id>
```

## Run

```bash
# Dry-run (shows what would be triggered)
ayx one flows run <flow-id>

# Trigger an on-demand run
ayx one flows run <flow-id> --apply

# Pass a JSON body (e.g. run parameters)
ayx one flows run <flow-id> --body '<json>' --apply
```

`--body` accepts a raw JSON string. Use it to override parameters or pass run-time configuration accepted by the flow.

## Create and update

```bash
# Dry-run: preview the request
ayx one flows create --body '<json>'

# Create
ayx one flows create --body '<json>' --apply

# Update
ayx one flows update <flow-id> --body '<json>' --apply
```

Both commands require `--body` with the flow definition or patch as a JSON string.

## Copy and move

```bash
# Copy (body specifies the destination name / folder)
ayx one flows copy <flow-id> --body '<json>' --apply

# Move to a different folder
ayx one flows move <flow-id> --body '<json>' --apply
```

## Replace a dataset

```bash
ayx one flows replace-dataset <flow-id> --body '<json>' --apply
```

Replaces a dataset reference inside the flow without modifying the flow logic. Useful when promoting flows between environments that point at different data sources.

## Delete

```bash
# Dry-run
ayx one flows delete <flow-id>

# Commit
ayx one flows delete <flow-id> --apply

# Non-interactive (CI / scripts)
ayx one flows delete <flow-id> --apply --yes
```

## Automation patterns

### List all flows as JSON

```bash
ayx --output json one flows list --all \
  | jq '.data[]'
```

### Extract just IDs and names

```bash
ayx --output json one flows list --all \
  | jq -r '.data[] | [.id, .name] | @tsv'
```

### Run a flow and capture the job reference

```bash
result=$(ayx --output json one flows run <flow-id> --apply)
ok=$(echo "$result" | jq -r '.ok')
```

### Validate before promoting

```bash
ayx --output json one flows validate <flow-id> \
  | jq -e '.ok'
```

### Target a specific environment

```bash
ayx --output json --env prod one flows list
```

`--env` is a root flag — place it before the subcommand.

## Related

- [Flow folders](/one/flows/folders/)
- [Import & export](/one/flows/import-export/)
- [Flow permissions](/one/flows/permissions/)
- [Plans](/one/plans/)
- [Safety model](/safety-model/)
- [Output & automation](/output-automation/)
