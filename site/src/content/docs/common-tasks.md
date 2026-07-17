---
title: Common tasks
description: Copy-paste recipes for the things you'll do most with ayx — each one verified against the CLI.
sidebar:
  order: 6
---

Recipes for everyday work. Read-only commands are safe to run anytime; anything that changes your server is dry-run by default and only commits when you add `--apply`.

## Check that everything's connected

```bash
ayx doctor
```

Runs config, auth, network, and product checks — without touching remote state.

## See your active profile and workspace

```bash
ayx whoami
```

Active profile, account email, workspace, and environment, in one shot.

## List flows

```bash
ayx one flows list --all --output json
```

`--all` pages through every result. Leave it off for just the first page.

## Inspect a flow before you touch it

```bash
ayx one flows detail <id> --output json
```

Read-only — returns the flow's metadata, parameters, and permissions.

## Delete a flow, safely

```bash
# Dry-run: shows what would happen, deletes nothing
ayx one flows delete <id>

# Commit it
ayx one flows delete <id> --apply
```

Add `--yes` to skip the confirmation prompt in scripts.

## Back up MongoDB

```bash
# Dry-run
ayx mongo backup

# Run it
ayx mongo backup --output-dir /var/backups/ayx --apply
```

With `--apply`, ayx records an audit artifact under `${AYX_CONFIG_HOME}/audits/`.

## Run one command against a different environment

```bash
ayx one flows list --profile staging --output json
```

`--profile` switches profile for a single run. To change your default for good, use `ayx profile use <name>`.

## Get a telemetry overview

```bash
ayx telemetry summary --since 7d --top 10 --output json
```

Running jobs, recent history, top workflows, and errors — in one envelope.

## Plan an Alteryx Server upgrade

```bash
ayx server upgrade plan --from 2023.1 --to 2024.1
```

Read-only. Writes a structured upgrade plan you can review before you act.
