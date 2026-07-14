---
title: Output & automation
description: Machine-readable output, dry-runs, and wiring ayx into scripts and agents.
sidebar:
  order: 5
---

ayx is built to be driven by scripts and agents, not just typed at a prompt. Two things make that work: a uniform JSON envelope and a dry-run-by-default safety gate.

## JSON output

Add `--output json` for machine-readable output. It's a global flag, so it can appear before or after the subcommand:

```bash
ayx --output json one flows list
```

Every JSON response uses the same envelope:

```json
{
  "ok": true,
  "message": "…",
  "timestamp_utc": "…",
  "data": {}
}
```

Branch on `ok`, read `data` for the result. Failures use the same shape with `ok: false`, add an `error_code`, and are written to stderr instead of stdout.

## Dry-run by default

Mutating commands print what they *would* do and exit `0` unless you pass `--apply`. A pipeline can safely run the dry-run form against production:

```bash
# Preview — no changes; capture the planned request
ayx --output json one flows delete <id>

# Commit, non-interactively
ayx one flows delete <id> --apply --yes
```

`--yes` skips the confirmation prompt that destructive commands show in a terminal — required for CI and pipes.

## Exit codes

- `0` — success, or a dry-run that completed
- non-zero — the command failed; read `message` (and `data`) in the envelope for why

## Agent-friendly by design

Because every command speaks the same JSON envelope and gates destructive actions behind an explicit flag, ayx makes a clean tool surface for AI agents: predictable output to parse, and a safety rail so an agent can't change remote state without an explicit `--apply`.
