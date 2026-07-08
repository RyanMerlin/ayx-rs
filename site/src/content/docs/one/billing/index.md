---
title: Billing
description: Retrieve current account billing information and export usage data from Alteryx One.
sidebar:
  order: 4
---

`ayx one billing` provides read-only access to billing and consumption data for your Alteryx One account. Use it to retrieve current account details or export usage records for cost tracking and chargebacks.

> **Enterprise tier required.** Billing endpoints return 404 on some workspace tiers. Commands are present in all builds but will only succeed on enterprise-tier accounts.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one billing current-account` | Fetch current account billing information |
| `ayx one billing usage-export` | Export usage data for the account |
| `ayx one doctor billing` | Run a billing health check |

## Current account

Retrieve the billing status and plan details for the active account:

```bash
ayx one billing current-account
```

Add `--profile <name>` to target a non-default workspace:

```bash
ayx one billing current-account --profile prod
```

## Usage export

Export usage records. Useful for cost reporting, chargebacks, or tracking consumption trends:

```bash
ayx one billing usage-export
```

## Billing health check

`ayx one doctor billing` runs a targeted check against the billing API. Use it to confirm the billing surface is reachable and returning expected data:

```bash
ayx one doctor billing
```

See [Diagnostics](/one/diagnostics/) for the full doctor command set.

## JSON output

Pass `--output json` before the subcommand for structured output:

```bash
ayx --output json one billing current-account
ayx --output json one billing usage-export
```

The envelope is `{ ok, message, timestamp_utc, data }`.

## Related

- [Diagnostics](/one/diagnostics/) — full health check suite, including `doctor billing`
- [Alteryx One overview](/one/) — all `ayx one` areas
