---
title: Run and cancel
description: Queue or cancel a saved Alteryx One cloud-native workflow run from the CLI.
sidebar:
  order: 3
---

Running a cloud-native workflow is a real cloud operation. The CLI shows the
request first and does nothing until you add `--apply`.

## Run a workflow

1. Find the workflow ULID:

   ```bash
   ayx one workflows list
   ```

2. Preview the request:

   ```bash
   ayx --output json one workflows run <workflow-ulid>
   ```

3. Check the workflow id, then queue it:

   ```bash
   ayx --output json one workflows run <workflow-ulid> --apply --yes
   ```

The applied response comes from the Workflow Service. Keep the returned run/job
id; it identifies this particular execution.

### Runtime parameters

If the workflow supports runtime overrides or input parameters, put the JSON
body required by that workflow in a local file and preview it first:

```bash
ayx --output json one workflows run <workflow-ulid> --body run-input.json
ayx --output json one workflows run <workflow-ulid> --body run-input.json --apply --yes
```

Do not put tokens or other secrets in the body file. Use the normal secret
store and connection configuration for credentials.

## Cancel a run

Use the run/job id returned by `run`, not the workflow definition ULID:

```bash
ayx --output json one workflows cancel <run-id>
ayx --output json one workflows cancel <run-id> --apply --yes
```

Cancellation is also confirmation-gated when applied. It targets the
cloud-native Workflow Service and never treats the run id as a legacy
`/v4/jobGroups` id. Some workspaces may not have the provider's WFS Jobs
capability enabled; in that case the API returns a clear provider error and
the run must be managed through the workspace's supported execution surface.

## Test a disposable copy

For a release check, use the opt-in canary test. It copies a workflow, previews
and runs the copy, tries cancellation with the returned run/job id, and deletes
the copy by id:

```bash
AYX_ONE_LIVE_CRUD=1 \
AYX_ONE_LIVE_WORKFLOW_RUN=1 \
AYX_ONE_LIVE_PROFILE=local-dev \
AYX_ONE_LIVE_WORKFLOW_ID='<known-runnable-workflow-ulid>' \
cargo test -p ayx-rs --test one_live_crud --locked -- --nocapture
```

Use a small workflow that is safe to run in the validation workspace. If WFS
Jobs is disabled, the test records cancellation as a workspace capability block
and still removes the disposable copy.

## Safety reminders

- Omitting `--apply` is a safe preview and makes no server request.
- Use `--yes` only in a reviewed script or CI job.
- A workflow may still fail after it is queued because its permissions,
  connections, datasets, or execution engine are not ready.
- Run and cancellation responses are provider-shaped JSON; use
  `--output json` when a script needs the returned identifier.

## Related

- [Workflows](/one/workflows/)
- [Inspect](/one/workflows/inspect/)
- [Safety model](/safety-model/)
