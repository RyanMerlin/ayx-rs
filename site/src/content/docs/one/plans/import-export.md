---
title: Import & export
description: Export Alteryx One plans and track the provider-gated import contract.
sidebar:
  order: 3
---

Plan export produces a portable package that can be committed to version control. Import is retained as a discoverable command, but remains provider-contract-gated until its package input and upload route are verified.

Mutating commands are dry-run by default — add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one plans export` | Export a plan to a portable file |
| `ayx one plans import` | Provider-contract-gated plan import |

## Export

```bash
# Dry-run — shows export metadata, writes nothing
ayx one plans export <plan-id>

# Commit — write the export
ayx one plans export <plan-id> --apply
```

Unlike flow export, `plans export` does not take an `--output` path flag — the server controls the output location or returns the content inline. Use `-o json` to capture the full response including any returned artifact data.

## Import

`one plans import` is currently contract-gated. The provider's package input
mechanism is undocumented and the route is not live-verified, so the CLI
returns a structured validation response without making a network call. Do
not use it for an applied migration until the contract is verified.

Use `one plans export` to capture the source package while the import contract
is being resolved.

## Promote a plan between environments

```bash
# 1. Export from dev
ayx -o json --profile dev one plans export <plan-id> --apply \
  > plan-export.json

# 2. Stop here until a release documents and verifies the provider import contract.
#    `one plans import` currently returns validation without making a request.
```

## Related

- [Plans](/one/plans/)
- [Plan schedules](/one/plans/schedules/)
- [Safety model](/safety-model/)
- [Output & automation](/output-automation/)
