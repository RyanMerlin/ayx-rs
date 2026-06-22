---
title: Platform overview
description: What ayx one platform covers — status, inventory, workspace management, people, tokens, and roles.
sidebar:
  order: 1
---

`ayx one platform` is the administrative surface for Alteryx One. It covers the operations that affect your organization's workspaces, users, API credentials, and role assignments.

All mutating commands (anything that creates, updates, suspends, removes, or deletes) are dry-run by default. Add `--apply` to commit the change. Add `--yes` to skip the TTY confirmation in scripts.

## What's here

| Area | Command | What you do |
|---|---|---|
| Platform status | `ayx one platform status` | Check platform health |
| Asset inventory | `ayx one platform inventory` | List platform assets |
| Workspaces | `ayx one platform workspace` | Manage workspaces, membership, configuration, transfers, and workspace switching |
| People | `ayx one platform person` | Create, update, delete users; reset passwords |
| API tokens | `ayx one platform token` | Issue and revoke API tokens |
| Roles | `ayx one platform role` | Assign and unassign roles to subjects |

## Status and inventory

Quick read-only checks — no flags required.

```bash
# Platform health
ayx one platform status

# Asset inventory
ayx one platform inventory
```

Both commands accept `--profile <name>` to target a non-default environment and `--output json` (root flag, before the subcommand) for machine-readable output.

## JSON output

Pass `--output json` as a root flag to get a structured envelope on stdout:

```bash
ayx --output json one platform status
```

The envelope shape is `{ ok, message, timestamp_utc, data }`. Combine with `--verbose` to see progress on stderr without polluting stdout.

## Related

- [Workspace](/one/platform/workspace/) — workspace CRUD, membership, configuration, and transfers
- [People](/one/platform/people/) — user lifecycle and password management
- [API tokens](/one/platform/tokens/) — token issuance and revocation
- [Roles](/one/platform/roles/) — role assignment and unassignment
- [Safety model](/safety-model/) — how dry-run and `--apply` work
