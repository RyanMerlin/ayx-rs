---
title: Safety model
description: Read-only by default; anything that changes your server stays a dry-run until you add --apply.
sidebar:
  order: 4
---

ayx is built so you can explore and automate without fear of breaking something. The rule is simple: **read-only commands just run; commands that change remote state do nothing until you add `--apply`.**

## Read-only vs mutating

Every command carries an annotation:

| Annotation | Meaning |
|------------|---------|
| `read-only` | Never changes remote state. No flags needed. |
| `mutating` | Changes remote state. Needs `--apply` to take effect. |

You'll also see finer-grained variants like `mutating-local` (writes a local file) and `mutating-or-read-only` (depends on the arguments). The [command surface](/reference/command-surface/) lists the annotation for every command.

## The `--apply` gate

Run a mutating command without `--apply` and ayx prints a structured **dry-run** of exactly what it would send, then exits cleanly. Nothing leaves your machine.

```bash
# Dry-run — shows the request, deletes nothing
ayx one flows delete --flow-id <id>

# Commit it
ayx one flows delete --flow-id <id> --apply
```

That's what makes ayx safe to wire into scripts: a pipeline can run the dry-run form against production and never change anything by accident.

For non-interactive automation, add `--yes` to skip the confirmation prompt destructive commands show in a terminal:

```bash
ayx one flows delete --flow-id <id> --apply --yes
```

## Audit artifacts

Destructive operations can record an audit artifact — a file describing what ran — under `${AYX_CONFIG_HOME}/audits/`. List or clean them up with `ayx audit`.

## Check before you act

`ayx doctor` validates config, auth, and connectivity without touching anything:

```bash
ayx doctor          # config, auth, and connectivity
ayx doctor auth     # just the auth path
```
