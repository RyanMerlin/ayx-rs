---
title: Workspace
description: List, inspect, configure, and manage membership for Alteryx One workspaces.
sidebar:
  order: 2
---

`ayx one platform workspace` manages Alteryx One workspaces — listing them, reading and writing configuration, controlling membership, and transferring ownership. Mutating commands are dry-run by default; add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `workspace list` | List all workspaces |
| `workspace current` | Show the workspace for the active profile |
| `workspace current-configuration` | Read configuration for the current workspace |
| `workspace current-configuration-schema` | Read the configuration schema for the current workspace |
| `workspace configuration --workspace-id <id>` | Read configuration for a specific workspace |
| `workspace configuration-v4 --workspace-id <id>` | Read v4 configuration for a specific workspace |
| `workspace configuration-schema --workspace-id <id>` | Read the configuration schema for a specific workspace |
| `workspace save-current-configuration --body <json>` | Write configuration to the current workspace |
| `workspace save-configuration-v4 --workspace-id <id> --body <json>` | Write v4 configuration to a specific workspace |
| `workspace delete-current-configuration` | Delete configuration on the current workspace |
| `workspace delete-configuration --workspace-id <id>` | Delete configuration on a specific workspace |
| `workspace people --workspace-id <id>` | List members of a workspace |
| `workspace admins --workspace-id <id>` | List admins of a workspace |
| `workspace invite-users --workspace-id <id>` | Invite users to a workspace |
| `workspace remove-user --workspace-id <id> --person-id <id>` | Remove a user from a workspace |
| `workspace suspend-users --workspace-id <id>` | Suspend users in a workspace |
| `workspace unsuspend-users --workspace-id <id>` | Unsuspend users in a workspace |
| `workspace transfer --workspace-id <id>` | Transfer workspace ownership |
| `workspace transfer-assets --body <json>` | Transfer assets between workspaces |

## Listing workspaces

```bash
# All workspaces, paginated
ayx one platform workspace list

# All workspaces in one call (auto-paginate)
ayx one platform workspace list --all

# Limit page size
ayx one platform workspace list --limit 50

# Machine-readable
ayx --output json one platform workspace list --all
```

`--all` follows all pagination tokens automatically. Use `--max-pages <n>` to cap how many pages it fetches.

## Current workspace

```bash
# Show the workspace tied to the active profile
ayx one platform workspace current
```

## Reading configuration

```bash
# Configuration for the current workspace
ayx one platform workspace current-configuration

# Configuration schema (to understand what fields are writable)
ayx one platform workspace current-configuration-schema

# Configuration for a specific workspace
ayx one platform workspace configuration --workspace-id <id>

# Configuration schema for a specific workspace
ayx one platform workspace configuration-schema --workspace-id <id>

# v4 configuration for a specific workspace
ayx one platform workspace configuration-v4 --workspace-id <id>
```

## Writing configuration

These commands are mutating. Without `--apply` they return a dry-run envelope showing the request that would be sent.

```bash
# Dry-run: preview the request
ayx one platform workspace save-current-configuration --body '{"key":"value"}'

# Commit: write to the current workspace
ayx one platform workspace save-current-configuration --body '{"key":"value"}' --apply

# Write to a specific workspace (v4 endpoint)
ayx one platform workspace save-configuration-v4 \
  --workspace-id <id> \
  --body '{"key":"value"}' \
  --apply
```

Pass `--profile <name>` to target a non-default environment on commands that support it.

## Deleting configuration

Destructive. Add `--yes` in non-interactive contexts.

```bash
# Preview
ayx one platform workspace delete-current-configuration

# Commit
ayx one platform workspace delete-current-configuration --apply --yes

# Delete configuration on a specific workspace
ayx one platform workspace delete-configuration --workspace-id <id> --apply --yes
```

## Workspace membership

### List members and admins

```bash
ayx one platform workspace people --workspace-id <id>
ayx one platform workspace admins --workspace-id <id>
```

### Invite users

```bash
# Preview
ayx one platform workspace invite-users --workspace-id <id>

# Commit
ayx one platform workspace invite-users --workspace-id <id> --apply
```

### Remove a user

```bash
# Preview
ayx one platform workspace remove-user --workspace-id <id> --person-id <id>

# Commit (non-interactive)
ayx one platform workspace remove-user \
  --workspace-id <id> \
  --person-id <id> \
  --apply --yes
```

### Suspend and unsuspend users

```bash
# Suspend
ayx one platform workspace suspend-users --workspace-id <id> --apply --yes

# Unsuspend
ayx one platform workspace unsuspend-users --workspace-id <id> --apply
```

## Transferring ownership

```bash
# Transfer workspace ownership (preview)
ayx one platform workspace transfer --workspace-id <id>

# Commit
ayx one platform workspace transfer --workspace-id <id> --apply --yes

# Transfer assets between workspaces
ayx one platform workspace transfer-assets --body '<json>' --apply
```

`transfer-assets` requires a JSON `--body` describing the transfer. Use `--profile <name>` to target a specific environment.

## Automation patterns

```bash
# Audit: list all workspaces as JSON and extract IDs
ayx --output json one platform workspace list --all \
  | jq -r '.data[].id'

# Bulk suspend users in a workspace (CI/script)
ayx one platform workspace suspend-users \
  --workspace-id <id> \
  --apply --yes

# Export current workspace config for review
ayx --output json one platform workspace current-configuration \
  | jq '.data'
```

## Related

- [Platform overview](/one/platform/)
- [People](/one/platform/people/) — user lifecycle across the organization
- [Safety model](/safety-model/)
