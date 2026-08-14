---
title: Workflows
description: List, inspect, copy, and share Alteryx One cloud-native canvas workflows from the CLI.
sidebar:
  order: 2
---

Alteryx One **workflows** are not the same resource as `ayx one flows`. `one flows` is the older Designer Cloud family at `/v4/flows`, keyed by integer ids. These cloud-native canvas workflows are keyed by ULIDs and served by the separate `/svc-workflow/api/vN` service. A workspace can hold dozens of cloud-native workflows while `ayx one flows list` returns zero items.

The workflows surface is for browsing and managing existing canvas workflows. Authoring arbitrary visual workflow logic is out of scope: no public endpoint accepts it.

Mutating commands are dry-run by default — add `--apply` to commit.

## Quick reference

| Command | Key options | What it does |
|---|---|---|
| `ayx one workflows list` | `--profile`, `--env`, `--limit`, `--page-token`, `--all`, `--max-pages` | List cloud-native workflows |
| `ayx one workflows count` | `--profile`, `--env` | Return the workspace workflow count |
| `ayx one workflows assets` | `--profile`, `--env`, `--limit`, `--page-token`, `--all`, `--max-pages` | List the richer workflow-asset projection |
| `ayx one workflows detail <id>` | `--profile`, `--env`, `--include-dependencies` | Inspect one ULID-keyed workflow |
| `ayx one workflows dependencies <id>` | `--profile`, `--env` | List its connections, datasets, and macros |
| `ayx one workflows engines <id>` | `--profile`, `--env` | Show available execution engines |
| `ayx one workflows tools` | `--env` | List tools available to cloud-native workflows |
| `ayx one workflows copy <id>` | `--profile`, `--env`, `--name`, `--version` | Duplicate a workflow |
| `ayx one workflows share <id>` | `--profile`, `--env`, `--to-person`, `--to-group`, `--privilege`, `--include-dependencies`, `--send-email`, `--message`, `--body`, `--no-resolve-emails` | Share a workflow with people or groups |

Every leaf also accepts the global `--output`, `--apply`, `--verbose`, `--debug`, `--no-verify-tls`, and `--yes` flags. Use `--output json` for automation, `--env <ENVIRONMENT_FLAG>` to select a named environment, and `--profile <name>` on the leaves that expose it.

## List and count

### List workflows

```bash
# Use the server's default page size
ayx one workflows list

# Set an explicit limit or start from a returned page token
ayx one workflows list --limit 100
ayx one workflows list --page-token <token>

# Request the all-items form and inspect data.complete in the envelope
ayx --output json one workflows list --all
```

The list response uses `data.items`. The current `/v4/workflows` endpoint reports a collection `count` but does not provide reliable cursor pagination; `data.complete` tells you whether the fetched item count reached that total. If it is `false`, increase `--limit` and check again.

### Count workflows

```bash
ayx one workflows count
ayx one workflows count --profile <name>
ayx --output json one workflows count
```

`count` is synthesized client-side from the workflow-list response because the API has no `/v4/workflows/count` route. Its output includes `count_source`, so consumers can distinguish this assembly from a server-side count lookup.

## Inspect

### Workflow detail

```bash
ayx one workflows detail <workflow-ulid>
ayx one workflows detail <workflow-ulid> --include-dependencies
```

`detail` is also synthesized client-side: the service has no `GET /v4/workflows/{id}` route, so the CLI searches the richer assets projection. The envelope includes `detail_source` to make that origin explicit.

### Dependencies

```bash
ayx one workflows dependencies <workflow-ulid>
```

Returns the connections, datasets, and macros referenced by the workflow.

### Engines

```bash
ayx one workflows engines <workflow-ulid>
```

Shows the execution engines available for the selected workflow.

### Tools

```bash
ayx one workflows tools
```

Lists the tools exposed to cloud-native workflows. The result is workspace/service metadata, not a workflow editor.

### Assets

```bash
ayx one workflows assets
ayx one workflows assets --limit 100 --all
```

Fetches the richer `/svc-workflow` asset projection used for detail resolution and version-aware operations.

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

## Honesty notes

- `detail` and `count` are client-side syntheses, not real server routes. Their envelopes include `detail_source` and `count_source` respectively.
- `share` uses a request shape recovered from the service's schema-validation errors rather than a published spec. Treat the CLI-generated body or a dry-run envelope as the reliable shape.
- This command family manages existing cloud-native workflows; it does not author arbitrary canvas logic because no endpoint accepts it.

## Known limitations

- `--output table` is currently an alias for `--output text`, not a distinct table renderer. It goes through the same text-mode rendering path.
- In text mode, `tools`, `engines`, and `dependencies` render nested response data as one unformatted line. Use `--output json` for those three commands when you need to inspect or process the nested structure.
- `list --all` currently returns a `data.complete` boolean rather than guaranteeing that every item was fetched through cursor pagination. The `/v4/workflows` endpoint is limit-based and does not expose reliable cursor pagination; if `complete` is `false`, use an explicit `--limit` above your expected total and verify it becomes `true`.

## Related

- [Flows](/one/flows/) — the separate integer-id-keyed Designer Cloud `/v4/flows` family
- [Datasets](/one/datasets/) — the One dataset library
- [Plans](/one/plans/) — orchestrate multi-flow plans
- [Safety model](/safety-model/) — dry-run and `--apply` in detail
- [Output & automation](/output-automation/) — structured envelopes and JSON pipelines
