---
title: Flow permissions
description: Set access permissions on individual Alteryx One flows.
sidebar:
  order: 4
---

The `ayx one flows permissions` command sets the access permissions on a flow. It takes a JSON body describing the desired permission state and is mutating — nothing changes until you add `--apply`.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one flows permissions` | Set permissions on a flow |

> **Note:** `GET /v4/flows/{id}/permissions` returns 403 when authenticated via PAT and is not currently accessible through the CLI. The `permissions` subcommand (POST) for setting permissions works as documented below.

## Set permissions

```bash
# Dry-run — shows what the request would contain
ayx one flows permissions \
  --flow-id <flow-id> \
  --body '<json>'

# Commit
ayx one flows permissions \
  --flow-id <flow-id> \
  --body '<json>' \
  --apply

# Non-interactive (CI / scripts)
ayx one flows permissions \
  --flow-id <flow-id> \
  --body '<json>' \
  --apply --yes
```

`--body` is required. It accepts a raw JSON string describing the permission update in the format expected by the Alteryx One API.

## Target a profile

```bash
ayx one flows permissions \
  --profile <name> \
  --flow-id <flow-id> \
  --body '<json>' \
  --apply
```

## Automation pattern

### Apply the same permissions to multiple flows

```bash
BODY='{"permissions": [...]}'

while IFS= read -r id; do
  ayx --output json one flows permissions \
    --flow-id "$id" \
    --body "$BODY" \
    --apply --yes \
    | jq -r '[.ok, "'"$id"'"] | @tsv'
done < flow-ids.txt
```

## Related

- [Flows](/one/flows/)
- [Plans](/one/plans/)
- [Safety model](/safety-model/)
