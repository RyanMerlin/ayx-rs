---
title: Import & export
description: Move flows between Alteryx One environments using import and export commands.
sidebar:
  order: 3
---

Import and export let you move flows between Alteryx One environments — for example, promoting a flow from a development workspace to production. The binary format is the same package the Alteryx One UI produces, so the CLI and the UI are interoperable.

Mutating commands are dry-run by default — add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one flows export` | Export a flow to a local file |
| `ayx one flows export-dry-run` | Preview what export would produce |
| `ayx one flows import` | Import a flow package into the workspace |
| `ayx one flows import-dry-run` | Preview an import without applying it |

## Export

### Export a flow to a file

```bash
# Dry-run — shows export metadata, writes nothing
ayx one flows export-dry-run --flow-id <flow-id>

# Write the package to disk
ayx one flows export --flow-id <flow-id> --output-file <path/to/file> --apply
```

`--output-file` is required for `export` and specifies the local file path to write. The resulting file can be committed to version control or passed directly to `import` on a target environment.

> **Note:** The file path flag is `--output-file`, not `--output`. The global `--output` flag (placed before the subcommand, e.g. `ayx --output json`) is reserved for selecting the text/json output format and is a separate argument.

## Import

### Preview an import

```bash
ayx one flows import-dry-run --input <path/to/file>
ayx one flows import-dry-run --input <path/to/file> --folder-id <folder-id>
```

`import-dry-run` sends the package to the server for validation and returns what would happen, without committing anything. Run this before every import to catch conflicts early.

### Import a flow

```bash
# Basic import into the default location
ayx one flows import --input <path/to/file> --apply

# Import into a specific folder
ayx one flows import --input <path/to/file> --folder-id <folder-id> --apply

# Override JavaScript UDFs on conflict
ayx one flows import --input <path/to/file> --override-js-udfs --apply

# Skip the confirmation prompt (CI / scripts)
ayx one flows import --input <path/to/file> --apply --yes
```

`--from-ui` is available on both `import` and `import-dry-run` for packages produced by the Alteryx One web UI rather than by `ayx one flows export`. You generally do not need it when round-tripping through the CLI.

## Promote a flow between environments

This pattern exports from one environment and imports into another using `--profile` to target each:

```bash
# 1. Export from dev
ayx one flows export \
  --profile dev \
  --flow-id <flow-id> \
  --output-file /tmp/my-flow.yxzp \
  --apply

# 2. Preview the import on prod
ayx one flows import-dry-run \
  --profile prod \
  --input /tmp/my-flow.yxzp \
  --folder-id <prod-folder-id>

# 3. Commit
ayx one flows import \
  --profile prod \
  --input /tmp/my-flow.yxzp \
  --folder-id <prod-folder-id> \
  --apply --yes
```

## Automation patterns

### Export multiple flows from a list of IDs

```bash
while IFS= read -r id; do
  ayx one flows export --flow-id "$id" --output-file "./exports/${id}.yxzp" --apply
done < flow-ids.txt
```

### Validate before importing in CI

```bash
ayx --output json one flows import-dry-run --input my-flow.yxzp \
  | jq -e '.ok'
# Exits non-zero if the dry-run reports a problem
```

## Related

- [Flows](/one/flows/)
- [Flow folders](/one/flows/folders/)
- [Plans import & export](/one/plans/import-export/)
- [Safety model](/safety-model/)
- [Output & automation](/output-automation/)
