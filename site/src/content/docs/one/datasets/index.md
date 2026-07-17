---
title: Datasets
description: Read the Alteryx One dataset library — imported source datasets and the wrangled datasets derived from them — from the command line.
sidebar:
  order: 6
---

`ayx one datasets` provides read-only access to the Alteryx One dataset library: the imported source datasets and the wrangled datasets derived from them.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one datasets list` | List datasets in the user-facing dataset library |
| `ayx one datasets count` | Count datasets in the library |
| `ayx one datasets wrangled list` | List wrangled datasets |
| `ayx one datasets wrangled count` | Count wrangled datasets |
| `ayx one datasets wrangled detail <id>` | Inspect a wrangled dataset by id |
| `ayx one datasets imported detail <id>` | Inspect an imported dataset by id |

## List the library

```bash
ayx one datasets list
ayx one datasets count
```

Add `--profile <name>` to target a non-default workspace.

## Wrangled datasets

Wrangled datasets are the recipe-driven outputs derived from imported sources:

```bash
ayx one datasets wrangled list
ayx one datasets wrangled detail <id>
```

## Imported datasets

Inspect an imported source dataset by id:

```bash
ayx one datasets imported detail <id>
```

## JSON output

Pass `--output json` for structured output. `--output` is a global flag, so it can appear before or after the subcommand:

```bash
ayx one datasets list --output json
```

The envelope is `{ ok, message, timestamp_utc, data }` on success; failures also include `error_code`.

## Related

- [Connections](/one/connections/) — the data connections that back imported datasets
- [Alteryx One overview](/one/) — all `ayx one` areas
