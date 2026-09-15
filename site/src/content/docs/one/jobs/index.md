---
title: Jobs
description: List, execute, inspect, cancel, and publish Alteryx One Job Library entries from the CLI.
sidebar:
  order: 1
---

The Job Library is Alteryx One's executable-unit surface. A Job Library entry — a *Job Group*
identified by `JOB-GROUP-ID` — is a workflow or set of workflows that runs together and produces
outputs. You can list, execute, cancel, and inspect Job Library entries from the CLI. Mutating
commands are dry-run by default — add `--apply` to commit.

`ayx one jobs <JOB-GROUP-ID>` (no verb) inspects that aggregate job. The provider exposes no full
child-run detail endpoint of its own — the complete child records are returned by
`ayx one jobs runs <JOB-GROUP-ID>`.

When `ayx one workflows run <WORKFLOW-ULID>` starts a cloud-native workflow, the response
contains both a `jobId` and a `jobgroupId`. Use `jobId` with `one workflows cancel`; use
`jobgroupId` with `one jobs runs` to inspect the child workflow runs. There is no separate
`one workflows runs` command because Job Group is the provider's run-history boundary.

:::note[`ayx one job-groups` compatibility]
`ayx one jobs` is the canonical, documented command family. `ayx one job-groups` is a hidden
compatibility alias kept for this release cycle — it still works (`job-groups run` ==
`jobs execute`, `job-groups jobs` == `jobs runs`, `job-groups detail <id>` == `jobs <id>`), but
it is intentionally absent from `--help`, `ayx one discover`, and the command catalog. New
scripts and automation should use `ayx one jobs`.
:::

## Quick reference

| Command | What it does |
|---|---|
| `ayx one jobs <JOB-GROUP-ID>` | Inspect an aggregate Job Group (no verb) |
| `ayx one jobs list` | List Job Library entries |
| `ayx one jobs count` | Count Job Library entries |
| `ayx one jobs status <JOB-GROUP-ID>` | Check the execution status of an aggregate Job Group |
| `ayx one jobs inputs <JOB-GROUP-ID>` | List aggregate Job Group inputs |
| `ayx one jobs outputs <JOB-GROUP-ID>` | List aggregate Job Group outputs |
| `ayx one jobs runs <JOB-GROUP-ID>` | List every child run for a Job Group execution |
| `ayx one jobs execute` | Submit a Job Group from a JSON request body |
| `ayx one jobs publish <JOB-GROUP-ID>` | Publish job results to a target |
| `ayx one jobs cancel <JOB-GROUP-ID>` | Cancel a Job Library entry |

Because a bare `JOB-GROUP-ID` and a verb subcommand are two different invocation shapes, they cannot be
combined — `ayx one jobs 42 list` is a usage error. Pick one: `ayx one jobs 42` (aggregate lookup)
or `ayx one jobs list` (list all entries). `--profile` goes after the verb on subcommand
invocations (`ayx one jobs runs <JOB-GROUP-ID> --profile <profile-id>`), and directly on a bare lookup
(`ayx one jobs <JOB-GROUP-ID> --profile <profile-id>`).

## Listing Job Library entries

```bash
# All entries (first page)
ayx one jobs list

# All entries, all pages
ayx one jobs list --all

# Scoped to a profile
ayx one jobs list --profile <profile-id>

# Limit per page
ayx one jobs list --limit 50

# Machine-readable
ayx -o json one jobs list --all
```

## Inspecting an aggregate job

```bash
# Aggregate-job lookup (bare JOB-GROUP-ID)
ayx one jobs <JOB-GROUP-ID>

# Execution status
ayx one jobs status <JOB-GROUP-ID>

# Input parameters (useful before submitting)
ayx one jobs inputs <JOB-GROUP-ID>

# Outputs produced by the last run
ayx one jobs outputs <JOB-GROUP-ID>

# Every child run record for a Job Group execution
ayx one jobs runs <JOB-GROUP-ID>
```

`inputs` tells you which parameters a Job Group accepts so you can build the correct submission
payload. `runs` lists the constituent child runs and their status, which is useful for diagnosing
partial failures — it's the only way to see full child-run detail, since the provider has no
dedicated endpoint for that.

## Submitting a job

`--body <FILE|JSON|->` accepts a JSON file, inline non-secret JSON, or `-` for
piped stdin. Inline values are visible in shell history and process listings;
prefer a file or stdin for sensitive payloads:

```bash
# Write the request body to a file
cat > body.json <<'EOF'
{"jobGroupId":"<id>"}
EOF

# Dry-run — shows the request, submits nothing
ayx one jobs execute --body body.json

# Commit
ayx one jobs execute --body body.json --apply

# With input overrides
cat > body-with-inputs.json <<'EOF'
{"jobGroupId":"<id>","inputs":{"param":"value"}}
EOF
ayx one jobs execute --body body-with-inputs.json --apply
```

## Publishing results

`--body <FILE|JSON|->` accepts the same file, inline, and stdin sources:

```bash
cat > publish-body.json <<'EOF'
{"target":"<target>","...":{}}
EOF

# Dry-run
ayx one jobs publish <JOB-GROUP-ID> --body publish-body.json

# Commit
ayx one jobs publish <JOB-GROUP-ID> --body publish-body.json --apply
```

For profile and publication queries see [Results & publications](/one/jobs/results/).

## Cancelling a job

```bash
# Dry-run
ayx one jobs cancel <JOB-GROUP-ID>

# Commit (skips TTY prompt in CI)
ayx one jobs cancel <JOB-GROUP-ID> --apply --yes
```

Cancel is a best-effort operation. Jobs that have already completed are not affected.

## Automation patterns

Find all Job Library entries and show their status in one pass:

```bash
ayx -o json one jobs list --all \
  | jq -r '.data.items[].id' \
  | while read -r id; do
      STATUS=$(ayx -o json one jobs status "$id" | jq -r '.data.response')
      printf '%s\t%s\n' "$id" "$STATUS"
    done
```

Submit a job and poll until complete:

```bash
ayx one jobs execute --body body.json --apply

# Poll status
while true; do
  STATUS=$(ayx -o json one jobs status <JOB-GROUP-ID> | jq -r '.data.response')
  echo "$STATUS"
  [[ "$STATUS" == "Completed" || "$STATUS" == "Failed" ]] && break
  sleep 10
done
```

## Related

- [Results & publications](/one/jobs/results/) — profile data, publication history, PDF results
- [Scheduling](/one/scheduling/) — view and manage the schedules that trigger jobs
- [Safety model](/safety-model/) — how dry-run and `--apply` work
- [Output & automation](/output-automation/) — JSON envelope and scripting patterns
