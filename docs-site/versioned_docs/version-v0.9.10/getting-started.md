---
title: Getting Started
sidebar_position: 2
---

# Getting started

Use the install scripts for the fastest path, then run `ayx onboard` to create a central profile.

```bash
curl -fsSL https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.sh | bash
```

```powershell
iwr https://raw.githubusercontent.com/RyanMerlin/ayx-rs/main/scripts/install.ps1 | iex
```

Then validate the active profile:

```powershell
ayx onboard
ayx profile current
ayx one platform workspace current --output json
```
