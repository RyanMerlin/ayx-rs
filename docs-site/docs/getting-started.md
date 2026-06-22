---
title: Getting Started
sidebar_position: 2
---

# Getting started

## Install

Use the platform install scripts for the fastest path:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.sh | bash
```

```powershell
# Windows
iwr https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.ps1 | iex
```

Both scripts download the latest release binary, verify its SHA-256 checksum, and place `ayx` on your `PATH`.

## Onboard

Run the onboarding wizard to create a central profile:

```bash
ayx onboard
```

Then verify the active profile and connectivity:

```bash
ayx profile current
ayx doctor
ayx one platform workspace current --output json
```

## Next steps

- [Configuration](./configuration) — profile shape, environment files, env overrides
- [Command surface](./reference/command-surface) — full command inventory with safety annotations
- [Safety model](./safety-model) — how read-only vs. mutating commands work
