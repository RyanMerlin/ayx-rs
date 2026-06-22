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

All commands require `--connector <connector-type>`.

## Inspecting connector metadata

```bash
# Default field schema for a connector type
ayx one connections connector-metadata defaults --connector <connector>

# Full metadata record
ayx one connections connector-metadata detail --connector <connector>

# Publish configuration (output targets, supported modes)
ayx one connections connector-metadata publish-info --connector <connector>

# Scoped to a specific Alteryx One profile
ayx one connections connector-metadata detail --connector <connector> --profile <profile-id>
```

`defaults` is useful when building a create payload — it shows you which fields the connector expects and what their default values are. `publish-info` tells you how results from workflows using this connector can be published.

## Managing overrides

Overrides let you change connector metadata at the environment level without touching the connector type itself. They apply to every connection of that type in the profile.

### List overrides

```bash
ayx one connections connector-metadata overrides list --connector <connector>

ayx --output json one connections connector-metadata overrides list --connector <connector>
```

### Create overrides

```bash
# Dry-run
ayx one connections connector-metadata overrides create \
  --connector <connector> \
  --body '{"fieldName":"value"}'

# Commit
ayx one connections connector-metadata overrides create \
  --connector <connector> \
  --body '{"fieldName":"value"}' \
  --apply
```

### Delete overrides

```bash
# Dry-run
ayx one connections connector-metadata overrides delete --connector <connector>

# Commit
ayx one connections connector-metadata overrides delete --connector <connector> --apply --yes
```

Deleting overrides reverts the connector to its platform defaults.

## Automation patterns

Dump all metadata for a connector to a file for auditing:

```bash
ayx --output json one connections connector-metadata detail --connector <connector> \
  | jq '.data' > connector-<connector>-metadata.json
```

Compare defaults against active overrides to detect drift:

```bash
ayx --output json one connections connector-metadata defaults --connector <connector> | jq '.data' > defaults.json
ayx --output json one connections connector-metadata overrides list --connector <connector> | jq '.data' > overrides.json
diff defaults.json overrides.json
```

## Related

- [Connections](/one/connections/) — create and manage connection records
- [Connection permissions](/one/connections/permissions/) — access control for connections
- [Safety model](/safety-model/) — how dry-run and `--apply` work
