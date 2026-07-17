---
title: SQL Server
description: Check status, run prechecks, validate connection strings, and plan a SQL Server migration for Alteryx Server using ayx sqlserver.
sidebar:
  order: 5
---

Alteryx One runs on MongoDB; Alteryx Server can run on SQL Server. `ayx sqlserver` manages the SQL Server databases behind an Alteryx Server deployment — verify configuration, validate connection strings, and plan migrations. All commands are read-only unless `--apply` is passed.

## Quick reference

| Command | What it does |
|---------|-------------|
| `ayx sqlserver status` | Check SQL Server connectivity and database state |
| `ayx sqlserver inventory` | List databases and schemas |
| `ayx sqlserver precheck` | Run pre-migration compatibility checks |
| `ayx sqlserver validate-strings` | Validate connection strings in the config |
| `ayx sqlserver connection-string` | Build and emit a connection string for a given scope |
| `ayx sqlserver migrate` | Plan or apply a schema migration |
| `ayx sqlserver prepare` | Prepare the SQL Server environment for an Alteryx migration |

## Checking status

Verify the SQL Server instance is reachable and the Alteryx databases exist:

```sh
ayx sqlserver status --profile prod
```

Get an inventory of databases and schemas:

```sh
ayx sqlserver inventory --profile prod
```

## Prechecks

Run compatibility checks before a migration or upgrade. Specify a collation if your environment requires a specific one:

```sh
ayx sqlserver precheck --profile prod
ayx sqlserver precheck --profile prod --collation SQL_Latin1_General_CP1_CI_AS
```

## Validating connection strings

Check that all connection strings in the active config resolve and authenticate:

```sh
ayx sqlserver validate-strings --profile prod
```

## Building a connection string

Generate a connection string for a given scope. The default scope is `controller`:

```sh
ayx sqlserver connection-string --profile prod
```

Specify a different scope, server, database, or auth method:

```sh
ayx sqlserver connection-string \
  --profile prod \
  --scope controller \
  --server sql-prod.internal \
  --database AlteryxGallery \
  --auth sql \
  --port 1433 \
  --encrypt \
  --trust-server-certificate
```

Available `--auth` values: `sql` (default). See `--help` for the full list as it expands.

For AlwaysOn Availability Group environments:

```sh
ayx sqlserver connection-string --profile prod --multi-subnet-failover
```

## Migration planning and execution

`prepare` and `migrate` support the same flags. Always run `prepare` first — it sets up the target environment. Then run `migrate` to apply schema changes.

Both commands default to a dry run. Review the plan before adding `--apply`.

```sh
# Prepare the environment (dry run)
ayx sqlserver prepare --profile prod --target-version 2024.2

# Inspect the output, then commit
ayx sqlserver prepare --profile prod --target-version 2024.2 --apply

# Plan the migration (dry run)
ayx sqlserver migrate --profile prod --target-version 2024.2

# Apply
ayx sqlserver migrate --profile prod --target-version 2024.2 --apply
```

Both commands also accept `--dry-run` explicitly if you prefer to make the intent clear in scripts.

## Common flags

| Flag | Default | Notes |
|------|---------|-------|
| `--profile <name>` | active profile | Named profile from config |
| `--collation <value>` | (server default) | Collation for precheck |
| `--scope <value>` | `controller` | Scope for connection-string |
| `--auth <value>` | `sql` | Auth method for connection-string |
| `--target-version <ver>` | (none) | Required for prepare and migrate |
| `--apply` | (off) | Required to commit prepare and migrate |
| `--dry-run` | (off) | Explicit dry-run flag on prepare and migrate |
| `--yes` | (off) | Skip TTY confirmation |

## JSON output

All commands accept `--output json` as a global flag:

```sh
ayx sqlserver status --profile prod --output json
```

## Related

- [Alteryx Server overview](/server/)
- [MongoDB](/server/mongo/)
- [Upgrade](/server/upgrade/)
