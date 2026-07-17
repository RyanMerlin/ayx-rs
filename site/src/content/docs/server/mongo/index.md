---
title: MongoDB
description: Inspect, back up, restore, and query Alteryx Server's embedded or managed MongoDB using ayx mongo.
sidebar:
  order: 4
---

`ayx mongo` covers the MongoDB databases that back Alteryx Server — both embedded (default) and managed deployments. `backup`, `restore`, `mutate`, and `undo` all use `--apply` as the single execute gate — omitting `--apply` is how each of them previews. There is no separate `--dry-run` flag anywhere on the Mongo command tree. `status`, `inventory`, `query`, and `doctor` are always read-only.

## Quick reference

| Command | What it does |
|---------|-------------|
| `ayx mongo status` | Check Mongo connection and replication state |
| `ayx mongo inventory` | List databases and collections |
| `ayx mongo backup` | Snapshot the databases (preview by default — omit `--apply` to dry-run) |
| `ayx mongo restore` | Restore from a snapshot (preview by default — omit `--apply` to dry-run) |
| `ayx mongo query` | Run a read query against a collection |
| `ayx mongo mutate` | Run a named, bounded mutation template against a collection (preview by default; `--apply` requires `--accept-mutation-risk`, `--backup-audit-artifact`, `--approval-artifact`, and `--approve` together) |
| `ayx mongo undo` | Reverse a prior applied mutation from its execution audit artifact (guarded — preview by default; `--apply` requires the same approval gates, minus a backup) |
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

`mongo mutate` does not take a free-form filter/update. It runs a named, bounded template from the mutation registry (`knowledge/mongo/mutations.yaml`) with typed `--param` bindings. Every template is a single reviewed `$set` update with a capped `max_affected` document count, and a template stays `preview_only` — refused outright by `--apply` — until an owner deliberately promotes it to `executable`.

There is no `--dry-run` flag on the Mongo command tree. Omitting `--apply` **is** the dry-run: it runs the template's filter as a live read-only query, prints the matched documents and the field-level diff the `$set` would produce, and writes a preview approval artifact to the audit directory (default `audits/`). It never writes to the database.

```sh
# Preview — read-only. Prints the diff and an approval_digest (sha256:...),
# and writes an approval artifact under audits/.
ayx mongo mutate \
  --profile prod \
  --template user_email_domain_migration \
  --param new_email=someone@companyb.com
```

Review the printed diff. If it's correct, apply it with the exact digest the preview printed, proof of a current backup, and the preview artifact itself:

```sh
# Apply — requires --apply, --accept-mutation-risk, --backup-audit-artifact,
# --approval-artifact, and --approve together; any missing piece is reported
# all at once, not one at a time. --yes is required outside an interactive TTY.
ayx --yes mongo mutate \
  --apply \
  --profile prod \
  --template user_email_domain_migration \
  --param new_email=someone@companyb.com \
  --accept-mutation-risk \
  --backup-audit-artifact audits/mongo-backup-2026-07-16T12-00-00Z.json \
  --approval-artifact audits/mongo-mutate-preview-2026-07-16T12-05-00Z.json \
  --approve sha256:<digest printed by the preview>
```

What each gate proves:

- **`--approve <digest>`** binds the apply to the *exact* candidate diff a human reviewed in the preview step. The digest is re-derived from the approval artifact's stored snapshot at apply time, so a hand-edited or tampered artifact fails validation even if the digest string still matches.
- **`--approval-artifact`** is the artifact the preview run wrote. It must match the resolved template's id/revision/source digest and the caller's parameter digest, carry a non-zero matched-document count, and not have expired — approvals expire after the template's `max_backup_age_minutes`, capped at 4 hours regardless of what the template declares.
- **`--backup-audit-artifact`** must point at a *successful, applied* `mongo backup` audit artifact (not a preview) for the same profile, whose backup directory still exists on disk and whose age is within the template's `max_backup_age_minutes`. Take a fresh backup before every apply — see [Backup](#backup).
- **`--yes`** is the global flag that skips the interactive TTY confirmation prompt. It's required for any non-interactive `--apply` run (CI, cron, scripts) — without a TTY there's nothing to confirm against, so the command refuses to run destructively unattended. Interactively, `--yes` is optional because the confirmation prompt itself is the gate, and it names the real resolved database/collection and the real approved matched-document count — not just the raw request.

