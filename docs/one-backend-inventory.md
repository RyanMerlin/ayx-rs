# One Backend Inventory

This document tracks the exposed Alteryx One backend surface that `ayx` can already reach, plus the remaining gaps we still need to wire.

The purpose is to keep backend work separate from the experimental UI shell and to make the remaining implementation surface obvious before we add more commands.

## Known Boundary

The public One API surface exposed here does not provide a general-purpose workflow designer.

- `POST /v4/flows` creates flow metadata only.
- Dataset and edge endpoints can wire existing objects together.
- Validation, package, and import/export endpoints can inspect or move flow artifacts.
- No public endpoint in this surface accepts arbitrary visual workflow logic or creates a `.yxmd` design document.

## Status Buckets

- `implemented`: wired in the CLI and represented in the One API inventory.
- `partial`: several commands are wired, but deeper subfamilies still need work.
- `documented-only`: known in the upstream API docs or UI, but not yet wired here.
- `deferred`: intentionally postponed for now.

## Implemented Surfaces

- `platform.iam`
  - `GET /v4/workspaces/current`
  - `GET /v4/workspaces/{id}/configuration`
  - `GET /v4/people` (workspace context via the `x-alteryx-workspace-gid` header;
    `/v4/workspaces/{id}/people` 404s)
  - `GET /v4/people?role=admin` (same header-scoped workspace context;
    `/v4/workspaces/{workspaceId}/admins` 404s)
  - `GET /v4/workspaces/{id}/groups`
  - `GET /v4/groups`
  - `GET /v4/workspaces/{id}/invitationLink` (required `personId` query parameter)
  - `GET /v4/workspaces/{workspaceId}/cloudConfigs`
  - `POST /v4/workspaces/{id}/people/batch`
  - `DELETE /v4/workspaces/{workspaceId}/people/{id}`
  - `POST /v4/workspaces/{id}/people/suspend`
  - `POST /v4/workspaces/{id}/people/unsuspend`
  - `POST /v4/workspaces/{id}/transfer`
  - `GET /v4/authorization/roles/{id}/people`
  - `GET /v4/authorization/roles`
  - `GET /v4/authorization/roles/{id}`
  - `PUT /v4/authorization/roles/{id}/people` (request body: `{"items":[subjectId]}`)
  - `DELETE /v4/authorization/roles/{id}/people/{subjectId}`
- `misc`
  - `GET /v4/open-api-spec`
- `plan`
  - `POST /v4/plans`
  - `POST /v4/plans/{id}/permissions`
  - `PATCH /v4/plans/{id}`
  - `DELETE /v4/plans/{id}`
  - Notes:
    - Only the `/v4` plan endpoints the CLI actually dispatches are listed here. Read paths (list/count/run/permissions/package/runParameters/schedules) now use the spec-documented `/v4` paths instead. See the `plans` surface below.
- `plans`
  - `GET /v4/plans`
  - `GET /v4/plans/{id}/full` (`one plans detail` and `one plans full`)
  - `POST /v4/plans/{id}/run`
  - `GET /v4/plans/count`
  - `GET /v4/plans/{id}/runParameters`
  - `GET /v4/plans/{id}/schedules`
  - `GET /v4/plans/{id}/package`
  - `POST /v4/plans/package`
  - `GET /v4/plans/{id}/permissions`
  - `DELETE /v4/plans/{id}/permissions/{subjectId}`
- `scheduling`
  - `POST /v4/schedules`
  - `GET /v4/schedules`
  - `GET /v4/schedules/{id}`
  - `PUT /v4/schedules/{id}`
  - `POST /v4/schedules/{id}/enable`
  - `POST /v4/schedules/{id}/disable`
  - `DELETE /v4/schedules/{id}`
  - `GET /v4/schedules/count`
- `workflow`
  - `GET /v4/workflows`
  - `GET /v4/workflows?limit=1`
  - `GET /svc-workflow/api/v1/assets`
  - `GET /svc-workflow/api/v1/assets/{id}/dependencies`
  - `GET /svc-workflow/api/v0/workflows/{id}/availableEngines`
  - `GET /svc-workflow/api/v1/tools`
  - `POST /svc-workflow/api/v2/workflows/{id}/duplicate`
  - `POST /svc-workflow/api/v2/workflows/{id}/share`
  - `DELETE /svc-workflow/api/v2/workflows/{id}`
  - Notes:
    - Alteryx One cloud-native (canvas) workflows, ULID-keyed, served by `/svc-workflow`.
    - Distinct from the `flow` family below, which is Designer Cloud `/v4/flows` keyed by integer ids.
    - `GET /v4/workflows` is the one listing route the gateway exposes; it is absent from the published `/v4/open-api-spec`, so `one api coverage` reports it as stale even though it is live-wired.
    - `detail` and `count` are synthesized client-side; the API exposes no per-id or count route.
