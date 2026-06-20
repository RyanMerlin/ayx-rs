---
title: Getting Started
description: Install ayx, create your first profile, and verify connectivity.
sidebar:
  order: 1
---

## Install

macOS / Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.sh | bash
```

Windows (PowerShell):

```powershell
iwr https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.ps1 | iex
```

The install script downloads the latest release binary and verifies its SHA-256 checksum. On macOS/Linux it installs `ayx` to a user bin directory and appends that directory to `~/.profile`, so you may need to open a new shell (or add the directory to `PATH` for the current session) before `ayx` resolves. The Windows installer updates your user `PATH` directly.

## Onboard

Create a central profile with the onboarding wizard:

```bash
ayx onboard
```

Confirm the active profile and check connectivity:

```bash
ayx profile current
ayx doctor
```

`ayx doctor` validates configuration, authentication, and connectivity without touching remote state.

To inspect the Alteryx One workspace the active profile points at, ask for JSON. `--output` is a global flag, so it comes **before** the subcommand:

```bash
ayx --output json one platform workspace current
```

## Next steps

- [Configuration](/configuration/) — profile shape, environment files, and environment-variable overrides
- [Command surface](/reference/command-surface/) — the full command inventory with safety annotations
- [Safety model](/safety-model/) — how read-only and mutating commands differ
