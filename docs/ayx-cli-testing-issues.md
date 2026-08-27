# AYX CLI Testing Issues

This log tracks issues found while exercising the AYX CLI through its live
discovery surface. Commands in this document put `--output json` last for
readability.

## Test context

- Date: 2026-08-27
- CLI: `ayx 0.17.0`
- Active profile: `local-dev`
- Active workspace: `alteryx-fde` (ID `91946`)
- Authentication: One API probe succeeded with HTTP 200

## Issue 1 — `workspace admins` returns all workspace people

Status: Open

### Reproduction

1. Discover the command:

   ```text
   ayx discover one --deep --output json
   ```

   Discovery identifies `ayx one workspace admins` with the description
   `List workspace admins`.

2. Run the discovered command:

   ```text
   ayx one workspace admins --output json
   ```

3. Compare it with the neighboring people command:

   ```text
   ayx one workspace people --output json
   ```

### Observed

- `workspace admins` succeeds with HTTP 200.
- It calls `GET /v4/people?role=admin`.
- It returns 18 records.
- `workspace people` also returns 18 records with the exact same IDs.
- The returned admin list includes the current user, whose detailed record has
  `isAdmin: false`.

### Expected

`workspace admins` should return only users recognized as workspace admins, or
the CLI should clearly report that the upstream response cannot be verified as
an admin-filtered result.

### Likely boundary

The CLI route and authentication work. The remaining question is whether the
One API ignores `role=admin`, requires another filter, or returns insufficient
role metadata for the CLI to validate the result. The command should gain a
regression test asserting that its result is not silently identical to the
unfiltered people result.

## Testing notes

- Use `ayx discover --deep --output json` before selecting unfamiliar commands.
- Prefer read-only commands while investigating.
- Standard command envelopes place the upstream payload under
  `data.response`; paginated CLI-normalized results may place records under
  `data.items`.
- Compact `json` intentionally omits the large discovery tree; use
  `json-full` only for discovery/tree traversal or raw payload inspection.

## Issue 2 — managed-IAM role assignments are permission denied

Status: Open / permission boundary confirmed

### Reproduction

1. Discover the role commands:

   ```text
   ayx discover one --deep --output json
   ```

2. List roles and identify the workspace-admin role:

   ```text
   ayx one role list --output json
   ```

   The active workspace has a `workspace_admin` role with policy ID `25703770`.

3. Request its assignments:

   ```text
   ayx one role list-assignments 25703770 --output json
   ```

### Observed

- The CLI reaches `GET /v4/authorization/roles/25703770/people`.
- The API returns HTTP 403 with `AccessControlException` / `permission_denied`.
- This prevents validating individual workspace-admin assignments through the
  managed-IAM endpoint with the current credential.

### Expected / next decision

The CLI correctly exposes the permission failure. Determine whether the
credential should be granted this read permission, or whether the admin-list
command must use a different supported endpoint. Do not treat the 18 records
from `workspace admins` as verified assignments until this is resolved.

## Issue 3 — compact discovery output omits the command tree

Status: Documented / agent guidance updated

### Reproduction

```text
ayx discover one --deep --output json
```

### Observed

The command succeeds but returns the compact `ayx.output.v1` envelope with
`data.omitted_fields` containing `tree`, `path`, `deep`, and `version`. The
command tree is available with:

```text
ayx discover one --deep --output json-full
```

### Resolution

Agent guidance now uses compact `json` for ordinary results and escalates to
`json-full` only when progressive discovery needs the omitted tree. This keeps
normal responses small while preserving discovery as the source of truth.

## Issue 4 — raw-field tests must opt into `json-full`

Status: Fixed in test harness

### Reproduction

The compact presentation intentionally omits raw fields such as `surface`,
`operation`, and nested response data. A test that asserted those fields after
running a command with `--output json` failed even though the API call returned
HTTP 200.

### Resolution

Agent and test guidance now uses compact `json` for routing/status checks and
`json-full` when asserting raw response fields, mutation previews, or discovery
trees. The exit-regression test was updated accordingly.

## Live canary run — 2026-08-27

Command lane:

```text
cargo nextest run -p ayx-rs -E 'binary(one_live_crud)' --no-fail-fast
```

Profile/workspace: `local-dev` / `alteryx-fde` (ID `91946`)

- Result: passed with zero canary residue.
- Groups: create, list/read, update, verify, delete — validated live.
- Plans: create, detail, update, verify, delete — validated live.
- Workflows: blocked by fixture; no source workflow existed in the workspace.
- Schedules: blocked by fixture; no workflow target existed.
- Connections: dry-run only; no disposable connector credentials were configured.
