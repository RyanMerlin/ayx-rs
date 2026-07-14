---
title: People
description: Create, update, delete, and manage passwords for Alteryx One users.
sidebar:
  order: 3
---

`ayx one platform person` manages users in Alteryx One. It covers the full lifecycle: listing, creating, updating, deleting, and handling password resets. Mutating commands are dry-run by default; add `--apply` to commit. Password operations and deletes are sensitive — review dry-run output before adding `--apply`.

## Quick reference

| Command | What it does |
|---|---|
| `person list` | List all users |
| `person current` | Show the user tied to the active profile |
| `person count` | Return the total user count |
| `person detail <id>` | Show detail for a specific user |
| `person create --body <json>` | Create a new user |
| `person update <id> --body <json>` | Replace a user record (PUT) |
| `person patch <id> --body <json>` | Partially update a user record (PATCH) |
| `person delete <id>` | Delete a user |
| `person update-password --body <json>` | Update the current user's password |
| `person password-reset-request --body <json>` | Send a password reset email |

## Listing and inspecting users

```bash
# Paginated list
ayx one platform person list

# All users (auto-paginate)
ayx one platform person list --all

# Limit page size
ayx one platform person list --limit 100

# Total user count
ayx one platform person count

# The authenticated caller
ayx one platform person current

# Specific user
ayx one platform person detail <id>

# Machine-readable
ayx --output json one platform person list --all
```

`--profile <name>` switches the target environment on commands that support it. Use `--max-pages <n>` to cap auto-pagination.

## Creating a user

```bash
# Preview the request
ayx one platform person create --body '{"email":"<email>","firstName":"...","lastName":"..."}'

# Commit
ayx one platform person create \
  --body '{"email":"<email>","firstName":"...","lastName":"..."}' \
  --apply
```

Pass `--profile <name>` to create in a specific environment.

## Updating a user

`update` replaces the full record (PUT). `patch` applies partial changes (PATCH).

```bash
# Full replace (preview)
ayx one platform person update \
  <id> \
  --body '{"email":"<email>","firstName":"...","lastName":"..."}'

# Commit
ayx one platform person update \
  <id> \
  --body '{"email":"<email>","firstName":"...","lastName":"..."}' \
  --apply

# Partial update (patch a single field)
ayx one platform person patch \
  <id> \
  --body '{"firstName":"NewName"}' \
  --apply
```

## Deleting a user

Destructive. Review the dry-run output carefully before adding `--apply`.

```bash
# Preview
ayx one platform person delete <id>

# Commit
ayx one platform person delete <id> --apply --yes
```

`--yes` suppresses the TTY confirmation, required in CI or piped scripts.

## Password management

### Update current user's password

```bash
# Preview
ayx one platform person update-password --body '{"currentPassword":"...","newPassword":"..."}'

# Commit
ayx one platform person update-password \
  --body '{"currentPassword":"...","newPassword":"..."}' \
  --apply
```

### Send a password reset email

```bash
# Preview
ayx one platform person password-reset-request --body '{"email":"<email>"}'

# Commit
ayx one platform person password-reset-request \
  --body '{"email":"<email>"}' \
  --apply
```

This sends the reset email to the user. No `--yes` is required — it is not considered a destructive operation.

## Automation patterns

```bash
# Export all users as JSON for auditing
ayx --output json one platform person list --all | jq '.data'

# Get a user's ID by email
ayx --output json one platform person list --all \
  | jq -r '.data[] | select(.email == "<email>") | .id'

# Bulk delete: pipe IDs into xargs (dry-run first)
ayx --output json one platform person list --all \
  | jq -r '.data[] | select(.someField == "value") | .id' \
  | xargs -I{} ayx one platform person delete {}

# Add --apply once the dry-run output looks right
```

## Related

- [Platform overview](/one/platform/)
- [Workspace](/one/platform/workspace/) — workspace membership (invite, remove, suspend)
- [Roles](/one/platform/roles/) — assign roles to users
- [Safety model](/safety-model/)
