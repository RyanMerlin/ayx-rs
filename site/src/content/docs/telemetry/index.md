---
title: Telemetry
description: Query running jobs, run history, workflow performance, errors, queue depth, and access permissions using ayx telemetry.
sidebar:
  order: 0
---

`ayx telemetry` surfaces operational data about your Alteryx One or Alteryx Server environment — what is running, what failed, how long things take, who has access to what. All commands are read-only. Backend is auto-detected from your profile; use `--source one` or `--source server` to override.

## Quick reference

| Command | What it does |
|---------|-------------|
| `ayx telemetry summary` | One-shot overview across all telemetry surfaces |
| `ayx telemetry jobs running` | Jobs currently in Running or Queued state |
| `ayx telemetry jobs history` | Recent job history (succeeded, failed, cancelled) |
| `ayx telemetry jobs top` | Top flows by run count |
| `ayx telemetry workflows top` | Top workflows by run count, failure rate, or duration |
| `ayx telemetry workflows performance` | Per-workflow duration percentiles (p50/p95/p99) |
| `ayx telemetry workflows errors` | Workflows ordered by failure count |
| `ayx telemetry plans top` | Top plans by run count |
| `ayx telemetry plans performance` | Per-plan duration percentiles |
| `ayx telemetry errors recent` | Recent failed job groups with error messages |
| `ayx telemetry weekly run-counts` | 168-bucket run-count matrix (7 days x 24 hours) |
| `ayx telemetry queue status` | Currently running and queued jobs (Server) |
| `ayx telemetry queue wait-time` | Wait-time stats for recent queue entries (Server) |
| `ayx telemetry permissions connections` | DCM connections and who has access |
| `ayx telemetry permissions workflows` | Who has workflow access |
| `ayx telemetry permissions collections` | Collection/Gallery ACLs (Server only) |
| `ayx telemetry permissions summary` | Access count rollup across subjects |

## Getting a quick overview

Start here. The summary pulls from all surfaces and returns a single envelope:

```sh
ayx telemetry summary --profile prod
```

Control the time window and result cap:

```sh
ayx telemetry summary --profile prod --since 24h --top 5
```

`--since` accepts `<N>h`, `<N>d`, or `<N>w`. Default is `7d`.

## Jobs

See what is running right now:

```sh
ayx telemetry jobs running --profile prod
```

Review the last 24 hours of job history:

```sh
ayx telemetry jobs history --profile prod --since 24h
```

Find the most-run flows over the past week:

```sh
ayx telemetry jobs top --profile prod --since 7d --top 10
```

## Workflow performance

Top workflows by run count:

```sh
ayx telemetry workflows top --profile prod --since 7d
```

Identify slow workflows using duration percentiles:

```sh
ayx telemetry workflows performance --profile prod --since 7d
```

Find the most-failing workflows:

```sh
ayx telemetry workflows errors --profile prod --since 7d
```

## Plans

Top Alteryx One plans by run count:

```sh
ayx telemetry plans top --profile prod --since 7d
```

Plan duration percentiles:

```sh
ayx telemetry plans performance --profile prod --since 7d
```

## Recent errors

Pull recent failed job groups with their error messages:

```sh
ayx telemetry errors recent --profile prod --since 24h
```

## Weekly run pattern

Get the 168-bucket (7 days x 24 hours) run-count matrix. Useful for capacity planning and scheduling decisions:

```sh
ayx telemetry weekly run-counts --profile prod
```

## Queue depth (Server only)

The `queue` subcommand currently supports the Server backend only.

Check current queue depth:

```sh
ayx telemetry queue status --profile prod --source server
```

Review wait-time statistics:

```sh
ayx telemetry queue wait-time --profile prod --source server
```

## Permissions

Audit who has access to what across your environment.

DCM connections and their authorized subjects:

```sh
ayx telemetry permissions connections --profile prod
```

Workflow access (workspace members on Alteryx One; collections on Alteryx Server):

```sh
ayx telemetry permissions workflows --profile prod
```

Gallery collection ACLs (Server only):

```sh
ayx telemetry permissions collections --profile prod --source server
```

Access count summary per subject:

```sh
ayx telemetry permissions summary --profile prod
```

## Pagination

Alteryx One list endpoints are paginated. Use `--all` to auto-paginate:

```sh
ayx telemetry workflows top --profile prod --all
```

Cap the number of pages with `--max-pages <N>` (default: 50):

```sh
ayx telemetry workflows top --profile prod --all --max-pages 20
```

## JSON output

All commands accept `--output json` as a global flag:

```sh
ayx telemetry summary --profile prod --output json
```

The envelope is always `{ ok, message, timestamp_utc, data }` on success; failures also include `error_code`.

## Related

- [Actions & workflows](/telemetry/actions/)
- [Alteryx Server overview](/server/)
- [Safety model](/safety-model/)
