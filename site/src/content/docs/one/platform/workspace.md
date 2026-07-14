---
title: Workspace
description: List, inspect, configure, and manage membership for Alteryx One workspaces.
sidebar:
  order: 2
---

`ayx one platform workspace` manages Alteryx One workspaces — listing them, reading and writing configuration, controlling membership, and transferring ownership. Mutating commands are dry-run by default; add `--apply` to commit.

## Workspaces are token-bound

The Alteryx One PAT you authenticated with determines your active workspace. The `x-alteryx-workspace-gid` header that some API clients send is ignored server-side — the token rules. You cannot change the active workspace by editing a workspace GID in your profile. Instead, use `workspace switch` to re-point to any workspace credential you have already authenticated, or run `auth login` for a new workspace if you haven't authenticated there yet.

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
| `workspace people` | List members of the active workspace |
| `workspace admins` | List admins of the active workspace |
| `workspace switch --workspace-id <id>` | Make an already-authenticated workspace credential active |
| `workspace invite-users` | Invite users to the active workspace |
| `workspace remove-user <id>` | Remove a user from the active workspace |
| `workspace suspend-users` | Suspend users in the active workspace |
| `workspace unsuspend-users` | Unsuspend users in the active workspace |
| `workspace transfer` | Transfer active workspace ownership |
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
ayx one platform workspace people
ayx one platform workspace admins
```

`people` queries `GET /v4/people` and `admins` queries `GET /v4/people?role=admin`. Both are scoped to the active workspace via the token — they no longer accept `--workspace-id`.

### Invite users

```bash
# Preview
ayx one platform workspace invite-users

# Commit
ayx one platform workspace invite-users --apply
```

### Remove a user

```bash
# Preview
ayx one platform workspace remove-user <id>

# Commit (non-interactive)
ayx one platform workspace remove-user \
  <id> \
  --apply --yes
```

### Suspend and unsuspend users

```bash
# Suspend
ayx one platform workspace suspend-users --apply --yes

# Unsuspend
ayx one platform workspace unsuspend-users --apply
```

## Transferring ownership

```bash
# Transfer active workspace ownership (preview)
ayx one platform workspace transfer

# Commit
ayx one platform workspace transfer --apply --yes

# Transfer assets between workspaces
ayx one platform workspace transfer-assets --body '<json>' --apply
```

`transfer-assets` requires a JSON `--body` describing the transfer. Use `--profile <name>` to target a specific environment.

### Workspace mismatch errors

The membership and ownership mutation commands (`invite-users`, `remove-user`, `suspend-users`, `unsuspend-users`, `transfer`, `transfer-assets`) operate on the active workspace determined by the token. Passing a `--workspace-id` that does not match the active workspace will be rejected. Omit the flag and use `workspace switch` to change the active workspace instead.

## Switching workspaces

`workspace switch` re-points the CLI to a workspace credential you have already authenticated. It takes effect immediately — no profile reload required.

```bash
# Switch to a workspace you've previously logged into
ayx one platform workspace switch --workspace-id <id>
```

If you haven't authenticated for that workspace yet, the command errors and directs you to run `auth login` for that workspace first. This is the correct path for changing workspaces — you cannot switch by editing the workspace GID directly, because the active workspace is determined by the token, not a config value.

## Automation patterns

```bash
# Audit: list all workspaces as JSON and extract IDs
ayx --output json one platform workspace list --all \
  | jq -r '.data[].id'

# Bulk suspend users in the active workspace (CI/script)
ayx one platform workspace suspend-users \
  --apply --yes

# Export current workspace config for review
ayx --output json one platform workspace current-configuration \
  | jq '.data'
```

## Related

- [Platform overview](/one/platform/)
- [People](/one/platform/people/) — user lifecycle across the organization
- [Safety model](/safety-model/)
