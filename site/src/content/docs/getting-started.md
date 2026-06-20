---
title: Getting started
description: Install ayx, connect it to your Alteryx One workspace, and run your first command in a couple of minutes.
sidebar:
  order: 1
---

`ayx` is a single binary. Install it, point it at your Alteryx One workspace, and you're running commands in a couple of minutes.

## Install

**macOS / Linux**

```bash
curl -fsSL https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.sh | bash
```

**Windows (PowerShell)**

```powershell
iwr https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.ps1 | iex
```

The installer downloads the latest release, verifies its SHA-256 checksum, and puts the `ayx` binary on your PATH. On macOS and Linux it adds the install directory to `~/.profile`, so open a new terminal (or run the `export PATH=…` line it prints) before your first command.

Confirm it's installed:

```bash
ayx --version
```

## Connect

Run the setup wizard. It creates a **profile** — your saved connection to an Alteryx One workspace — and validates each piece as you enter it.

```bash
ayx onboard
```

You'll need your workspace URL and an OAuth client and token. [Profiles & configuration](/configuration/) covers exactly which fields go where.

Then check the whole path — config, auth, and connectivity — without changing anything on the server:

```bash
ayx doctor
```

## Your first command

Confirm who you are and which workspace you're pointed at:

```bash
ayx whoami
```

Want machine-readable output? Ask for JSON. One thing to know up front: `--output` is a top-level flag, so it goes **before** the command, not after.

```bash
ayx --output json one platform workspace current
```

## Where to go next

- **[Common tasks](/common-tasks/)** — copy-paste recipes for the things you'll do most
- **[The safety model](/safety-model/)** — why nothing destructive runs without `--apply`
- **[Profiles & configuration](/configuration/)** — multiple environments, tokens, and per-run overrides
