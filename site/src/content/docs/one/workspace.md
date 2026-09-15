---
title: Workspace
description: Inspect and administer Alteryx One workspaces with a stable, current-workspace hierarchy.
sidebar:
  order: 2
---

`ayx one workspace` has a deliberate, breaking hierarchy. The old flat verbs are not aliases: endpoint and API-version names are not part of the public CLI contract.

Mutating commands dry-run unless `--apply` is passed. Applied destructive changes prompt for confirmation; use `--yes` for non-interactive automation.

## Command tree

```text
ayx one workspace
  list | create | delete | current | detail | use
  config get | set | schema | reset
  members list | admins | invite | reinvite | remove | suspend | unsuspend | update | invitation-link
  groups list | create | update | delete
         members add | members remove
         roles set
  cloud-configs list | create | update
  transfer start | assets
```

Commands beneath `config`, `members`, `groups`, `cloud-configs`, and `transfer` operate on the active workspace. Select another already-authenticated workspace with `workspace use <id>` or authenticate to it first with `ayx one login`.

## Read-only inspection

```bash
ayx one workspace list --all
ayx one workspace current
ayx one workspace detail <numeric-id>
ayx one workspace config get
ayx one workspace config schema
ayx one workspace members list
ayx one workspace members admins
ayx one workspace groups list
ayx one workspace cloud-configs list
```

`members list` uses the active-workspace people directory. `members admins` uses the dedicated administrator endpoint and resolves the required numeric workspace ID before requesting it; it does not infer admins from a general people response.

Use JSON or YAML for the full redacted machine envelope:

```bash
ayx one workspace members list -o json
```

## Configuration

```bash
# Preview an update
ayx one workspace config set --body config.json

# Apply it
ayx one workspace config set --body config.json --apply --yes

# Reset the current configuration
ayx one workspace config reset --apply --yes
```

The hierarchy intentionally exposes current-workspace configuration only. It does not publish endpoint-version variants or separate by-ID configuration verbs.

## Members and groups

```bash
# Invite using a JSON payload
ayx one workspace members invite --body invite.json --apply --yes

# Patch a member using a JSON payload
ayx one workspace members update <person-id> --body member.json --apply --yes

# Suspend one member; the vendor's unsuspend endpoint is workspace-wide
ayx one workspace members suspend <person-id> --apply --yes
ayx one workspace members unsuspend --apply --yes

# Manage groups
ayx one workspace groups create --body group.json --apply --yes
ayx one workspace groups members add <group-id> --user-id <person-id> --apply --yes
ayx one workspace groups roles set <group-id> --body roles.json --apply --yes
```

`members update` maps to the PATCH operation. The more destructive replace operation is intentionally not exposed. `members unsuspend` preserves the vendor API's bulk scope; it is named as a workspace operation rather than pretending it affects one person.

## Cloud configuration and transfer

```bash
ayx one workspace cloud-configs create aws --body cloud.json --apply --yes
ayx one workspace cloud-configs update aws --body cloud.json --apply --yes

ayx one workspace transfer start --apply --yes
ayx one workspace transfer assets --body transfer.json --apply --yes
```

## Selecting the active workspace

```bash
# List saved workspace credentials
ayx one workspace use

# Select one only after its token identity verifies
ayx one workspace use <id>
```

`workspace use` does not merely edit a profile value: it probes the selected credential's current-workspace identity before persisting the active selection.

## Removed flat commands

There are no compatibility aliases for the former flat forms such as `workspace people`, `workspace current-configuration`, `workspace switch`, `workspace create-group`, or `workspace groups-global`. Use the tree above. The global-groups endpoint and by-ID/version-named configuration forms are intentionally not part of this public contract.

## Related

- [Identity & auth](/one/identity/)
- [Person](/one/person/)
- [Safety model](/safety-model/)
