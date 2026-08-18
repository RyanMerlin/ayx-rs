---
title: Copy & share
description: Copy workflows or share them with people and groups from the CLI.
sidebar:
  order: 3
---

The `ayx one workflows copy` and `share` commands duplicate a workflow or grant access to it. Mutating commands are dry-run by default — add `--apply` to commit.

## Quick reference

| Command | Key options | What it does |
|---|---|---|
| `ayx one workflows copy <id>` | `--profile`, `--env`, `--name`, `--version` | Duplicate a workflow |
| `ayx one workflows share <id>` | `--profile`, `--env`, `--to-person`, `--to-group`, `--privilege`, `--include-dependencies`, `--send-email`, `--message`, `--body`, `--no-resolve-emails` | Share a workflow with people or groups |

Every leaf also accepts the global `--output`, `--apply`, `--verbose`, `--debug`, `--no-verify-tls`, and `--yes` flags. Use `--output json` for automation, `--env <ENVIRONMENT_FLAG>` to select a named environment, and `--profile <name>` on the leaves that expose it.

## Copy

```bash
# Dry-run: preview the duplicate request; no remote change occurs
ayx one workflows copy <workflow-ulid> --name "Revenue - copy"

# Apply the copy
ayx one workflows copy <workflow-ulid> --name "Revenue - copy" --apply

# Pin the source version instead of resolving the current version
ayx one workflows copy <workflow-ulid> --name "Revenue v3 copy" --version 3 --apply
```

`--name` is required. Without `--version`, the CLI resolves the workflow's current stored version before constructing the request.

## Share

```bash
# Dry-run: resolve the email and preview the access change
ayx one workflows share <workflow-ulid> \
  --to-person analyst@example.com \
  --privilege read

# Apply the share and notify recipients
ayx one workflows share <workflow-ulid> \
  --to-person 12345 \
  --to-group 67890 \
  --privilege read \
  --send-email \
  --message "Quarterly reporting access" \
  --apply

# Reuse a complete JSON request body from a file
ayx one workflows share <workflow-ulid> --body share.json --apply
```

Repeat `--to-person`, `--to-group`, and `--privilege` as needed. Person values may be numeric ids or email addresses; email addresses are resolved through `GET /v4/people` before the share body is built. Use `--no-resolve-emails` when every person value is already numeric.

The share body shape was recovered from the service's own schema-validation errors; it is not described by a published specification. When constructing flags, `--privilege` is required unless `--body` is supplied, and the recipient lists must contain at least one person or group. `--include-dependencies` includes the workflow's connections and datasets in the same share request.

## Automation patterns

```bash
# Capture the exact request a share would send, without sending it
ayx --output json one workflows share <workflow-ulid> \
  --to-person analyst@example.com \
  --privilege read \
  | jq '.data.would_send'

# Copy in automation, then read the new workflow's identifiers off the envelope
ayx --output json one workflows copy <workflow-ulid> \
  --name "Revenue - copy" --apply --yes \
  | jq '.data'

# Grant several recipients access in a single request
ayx one workflows share <workflow-ulid> \
  --to-person analyst@example.com \
  --to-person lead@example.com \
  --privilege read \
  --apply --yes
```

In non-interactive contexts (CI, pipes) pair `--apply` with `--yes`, or the confirmation prompt has no TTY to read from. Use the dry-run form above to capture a known-good body before committing a share in automation.

## Related

- [Workflows](/one/workflows/)
- [Inspect](/one/workflows/inspect/)
- [Safety model](/safety-model/)
- [Output & automation](/output-automation/)
