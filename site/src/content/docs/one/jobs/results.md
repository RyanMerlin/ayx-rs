---
title: Results & publications
description: Retrieve profile data, publication history, and PDF results for Alteryx One Job Library entries.
sidebar:
  order: 2
---

After a Job Library entry runs, Alteryx One stores profile data, tabular results, publication
records, and optional PDF outputs. These commands let you retrieve them from the CLI. All
commands on this page are read-only.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one jobs profile <JOB-ID>` | Inspect profile data for an aggregate job |
| `ayx one jobs profile-results <JOB-ID>` | Retrieve profile result details |
| `ayx one jobs pdf-results <JOB-ID>` | Retrieve PDF output for an aggregate job |
| `ayx one jobs publications <JOB-ID>` | List publication records for an aggregate job |

All commands accept `<JOB-ID>` as the positional argument and `--profile <profile-id>`.

## Profile data

Profile data describes the data quality and shape of the job's outputs — row counts, column
types, null rates, and similar statistics.

```bash
# Summary profile
ayx one jobs profile <JOB-ID>

# Detailed profile results
ayx one jobs profile-results <JOB-ID>

# Scoped to a specific Alteryx One profile
ayx one jobs profile <JOB-ID> --profile <profile-id>

# Machine-readable
ayx --output json one jobs profile <JOB-ID>
```

`profile` returns a summary view. `profile-results` returns a more detailed breakdown. Use
`profile-results` when you need field-level statistics or are building data quality checks.

## PDF results

Some Job Library entries produce PDF outputs when configured to do so.

```bash
ayx one jobs pdf-results <JOB-ID>

ayx --output json one jobs pdf-results <JOB-ID>
```

The response includes the PDF data or a reference to where it can be retrieved.

## Publications

Publications are records of when and where job results were pushed to downstream targets.

```bash
# All publications for an aggregate job
ayx one jobs publications <JOB-ID>

# Scoped to a profile
ayx one jobs publications <JOB-ID> --profile <profile-id>

# Machine-readable
ayx --output json one jobs publications <JOB-ID>
```

To publish new results to a target, use `ayx one jobs publish` — see [Jobs](/one/jobs/).

## Automation patterns

Check profile results for data quality after every run:

```bash
PROFILE=$(ayx --output json one jobs profile-results <JOB-ID>)
echo "$PROFILE" | jq '.data.response'
```

Audit all publication targets for a job:

```bash
ayx --output json one jobs publications <JOB-ID> \
  | jq -r '.data.response[] | [.target, .publishedAt, .status] | @tsv'
```

List Job Library entries that have produced PDF results:

```bash
ayx --output json one jobs list --all \
  | jq -r '.data.items[].id' \
  | while read -r id; do
      COUNT=$(ayx --output json one jobs pdf-results "$id" \
               | jq '.data.response | length')
      [[ "$COUNT" -gt 0 ]] && echo "$id: $COUNT PDF result(s)"
    done
```

## Related

- [Jobs](/one/jobs/) — execute, cancel, and inspect Job Library entries
- [Safety model](/safety-model/) — how dry-run and `--apply` work
- [Output & automation](/output-automation/) — JSON envelope and scripting patterns
