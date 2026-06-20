---
title: Safety Model
description: Read-only by default; mutating commands are gated behind --apply.
sidebar:
  order: 3
---

ayx has an explicit safety contract across the whole command surface.

- **Read-only commands** run without extra flags and never modify remote state.
- **Mutating commands** require `--apply`. Without it they print a dry-run of what would change and exit with status&nbsp;0.
- **Unsupported surfaces** fail explicitly rather than silently succeeding.

## Command annotations

Every command in the [command surface](/reference/command-surface/) is annotated:

| Field | Values |
|-------|--------|
| `Safety` | `read-only`, `mutating`, and finer-grained variants such as `mutating-local` and `mutating-or-read-only` |
| `Mutating` | `yes` when the command requires `--apply`, otherwise `no` |

## The `--apply` gate

Mutating remote operations (delete, import, transfer, migrate) require `--apply`. Without it they print what *would* happen and exit cleanly, so automation can run against production without committing changes:

```bash
# dry-run — nothing is deleted
ayx one flows delete --flow-id <id>

# commit the delete
ayx one flows delete --flow-id <id> --apply
```

## Doctor

`ayx doctor` validates config, auth, and connectivity without touching remote state:

```bash
ayx doctor
ayx one doctor discover
```