- `flow`
  - `POST /v4/flows`
  - `GET /v4/flows`
  - `GET /v4/flows/count`
  - `GET /v4/flows/{id}`
  - `PATCH /v4/flows/{id}`
  - `DELETE /v4/flows/{id}`
  - `POST /v4/flows/{id}/copy`
  - `POST /v4/flows/{id}/run`
  - `GET /v4/flows/{id}/validate`
  - `GET /v4/flows/{id}/recipeParameters`
  - `GET /v4/flows/{id}/inputs`
  - `GET /v4/flows/{id}/outputs`
  - `POST /v4/flows/package`
  - `POST /v4/flows/package/dryRun`
  - `GET /v4/flows/{id}/package`
  - `GET /v4/flows/{id}/package/dryRun`
  - `GET /v4/flowsLibrary`
  - `GET /v4/flowsLibrary/count`
  - `GET /v4/folders`
  - `GET /v4/folders/count`
  - `GET /v4/folders/{id}`
  - `POST /v4/folders`
  - `PATCH /v4/folders/{id}`
  - `DELETE /v4/folders/{id}`
  - `GET /v4/folders/{id}/flows`
  - `GET /v4/folders/{id}/flows/count`
  - `POST /v4/flows/{id}/permissions`
  - `GET /v4/flows/{id}/permissions` (`flows permissions-get`; read side of the same path)
  - `POST /v4/flows/{id}/move`
  - `PATCH /v4/flows/{id}/replaceDataset`
  - Notes:
    - Lifecycle, package, parameter, library, folder, and permission commands are wired.
    - The One surface does not expose arbitrary workflow authoring through this family.
    - Destructive deletes on flows and folders prompt for TTY confirmation unless `--yes` is supplied.

## Partial Surfaces

- `connection`
  - `GET /v4/connections`
  - `GET /v4/connections/count`
  - `POST /v4/connections`
  - `POST /v4/connections/dryRun`
  - `GET /v4/connections/{id}`
  - `GET /v4/connections/{id}/status`
  - `PATCH /v4/connections/{id}`
  - `DELETE /v4/connections/{id}`
  - `GET /v4/connections/{id}/permissions/sharedSubjects` (permissions read; the old `/v4/connections/{id}/permissions[/{aid}]` paths 404 with `RouteNotFoundException`)
  - `POST /v4/connections/share` (permissions create; the connection id travels in the request body, not the path)
  - `DELETE /v4/connections/share` (permissions delete; the connection id travels in the query string, not the path)
  - `GET /v4/connectorMetadata/{connector}/defaults`
  - `GET /v4/connectorMetadata/{connector}`
  - `GET /v4/connectorMetadata/{connector}/publish/info`
  - `GET /v4/connectorMetadata/{connector}/overrides`
  - `POST /v4/connectorMetadata/{connector}/overrides`
  - `DELETE /v4/connectorMetadata/{connector}/overrides`
  - Notes:
    - Connection lifecycle, dry-run, status, and permission commands are wired.
    - Connector metadata defaults, current values, and overrides are wired for JDBC behavior control.
    - Credential-backend specifics still live in the API payloads rather than a local domain model.
    - Delete operations prompt for TTY confirmation unless `--yes` is supplied, so destructive runs stay explicit in automation and interactive use.
- `dataset`
  - `GET /v4/datasetLibrary`
  - `GET /v4/datasetLibrary/count`
  - `GET /v4/wrangledDatasets`
  - `GET /v4/wrangledDatasets/count`
  - `GET /v4/wrangledDatasets/{id}`
  - `GET /v4/importedDatasets/{id}`
  - Notes:
    - Dataset library list/count plus wrangled and imported dataset detail reads are wired.
    - Mutating dataset lifecycle operations remain documented-only in this first cut.
