# AYX One Live CRUD Protocol

This protocol is for an explicitly enabled canary run. Ordinary CI must remain
read-only.

## Gate

Use a named profile and verify the target before mutation:

```powershell
$env:AYX_ONE_LIVE_CRUD = '1'
$env:AYX_ONE_LIVE_PROFILE = 'local-dev'
ayx --version
ayx profile current --output json
ayx one auth status --output json
ayx one workspace current --output json
```

The active workspace and display name must be recorded before applying writes.
Use a unique prefix such as `ayx-agent-canary-20260827-<run-id>`.

## Sequence

1. Run `ayx discover one --deep --output json-full` and select commands from the live tree.
2. Capture baseline lists/counts for workflows, groups, connections, plans, and schedules.
3. Create JSON payloads with the unique prefix.
4. Run every mutation once without `--apply`; inspect the dry-run body and target.
5. Apply only approved canary operations with `--apply --yes`.
6. Capture IDs from the applied response; fail if an ID is missing.
7. Detail/read the resource and verify the update.
8. Clean up in reverse creation order, by captured ID only.
9. Re-list the families and require baseline equality. Record any residue.

## Family rules

- Workflows have no native create/update command. Copy a real source workflow,
  detail the copy, then delete the copy and verify `not_found`.
- Groups support create, read/list, update, and delete. Membership mutations are
  separate and must be independently verified.
- Plans support create, detail, update, and delete, subject to workspace tier.
- Schedules support create, detail, update, enable/disable, and delete. They
  require a valid disposable workflow or plan target.
- Connections may only be created when a valid disposable connector fixture and
  credentials exist. Never update or delete an existing production connection.
  Otherwise run the dry-run/schema path and classify the live cycle as
  `blocked_by_fixture`.

## Failure rules

Stop and log the result if cleanup fails, an applied mutation has no ID, a list
reports `ok: true` with a non-2xx page status, or the final baseline differs.
Never silently convert a skipped family into a passing CRUD result.
