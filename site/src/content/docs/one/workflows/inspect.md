---
title: Inspect
description: Inspect cloud-native workflows and their related projections from the CLI.
sidebar:
  order: 2
---

The `ayx one workflows` inspection commands expose a cloud-native workflow and its related projections for dependencies, engines, tools, and assets.

## Quick reference

| Command | Key options | What it does |
|---|---|---|
| `ayx one workflows detail <id>` | `--profile`, `--env`, `--include-dependencies` | Inspect one ULID-keyed workflow |
| `ayx one workflows dependencies <id>` | `--profile`, `--env` | List its connections, datasets, and macros |
| `ayx one workflows engines <id>` | `--profile`, `--env` | Show available execution engines |
| `ayx one workflows tools` | `--env` | List tools available to cloud-native workflows |
| `ayx one workflows assets` | `--profile`, `--env`, `--limit`, `--page-token`, `--all`, `--max-pages` | List the richer workflow-asset projection |

Every command on this page is read-only, so `--apply` is a no-op here. The global `--output`, `--verbose`, `--debug`, and `--no-verify-tls` flags all apply. Use `--output json` for automation, `--env <ENVIRONMENT_FLAG>` to select a named environment, and `--profile <name>` on the leaves that expose it.

## Workflow detail

```bash
ayx one workflows detail <workflow-ulid>
ayx one workflows detail <workflow-ulid> --include-dependencies
```

`detail` is also synthesized client-side: the service has no `GET /v4/workflows/{id}` route, so the CLI searches the richer assets projection. The envelope includes `detail_source` to make that origin explicit.

## Dependencies

```bash
ayx one workflows dependencies <workflow-ulid>
```

Returns the connections, datasets, and macros referenced by the workflow.

## Engines

```bash
ayx one workflows engines <workflow-ulid>
```

Shows the execution engines available for the selected workflow.

## Tools

```bash
ayx one workflows tools
```

Lists the tools exposed to cloud-native workflows. The result is workspace/service metadata, not a workflow editor.

## Assets

```bash
ayx one workflows assets
ayx one workflows assets --limit 100 --all
```

Fetches the richer `/svc-workflow` asset projection used for detail resolution and version-aware operations.

## Honesty notes

- `detail` is a client-side synthesis, not a server route. The envelope includes `detail_source`.

## Known limitations

- In text mode, `tools`, `engines`, and `dependencies` render nested response data as one unformatted line. Use `--output json` for those three commands when you need to inspect or process the nested structure.

## Automation patterns

```bash
# Resolve a workflow id by name, then inspect it
id=$(ayx --output json one workflows list --all \
  | jq -r '.data.items[] | select(.name == "Q3 Revenue Rollup") | .id')
ayx --output json one workflows detail "$id" --include-dependencies | jq '.data'

# Pull every name referenced in the dependency tree, whatever its nesting
ayx --output json one workflows dependencies "$id" | jq -r '.data | .. | .name? // empty'

# Confirm whether detail came from a server route or a client-side synthesis
ayx --output json one workflows detail "$id" | jq '.data.detail_source'
```

## Related

- [Workflows](/one/workflows/)
- [Copy & share](/one/workflows/share/)
- [Datasets](/one/datasets/)
- [Output & automation](/output-automation/)