Every preview and every apply — success or failure — writes a JSON audit artifact to the audit directory. Connection details (the Mongo URI, the temporary password-file path) are always redacted in both the printed output and the artifact.

Use `--print` to render the resolved mongosh invocation with no query and no artifact at all — useful for sanity-checking parameter substitution before running a real preview. `--print` conflicts with `--apply` and all three approval flags.

## Undoing a mutation

`mongo undo` reverses a prior `mongo mutate --apply` run from the execution audit artifact it wrote, by re-applying the recorded pre-mutation field values (`guarded_set_inverse`). It follows the same preview/approve/apply shape as `mutate`:

```sh
# Preview — live, read-only staleness check against the recorded candidates.
ayx mongo undo \
  --profile prod \
  --mutation-audit-artifact audits/mongo-mutate-execute-2026-07-16T12-10-00Z.json

# Apply — same gate shape as mutate --apply, minus --backup-audit-artifact
# (undo restores from the mutation's own recorded prior values, not a backup).
ayx --yes mongo undo \
  --apply \
  --profile prod \
  --mutation-audit-artifact audits/mongo-mutate-execute-2026-07-16T12-10-00Z.json \
  --accept-mutation-risk \
  --approval-artifact audits/mongo-undo-preview-2026-07-16T12-15-00Z.json \
  --approve sha256:<digest printed by the undo preview>
```

**Undo is guarded, not a general rollback, and it does not overpromise recovery.** It is only available when both hold:

- the source artifact recorded a **successfully applied** mutation using the `guarded_set_inverse` rollback strategy — the only strategy the executor supports; a template declaring anything else fails validation before it can ever run;
- **every** affected field on **every** candidate document still holds the exact value the original mutation wrote. Undo re-checks this live, immediately before restoring, and refuses the **entire batch** if even one document has drifted (edited again, deleted, or touched by something else since the mutation) — it will not partially undo a batch.

Undo is **not** a substitute for backup/restore. It does not repair a mutation whose transaction outcome is unknown (a partial write, a timeout, or a lost connection mid-apply), and it deliberately refuses stale data rather than guessing. For any of those situations, restore from the backup artifact taken before the mutation — see [Restore](#restore) — using a tested restore procedure, not undo.

## Doctor

Run a structured health sweep to surface configuration issues, replication lag, and index problems:

```sh
ayx mongo doctor --profile prod
```

For the guided backup-then-restore action:

```sh
ayx actions run mongo.backup-restore
```

## Common flags

| Flag | Default | Notes |
|------|---------|-------|
| `--profile <name>` | active profile | Named profile from config |
| `--output-dir <dir>` | `backups` | Backup destination (backup command) |
| `--input-path <path>` | (required) | Backup source path (restore command) |
| `--template <name>` | (required for apply) | Named template from the mutation registry (mutate, query) |
| `--param key=value` | none | Bind a typed template parameter; repeatable (mutate) |
| `--audit-dir <dir>` | `audits` | Where audit artifacts are written |
| `--apply` | (off) | Execute gate for backup, restore, mutate, and undo — there is no separate `--dry-run` flag; omitting `--apply` previews |
| `--yes` | (off) | Skip TTY confirmation; required for `--apply` in non-interactive automation |
| `--accept-mutation-risk` | (off) | Required together with the flags below for `mutate --apply` and `undo --apply` |
| `--backup-audit-artifact <path>` | (required for mutate apply) | Path to a current, successful `mongo backup` audit artifact |
| `--approval-artifact <path>` | (required for apply) | Path to the artifact the matching preview run wrote (mutate, undo) |
| `--approve <sha256:digest>` | (required for apply) | The digest printed by the matching preview run (mutate, undo) |
| `--mutation-audit-artifact <path>` | (required) | The `mongo mutate --apply` execution artifact being reversed (undo command) |
| `--print` | (off) | Render the resolved mongosh invocation only — no query, no artifact (mutate, undo, query) |

## JSON output

All commands accept `--output json` as a global flag:

```sh
ayx --output json mongo status --profile prod
```

## Related

- [Alteryx Server overview](/server/)
- [Upgrade](/server/upgrade/)
- [Actions & workflows](/telemetry/actions/) — see `mongo.backup-restore`, `mongo.doctor`, `mongo.queue.stuck`
