---
title: Datasets
description: Read and create Alteryx One dataset references, plus inspect wrangled datasets, from the command line.
sidebar:
  order: 6
---

`ayx one datasets` provides access to the Alteryx One dataset library: imported source datasets and the wrangled datasets derived from them. Dataset creation creates a URI-backed imported dataset reference; it does not upload local file bytes.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one datasets create --body <file>` | Create an imported dataset reference from JSON |
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

## Create an imported dataset reference

The public One API creates an imported dataset record from a JSON body. The
source URI must already be reachable by the provider:

```json
{
  "uri": "s3://bucket/path/data.csv",
  "name": "Demo dataset",
  "description": "Dataset used by the MCP demo"
}
```

Preview the request, then apply it explicitly:

```bash
ayx one datasets create --body dataset.json
ayx --apply one datasets create --body dataset.json
```

For local-file upload/staging, use the Alteryx One UI until a supported upload
contract is available.

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
ayx --output json one datasets list
```

The envelope is `{ ok, message, timestamp_utc, data }` on success; failures also include `error_code`.

## Related

- [Connections](/one/connections/) — the data connections that back imported datasets
- [Alteryx One overview](/one/) — all `ayx one` areas
