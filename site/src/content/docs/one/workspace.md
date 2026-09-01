---
title: Workspace
description: List, inspect, configure, and manage membership for Alteryx One workspaces.
sidebar:
  order: 2
---

`ayx one workspace` manages Alteryx One workspaces — listing them, reading and writing configuration, controlling membership, and transferring ownership. Mutating commands are dry-run by default; add `--apply` to commit. Applied mutations prompt for confirmation; add `--yes` for non-interactive scripts.

## Workspaces are token-bound

The Alteryx One PAT you authenticated with determines your active workspace. The `x-alteryx-workspace-gid` header that some API clients send is ignored server-side — the token rules. You cannot change the active workspace by editing a workspace GID in your profile. Instead, use `workspace switch` to re-point to any workspace credential you have already authenticated, or run `ayx one login` for a new workspace if you haven't authenticated there yet.

## Quick reference

| Command | What it does |
|---|---|
| `workspace list` | List all workspaces |
| `workspace current` | Show the workspace for the active profile |
| `workspace current-configuration` | Read configuration for the current workspace |
| `workspace current-configuration-schema` | Read the configuration schema for the current workspace |
| `workspace configuration <id>` | Read configuration for a specific workspace |
| `workspace configuration-v4 <id>` | Read v4 configuration for a specific workspace |
| `workspace configuration-schema <id>` | Read the configuration schema for a specific workspace |
| `workspace save-current-configuration --body <file>` | Write configuration to the current workspace |
| `workspace save-configuration-v4 <id> --body <file>` | Write v4 configuration to a specific workspace |
| `workspace delete-current-configuration` | Delete configuration on the current workspace |
| `workspace delete-configuration <id>` | Delete configuration on a specific workspace |
| `workspace people` | List members of the active workspace |
| `workspace admins` | List admins of the active workspace |
| `workspace switch <id>` | Make an already-authenticated workspace credential active |
| `workspace invite-users` | Invite users to the active workspace |
| `workspace remove-user <id>` | Remove a user from the active workspace |
| `workspace suspend-users` | Suspend users in the active workspace |
| `workspace unsuspend-users` | Unsuspend users in the active workspace |
| `workspace transfer` | Transfer active workspace ownership |
| `workspace transfer-assets --body <file>` | Transfer assets between workspaces |

## Listing workspaces

```bash
# All workspaces, paginated
ayx one workspace list

# All workspaces in one call (auto-paginate)
ayx one workspace list --all

# Limit page size
ayx one workspace list --limit 50

# Machine-readable
ayx --output json one workspace list --all
```

`--all` follows all pagination tokens automatically. Use `--max-pages <n>` to cap how many pages it fetches.

## Current workspace

```bash
# Show the workspace tied to the active profile
ayx one workspace current
```

## Reading configuration

```bash
# Configuration for the current workspace
ayx one workspace current-configuration

# Configuration schema (to understand what fields are writable)
ayx one workspace current-configuration-schema

# Configuration for a specific workspace
ayx one workspace configuration <id>

# Configuration schema for a specific workspace
ayx one workspace configuration-schema <id>

# v4 configuration for a specific workspace
ayx one workspace configuration-v4 <id>
```

## Writing configuration

These commands are mutating. Without `--apply` they return a dry-run envelope showing the request that would be sent.

```bash
# Dry-run: preview the request from a JSON file
ayx one workspace save-current-configuration --body config.json

# Commit: write to the current workspace
ayx one workspace save-current-configuration --body config.json --apply

# Write to a specific workspace (v4 endpoint)
ayx one workspace save-configuration-v4 \
  <id> \
  --body config.json \
  --apply
```

Pass `--profile <name>` to target a non-default environment on commands that support it.

## Deleting configuration

Destructive. Add `--yes` in non-interactive contexts.

```bash
# Preview
ayx one workspace delete-current-configuration

# Commit
ayx one workspace delete-current-configuration --apply --yes

# Delete configuration on a specific workspace
ayx one workspace delete-configuration <id> --apply --yes
```

## Workspace membership

### List members and admins

```bash
ayx one workspace people
ayx one workspace admins
```

`people` queries `GET /v4/people`, scoped to the active workspace via the token. `admins` queries the dedicated `GET /v4/workspaces/{workspaceId}/admins` route, where `workspaceId` is the *numeric* workspace id — the CLI resolves it from the active workspace with the same `/v4/workspaces/current` preflight the other path-scoped workspace commands use. Neither command accepts `--workspace-id`.

`admins` deliberately does not reuse `/v4/people?role=admin`: the gateway ignores `role=admin`, and `/v4/people` sets the `isAdmin` flag only on the calling user's own record, so the people list cannot be filtered down to admins on the client.

### Invite users

```bash
# Preview
ayx one workspace invite-users

# Commit
ayx one workspace invite-users --apply
```

### Remove a user

```bash
# Preview
ayx one workspace remove-user <id>

# Commit (non-interactive)
ayx one workspace remove-user \
  <id> \
  --apply --yes
```

### Suspend and unsuspend users

```bash
# Suspend
ayx one workspace suspend-users --apply --yes

# Unsuspend
ayx one workspace unsuspend-users --apply
```

## Transferring ownership

```bash
# Transfer active workspace ownership (preview)
ayx one workspace transfer

# Commit
ayx one workspace transfer --apply --yes

# Transfer assets between workspaces
ayx one workspace transfer-assets --body transfer.json --apply --yes
```

`transfer-assets` requires a JSON file passed with `--body` describing the transfer. Use `--profile <name>` to target a specific environment.

### Workspace mismatch errors

Path-scoped workspace commands resolve `/v4/workspaces/current` before dispatch. If `--workspace-id` is supplied, it must match the current numeric workspace ID; the current workspace GID must also match the profile context. This applies to group, invitation, suspension, transfer, cloud-config, and workspace-user commands. Omit the flag where supported and use `workspace switch` to change the active workspace instead. `transfer-assets` does not take a `--workspace-id` at all — it always operates on the current workspace.

## Switching workspaces

`workspace switch` re-points the CLI to a workspace credential you have already authenticated. It takes effect immediately — no profile reload required.

```bash
# Switch to a workspace you've previously logged into
ayx one workspace switch <id>
```

If you haven't authenticated for that workspace yet, the command errors and directs you to run `ayx one login` for that workspace first. This is the correct path for changing workspaces — you cannot switch by editing the workspace GID directly, because the active workspace is determined by the token, not a config value.

## Automation patterns

```bash
# Audit: list all workspaces as JSON and extract IDs
ayx --output json one workspace list --all \
  | jq -r '.data.items[].id'

# Bulk suspend users in the active workspace (CI/script)
ayx one workspace suspend-users \
  --apply --yes

# Export current workspace config for review
ayx --output json one workspace current-configuration \
  | jq '.data'
```

## Related

- [Identity & auth](/one/identity/)
- [Person](/one/person/) — user lifecycle across the organization
- [Safety model](/safety-model/)
