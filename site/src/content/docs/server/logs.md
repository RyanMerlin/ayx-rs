---
title: Logs & diagnostics
description: Use ayx server server-logs to discover, inventory, tail, parse, and query Alteryx Server log files.
sidebar:
  order: 1
---

`ayx server server-logs` gives you structured access to Alteryx Server log files — without manually navigating the Windows filesystem or grepping raw text. All commands here are read-only.

## Quick reference

| Command | What it does |
|---------|-------------|
| `ayx server server-logs discover` | Find all log files on the configured server |
| `ayx server server-logs inventory` | List log files with sizes and timestamps |
| `ayx server server-logs summary --path <path>` | Summarise a log file's error/warning counts |
| `ayx server server-logs context --path <path> --query <query>` | Show lines around a matched query string |
| `ayx server server-logs parse-csv --path <path>` | Parse a CSV-format log file into structured output |
| `ayx server server-logs service-events --path <path>` | Extract structured service events |
| `ayx server server-logs gallery-events --path <path>` | Extract Gallery-specific events |
| `ayx server server-logs tail --path <path>` | Show the last N lines of a log file (default: 100) |
| `ayx server server-logs recent` | List log files modified in the last N days (default: 7) |

## Discovering log files

Start here when you do not know where the logs are:

```sh
ayx server server-logs discover --profile prod
```

Then get a full inventory with file sizes and modified timestamps:

```sh
ayx --output json server server-logs inventory --profile prod
```

## Tailing and recent files

See the tail of a specific file:

```sh
ayx server server-logs tail --path "C:\ProgramData\Alteryx\Logs\AlteryxService.log"
```

Override the default 100 lines:

```sh
ayx server server-logs tail --path "C:\ProgramData\Alteryx\Logs\AlteryxService.log" --lines 250
```

List all log files touched in the last three days:

```sh
ayx server server-logs recent --profile prod --days 3
```

## Summarising and searching

Get error and warning counts for a file:

```sh
ayx server server-logs summary --path "C:\ProgramData\Alteryx\Logs\AlteryxService.log"
```

Search for a keyword and see surrounding context (25 lines before and after by default):

```sh
ayx server server-logs context \
  --path "C:\ProgramData\Alteryx\Logs\AlteryxService.log" \
  --query "OutOfMemory"
```

Adjust the context window:

```sh
ayx server server-logs context \
  --path "C:\ProgramData\Alteryx\Logs\AlteryxService.log" \
  --query "connection refused" \
  --before 10 \
  --after 10
```

## Parsing structured log formats

Alteryx Server produces some logs in CSV format. Parse them into a clean JSON envelope:

```sh
ayx --output json server server-logs parse-csv \
  --path "C:\ProgramData\Alteryx\Logs\AlteryxGallery.csv"
```

Note: `--output json` is a root flag and must come before the subcommand.

## Service and Gallery events

Extract structured service lifecycle events:

```sh
ayx server server-logs service-events \
  --path "C:\ProgramData\Alteryx\Logs\AlteryxService.log"
```

Extract Gallery API events (authentication, job submissions, errors):

```sh
ayx server server-logs gallery-events \
  --path "C:\ProgramData\Alteryx\Logs\AlteryxGallery.log"
```

## Machine-readable output

Every command in this branch accepts `--output json` as a root flag:

```sh
ayx --output json server server-logs summary \
  --path "C:\ProgramData\Alteryx\Logs\AlteryxService.log"
```

The envelope is always `{ ok, message, timestamp_utc, data }`.

## Related

- [Alteryx Server overview](/server/)
- [Diagnose & auth](/server/diagnose/)
- [Tactics — server.logs.triage](/telemetry/tactics/)
