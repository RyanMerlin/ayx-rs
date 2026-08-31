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

Status: Fixed 2026-08-31 (live re-verification pending)

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

### Root cause

Three compounding faults, the last of which only surfaced during live
verification of a first (wrong) fix:

1. **`role=admin` is ignored.** The One `/v4` gateway accepts the query
   parameter and returns HTTP 200 with the complete, unfiltered people list, so
   `workspace admins` was an alias for `workspace people`.
2. **`isAdmin` only decorates the caller.** `GET /v4/people` sets `isAdmin` on
   the *requesting user's own* record; every other person record carries only
   `{ email, id, name }`. A client-side `isAdmin` filter therefore cannot
   identify the admins at all — the first attempted fix filtered correctly
   (`items_before: 18`, `items_after: 0`) and returned **zero** admins, which is
   as wrong as returning all 18.
3. **The "404" on the real endpoint was a probe error.** The tenant's live
   OpenAPI spec (`ayx one api open-api-spec`) *does* declare
   `GET /v4/workspaces/{workspaceId}/admins`, with `workspaceId` typed as an
   **integer** — the numeric workspace id (e.g. `91946`), not the workspace GID
   (`01KMGF85WTTEJZ397MW1RBD9ZB`). The earlier probe substituted the GID, got a
   404, and that route was written off as non-existent — which is how the
   command ended up on `/v4/people?role=admin` in the first place.

### Fix (2026-08-31)

`ayx-rs/src/cmd/one_platform/workspace.rs` — the `Admins` arm now calls the
dedicated, server-side-filtered endpoint:

- Endpoint template `WORKSPACE_ADMINS_ENDPOINT` =
  `/v4/workspaces/{workspaceId}/admins`.
- The numeric `workspaceId` comes from `resolve_workspace_path_id`, the same
  preflight (`GET /v4/workspaces/current`, plus a profile-GID cross-check)
  every other path-scoped workspace command uses. No new resolution logic.
- The command stays argless; the id is resolved from the active workspace.
- The spec's optional query params (`accountId`, `fields`, `includeStatus`) are
  not wired up — the default response is the full admin list.
- The client-side `isAdmin` filter from the first attempt (`person_is_admin`,
  `retain_admins`, `filter_admins_envelope`) and its tests are **deleted**:
  the server now filters, and the payload cannot support a client-side filter
  anyway (see root cause 2). Nothing else referenced those helpers.

Supporting updates: `ayx-one-api/src/inventory.rs` records the real endpoint
template for `one workspace admins` (so `ayx one api coverage` stays truthful),
`ayx-rs/src/cmd/catalog.rs` carries the corrected note, and
`docs/one-endpoint-matrix.md`, `docs/one-backend-inventory.md`, and
`docs/one-api-surface-audit.md` are amended.

Unit tests in the same module assert the endpoint template is the numeric
path-scoped admins route (never `/v4/people`), that the resolved path has no
unsubstituted placeholders, and that the path-id preflight rejects a workspace
GID where the numeric id is required.

Live re-verification against the `alteryx-fde` workspace (numeric id `91946`)
is still pending: confirm `ayx one workspace admins` returns HTTP 200 with a
non-empty admin list that is a strict subset of `ayx one workspace people` and
excludes the current (`isAdmin: false`) user.

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
