---
title: Results & publications
description: Retrieve profile data, publication history, and PDF results for Alteryx One job groups.
sidebar:
  order: 2
---

After a job group runs, Alteryx One stores profile data, tabular results, publication records, and optional PDF outputs. These commands let you retrieve them from the CLI. All commands on this page are read-only.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one job-groups profile` | Inspect profile data for a job group |
| `ayx one job-groups profile-results` | Retrieve profile result details |
| `ayx one job-groups pdf-results` | Retrieve PDF output for a job group |
| `ayx one job-groups publications` | List publication records for a job group |

All commands accept `<id>` as the job group positional argument and `--profile <profile-id>`.

## Profile data

Profile data describes the data quality and shape of the job group's outputs — row counts, column types, null rates, and similar statistics.

```bash
# Summary profile
ayx one job-groups profile <id>

# Detailed profile results
ayx one job-groups profile-results <id>

# Scoped to a specific Alteryx One profile
ayx one job-groups profile <id> --profile <profile-id>

# Machine-readable
ayx --output json one job-groups profile <id>
```

`profile` returns a summary view. `profile-results` returns a more detailed breakdown. Use `profile-results` when you need field-level statistics or are building data quality checks.

## PDF results

Some job groups produce PDF outputs when configured to do so.

```bash
ayx one job-groups pdf-results <id>

ayx --output json one job-groups pdf-results <id>
```

The response includes the PDF data or a reference to where it can be retrieved.

## Publications

Publications are records of when and where job group results were pushed to downstream targets.

```bash
# All publications for a job group
ayx one job-groups publications <id>

# Scoped to a profile
ayx one job-groups publications <id> --profile <profile-id>

# Machine-readable
ayx --output json one job-groups publications <id>
```

To publish new results to a target, use `ayx one job-groups publish` — see [Job groups](/one/job-groups/).

## Automation patterns

Check profile results for data quality after every run:

```bash
PROFILE=$(ayx --output json one job-groups profile-results <id>)
echo "$PROFILE" | jq '.data'
```

Audit all publication targets for a job group:

```bash
ayx --output json one job-groups publications <id> \
  | jq -r '.data[] | [.target, .publishedAt, .status] | @tsv'
```

List job groups that have produced PDF results:

```bash
ayx --output json one job-groups list --all \
  | jq -r '.data[].id' \
  | while read id; do
      COUNT=$(ayx --output json one job-groups pdf-results "$id" \
               | jq '.data | length')
      [[ "$COUNT" -gt 0 ]] && echo "$id: $COUNT PDF result(s)"
    done
```

## Related

- [Job groups](/one/job-groups/) — run, cancel, and inspect job groups
- [Safety model](/safety-model/) — how dry-run and `--apply` work
- [Output & automation](/output-automation/) — JSON envelope and scripting patterns
