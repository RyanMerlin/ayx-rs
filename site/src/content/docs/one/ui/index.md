---
title: Visual interface
description: Experimental commands that target the Alteryx One visual-interface surface — sessions, workflow editing, data, library, schedules, and jobs.
sidebar:
  order: 6
  badge:
    text: Experimental
    variant: caution
---

:::caution[Experimental]
These commands target a visual-interface surface that may change. APIs and behavior are not stable. Use them for exploration and tooling development, not production automation.
:::

`ayx one ui` provides CLI access to the Alteryx One visual interface layer. It covers session lifecycle, the workflow editing surface, data management, the asset library, schedules, and job inspection.

## Quick reference

| Command group | What it covers |
|---|---|
| `ayx one ui session` | Session status, ensure, attach, inventory |
| `ayx one ui workflow` | Open, create, inventory, pane config/results, tool list/select/inspect, graph get/put |
| `ayx one ui data` | List datasets, dataset detail, preview, upload, list connections |
| `ayx one ui library` | Library inventory |
| `ayx one ui schedules` | Schedule inventory |
| `ayx one ui jobs` | Job inventory |

## Sessions

Manage the visual-interface session that backs the UI commands:

```bash
# Check session status
ayx one ui session status

# Ensure a session exists (creates one if needed)
ayx one ui session ensure

# Attach to an existing session
ayx one ui session attach

# Inventory active sessions
ayx one ui session inventory
```

## Workflow surface

Interact with the workflow editor:

```bash
# Open a workflow in the visual surface
ayx one ui workflow open

# Create a workflow
ayx one ui workflow create

# List workflows available in the surface
ayx one ui workflow inventory

# Pane configuration and results
ayx one ui workflow pane-config
ayx one ui workflow pane-results

# Tool operations
ayx one ui workflow tool-list
ayx one ui workflow tool-select
ayx one ui workflow tool-inspect

# Graph access
ayx one ui workflow graph-get
ayx one ui workflow graph-put
```

## Data

```bash
ayx one ui data list-datasets
ayx one ui data dataset-detail
ayx one ui data dataset-preview
ayx one ui data upload
ayx one ui data list-connections
```

## Library, schedules, and jobs

Each of these groups currently exposes an `inventory` command:

```bash
ayx one ui library inventory
ayx one ui schedules inventory
ayx one ui jobs inventory
```

## JSON output

```bash
ayx --output json one ui session status
```

The envelope is `{ ok, message, timestamp_utc, data }`.

## Related

- [Alteryx One overview](/one/) — all `ayx one` areas
- [Safety model](/safety-model/) — dry-run and `--apply`
