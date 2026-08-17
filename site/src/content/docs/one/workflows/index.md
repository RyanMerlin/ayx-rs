---
title: Workflows
description: List, count, inspect, copy, and share Alteryx One cloud-native canvas workflows from the CLI.
sidebar:
  order: 1
---

Alteryx One **workflows** are not the same resource as `ayx one flows`. `one flows` is the older Designer Cloud family at `/v4/flows`, keyed by integer ids. These cloud-native canvas workflows are keyed by ULIDs and served by the separate `/svc-workflow/api/vN` service. A workspace can hold dozens of cloud-native workflows while `ayx one flows list` returns zero items.

The workflows surface is for browsing and managing existing canvas workflows. Authoring arbitrary visual workflow logic is out of scope: no public endpoint accepts it.

Mutating commands are dry-run by default — add `--apply` to commit.

## Quick reference

| Command | Key options | What it does |
|---|---|---|
| `ayx one workflows list` | `--profile`, `--env`, `--limit`, `--page-token`, `--all`, `--max-pages` | List cloud-native workflows |
| `ayx one workflows count` | `--profile`, `--env` | Return the workspace workflow count |
| `ayx one workflows detail <id>` | `--profile`, `--env`, `--include-dependencies` | Inspect one ULID-keyed workflow — see [Inspect](/one/workflows/inspect/) |
| `ayx one workflows dependencies <id>` | `--profile`, `--env` | List its connections, datasets, and macros — see [Inspect](/one/workflows/inspect/) |
| `ayx one workflows engines <id>` | `--profile`, `--env` | Show available execution engines — see [Inspect](/one/workflows/inspect/) |
| `ayx one workflows tools` | `--env` | List tools available to cloud-native workflows — see [Inspect](/one/workflows/inspect/) |
| `ayx one workflows assets` | `--profile`, `--env`, `--limit`, `--page-token`, `--all`, `--max-pages` | List the richer workflow-asset projection — see [Inspect](/one/workflows/inspect/) |
| `ayx one workflows copy <id>` | `--profile`, `--env`, `--name`, `--version` | Duplicate a workflow — see [Copy & share](/one/workflows/share/) |
| `ayx one workflows share <id>` | `--profile`, `--env`, `--to-person`, `--to-group`, `--privilege`, `--include-dependencies`, `--send-email`, `--message`, `--body`, `--no-resolve-emails` | Share a workflow with people or groups — see [Copy & share](/one/workflows/share/) |

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

## Automation patterns

```bash
# Extract every workflow id and name as TSV
ayx --output json one workflows list --all \
  | jq -r '.data.items[] | [.id, .name] | @tsv'

# Verify a --all fetch actually reached the reported total
ayx --output json one workflows list --all | jq '.data.complete'

# Compare the synthesized count against the number of items returned
ayx --output json one workflows count | jq '{count: .data.count, source: .data.count_source}'
```

## Honesty notes

- `count` is a client-side synthesis, not a real server route. Its envelope includes `count_source`. The same applies to `detail` — see [Inspect](/one/workflows/inspect/).
- This command family manages existing cloud-native workflows; it does not author arbitrary canvas logic because no endpoint accepts it.

## Known limitations

- `--output table` is currently an alias for `--output text`, not a distinct table renderer. It goes through the same text-mode rendering path.
- `list --all` currently returns a `data.complete` boolean rather than guaranteeing that every item was fetched through cursor pagination. The `/v4/workflows` endpoint is limit-based and does not expose reliable cursor pagination; if `complete` is `false`, use an explicit `--limit` above your expected total and verify it becomes `true`.

## Related

- [Inspect](/one/workflows/inspect/) — detail, dependencies, engines, tools, and assets
- [Copy & share](/one/workflows/share/) — duplicate a workflow or grant access
- [Flows](/one/flows/) — the separate integer-id-keyed Designer Cloud `/v4/flows` family
- [Datasets](/one/datasets/) — the One dataset library
- [Plans](/one/plans/) — orchestrate multi-flow plans
- [Safety model](/safety-model/) — dry-run and `--apply` in detail
- [Output & automation](/output-automation/) — structured envelopes and JSON pipelines
