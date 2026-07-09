---
title: MongoDB
description: Inspect, back up, restore, and query Alteryx Server's embedded or managed MongoDB using ayx mongo.
sidebar:
  order: 4
---

`ayx mongo` covers the MongoDB databases that back Alteryx Server — both embedded (default) and managed deployments. Backup and restore require `--apply` to commit. All other commands are read-only.

## Quick reference

| Command | What it does |
|---------|-------------|
| `ayx mongo status` | Check Mongo connection and replication state |
| `ayx mongo inventory` | List databases and collections |
| `ayx mongo backup` | Snapshot the databases (dry-run by default) |
| `ayx mongo restore` | Restore from a snapshot (dry-run by default) |
| `ayx mongo query` | Run a read query against a collection |
| `ayx mongo mutate` | Apply an update to a collection (destructive — requires `--apply` and `--accept-mutation-risk`) |
| `ayx mongo doctor` | Run a structured health sweep |

## Checking status

Confirm Mongo is reachable and healthy before any backup or restore:

```sh
ayx mongo status --profile prod
```

Get a list of databases and collections:

```sh
ayx mongo inventory --profile prod
```

## Backup

Backup is the most common pre-change operation. The workflow is always: dry run first, then commit.

```sh
# Dry run — review the plan envelope
ayx mongo backup --profile prod --output-dir backups/pre-upgrade

# Commit the snapshot
ayx mongo backup --profile prod --output-dir backups/pre-upgrade --apply
```

An audit artifact is written to the audit directory (default: `audits/`) on every committed backup.

Override the audit directory:

```sh
ayx mongo backup --profile prod --output-dir backups/pre-upgrade --audit-dir /var/audit --apply
```

## Restore

Restore is destructive. Test on a non-production profile before running against production.

```sh
# Dry run — confirm the source path resolves
ayx mongo restore --profile staging --input-path backups/pre-upgrade

# Commit
ayx mongo restore --profile staging --input-path backups/pre-upgrade --apply
```

An audit artifact is written on every committed restore.

## Querying

Run a read query against any collection. All query flags are optional — omit them to see everything (subject to a default limit):

```sh
ayx mongo query \
  --profile prod \
  --database AlteryxGallery \
  --collection AS_Queue \
  --filter '{"Status": "Running"}' \
  --limit 20
```

Project specific fields:

```sh
ayx mongo query \
  --profile prod \
  --database AlteryxGallery \
  --collection AS_Queue \
  --projection '{"_id": 1, "Status": 1, "CreatedDate": 1}' \
  --sort '{"CreatedDate": -1}' \
  --limit 10
```

Use `--print` to emit raw document output alongside the JSON envelope.

Use a named template with `--template <name>` to run a pre-defined query shape.

## Mutating documents

`mongo mutate` directly modifies documents. It requires both `--apply` and `--accept-mutation-risk`. Use this only when directed by Alteryx support or when you fully understand the target documents.

```sh
ayx mongo mutate \
  --profile prod \
  --database AlteryxGallery \
  --collection AS_Queue \
  --filter '{"Status": "Stuck"}' \
  --update '{"$set": {"Status": "Cancelled"}}' \
  --apply \
  --accept-mutation-risk
```

## Doctor

Run a structured health sweep to surface configuration issues, replication lag, and index problems:

```sh
ayx mongo doctor --profile prod
```

For the guided backup-then-restore tactic:

```sh
ayx tactics run mongo.backup-restore
```

## Common flags

| Flag | Default | Notes |
|------|---------|-------|
| `--profile <name>` | active profile | Named profile from config |
| `--output-dir <dir>` | `backups` | Backup destination (backup command) |
| `--input-path <path>` | (required) | Backup source path (restore command) |
| `--audit-dir <dir>` | `audits` | Where audit artifacts are written |
| `--apply` | (off) | Required for backup, restore, and mutate |
| `--yes` | (off) | Skip TTY confirmation; required in automation |
| `--accept-mutation-risk` | (off) | Required for `mutate` |

## JSON output

All commands accept `--output json` as a global flag:

```sh
ayx --output json mongo status --profile prod
```

## Related

- [Alteryx Server overview](/server/)
- [Upgrade](/server/upgrade/)
- [Tactics & workflows](/telemetry/tactics/) — see `mongo.backup-restore`, `mongo.doctor`, `mongo.queue.stuck`
