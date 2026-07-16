---
title: Workflows & packages
description: Inspect, validate, unpack, modify, repackage, scan, publish, and migrate Alteryx workflow packages using ayx designer workflow.
sidebar:
  order: 6
---

`ayx designer workflow` provides local tooling for `.yxmd`, `.yxmc`, `.yxzp`, and `.yxdb` files. Most commands operate on local files and do not require a connected profile. Publishing and cloud conversion do require one.

## Quick reference

| Command | What it does |
|---------|-------------|
| `ayx designer workflow inspect --input <file>` | Show metadata and structure of a workflow package |
| `ayx designer workflow validate --input <file>` | Check the package for errors |
| `ayx designer workflow unpack --input <file> --output-dir <dir>` | Extract a `.yxzp` into its component files |
| `ayx designer workflow replace --input <file> --output <file> --find <str> --replace <str>` | Replace a string in a workflow's XML |
| `ayx designer workflow repackage --input-dir <dir> --output <file>` | Re-zip a directory into a `.yxzp` |
| `ayx designer workflow recurse --input <dir> --output <dir>` | Apply rules or replacements across all workflows in a directory tree |
| `ayx designer workflow scan --input <file>` | Scan for credential references, hardcoded paths, or rule violations |
| `ayx designer workflow convert-cloud --input <file> --output <file>` | Convert a Desktop workflow for Alteryx One cloud execution |
| `ayx designer workflow publish --input <file> --workflow-id <id> --name <name> --owner-id <id>` | Publish a workflow to Alteryx One or Alteryx Server |
| `ayx designer workflow migrate --input <file> --output <file> --find <str> --replace <str>` | Combined replace + validate in one step |
| `ayx designer workflow yxdb --input <file>` | Read a `.yxdb` file; export to CSV with `--csv <path>` |

## Inspecting and validating

Check what is inside a workflow package:

```sh
ayx designer workflow inspect --input reports.yxzp
```

Validate the package structure:

```sh
ayx designer workflow validate --input reports.yxzp
```

## Unpacking and repackaging

Unpack a `.yxzp` into its component files for inspection or editing:

```sh
ayx designer workflow unpack --input reports.yxzp --output-dir reports-unpacked/
```

After making changes, repackage:

```sh
ayx designer workflow repackage --input-dir reports-unpacked/ --output reports-patched.yxzp
```

## String replacement and migration

Replace a hardcoded server reference across a single workflow:

```sh
ayx designer workflow replace \
  --input reports.yxmd \
  --output reports-prod.yxmd \
  --find "server=dev-db.internal" \
  --replace "server=prod-db.internal"
```

Add `--validate` to check the output after replacement:

```sh
ayx designer workflow replace \
  --input reports.yxmd \
  --output reports-prod.yxmd \
  --find "server=dev-db.internal" \
  --replace "server=prod-db.internal" \
  --validate
```

Use `migrate` for a combined replace-then-validate in a single command:

```sh
ayx designer workflow migrate \
  --input reports.yxmd \
  --output reports-prod.yxmd \
  --find "server=dev-db.internal" \
  --replace "server=prod-db.internal"
```

## Bulk operations across a directory tree

Apply a replacement to every workflow under a directory:

```sh
ayx designer workflow recurse \
  --input workflows-dev/ \
  --output workflows-prod/ \
  --find "server=dev-db.internal" \
  --replace "server=prod-db.internal" \
  --validate
```

Pass `--rules <file>` to apply a YAML rule set instead of a single find/replace pair.

## Scanning

Scan a workflow for hardcoded credentials, file paths, or rule violations:

```sh
ayx designer workflow scan --input reports.yxzp
```

Apply a custom rule set:

```sh
ayx designer workflow scan --input reports.yxzp --rules /etc/ayx/scan-rules.yaml
```

## Cloud conversion

Convert a Desktop-authored `.yxmd` for execution in Alteryx One:

```sh
ayx designer workflow convert-cloud \
  --input reports.yxmd \
  --output reports-cloud.yxmd
```

To fail immediately on any unsupported tool (rather than warn):

```sh
ayx designer workflow convert-cloud \
  --input reports.yxmd \
  --output reports-cloud.yxmd \
  --fail-on-unsupported
```

The bundled action for bulk cloud conversion is `workflow.cloud-convert.bulk`:

```sh
ayx actions run workflow.cloud-convert.bulk
```

## Publishing

Publish a workflow to Alteryx One or Alteryx Server. Requires a connected profile.

Dry run (default — no changes sent):

```sh
ayx designer workflow publish \
  --profile prod \
  --input reports.yxzp \
  --workflow-id abc123 \
  --name "Monthly Reports" \
  --owner-id user456
```

Commit the publish:

```sh
ayx designer workflow publish \
  --profile prod \
  --input reports.yxzp \
  --workflow-id abc123 \
  --name "Monthly Reports" \
  --owner-id user456 \
  --apply
```

Optional publish flags:

| Flag | Notes |
|------|-------|
| `--make-published` | Mark the workflow as published on the server |
| `--others-may-download` | Allow other users to download |
| `--others-can-execute` | Allow other users to run |
| `--execution-mode <mode>` | Default: `Standard` |
| `--workflow-credential-type <type>` | Default: `Default` |
| `--comments <text>` | Version comment |
| `--bypass-workflow-version-check` | Skip the version compatibility check |

## Reading .yxdb files

Inspect a `.yxdb` record store:

```sh
ayx designer workflow yxdb --input output.yxdb
```

Export to CSV:

```sh
ayx designer workflow yxdb --input output.yxdb --csv output.csv
```

For machine-readable output, use the global `--output json` flag:

```sh
ayx --output json designer workflow yxdb --input output.yxdb
```

## JSON output

All commands accept `--output json` as a global flag:

```sh
ayx --output json designer workflow inspect --input reports.yxzp
```

## Related

- [Alteryx Server overview](/server/)
- [Telemetry](/telemetry/)
- [Actions & workflows](/telemetry/actions/) — see `workflow.cloud-convert.bulk`
