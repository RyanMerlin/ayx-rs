---
title: Jobs
description: List, execute, inspect, cancel, and publish Alteryx One Job Library entries from the CLI.
sidebar:
  order: 1
---

The Job Library is Alteryx One's executable-unit surface. A Job Library entry — a *Job Group*
identified by `JOB-ID` — is a workflow or set of workflows that runs together and produces
outputs. You can list, execute, cancel, and inspect Job Library entries from the CLI. Mutating
commands are dry-run by default — add `--apply` to commit.

`ayx one jobs <JOB-ID>` (no verb) inspects that aggregate job. The provider exposes no full
child-run detail endpoint of its own — the complete child records are returned by
`ayx one jobs runs <JOB-ID>`.

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
| `ayx one jobs <JOB-ID>` | Inspect an aggregate job (no verb) |
| `ayx one jobs list` | List Job Library entries |
| `ayx one jobs count` | Count Job Library entries |
| `ayx one jobs status <JOB-ID>` | Check the execution status of an aggregate job |
| `ayx one jobs inputs <JOB-ID>` | List aggregate job inputs |
| `ayx one jobs outputs <JOB-ID>` | List aggregate job outputs |
| `ayx one jobs runs <JOB-ID>` | List every child run record for an aggregate job |
| `ayx one jobs execute` | Submit a Job Group from a JSON request body |
| `ayx one jobs publish <JOB-ID>` | Publish job results to a target |
| `ayx one jobs cancel <JOB-ID>` | Cancel a Job Library entry |

Because a bare `JOB-ID` and a verb subcommand are two different invocation shapes, they cannot be
combined — `ayx one jobs 42 list` is a usage error. Pick one: `ayx one jobs 42` (aggregate lookup)
or `ayx one jobs list` (list all entries). `--profile` goes after the verb on subcommand
invocations (`ayx one jobs runs <JOB-ID> --profile <profile-id>`), and directly on a bare lookup
(`ayx one jobs <JOB-ID> --profile <profile-id>`).

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
ayx --output json one jobs list --all
```

## Inspecting an aggregate job

```bash
# Aggregate-job lookup (bare JOB-ID)
ayx one jobs <JOB-ID>

# Execution status
ayx one jobs status <JOB-ID>

# Input parameters (useful before submitting)
ayx one jobs inputs <JOB-ID>

# Outputs produced by the last run
ayx one jobs outputs <JOB-ID>

# Every child run record
ayx one jobs runs <JOB-ID>
```

`inputs` tells you which parameters a Job Group accepts so you can build the correct submission
payload. `runs` lists the constituent child runs and their status, which is useful for diagnosing
partial failures — it's the only way to see full child-run detail, since the provider has no
dedicated endpoint for that.

## Submitting a job

`--body <FILE>` is a path to a JSON body file, not inline JSON:

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

`--body <FILE>` is likewise a path to a JSON body file:

```bash
cat > publish-body.json <<'EOF'
{"target":"<target>","...":{}}
EOF

# Dry-run
ayx one jobs publish <JOB-ID> --body publish-body.json

# Commit
ayx one jobs publish <JOB-ID> --body publish-body.json --apply
```

For profile and publication queries see [Results & publications](/one/jobs/results/).

## Cancelling a job

```bash
# Dry-run
ayx one jobs cancel <JOB-ID>

# Commit (skips TTY prompt in CI)
ayx one jobs cancel <JOB-ID> --apply --yes
```

Cancel is a best-effort operation. Jobs that have already completed are not affected.

## Automation patterns

Find all Job Library entries and show their status in one pass:

```bash
ayx --output json one jobs list --all \
  | jq -r '.data.items[].id' \
  | while read -r id; do
      STATUS=$(ayx --output json one jobs status "$id" | jq -r '.data.response')
      printf '%s\t%s\n' "$id" "$STATUS"
    done
```

Submit a job and poll until complete:

```bash
ayx one jobs execute --body body.json --apply

# Poll status
while true; do
  STATUS=$(ayx --output json one jobs status <JOB-ID> | jq -r '.data.response')
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
