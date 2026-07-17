---
title: API tokens
description: List, create, inspect, and delete API tokens in Alteryx One.
sidebar:
  order: 4
---

`ayx one token` manages API tokens for the active Alteryx One profile. Tokens are scoped to the authenticated caller. Creating and deleting tokens are mutating; add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `token list` | List API tokens for the current user |
| `token create --body <json>` | Create a new API token |
| `token detail <id>` | Show details for a specific token |
| `token delete <id>` | Delete a token |

## Listing tokens

```bash
# List all tokens for the current user
ayx one token list

# Machine-readable
ayx one token list --output json
```

`token list` takes no filter flags. Use `jq` to filter the JSON output.

## Creating a token

```bash
# Preview the request
ayx one token create --body '{"name":"ci-bot","description":"..."}'

# Commit
ayx one token create \
  --body '{"name":"ci-bot","description":"..."}' \
  --apply
```

The token value is returned once at creation time. Store it immediately — it cannot be retrieved again.

Pass `--profile <name>` to create the token against a non-default environment.

## Inspecting a token

```bash
ayx one token detail <id>

# JSON for scripting
ayx one token detail <id> --output json
```

Pass `--profile <name>` to query a specific environment.

## Deleting a token

Deleting a token immediately revokes it. Any automation using it will stop working.

```bash
# Preview
ayx one token delete <id>

# Commit
ayx one token delete <id> --apply --yes
```

`--yes` skips the TTY confirmation, required in non-interactive scripts.

## Automation patterns

```bash
# Audit: list all token IDs and names
ayx one token list --output json \
  | jq -r '.data[] | "\(.id)\t\(.name)"'

# Rotate: create new, then delete old
NEW_ID=$(ayx one token create --output json \
  --body '{"name":"ci-bot-new"}' --apply \
  | jq -r '.data.id')

# Store the new token value from that response, then:
ayx one token delete <old-id> --apply --yes
```

## Related

- [Identity & auth](/one/identity/)
- [Person](/one/person/) — manage the users who own tokens
- [Safety model](/safety-model/)
