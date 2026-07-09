---
title: Upgrade
description: Plan, pre-check, back up, apply, and post-check an Alteryx Server upgrade using ayx server upgrade.
sidebar:
  order: 3
---

`ayx server upgrade` guides you through an Alteryx Server upgrade in discrete, auditable steps. Every step is read-only by default. Nothing executes without `--apply`. Do your planning and pre-checks before you touch any running system.

## Quick reference

| Command | What it does |
|---------|-------------|
| `ayx server upgrade path --from <ver> --to <ver>` | Compute the version upgrade path |
| `ayx server upgrade plan --from <ver> --to <ver>` | Generate a full upgrade plan document |
| `ayx server upgrade precheck --target <ver>` | Run pre-upgrade checks against a target version |
| `ayx server upgrade backup --type <type>` | Take a typed backup as part of the upgrade flow |
| `ayx server upgrade apply --manifest <manifest>` | Apply the upgrade (requires `--apply`) |
| `ayx server upgrade postcheck --manifest <manifest>` | Verify the upgrade result |
| `ayx server upgrade bundle --input <dir> --out <dir>` | Bundle upgrade artifacts for transport |

## Recommended sequence

Work through this order. Each step is a gate — do not skip forward.

### 1. Check the upgrade path

Find out which intermediate versions are required:

```sh
ayx server upgrade path --from 2023.1 --to 2024.2
```

For installations using managed MongoDB, specify the deployment type:

```sh
ayx server upgrade path --from 2023.1 --to 2024.2 --deployment managed-mongo
```

The default is `embedded-mongo`.

### 2. Generate the plan

Produce the upgrade plan document (written to `upgrade-plan/` by default):

```sh
ayx server upgrade plan --from 2023.1 --to 2024.2
```

Override the output directory:

```sh
ayx server upgrade plan --from 2023.1 --to 2024.2 --out /tmp/my-upgrade-plan
```

Review the plan file before proceeding.

### 3. Run the pre-upgrade preflight

The bundled `server.upgrade.preflight` tactic automates steps 1–4 of the manual sequence. It validates config, captures auth posture, takes a Mongo snapshot, and checks that the job queue is empty:

```sh
ayx tactics run server.upgrade.preflight
```

To run the precheck command directly:

```sh
ayx server upgrade precheck --target 2024.2 --profile prod
```

Output goes to `upgrade-precheck/` by default. Override with `--out <dir>`.

### 4. Take a backup

Capture a backup within the upgrade flow (separate from `ayx server backup`):

```sh
# Dry run first — confirm what will be captured
ayx server upgrade backup --type mongo --profile prod

# Commit
ayx server upgrade backup --type mongo --profile prod --apply
```

The `--type` value corresponds to the backup category defined in your upgrade plan. Inspect the plan output to confirm the correct type for your environment.

Output goes to `upgrade-backup/` by default.

### 5. Apply the upgrade

This step is destructive. It will not run without `--apply`.

```sh
ayx server upgrade apply --manifest upgrade-plan/manifest.json --apply --yes
```

`--yes` skips the TTY confirmation prompt. Required in non-interactive (CI/automation) environments.

### 6. Run post-upgrade checks

After the installer completes, verify the outcome:

```sh
ayx server upgrade postcheck --manifest upgrade-plan/manifest.json --profile prod
```

Output goes to `upgrade-postcheck/` by default.

## Bundling artifacts

If you need to transport upgrade artifacts between environments (e.g., from a staging area to an air-gapped server):

```sh
ayx server upgrade bundle --input upgrade-plan/ --out upgrade-bundle.zip
```

## Common flags

| Flag | Default | Notes |
|------|---------|-------|
| `--from <ver>` | (required for `path`, `plan`) | Source version string |
| `--to <ver>` | (required for `path`, `plan`) | Target version string |
| `--target <ver>` | (required for `precheck`) | Target version for compatibility checks |
| `--deployment` | `embedded-mongo` | Set to `managed-mongo` for external MongoDB |
| `--out <dir>` | varies by command | Override the output directory |
| `--profile <name>` | active profile | Named profile — accepted by `precheck`, `backup`, and `postcheck` only (not `plan`, `path`, `apply`, or `bundle`) |
| `--apply` | (off) | Required for `backup apply` and `apply` |
| `--yes` | (off) | Skip TTY confirmation; required in automation |

## JSON output

All commands accept `--output json` as a global flag:

```sh
ayx --output json server upgrade plan --from 2023.1 --to 2024.2
```

## Related

- [Alteryx Server overview](/server/)
- [MongoDB](/server/mongo/)
- [Diagnose & auth](/server/diagnose/)
- [Tactics & workflows](/telemetry/tactics/) — see `server.upgrade.preflight`
