---
title: Flow permissions
description: Set access permissions on individual Alteryx One flows.
sidebar:
  order: 4
---

The `ayx one flows permissions` command sets the access permissions on a flow. It takes a JSON body describing the desired permission state and is mutating — nothing changes until you add `--apply`. `permissions-get` reads the current permissions for a flow.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one flows permissions-get` | Read current permissions on a flow |
| `ayx one flows permissions` | Set permissions on a flow |

> **Note:** `permissions-get` calls `GET /v4/flows/{id}/permissions`, which returns 403 under PAT authentication. The command is available but will not succeed until the scope restriction is lifted or an OAuth token with the required scope is used. The `permissions` subcommand (POST) for setting permissions works as documented below.

## Get permissions

```bash
ayx one flows permissions-get <flow-id>
```

Returns the current permission state for the flow as reported by `GET /v4/flows/{id}/permissions`. Under PAT authentication this call returns 403 (`permission_denied`) — the command will surface the error directly.

## Set permissions

```bash
# Dry-run — shows what the request would contain
ayx one flows permissions \
  <flow-id> \
  --body '<json>'

# Commit
ayx one flows permissions \
  <flow-id> \
  --body '<json>' \
  --apply

# Non-interactive (CI / scripts)
ayx one flows permissions \
  <flow-id> \
  --body '<json>' \
  --apply --yes
```

`--body` is required. It accepts a raw JSON string describing the permission update in the format expected by the Alteryx One API.

## Target a profile

```bash
ayx one flows permissions \
  --profile <name> \
  <flow-id> \
  --body '<json>' \
  --apply
```

## Automation pattern

### Apply the same permissions to multiple flows

```bash
BODY='{"permissions": [...]}'

while IFS= read -r id; do
  ayx one flows permissions --output json \
    "$id" \
    --body "$BODY" \
    --apply --yes \
    | jq -r '[.ok, "'"$id"'"] | @tsv'
done < flow-ids.txt
```

## Related

- [Flows](/one/flows/)
- [Plans](/one/plans/)
- [Safety model](/safety-model/)
