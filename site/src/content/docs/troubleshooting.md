---
title: Troubleshooting
description: Start with doctor, find the right reference surface, and trust the binary.
sidebar:
  order: 4
---

## Start with doctor

`ayx doctor` validates configuration, auth, and network connectivity without touching remote state:

```bash
ayx doctor
ayx one doctor discover
```

## Reference surfaces

| Problem | Go to |
|---------|-------|
| Command not found or unexpected behavior | [Command surface](/reference/command-surface/) |
| Config resolution order or profile shape | [Runtime config contract](/reference/runtime-config-contract/) |
| CLI flags and the stable behavior contract | [CLI spec](/reference/cli-spec/) |

## When the site and your binary disagree

Trust the binary and the checked-in release notes first. The command surface is generated from the CLI's command tree and **checked for staleness in CI** — it is not rewritten automatically. If it drifts, refresh it locally:

```bash
cargo run -q -p xtask -- refresh-command-surface
```

Check which release you are on with `ayx --version`, and compare against the [release notes](/releases/).
