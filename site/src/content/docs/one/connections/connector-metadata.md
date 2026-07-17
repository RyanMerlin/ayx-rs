---
title: Connector metadata
description: Inspect connector defaults, publish information, and manage per-environment overrides.
sidebar:
  order: 2
---

Connector metadata describes how Alteryx One handles a specific connector type: its default field values, publish configuration, and any environment-level overrides you have applied. Mutating commands are dry-run by default — add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one connections connector-metadata defaults` | Inspect default field values for a connector type |
| `ayx one connections connector-metadata detail` | Inspect the full metadata record for a connector |
| `ayx one connections connector-metadata publish-info` | Inspect publish configuration for a connector |
| `ayx one connections connector-metadata overrides list` | List active overrides for a connector |
| `ayx one connections connector-metadata overrides create` | Create overrides from a JSON payload |
| `ayx one connections connector-metadata overrides delete` | Delete overrides for a connector |
| `ayx one connections connector-metadata template` | Generate a fillable JSON create-body template for a connector |

All commands require `<connector-type>` as the connector positional argument.

> **No connector enumeration in v4.** There is no `/v4/connectors` endpoint — connector slugs cannot be listed via the API. Known working slugs verified against the live API: `gsheetsuser`, `remotefile`, `bigquery`. You must know the slug in advance to use these commands.

## Inspecting connector metadata

```bash
# Default field schema for a connector type
ayx one connections connector-metadata defaults <connector>

# Full metadata record
ayx one connections connector-metadata detail <connector>

# Publish configuration (output targets, supported modes)
ayx one connections connector-metadata publish-info <connector>

# Scoped to a specific Alteryx One profile
ayx one connections connector-metadata detail <connector> --profile <profile-id>
```

`defaults` is useful when building a create payload — it shows you which fields the connector expects and what their default values are. `publish-info` tells you how results from workflows using this connector can be published.

## Managing overrides

Overrides let you change connector metadata at the environment level without touching the connector type itself. They apply to every connection of that type in the profile.

### List overrides

```bash
ayx one connections connector-metadata overrides list <connector>

ayx one connections connector-metadata overrides list <connector> --output json
```

### Create overrides

```bash
# Dry-run
ayx one connections connector-metadata overrides create \
  <connector> \
  --body '{"fieldName":"value"}'

# Commit
ayx one connections connector-metadata overrides create \
  <connector> \
  --body '{"fieldName":"value"}' \
  --apply
```

### Delete overrides

```bash
# Dry-run
ayx one connections connector-metadata overrides delete <connector>

# Commit
ayx one connections connector-metadata overrides delete <connector> --apply --yes
```

Deleting overrides reverts the connector to its platform defaults.

## Generating a create-body template

`template` derives a fillable JSON skeleton for `connections create` directly from the connector's metadata. This is the recommended starting point for building a new connection body — it removes the guesswork about required fields and correct values.

```bash
# Generate the template and write it to a file
ayx one connections connector-metadata template bigquery --output json > body.json

# Edit body.json to fill in your values, then create the connection
ayx one connections create --body "$(cat body.json)" --apply
```

The command derives each field from the connector metadata:

- `type` — `jdbc` for relational connectors, `remotefile` for others; when the connector type cannot be confidently inferred, the field emits a `<jdbc|remotefile|…>` placeholder
- `vendor` / `vendorName` — taken from the connector slug
- `credentialType` — taken from the metadata (e.g. `apiKey`, `oauth2`)
- `params` — a skeleton of the connector-specific parameter fields

When the connector type is ambiguous, the template also adds a `_note` field explaining why a placeholder was used and what values are valid. Replace the placeholder before passing the body to `connections create`.

Example derivations:

| Connector | type | credentialType | params skeleton |
|---|---|---|---|
| `bigquery` | `jdbc` | `apiKey` | `{ "projectId": "" }` |
| `gsheetsuser` | `remotefile` | `oauth2` | connector-specific fields |
| unknown type | `<jdbc\|remotefile\|…>` | from metadata | `_note` field included |

Pipe the output to a file and pass it to `connections create --body <file>`. Use `connector-metadata defaults` alongside `template` to verify expected field values before submitting.

## Automation patterns

Dump all metadata for a connector to a file for auditing:

```bash
ayx one connections connector-metadata detail <connector> --output json \
  | jq '.data' > connector-<connector>-metadata.json
```

Compare defaults against active overrides to detect drift:

```bash
ayx one connections connector-metadata defaults <connector> --output json | jq '.data' > defaults.json
ayx one connections connector-metadata overrides list <connector> --output json | jq '.data' > overrides.json
diff defaults.json overrides.json
```

## Related

- [Connections](/one/connections/) — create and manage connection records
- [Connection permissions](/one/connections/permissions/) — access control for connections
- [Safety model](/safety-model/) — how dry-run and `--apply` work