- `jobGroup`
  - `GET /v4/jobLibrary`
  - `GET /v4/jobLibrary/count`
  - `POST /v4/jobGroups`
  - `PUT /v4/jobGroups/{id}/publish`
  - `GET /v4/jobGroups/{id}`
  - `POST /v4/jobGroups/{id}/cancel`
  - `GET /v4/jobGroups/{id}/status`
  - `GET /v4/jobGroups/{id}/inputs`
  - `GET /v4/jobGroups/{id}/pdfResults`
  - `GET /v4/jobGroups/{id}/outputs`
  - `GET /v4/jobGroups/{id}/jobs`
  - `GET /v4/jobGroups/{id}/publications`
  - `GET /v4/jobGroups/{id}/profile`
  - `GET /v4/jobGroups/{id}/profileResults`
  - Notes:
    - Execution, publish, status, and inspection commands are wired.
    - Live smoke coverage now exercises detail, status, inputs, outputs, jobs, publications, profile, profile-results, and pdf-results on real job groups.
- `outputObject`
  - `GET /v4/outputObjects`
  - `GET /v4/outputObjects/count`
  - `POST /v4/outputObjects`
  - `GET /v4/outputObjects/{id}`
  - `PATCH /v4/outputObjects/{id}`
  - `DELETE /v4/outputObjects/{id}`
  - `GET /v4/outputObjects/{id}/inputs`
  - `POST /v4/outputObjects/{id}/wrangleToPython`
  - Notes:
    - Lifecycle and wrangle-to-python commands are wired.
    - Additional nested resources remain open.
- `webhookFlowTask`
  - `POST /v4/webhookFlowTasks`
  - `GET /v4/webhookFlowTasks/{id}`
  - `DELETE /v4/webhookFlowTasks/{id}`
  - `POST /v4/webhooks/test`
- `writeSetting`
  - `GET /v4/writeSettings`
  - `GET /v4/writeSettings/count`
  - `POST /v4/writeSettings`
  - `GET /v4/writeSettings/{id}`
  - `PATCH /v4/writeSettings/{id}`
  - `DELETE /v4/writeSettings/{id}`
- `apiAccessTokens`
  - `GET /v4/apiAccessTokens`
  - `POST /v4/apiAccessTokens`
  - `GET /v4/apiAccessTokens/{tokenId}`
  - `DELETE /v4/apiAccessTokens/{tokenId}`
- `person`
  - `GET /v4/people/current`
  - `GET /v4/people`
  - `GET /v4/people/current`
  - `GET /v4/people/count`
  - `GET /v4/people/{id}`
  - `POST /v4/people`
  - `PUT /v4/people/{id}`
  - `PATCH /v4/people/{id}`
  - `DELETE /v4/people/{id}`
  - `PATCH /v4/people/current/updatePassword`
  - `POST /v4/passwordresetrequest`
  - Notes:
    - Current lookup plus list/count/detail/create/update/patch/delete/password flows are wired.
    - Remaining person-adjacent families stay open.
- `workspace`
  - `GET /v4/workspaces`
  - `POST /v4/workspaces`
  - `DELETE /v4/workspaces/{id}`
  - `POST /v4/workspaces/{id}/groups`
  - `DELETE /v4/workspaces/{id}/groups/{groupId}`
  - `PUT /v4/workspaces/{id}/groups/{groupId}`
  - `PUT /v4/workspaces/{id}/groups/{groupId}/roles`
  - `POST /v4/workspaces/{id}/groups/{groupId}/users`
  - `DELETE /v4/workspaces/{id}/groups/{groupId}/users`
  - `GET /v4/workspaces/{id}/configuration`
  - `PATCH /v4/workspaces/current/transfer`
  - `GET /v4/workspaces/current/configuration`
  - `PATCH /v4/workspaces/current/configuration`
  - `PATCH /v4/workspaces/{id}/configuration`
  - `GET /v4/workspaces/{id}/configuration-schema`
  - `GET /v4/workspaces/current/configuration-schema`
  - `POST /v4/workspaces/current/delete-configuration`
  - `POST /v4/workspaces/{id}/delete-configuration`
  - `POST /v4/workspaces/{id}/people`
  - `PATCH /v4/workspaces/{id}/people/batch`
  - `PUT /v4/workspaces/{id}/people/{personId}/suspended`
  - `POST /v4/workspaces/{workspaceId}/cloudConfigs/{cloudProvider}`
  - `PATCH /v4/workspaces/{workspaceId}/cloudConfigs/{cloudProvider}`
  - `PATCH /v4/workspaces/{workspaceId}/people/{id}`
  - `PUT /v4/workspaces/{workspaceId}/people/{id}`
  - Notes:
    - Workspace listing, lifecycle, groups, configuration, transfer, people, and cloud-config endpoints are wired.
    - Other workspace families remain open.

## Documented-Only Surfaces

- None yet in the current inventory, but this bucket is reserved for future upstream endpoints we have not wired or should not expose yet.

## Deferred Surfaces

