---
title: Connection permissions
description: Grant and revoke user or group access to Alteryx One connections.
sidebar:
  order: 3
---

Connection permissions control which users and groups can use a given connection in Alteryx One. You can list, inspect, create, and delete permissions from the CLI. Mutating commands are dry-run by default — add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one connections permissions list` | List all permissions for a connection |
| `ayx one connections permissions detail` | Inspect a single permission by subject ID |
| `ayx one connections permissions create` | Grant a permission from a JSON payload |
| `ayx one connections permissions delete` | Revoke a permission by subject ID |

All commands accept `<connection-id>` as the connection positional argument and `--profile <profile-id>`.

## Listing permissions

```bash
# All permissions for a connection
ayx one connections permissions list <id>

# Scoped to a profile
ayx one connections permissions list <id> --profile <profile-id>

# Machine-readable output
ayx --output json one connections permissions list <id>
```

## Inspecting a permission

A subject is a user or group that has been granted access.

```bash
ayx one connections permissions detail \
  <id> \
  <subject-id>
```

## Granting a permission

```bash
# Dry-run using the convenience flags
ayx one connections permissions create \
  <id> \
  --policy viewer \
  --to-person <subject-id>

# Dry-run using a raw body
ayx one connections permissions create \
  <id> \
  --body permissions.json

# Commit (requires --apply and confirmation; use --yes for non-interactive runs)
ayx one connections permissions create \
  <id> \
  --policy viewer \
  --to-person <subject-id> \
  --apply --yes
```

`permissions.json`:

```json
{
  "connectionId": "<id>",
  "policy": "VIEWER",
  "subjects": {"person": ["<subject-id>"]}
}
```

The raw body must contain only non-empty `person` and/or `group` subject buckets. If
`connectionId` is omitted, the CLI binds the positional id; if it is present and differs, the
request is rejected before confirmation or network I/O.

## Revoking a permission

```bash
# Dry-run
ayx one connections permissions delete \
  <id> \
  <subject-id>

# Commit (skips TTY prompt in CI)
ayx one connections permissions delete \
  <id> \
  <subject-id> \
  --apply --yes
```

## Automation patterns

Audit all subjects with access to a connection:

```bash
ayx --output json one connections permissions list <id> \
  | jq -r '.data.response.people[]? | [.subjectId, .roleType] | @tsv'
```

Remove all permissions for a decommissioned user across multiple connections:

```bash
# First collect connection IDs
ayx --output json one connections list --all | jq -r '.data.items[].id' > conn-ids.txt

# Then revoke per connection where the subject appears
while read conn_id; do
ayx --output json one connections permissions list "$conn_id" \
    | jq -r '.data.response.people[]? | select(.subjectId == "<subject-id>") | .subjectId' \
    | grep -q . && \
    ayx one connections permissions delete \
      "$conn_id" \
      <subject-id> \
      --apply --yes
done < conn-ids.txt
```

## Related

- [Connections](/one/connections/) — manage connection records
- [Connector metadata](/one/connections/connector-metadata/) — defaults and overrides
- [Safety model](/safety-model/) — how dry-run and `--apply` work
- [Output & automation](/output-automation/) — JSON envelope and scripting patterns
