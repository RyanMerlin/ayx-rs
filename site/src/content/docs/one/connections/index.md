---
title: Connections
description: List, create, update, delete, and inspect Alteryx One connections from the CLI.
sidebar:
  order: 1
---

Connections represent the data-source credentials Alteryx One uses to run workflows. You can list, create, update, delete, and inspect them from the CLI. Mutating commands are dry-run by default — add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one connections list` | List all connections |
| `ayx one connections count` | Count connections |
| `ayx one connections detail` | Inspect a single connection |
| `ayx one connections status` | Check connection health status |
| `ayx one connections dry-run` | Validate a create payload without writing |
| `ayx one connections create` | Create a connection from a JSON payload |
| `ayx one connections update` | Update a connection from a JSON payload |
| `ayx one connections delete` | Delete a connection |

## Listing connections

```bash
# All connections (paginated — first page)
ayx one connections list

# All connections, all pages
ayx one connections list --all

# Scoped to a specific Alteryx One profile
ayx one connections list --profile <profile-id>

# Limit results per page
ayx one connections list --limit 25

# Machine-readable output
ayx --output json one connections list --all
```

The `--all` flag follows pagination automatically and returns every record. For large environments pair it with `--output json` and pipe into `jq`.

## Inspecting a connection

```bash
# Full detail
ayx one connections detail --connection-id <id>

# Health status only
ayx one connections status --connection-id <id>
```

`detail` returns the full connection record including type, owner, and configuration keys. `status` returns a lighter response focused on whether the connection can reach its target.

## Validating a payload before creating

`dry-run` sends the request body through the same validation path as `create` but never writes anything. Use it to catch schema errors before committing.

```bash
ayx one connections dry-run --body '{"name":"My DB","type":"SQLServer","...":{}}'
```

## Creating a connection

```bash
# Dry-run (default — nothing is written)
ayx one connections create --body '{"name":"My DB","type":"SQLServer","...":{}}'

# Commit
ayx one connections create --body '{"name":"My DB","type":"SQLServer","...":"{}"}' --apply
```

The `--body` value is a JSON string. For larger payloads use a file and process substitution:

```bash
ayx one connections create --body "$(cat connection.json)" --apply
```

## Updating a connection

```bash
# Dry-run
ayx one connections update --connection-id <id> --body '{"name":"Renamed DB"}'

# Commit
ayx one connections update --connection-id <id> --body '{"name":"Renamed DB"}' --apply
```

Only the fields you include in `--body` are changed.

## Deleting a connection

```bash
# Dry-run
ayx one connections delete --connection-id <id>

# Commit (skips TTY prompt in CI)
ayx one connections delete --connection-id <id> --apply --yes
```

## Automation patterns

Parse the connection ID from a list to use in downstream commands:

```bash
ayx --output json one connections list --all \
  | jq -r '.data[] | select(.name == "My DB") | .id'
```

Audit connection health across all connections:

```bash
ayx --output json one connections list --all \
  | jq -r '.data[].id' \
  | xargs -I{} ayx --output json one connections status --connection-id {}
```

## Related

- [Connector metadata](/one/connections/connector-metadata/) — inspect and override connector defaults
- [Connection permissions](/one/connections/permissions/) — grant and revoke user/group access
- [Safety model](/safety-model/) — how dry-run and `--apply` work
- [Output & automation](/output-automation/) — JSON envelope and scripting patterns
