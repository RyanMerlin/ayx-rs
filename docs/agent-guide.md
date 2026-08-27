# AYX CLI Agent Guide

Give this document’s raw GitHub URL to an agent when it needs to use the AYX
CLI. The guide is intentionally short; the CLI’s live discovery output is the
source of truth for exact commands and flags.

## Start here

```text
ayx --version
ayx discover --deep --output json-full
ayx profile current --output json
ayx one auth status --output json
ayx one workspace current --output json
```

Always put `--output json` at the end of the command. For an unfamiliar One
family, discover it first:

```text
ayx discover one --deep --output json-full
```

Walk `data.tree` to select a command. Discovery requires `json-full` because
compact `json` omits the large tree. Do not infer commands from endpoint names
or from a previous workspace.

## Read structured results

- `data.response` usually contains an object response.
- `data.items` usually contains normalized list records.
- `data.page_envelopes[].status_code` proves each fetched page’s HTTP result.
- Preserve `surface`, `operation`, `status_code`, `error_code`, and request ID
  when reporting a result.
- Treat malformed, incomplete, or contradictory upstream data as an issue.

## Safe CRUD

Mutating commands are dry-run by default. Preview the exact command first, then
use `--apply` only for a user-authorized canary. Use a unique name, capture the
returned ID, verify create/read/update, and delete by ID. Re-list afterward and
confirm the baseline is restored.

The repository’s complete protocol is in
`skills/ayx-cli-agent/references/live-crud-protocol.md`, and the current issue
log is `docs/ayx-cli-testing-issues.md`.

Do not execute workflows, share assets, invite users, alter roles, delete
workspace configuration, or modify existing production connections as part of a
generic CRUD test.
