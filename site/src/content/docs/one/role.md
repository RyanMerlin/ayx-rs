---
title: Roles
description: List role assignments and assign or unassign roles to subjects in Alteryx One.
sidebar:
  order: 5
---

`ayx one role` manages role assignments in Alteryx One. You can inspect who holds a role, add a subject to a role, and remove them. Assign and unassign are mutating; add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `role list-assignments <id>` | List all subjects assigned to a role |
| `role assign <id> <id>` | Assign a role to a subject |
| `role unassign <id> <id>` | Remove a role from a subject |

## Listing assignments

```bash
# Who holds this role?
ayx one role list-assignments <id>

# Machine-readable
ayx one role list-assignments <id> --output json
```

## Assigning a role

```bash
# Preview
ayx one role assign <id> <id>

# Commit
ayx one role assign \
  <id> \
  <id> \
  --apply
```

The subject is typically a person ID. Use `ayx one person list --all` to find the right ID before assigning.

## Unassigning a role

```bash
# Preview
ayx one role unassign <id> <id>

# Commit
ayx one role unassign \
  <id> \
  <id> \
  --apply --yes
```

## Automation patterns

```bash
# Audit: dump all assignments for a role
ayx one role list-assignments <id> --output json \
  | jq '.data'

# Bulk assign: read subject IDs from a file, assign each
while IFS= read -r subject_id; do
  ayx one role assign \
    <id> \
    "$subject_id" \
    --apply
done < subject_ids.txt

# Verify a specific user holds a role
ayx one role list-assignments <id> --output json \
  | jq -e --arg uid "<person-id>" '.data[] | select(.id == $uid)' \
  && echo "assigned" || echo "not assigned"
```

## Related

- [Identity & auth](/one/identity/)
- [Person](/one/person/) — look up person IDs to use as subjects
- [Safety model](/safety-model/)