- None yet.

## Live Validation Coverage

The live smoke suite currently proves a representative path for:

- `platform.workspace.current`
- `platform.workspace.list`
- `platform.person.current`
- `platform.token`
- `plans.count`
- `plans.list`
- `flows.list`
- `connections.list`
- `connections.detail`
- `connections.permissions.list`
- `connections.connector-metadata.defaults`
- `connections.connector-metadata.publish-info`
- `job-groups.list`
- `job-groups.detail`
- `job-groups.status`
- `job-groups.inputs`
- `job-groups.outputs`
- `job-groups.jobs`
- `job-groups.publications`
- `job-groups.profile`
- `job-groups.profile-results`
- `job-groups.pdf-results`
- `output-objects.list`
- `write-settings.list`
- `scheduling.list`
- `workflows.list`
- `workflows.count`
- `workflows.tools`
- `workflows.detail`
- `workflows.dependencies`
- `datasets.count`
- `doctor.discover`
- `doctor.auth`
- `platform.api.status`
- `connections.dry-run`

It also exercises edge coverage for representative families:

- invalid-id detail failures across `flow` (flows and folders), `connection` (connections and permissions), `plans`, `platform.person`, `platform.token`, `jobGroup`, `outputObject`, `writeSetting`, and `workflow`
- pagination-boundary list checks on the major list families using `--limit 1 --all --max-pages 1`

## Live Coverage Baseline

Current measurement of `ayx one api coverage` against the live `GET /v4/open-api-spec`, taken 2026-08-20 against an authenticated disposable validation workspace.

For comparison, the previous baseline was 43.8% coverage (2026-07-30). The current source also
normalizes both `{param}` and `:param` path-template styles, removing two false missing rows and
two false stale rows from the prior inventory comparison. The transport-unwrapping fix remains
important historical context: before it, the command was handed the transport metadata envelope
instead of the spec body and reported `spec_operations: 0` with an empty `missing` list.

| metric | value |
|---|---|
| `coverage_pct` | **63.1%** |
| `spec_operations` | 233 |
| `covered` | 147 |
| `missing` (spec documents it, CLI does not wire it) | **86** |
| `stale` (inventory wires it, spec does not describe it) | 16 |
| `outside_spec_namespace` (sibling services, not comparable) | 7 |
| `inventory_total` / `inventory_operations` | 172 / 172 |

Missing operations concentrate in a few resources:

| count | resource |
|---|---|
| 8 | `accounts` |
| 7 | `people` |
| 7 | `importedDatasets` |
| 6 each | `authorization`, `environmentParameters`, `publications`, `sqlScripts`, `wrangledDatasets` |

26 resources in total; the remainder are five or fewer each.

### Reading these numbers correctly

**`stale` does not mean broken.** It means the published spec does not describe an endpoint the CLI wires. Several entries on that list are live-verified working: `one connections dry-run` reaches `POST /v4/connections/dryRun` and returns body validation, `one person count` reaches `GET /v4/people/count` but is intentionally retired with HTTP 410 `gone`, and `GET /v4/workflows` is live while absent from the published spec. Treat `stale` as "the spec is incomplete here", and only investigate a row after confirming the route is genuinely dead.

**`--check` currently exits 1.** It gates on `missing > 0`, and `missing` is 86. Wiring `ayx one api coverage --check` into CI — which `docs/one-roadmap.md` recommends — would red the build immediately. That is an honest signal rather than a bug, but it needs a decision first: either gate on a coverage threshold instead of `missing == 0`, or scope the gate to a resource allowlist expected to be complete. Do not wire it as-is.

## Next Backend Wiring Pass

The `connection` permissions gap and the `outputObject`/`webhookFlowTask`/`writeSetting` command-or-not decision that used to head this list are both resolved: permissions now ride the repaired `/v4/connections/share` route, and all three families have first-class CLI CRUD commands wired.

Priority order for the next implementation slice:

1. Decide the shape of the `--check` gate (see the coverage baseline above) before wiring it anywhere. A gate that cannot pass is a gate nobody turns on.
2. Work the `missing` list by resource, starting with `accounts` and `people` (8 and 7 operations respectively); the workspace slice is now largely wired, with only two documented routes known to 404 on this tenant.
3. Decide whether `dataset` needs mutating lifecycle commands (create/update/delete), or should stay a read-only surface; only list/count/detail reads are wired today.
4. Extend edge-case live tests (invalid id, empty page, pagination boundary) to the families that don't have them yet: `dataset`, `webhookFlowTask`, `workspace`, and `scheduling`.
