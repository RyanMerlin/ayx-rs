---
title: Safety Model
sidebar_position: 4
---

# Safety model

`ayx-rs` has an explicit safety contract across the entire command surface:

- **Read-only commands** are available without extra flags. They never modify remote state.
- **Mutating commands** require `--apply`. Omitting `--apply` prints a dry-run summary and exits cleanly.
- **Audit artifacts** — several workflow and migration commands produce structured output files so operations can be reviewed or replayed before committing.
- **Unsupported surfaces** fail explicitly rather than silently succeeding with incomplete behavior.

## Command safety annotations

Every command in the [command surface](./reference/command-surface) is annotated with:

| Field | Meaning |
|-------|---------|
| `Safety` | `safe` — read-only; `unsafe` — potential side effects |
| `Mutating` | `true` when the command requires `--apply` to take effect |

## The `--apply` gate

Mutating commands that touch remote resources (delete, import, patch, transfer, migrate) require `--apply` to execute. Without it they print what *would* happen and exit with code 0. This makes it safe to run automation scripts against production without accidentally committing changes.

```bash
# dry-run (no changes made)
ayx one flows delete <id>

# commit the delete
ayx one flows delete <id> --apply
```

## Doctor

`ayx doctor` validates config, auth, and connectivity without touching remote state. Use it to diagnose issues before running any mutating workflow.

```bash
ayx doctor
ayx one doctor discover
```
