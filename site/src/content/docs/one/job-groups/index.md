---
title: Job groups
description: List, run, inspect, cancel, and publish Alteryx One job groups from the CLI.
sidebar:
  order: 1
---

A job group is an executable unit in Alteryx One — a workflow or set of workflows that runs together and produces outputs. You can list, run, cancel, and inspect job groups from the CLI. Mutating commands are dry-run by default — add `--apply` to commit.

## Quick reference

| Command | What it does |
|---|---|
| `ayx one job-groups list` | List all job groups |
| `ayx one job-groups count` | Count job groups |
| `ayx one job-groups detail` | Inspect a job group record |
| `ayx one job-groups status` | Check the execution status of a job group |
| `ayx one job-groups inputs` | List input parameters for a job group |
| `ayx one job-groups outputs` | List outputs produced by a job group |
| `ayx one job-groups jobs` | List individual jobs within a job group |
| `ayx one job-groups run` | Trigger a job group run |
| `ayx one job-groups publish` | Publish job group results to a target |
| `ayx one job-groups cancel` | Cancel a running job group |

## Listing job groups

```bash
# All job groups (first page)
ayx one job-groups list

# All job groups, all pages
ayx one job-groups list --all

# Scoped to a profile
ayx one job-groups list --profile <profile-id>

# Limit per page
ayx one job-groups list --limit 50

# Machine-readable
ayx one job-groups list --all --output json
```

When the API returns a null `name` for a job group, `list` synthesizes a display name: `flow-{flowId}` if a `flowId` is present, otherwise `job-{id}`. The synthesized name appears in both text and JSON output.

## Inspecting a job group

```bash
# Full record
ayx one job-groups detail <id>

# Execution status
ayx one job-groups status <id>

# Input parameters (useful before triggering a run)
ayx one job-groups inputs <id>

# Outputs produced by the last run
ayx one job-groups outputs <id>

# Individual jobs within the group
ayx one job-groups jobs <id>
```

`inputs` tells you which parameters a job group accepts so you can build the correct run payload. `jobs` lists the constituent jobs and their status, which is useful for diagnosing partial failures.

## Triggering a run

```bash
# Dry-run — shows the request, triggers nothing
ayx one job-groups run --body '{"jobGroupId":"<id>"}'

# Commit
ayx one job-groups run --body '{"jobGroupId":"<id>"}' --apply

# With input overrides
ayx one job-groups run \
  --body '{"jobGroupId":"<id>","inputs":{"param":"value"}}' \
  --apply
```

## Publishing results

```bash
# Dry-run
ayx one job-groups publish \
  <id> \
  --body '{"target":"<target>","...":{}}'

# Commit
ayx one job-groups publish \
  <id> \
  --body '{"target":"<target>","...":"{}"}' \
  --apply
```

For profile and publication queries see [Results & publications](/one/job-groups/results/).

## Cancelling a run

```bash
# Dry-run
ayx one job-groups cancel <id>

# Commit (skips TTY prompt in CI)
ayx one job-groups cancel <id> --apply --yes
```

Cancel is a best-effort operation. Jobs that have already completed are not affected.

## Automation patterns

Find all job groups and show their status in one pass:

```bash
ayx one job-groups list --all --output json \
  | jq -r '.data[].id' \
  | xargs -I{} ayx one job-groups status {} --output json \
  | jq -r '[.data.id, .data.status] | @tsv'
```

Trigger a run and poll until complete:

```bash
ayx one job-groups run --body '{"jobGroupId":"<id>"}' --apply

# Poll status
while true; do
  STATUS=$(ayx one job-groups status <id> --output json | jq -r '.data.status')
  echo "$STATUS"
  [[ "$STATUS" == "Completed" || "$STATUS" == "Failed" ]] && break
  sleep 10
done
```

## Related

- [Results & publications](/one/job-groups/results/) — profile data, publication history, PDF results
- [Scheduling](/one/scheduling/) — view and manage the schedules that trigger job groups
- [Safety model](/safety-model/) — how dry-run and `--apply` work
- [Output & automation](/output-automation/) — JSON envelope and scripting patterns
