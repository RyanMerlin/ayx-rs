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
ayx one connections permissions list <id> --output json
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
# Dry-run
ayx one connections permissions create \
  <id> \
  --body '{"subjectId":"<subject-id>","role":"viewer"}'

# Commit
ayx one connections permissions create \
  <id> \
  --body '{"subjectId":"<subject-id>","role":"viewer"}' \
  --apply
```

The `--body` JSON structure depends on your Alteryx One version. Use `ayx one connections permissions list` on an existing connection to see the shape of current permission records.

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
ayx one connections permissions list <id> --output json \
  | jq -r '.data[] | [.subjectId, .role] | @tsv'
```

Remove all permissions for a decommissioned user across multiple connections:

```bash
# First collect connection IDs
ayx one connections list --all --output json | jq -r '.data[].id' > conn-ids.txt

# Then revoke per connection where the subject appears
while read conn_id; do
  ayx one connections permissions list "$conn_id" --output json \
    | jq -r '.data[] | select(.subjectId == "<subject-id>") | .subjectId' \
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
