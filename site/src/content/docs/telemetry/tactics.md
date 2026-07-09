---
title: Tactics & workflows
description: Use ayx tactics and ayx workflows to run named playbooks and higher-order automation against your Alteryx environment.
sidebar:
  order: 1
---

Tactics are named, safety-classified playbooks. Each tactic documents its steps, validation checks, and rollback notes. Workflows compose multiple tactics into an ordered chain. Both respect `--apply` — mutating and destructive tactics emit a structured plan and do nothing without it.

## Quick reference

### ayx tactics

| Command | What it does |
|---------|-------------|
| `ayx tactics list` | List all tactics with title, safety, and tags |
| `ayx tactics describe <id>` | Show steps, validations, and rollback for a tactic |
| `ayx tactics resolve --task <text>` | Match free-text to ranked candidate tactics |
| `ayx tactics run <id>` | Execute a tactic (dry-run by default; `--apply` to commit) |
| `ayx tactics validate` | Cross-check all tactics against the catalog |
| `ayx tactics export <id>` | Print a tactic's YAML for forking into your config |

### ayx workflows

| Command | What it does |
|---------|-------------|
| `ayx workflows list` | List all workflows with title, safety, and tactic count |
| `ayx workflows explain <id>` | Show a workflow's ordered tactics with summaries |
| `ayx workflows run <id>` | Execute a workflow chain (dry-run by default; `--apply` to commit) |

## Bundled tactics

Run `ayx tactics list` to see the full list. As of v0.11.0:

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

| ID | Title | Safety | Tactic count |
|----|-------|--------|-------------|
| `governance.go-live` | Governance go-live for a new Alteryx One environment | destructive | 4 |
| `ops.backup-restore` | Pre-change backup + post-change verification | mutating | 2 |

## Using tactics

### Discover

```sh
ayx tactics list
```

Filter by tag with `--output json` and a shell filter:

```sh
ayx --output json tactics list | jq '.data[] | select(.tags[] == "upgrade")'
```

### Inspect before running

Always read a tactic before running it the first time:

```sh
ayx tactics describe server.upgrade.preflight
```

This shows every step, the `why` behind each one, validation checks, and rollback instructions.

### Resolve a task to a tactic

Not sure which tactic to use? Describe what you need:

```sh
ayx tactics resolve --task "back up mongo before upgrade"
```

Returns a ranked list of candidates.

### Run a tactic

Read-only tactics always execute. Mutating and destructive tactics require `--apply`.

```sh
# Dry run — see the plan
ayx tactics run server.upgrade.preflight

# Commit
ayx tactics run server.upgrade.preflight --apply
```

Skip the TTY confirmation in automation:

```sh
ayx tactics run server.upgrade.preflight --apply --yes
```

## Using workflows

### Inspect a workflow

```sh
ayx workflows explain ops.backup-restore
```

Shows the ordered tactics with summaries.

### Run a workflow

Workflows execute their tactics in order. The same `--apply` semantics apply.

```sh
# Dry run
ayx workflows run ops.backup-restore

# Commit
ayx workflows run ops.backup-restore --apply --yes
```

## Forking and customising

Export a bundled tactic's YAML to your config home for local overrides:

```sh
ayx tactics export mongo.backup-restore > "${AYX_CONFIG_HOME}/registry/mongo.backup-restore.tactic.yaml"
```

Edit the YAML, then validate all tactics (including yours) against the catalog:

```sh
ayx tactics validate
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
ayx --output json tactics list
ayx --output json workflows list
```

## Related

- [Telemetry](/telemetry/)
- [Alteryx Server overview](/server/)
- [Upgrade](/server/upgrade/)
- [MongoDB](/server/mongo/)
- [Safety model](/safety-model/)
