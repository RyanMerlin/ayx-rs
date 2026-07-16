---
title: Actions & workflows
description: Use ayx actions and ayx actions workflows to run named playbooks and higher-order automation against your Alteryx environment.
sidebar:
  order: 1
---

Actions are named, safety-classified playbooks. Each action documents its steps, validation checks, and rollback notes. Workflows compose multiple actions into an ordered chain. Both respect `--apply` — mutating and destructive actions emit a structured plan and do nothing without it.

## Quick reference

### ayx actions

| Command | What it does |
|---------|-------------|
| `ayx actions list` | List all actions with title, safety, and tags |
| `ayx actions describe <id>` | Show steps, validations, and rollback for an action |
| `ayx actions resolve --task <text>` | Match free-text to ranked candidate actions |
| `ayx actions run <id>` | Execute an action (dry-run by default; `--apply` to commit) |
| `ayx actions validate` | Cross-check all actions against the catalog |
| `ayx actions export <id>` | Print an action's YAML for forking into your config |

### ayx actions workflows

| Command | What it does |
|---------|-------------|
| `ayx actions workflows list` | List all workflows with title, safety, and action count |
| `ayx actions workflows explain <id>` | Show a workflow's ordered actions with summaries |
| `ayx actions workflows run <id>` | Execute a workflow chain (dry-run by default; `--apply` to commit) |

## Bundled actions

Run `ayx actions list` to see the full list. As of v0.11.0:

| ID | Title | Safety | Tags |
|----|-------|--------|------|
| `mongo.backup-restore` | Back up and restore Alteryx Server Mongo | mutating | mongo, backup, server |
| `mongo.doctor` | Diagnose Alteryx Mongo health | read_only | mongo, diagnose, support |
| `mongo.queue.stuck` | Diagnose a stuck Alteryx Server job queue | read_only | mongo, queue, diagnose, support, incident |
| `one.flow.promote` | Promote a single Alteryx One flow between environments | mutating | one, flow, promotion, release |
| `one.scheduling.pause-all` | Pause every Alteryx One schedule for a maintenance window | mutating | one, scheduling, change-management |
| `one.workspace-migrate` | Migrate Alteryx One assets between workspaces | destructive | one, workspace, migration, governance |
| `server.auth.saml-diagnose` | Diagnose Alteryx Server SAML authentication | read_only | server, auth, saml, diagnose |
| `server.logs.triage` | Triage an Alteryx Server incident from logs | read_only | server, logs, diagnose, incident |
| `server.upgrade.preflight` | Pre-upgrade preflight for Alteryx Server | mutating | server, upgrade, change-management, safety |
| `workflow.cloud-convert.bulk` | Bulk-convert Desktop workflows for Alteryx One | read_only | workflow, migration, cloud |

## Bundled workflows

| ID | Title | Safety | Action count |
|----|-------|--------|-------------|
| `governance.go-live` | Governance go-live for a new Alteryx One environment | destructive | 4 |
| `ops.backup-restore` | Pre-change backup + post-change verification | mutating | 2 |

## Using actions

### Discover

```sh
ayx actions list
```

Filter by tag with `--output json` and a shell filter:

```sh
ayx --output json actions list | jq '.data[] | select(.tags[] == "upgrade")'
```

### Inspect before running

Always read an action before running it the first time:

```sh
ayx actions describe server.upgrade.preflight
```

This shows every step, the `why` behind each one, validation checks, and rollback instructions.

### Resolve a task to an action

Not sure which action to use? Describe what you need:

```sh
ayx actions resolve --task "back up mongo before upgrade"
```

Returns a ranked list of candidates.

### Run an action

Read-only actions always execute. Mutating and destructive actions require `--apply`.

```sh
# Dry run — see the plan
ayx actions run server.upgrade.preflight

# Commit
ayx actions run server.upgrade.preflight --apply
```

Skip the TTY confirmation in automation:

```sh
ayx actions run server.upgrade.preflight --apply --yes
```

## Using workflows

### Inspect a workflow

```sh
ayx actions workflows explain ops.backup-restore
```

Shows the ordered actions with summaries.

### Run a workflow

Workflows execute their actions in order. The same `--apply` semantics apply.

```sh
# Dry run
ayx actions workflows run ops.backup-restore

# Commit
ayx actions workflows run ops.backup-restore --apply --yes
```

## Forking and customising

Export a bundled action's YAML to your config home for local overrides:

```sh
ayx actions export mongo.backup-restore > "${AYX_CONFIG_HOME}/registry/mongo.backup-restore.action.yaml"
```

Edit the YAML, then validate all actions (including yours) against the catalog:

```sh
ayx actions validate
```

## Safety classifications

| Level | Meaning |
|-------|---------|
| `read_only` | No writes. Always runs. |
| `mutating` | Makes changes. Requires `--apply`. |
| `destructive` | Irreversible changes. Requires `--apply` + `--yes`. |

## JSON output

All commands accept `--output json` as a global flag:

```sh
ayx --output json actions list
ayx --output json actions workflows list
```

## Related

- [Telemetry](/telemetry/)
- [Alteryx Server overview](/server/)
- [Upgrade](/server/upgrade/)
- [MongoDB](/server/mongo/)
- [Safety model](/safety-model/)
