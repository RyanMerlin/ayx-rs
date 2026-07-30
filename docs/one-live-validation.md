# One Live Validation

> **Stale snapshot.** The "Known Endpoint Status" table below reflects a live probe taken
> 2026-06-22 against `ayx` v0.9.14. The repo is now at v0.14.0 (2026-07-28) and the One surface has
> moved since: connection permissions/sharing were repaired off the dead
> `/v4/connections/{id}/permissions[/{aid}]` routes (`RouteNotFoundException`) onto
> `/v4/connections/{id}/permissions/sharedSubjects` (read) and the shared `/v4/connections/share`
> route (create/delete), and a new cloud-native `workflow` surface (`ayx one workflows ...`,
> ULID-keyed, served by `/svc-workflow`) has since been wired and live-verified. Treat the
> endpoint-level claims below as historical evidence, not current status. For per-endpoint, dated
> live evidence against the current inventory, see `docs/one-endpoint-matrix.md`.

This document tracks the live validation strategy for the wired Alteryx One surface.

## Coverage Model

- `validated_live`: a real request returned from the One API host and the response was asserted.
- `validated_shape`: request construction, dry-run behavior, or envelope formatting was asserted without a live mutation.
- `blocked_by_auth`: the environment could not acquire usable live credentials.
- `blocked_by_scope`: the endpoint exists, but the current workspace/role does not have permission to exercise it.

## Surface Inventory

Test the currently wired One families in the CLI and API layers:

- platform / auth / workspace / person / token / role
- plans
- flows
- workflows (cloud-native, ULID-keyed, `/svc-workflow`)
- datasets
- connections
  - detail
  - permissions list
  - connector-metadata defaults
  - connector-metadata publish-info
- job-group
  - list
  - detail
  - status
  - inputs
  - outputs
  - jobs
  - publications
  - profile
  - profile-results
  - pdf-results
- output-object
- webhook-flow-task
- write-setting
- scheduling
- billing
- doctor / inventory / status helpers

## Validation Criteria

- One representative live read or discovery call per family.
- One edge case per family when the API supports it.
- For list endpoints: verify pagination or empty-result handling where possible.
- For mutating endpoints: prefer dry-run or a reversible safe case before any real mutation.
- Every result must record the command, endpoint family, status bucket, and whether it was truly live.
- Current smoke coverage includes invalid-id failures for representative detail commands and pagination-boundary checks for the major list families.

## Pressure Test Level

Use the default "happy path + one edge" matrix:

- happy path: prove the live endpoint is reachable and returning an expected envelope
- edge path: exercise invalid id, empty page, pagination boundary, or permission failure

Escalate to broader matrices only for families that are known to be flaky or stateful.

## Live Validation Hygiene

- Use `cargo nextest run` for all repo and smoke validation going forward.
- Keep One-only live tests on a minimal profile that still satisfies the config model, but avoid mixing in unrelated Server storage assumptions when validating the One cloud API.
- If auth fails, classify it as an environment blocker first. Only treat the surface as broken after a confirmed live request reaches the One host and returns a backend error.

## Current Harness

The current smoke harness lives in `ayx-rs/tests/one_live_smoke.rs` and already:

- uses the live CLI binary
- short-circuits cleanly when auth acquisition is unavailable
- validates the most important read paths across the One surface
- reports the surface and operation names in the envelope assertions

## Known Endpoint Status (as of v0.9.14)

Live-verified against the test workspace on 2026-06-22.

### Working surfaces (validated_live)

| Endpoint | Command | Notes |
|---|---|---|
| `POST /v4/flows` | `flows create` | Create flow |
| `PATCH /v4/flows/{id}` | `flows update` | Fixed in v0.9.12; was PUT (returned 403) |
| `DELETE /v4/flows/{id}` | `flows delete` | Returns 204 |
| `POST /v4/flows/{id}/copy` | `flows copy` | Returns 201; copies a flow to a new name |
| `GET /v4/flows/{id}/inputs` | `flows inputs` | Works on empty flow |
| `GET /v4/flows/{id}/outputs` | `flows outputs` | Works on empty flow |
| `GET /v4/flowsLibrary` | `flows library list` | Works (0 items) |
| `GET /v4/folders` | `flows folders list` | Works (0 items) |
| `GET /v4/people` | `platform workspace people` | Fixed v0.9.12; replaces broken `/v4/workspaces/{id}/people` |
| `GET /v4/people?role=admin` | `platform workspace admins` | Fixed v0.9.12; replaces broken `/v4/workspaces/{id}/admins` |
| `GET /v4/outputObjects` | `output-objects list` | Works (0 items) |
| `GET /v4/writeSettings` | `write-settings list` | Works (0 items) |
| `GET /v4/connections` | `connections list` | Works |
| `GET /v4/connectorMetadata/{slug}/defaults` | `connections connector-metadata defaults` | Source of the create-body schema; backs `connections template` |

### Blocked by PAT scope (blocked_by_scope)

The PAT minted via the workspace-bearer OIDC flow has create/read/delete on flows
and connections but lacks scope for these — all return `AccessControlException`
("User is not authorised to access this API.", HTTP 403). A UI-minted token or a
broader OAuth scope at the `POST /v4/apiAccessTokens` mint step would be required.

| Endpoint | Command | Notes |
|---|---|---|
| `GET /v4/flows/{id}/permissions` | `flows permissions-get` | 403 — command exists, surfaces `permission_denied` |
| `GET /v4/flows/{id}/recipeParameters` | `flows parameters` | 403 |
| `GET /v4/roles` | `platform role list` | 403 |
| `POST /v4/connections/dryRun` | `connections dry-run` | 403 — endpoint exists but PAT cannot exercise it |

### Absent / wrong-route (blocked: route not found)

| Endpoint | Command | Notes |
|---|---|---|
| `GET /v4/flows/{id}/validate` | `flows validate` | 404 — no validate route in this API version |
| `GET /v4/connectors` | (none) | 404 — no connector enumeration endpoint in v4 |
| `/v4/webhookFlowTasks` | `webhook-flow-tasks *` | 404 — not present on this workspace tier |

### Tier-gated (enterprise-only)

| Endpoint | Command | Notes |
|---|---|---|
| `/billing/v1/*` | `billing *` | 404 on the test workspace tier |
| `/plans/v1/*` | `plans *` | 404 on the test workspace tier |
| `/scheduling/v1/*` | `scheduling *` | 404 on the test workspace tier |

## Fixed panics (v0.9.14)

Four commands defined a local `--output <PathBuf>` arg that collided with the
global `--output <text|json>` format flag (same clap arg id, different type),
panicking at runtime on every invocation. All four renamed their file arg to
`--output-file`:

- `flows export`
- `server system-info`
- `server runtime-settings`
- `tools workspace init`

The following workflow commands use `--output-path` (not `--output`), so they
were never affected by the collision bug and work correctly at all versions:

- `workflow replace`
- `workflow repackage`
- `workflow recurse`
- `workflow convert-cloud`
- `workflow migrate`

These are the local Designer `.yxmd`/`.yxzp` tooling commands, unrelated to the Alteryx One
cloud-native `workflow` surface described in the banner above. As of v0.14.0 they moved to
`designer workflow *` with no back-compat alias (`ayx workflow *` no longer resolves).

## Follow-Up

As more endpoints are confirmed, add them to the live matrix and keep the coverage grouped by family so the report stays readable.
