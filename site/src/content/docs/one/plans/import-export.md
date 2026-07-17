---
title: Import & export
description: Move Alteryx One plans between environments using import and export commands.
sidebar:
  order: 3
---

Plan import and export let you move plans between Alteryx One workspaces — for example, promoting from development to production. The exported format is portable and can be committed to version control.

Mutating commands are dry-run by default — add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one plans export` | Export a plan to a portable file |
| `ayx one plans import` | Import a plan into the workspace |

## Export

```bash
# Dry-run — shows export metadata, writes nothing
ayx one plans export <plan-id>

# Commit — write the export
ayx one plans export <plan-id> --apply
```

Unlike flow export, `plans export` does not take an `--output` path flag — the server controls the output location or returns the content inline. Use `--output json` to capture the full response including any returned artifact data.

## Import

```bash
# Dry-run
ayx one plans import

# Commit
ayx one plans import --apply

# Non-interactive (CI / scripts)
ayx one plans import --apply --yes

# Target a specific profile
ayx one plans import --profile <name> --apply
```

In this version, `ayx one plans import --help` shows only the standard flags (`--profile`, `--apply`, `--yes`, and the diagnostic flags) — there is no `--body` or `--input`. If you need to automate plan imports, confirm the expected input mechanism for your Alteryx One version before scripting it.

## Promote a plan between environments

```bash
# 1. Export from dev
ayx --profile dev one plans export <plan-id> --apply --output json \
  > plan-export.json

# 2. Import to prod
ayx one plans import --profile prod --apply --yes
```

## Related

- [Plans](/one/plans/)
- [Plan schedules](/one/plans/schedules/)
- [Flow import & export](/one/flows/import-export/)
- [Safety model](/safety-model/)
- [Output & automation](/output-automation/)
