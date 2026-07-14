---
title: Flow folders
description: Create, inspect, and manage folders that organize flows in Alteryx One.
sidebar:
  order: 2
---

Flow folders organize flows into a hierarchy in Alteryx One. The `ayx one flows folders` branch lets you manage that hierarchy — creating, renaming, and deleting folders, and listing which flows live inside each one.

Mutating commands are dry-run by default — add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one flows folders list` | List all folders |
| `ayx one flows folders count` | Return total folder count |
| `ayx one flows folders detail` | Fetch a single folder's metadata |
| `ayx one flows folders create` | Create a new folder |
| `ayx one flows folders update` | Rename or update a folder |
| `ayx one flows folders delete` | Delete a folder |
| `ayx one flows folders flows list` | List flows inside a folder |
| `ayx one flows folders flows count` | Count flows inside a folder |

## List and inspect

### List folders

```bash
ayx one flows folders list
ayx one flows folders list --profile <name>
ayx one flows folders list --limit 50
ayx one flows folders list --offset 100
```

`--limit` and `--offset` control pagination for this command.

### Count folders

```bash
ayx one flows folders count
ayx one flows folders count --profile <name>
```

### Folder detail

```bash
ayx one flows folders detail <folder-id>
ayx one flows folders detail <folder-id> --profile <name>
```

Returns the full metadata record for a single folder.

### List flows in a folder

```bash
ayx one flows folders flows list <folder-id>
ayx one flows folders flows list <folder-id> --limit 50 --offset 0
```

### Count flows in a folder

```bash
ayx one flows folders flows count <folder-id>
```

## Create and update

```bash
# Preview
ayx one flows folders create --body '<json>'

# Create
ayx one flows folders create --body '<json>' --apply

# Update (rename, re-parent, etc.)
ayx one flows folders update <folder-id> --body '<json>' --apply
```

Both commands accept a JSON body specifying the folder attributes.

## Delete

```bash
# Dry-run
ayx one flows folders delete <folder-id>

# Commit
ayx one flows folders delete <folder-id> --apply

# Non-interactive
ayx one flows folders delete <folder-id> --apply --yes
```

Deleting a folder that still contains flows will be rejected by the server. Move or delete the flows first.

## Automation patterns

### List all folders as JSON

```bash
ayx --output json one flows folders list \
  | jq '.data[]'
```

### Find a folder by name

```bash
ayx --output json one flows folders list \
  | jq -r '.data[] | select(.name == "Production") | .id'
```

### List flows in a folder by name

```bash
folder_id=$(ayx --output json one flows folders list \
  | jq -r '.data[] | select(.name == "Production") | .id')

ayx --output json one flows folders flows list "$folder_id" \
  | jq '.data[]'
```

## Related

- [Flows](/one/flows/)
- [Import & export](/one/flows/import-export/)
- [Safety model](/safety-model/)
