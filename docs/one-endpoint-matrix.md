# Alteryx One endpoint matrix

A per-endpoint probe ledger for the Alteryx One surface (`ayx one ...`), with live evidence — not
just what the CLI is *supposed* to call, but what a real tenant actually returned when asked.

Most of the other One docs (`docs/command-surface.md`, `docs/one-backend-inventory.md`, and the
`one inventory` / `one api coverage` commands themselves) describe *what is wired*, not what a live
call returned. `docs/one-live-validation.md` and `docs/one-api-surface-audit.md` come closer — both
already track family-level live results, and `one-api-surface-audit.md`'s Phase 4 ("Dead Routes")
already flagged `billing`/`plans`/`scheduling` as 404 on its test workspace back on 2026-06-22,
open item: "Validate against an enterprise workspace before deciding whether these are bugs... or
genuinely tier-gated." **That question is now closed, and the answer was neither.** `GET
/v4/open-api-spec` (172 live paths) confirmed `/plans/v1` and `/scheduling/v1` were simply wrong
paths — the spec-documented `/v4/plans` and `/v4/schedules` exist and are what the web UI calls —
and the spec is not entitlement-filtered (this tenant lacks the Plans entitlement, yet 22
`/v4/plan*` paths still appear in its spec), so a 404 here was never evidence of tier-gating in the
first place. Both were repointed. `billing` had no `/v4` equivalent anywhere in the spec and no
usage/credit/license/quota route either; it was removed rather than kept as a permanently-failing
command. What none of the
existing docs record is **per-endpoint** evidence: exact error-body shape, whether a `list` command
reporting `"ok": true` can be trusted, and a literal command to re-run to check again. That gap is
why `one connections permissions` shipped for a full release cycle calling
`/v4/connections/{id}/permissions` — a path the API answers with `RouteNotFoundException` — while
`one inventory` faithfully mirrored the same wrong path back as fact (`docs/one-backend-inventory.md`
still lists that pre-fix path today — see Caveats). This doc exists to make that class of bug visible
before it ships, and to give the next person a ledger of live probe evidence instead of a blank slate.

Rows are derived mechanically from `ayx-one-api/src/inventory.rs` (`inventory_endpoints_full()`) —
see `ayx-rs/tests/one_endpoint_matrix_doc.rs` for the drift gate that keeps this file and the
inventory from diverging. **Do not hand-edit rows out of sync with the inventory** — regenerate them
(see below) and only hand-edit the `Live status` / `Verified (UTC)` / `Notes` evidence columns.

## How to re-verify

All commands below are read-only (`GET`, or a `list`/`count`/`detail`/`status`/`current` leaf) and
safe to run against a live tenant with no `--apply`. Never pass `--apply` while re-verifying this
doc; never run a `create`/`update`/`delete`/`run`/`cancel`/`enable`/`disable`/`publish`/`transfer`
command for this purpose.

```bash
# One-time: authenticate a profile (skip if already logged in). Prefer a
# keyring-backed OAuth refresh credential for unattended validation:
ayx one login --profile <profile> --auth-method oauth-refresh \\
  --refresh-token-env AYX_ONE_API_REFRESH_TOKEN --secret-policy secure

# Spot-check a representative command per surface (mirrors the live sweep this doc was built from)
ayx one workspace current --output json
ayx one workspace list --output json
ayx one person current --output json
ayx one person list --output json
ayx one token --output json
ayx one doctor discover --output json
ayx one doctor plans --output json
ayx one doctor scheduling --output json
ayx one plans list --output json
ayx one plans count --output json
ayx one flows list --output json
ayx one flows folders list --output json
ayx one datasets list --output json
ayx one datasets wrangled list --output json
ayx one connections list --output json
ayx one connections detail <connection_id> --output json
ayx one workflows list --output json
ayx one workflows count --output json
ayx one workflows tools --output json
ayx one job-groups list --output json
ayx one job-groups detail <job_group_id> --output json
ayx one output-objects list --output json
ayx one write-settings list --output json
ayx one scheduling list --output json
ayx one api open-api-spec --output json
ayx one api coverage --output json
```

**Reading list-command output**: `ok: true` alone does not prove the underlying route returned
`200`. Those raw transport fields are available with `--output json-full`: check
`data.page_envelopes[].status_code` (or, for the single-shot `detail`/`status`/`count`
shapes, `data.status_code`) — see Methodology below for why.

To regenerate the whole table mechanically instead of hand-verifying one row at a time, dump the
live-wired inventory grouped exactly like this doc's sections:

```bash
cargo test -p ayx-rs --test one_endpoint_matrix_doc -- --nocapture
```

## Column legend

| Column | Meaning |
|---|---|
| Method / Path | The literal `(METHOD, path-template)` pair from `inventory.rs`. `{param}` segments are path templates, not literal values. |
| Live status | What a real request against a live tenant returned in the most recent probe: `live 200`, `live 404 RouteNotFoundException`, `live 403 permission_denied`, etc. `unverified` means no live probe has recorded this row. |
| Verified (UTC) | When the `Live status` was last confirmed against a real tenant. `not probed this session` for rows with no evidence at all. |
| ayx command(s) | Every `ayx one ...` command that dispatches this endpoint (mirrors `inventory.rs`'s `commands` slice — one endpoint can back more than one command). |
| Response shape | The coarse shape of a successful response: paginated list, single object, or count object. Not a full schema. |
| Error-body flavor | What an error response looks like on this path family — the fact this doc exists to record. `json:X` names the JSON exception/error shape; `html:express` means an unrouted Express default 404 page instead of JSON. |
| Notes | Live evidence, caveats, and cross-references. |

## Implemented surfaces

Endpoints the CLI fully dispatches for this surface (`inventory.rs` `SURFACES`).

### platform.iam (implemented)

> Managed IAM / workspace-admin surface.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/workspaces/current` | live 200 | 2026-08-14T16:06Z | `one workspace current` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one workspace current` returned 200. Phase 2 `one doctor discover` also completed; its nested checks are recorded on the underlying endpoint rows. |
| GET | `/v4/workspaces/{workspaceId}` | unverified | not probed | `one workspace detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Added in 0.20.0; previously reachable only from the `ayx tui` One browser, which was removed in the same release. `workspaceId` is the numeric id, as for `admins`. |
| GET | `/v4/workspaces/{id}/configuration` | live 200 | 2026-07-27T00:55Z | `one workspace configuration`<br>`one workspace configuration-v4` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Probed both as `one workspace configuration-v4 <workspace-id>` and `one workspace configuration <workspace-id>`. |
| GET | `/v4/people` | live 200 | 2026-08-18T17:55Z | `one person list`<br>`one workspace people` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Current live probe returned 18 people. Both commands, and telemetry's `permissions workflows/summary --source one`, use this header-scoped route with `x-alteryx-workspace-gid`; the old `/v4/workspaces/{id}/people` route returns 404. |
| GET | `/v4/workspaces/{workspaceId}/admins` | declared in the tenant OpenAPI spec; live re-verification pending | 2026-08-31 | `one workspace admins` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `workspaceId` is the **numeric** workspace id (integer in the spec), not the workspace GID — an earlier GID probe 404'd and was misread as "route does not exist", which put this command on `/v4/people?role=admin`. That route is not a substitute: the gateway ignores `role=admin` and `/v4/people` only sets `isAdmin` on the caller's own record. Optional query params: `accountId`, `fields`, `includeStatus`. |
| GET | `/v4/workspaces/{id}/groups` | live 200 | 2026-08-18T18:08Z | `one workspace groups` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Numeric disposable workspace returned 200. |
| GET | `/v4/groups` | live 200 | 2026-08-18T18:08Z | `one workspace groups-global` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Live read-only probe returned 200. |
| GET | `/v4/workspaces/{id}/invitationLink` | live 200 | 2026-08-18T18:08Z | `one workspace invitation-link` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | The command supplies required query `personId`; synthetic workspace and person fixtures returned 200. |
| GET | `/v4/workspaces/{workspaceId}/cloudConfigs` | live 200 | 2026-08-18T18:08Z | `one workspace cloud-configs` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | A numeric disposable workspace returned 200. |
| POST | `/v4/workspaces/{id}/people/batch` | unverified | not probed this session | `one workspace invite-users`<br>`one workspace invite-list` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed (would invite real users). |
| DELETE | `/v4/workspaces/{workspaceId}/people/{id}` | unverified | not probed this session | `one workspace remove-user` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| POST | `/v4/workspaces/{id}/people/suspend` | unverified | not probed since repoint | `one workspace suspend-users` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Repointed from `/iam/v1/workspaces/{id}/people/suspend` (in no spec, live 404 RouteNotFoundException) to the spec-documented path. Not yet re-probed live. |
| POST | `/v4/workspaces/{id}/people/unsuspend` | unverified | not probed since repoint | `one workspace unsuspend-users` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Repointed from `/iam/v1/workspaces/{id}/people/unsuspend` (in no spec, live 404 RouteNotFoundException) to the spec-documented path. Not yet re-probed live. |
| PATCH | `/v4/workspaces/{id}/transfer` | unverified | not probed this session | `one workspace transfer` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Corrected to the live spec method. Mutating — not probed. |
| GET | `/v4/authorization/roles` | live 200 | 2026-09-01T21:41Z | `one role list` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Current live canary confirmed the least-privileged `Viewer` policy `25704008` is present. |
| GET | `/v4/authorization/roles/{id}` | live 404 not_found | 2026-08-18T18:08Z | `one role detail` | object: raw API resource body, JSON-passthrough | json:DataNotFoundException | Fake role id `0` returned application-level 404 `DataNotFoundException`, confirming the route is reached. |
| GET | `/v4/authorization/roles/{id}/people` | live 403 permission_denied | 2026-09-01T21:44Z | `one role list-assignments` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:AccessControlException | Current canary assigned Viewer to a temporary group, then confirmed this assignment-list read remains scope-blocked with HTTP 403; the CLI reports `blocked_by_scope`, not a command failure. |
| PUT | `/v4/authorization/roles/{id}/people` | unverified | not probed this session | `one role assign` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Corrected to the live spec contract; sends a bare JSON subject-id array (`[subjectId]`). Mutating — not part of the temporary-group role path tested below. |
| DELETE | `/v4/authorization/roles/{id}/people/{subjectId}` | unverified | not probed this session | `one role unassign` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |

### misc (implemented)

> The OpenAPI spec is now exposed through the CLI.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/open-api-spec` | live 200 | 2026-08-20T18:33Z | `one api coverage`<br>`one api open-api-spec` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Current coverage: `coverage_pct: 63.1`, `inventory_operations: 172`, `inventory_total: 172`, `spec_operations: 233`, `missing: 86`, `stale: 16`, and 7 `outside_spec_namespace` rows. `stale[].commands` is an array. |

### plan (implemented)

> Only the /v4 plan endpoints the CLI actually dispatches are listed. Read paths (list/count/run/permissions/package/runParameters/schedules) now use the spec-documented /v4 plan paths instead — see the `plans` surface.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| POST | `/v4/plans` | live 201 | 2026-08-18T20:26Z | `one plans create` | object / dry-run envelope | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Created disposable minimal plan `156184`, inspected it, and deleted it. |
| POST | `/v4/plans/{id}/permissions` | unverified | not probed this session | `one plans share` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| PATCH | `/v4/plans/{id}` | unverified | not probed this session | `one plans update` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| DELETE | `/v4/plans/{id}` | live 204 | 2026-08-18T20:26Z | `one plans delete` | object / dry-run envelope | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Deleted disposable plan `156184`; list returned to the original four-plan baseline. |

### plans (implemented)

> Managed plans surface.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/plans` | live 200 | 2026-08-18T17:55Z | `one plans list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Repointed from `/plans/v1/plans`; current live probe returned 4 plans. |
| GET | `/v4/plans/{id}/full` | live 200 | 2026-08-18T17:55Z | `one plans detail`<br>`one plans full` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Real plan `128557` returned 200 after the `/v4` repoint. |
| POST | `/v4/plans/{id}/run` | unverified | not probed since repoint | `one plans run` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Repointed from `/plans/v1/plans/{id}/run` (in no spec, live 404 RouteNotFoundException) to the spec-documented path. Not yet re-probed live. |
| GET | `/v4/plans/count` | live 200 | 2026-08-18T17:55Z | `one plans count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Repointed from `/plans/v1/plans/count`; current live probe returned 200. |
| GET | `/v4/plans/{id}/runParameters` | live 200 | 2026-08-18T17:55Z | `one plans run-parameters` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Real plan `128557` returned 200 after the `/v4` repoint. |
| GET | `/v4/plans/{id}/schedules` | live 200 | 2026-08-18T17:55Z | `one plans schedules` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Real plan `128557` returned 200 after the `/v4` repoint. |
| GET | `/v4/plans/{id}/package` | live 200 | 2026-08-18T17:55Z | `one plans export` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Real plan `128557` returned 200 after the `/v4` repoint. |
| POST | `/v4/plans/package` | unverified | not probed since repoint | `one plans import` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Repointed from `/plans/v1/plans/package` (in no spec, live 404 RouteNotFoundException) to the spec-documented path. Not yet re-probed live. |
| GET | `/v4/plans/{id}/permissions` | live 200 | 2026-08-18T17:55Z | `one plans permissions` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Real plan `128557` returned 200 after the `/v4` repoint. |
| DELETE | `/v4/plans/{id}/permissions/{subjectId}` | unverified | not probed since repoint | `one plans permissions` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Repointed from `/plans/v1/plans/{id}/permissions/{subjectId}` (in no spec, live 404 RouteNotFoundException) to the spec-documented path. Not yet re-probed live. |

### workflow (implemented)

> Alteryx One cloud-native (canvas) workflows, ULID-keyed, served by /svc-workflow.

> Distinct from the `flow` surface, which is Designer Cloud /v4/flows keyed by integer ids.

> detail and count are synthesized client-side; the API exposes no per-id or count route.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/workflows` | live 200 | 2026-09-01T21:44Z | `one workflows list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Current all-pages list supplied the cloud-native source baseline; final workflow IDs matched it after copy/delete. |
| GET | `/v4/workflows?limit=1` | live 200 (client-synthesized count) | 2026-08-14T16:10Z | `one workflows count` | object: `{ count, count_source: "server" }` (client-synthesized total) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Phase 2 returned trustworthy client-synthesized `count: 101`; the endpoint's list envelope supplied the server total. |
| GET | `/svc-workflow/api/v1/assets` | live 200 | 2026-09-01T21:44Z | `one workflows assets`<br>`one workflows detail` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:NotFoundError (`{ kind: "NotFoundError", message, errors: [], params: {} }`) for a valid route / unknown resource id | Current cloud-native asset list supplied a real ULID used for detail and the dependent endpoint probes. |
| GET | `/svc-workflow/api/v1/assets/{id}/dependencies` | live 200 (real id) / live 404 json:NotFoundError (bad id) | 2026-09-01T21:44Z | `one workflows dependencies` | object: raw API resource body, JSON-passthrough | json:NotFoundError (`{ kind: "NotFoundError", message, errors: [], params: {} }`) for a valid route / unknown resource id | A real cloud-native ULID returned 200. |
| GET | `/svc-workflow/api/v0/workflows/{id}/availableEngines` | live 200 | 2026-09-01T21:44Z | `one workflows engines` | object: raw API resource body, JSON-passthrough | json:NotFoundError (`{ kind: "NotFoundError", message, errors: [], params: {} }`) for a valid route / unknown resource id | A real cloud-native ULID returned 200. |
| GET | `/svc-workflow/api/v1/tools` | live 200 | 2026-09-01T21:44Z | `one workflows tools` | object: raw API resource body, JSON-passthrough | json:NotFoundError (`{ kind: "NotFoundError", message, errors: [], params: {} }`) for a valid route / unknown resource id | Current cloud-native tools inspection returned 200. |
| POST | `/svc-workflow/api/v1/workflows/{id}/run` | live 200 | 2026-09-02 | `one workflows run` | object: provider run/job result (`jobId`, `jobStatus`, `jobgroupId`) / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:BadRequestError (`{ kind: "BadRequestError", message, errors: [], params: {} }`) for an invalid workflow | Disposable copy of `ayx-rs-build (rc-demo)` was queued after a successful dry-run; provider returned job `4039768` and job group `4398357`. The run completed and the disposable workflow was deleted. |
| POST | `/svc-workflow/api/v1/jobs/{id}/cancel` | live 404 capability-blocked | 2026-09-02 | `one workflows cancel` | object: provider cancellation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:NotFoundError (`{ kind: "NotFoundError", message, errors: [], params: {} }`) | `id` is the job id returned by `one workflows run`, not the workflow definition ULID. The route is real, but this workspace returned `WFS Jobs is not enabled in this environment`; cancellation remains blocked by provider capability here. |
| POST | `/svc-workflow/api/v2/workflows/{id}/duplicate` | live 2xx | 2026-09-01T21:44Z | `one workflows copy` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:NotFoundError (`{ kind: "NotFoundError", message, errors: [], params: {} }`) for a valid route / unknown resource id | One disposable cloud-native copy was created after a successful dry-run, inspected, then deleted by captured ID; the final workflow baseline matched. |
| POST | `/svc-workflow/api/v2/workflows/{id}/share` | not probed | unverified | `one workflows share` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:NotFoundError (`{ kind: "NotFoundError", message, errors: [], params: {} }`) for a valid route / unknown resource id | Mutating; deliberately not probed — a live call shares a real workflow with a real person and sends mail. Body shape is recorded under Contracts below, recovered from the service's own schema-validation errors rather than a published spec. `one workflows share` gates it behind `--apply` and resolves `--to-person` emails to person ids before the dry-run body is shown. |
| DELETE | `/svc-workflow/api/v2/workflows/{id}` | live 200 | 2026-09-01T21:44Z | `one workflows delete` | empty object `{}` on success (HTTP 200) / dry-run shape (`{ dry_run, mutating, would_send: null }` when not `--apply`) | json:NotFoundError (`{ kind: "NotFoundError", message, errors: [], params: {} }`) for a valid route / unknown resource id | Deleted the disposable copy by ID; final cloud-native workflow IDs matched the baseline. |

### agentAssets (private-preview)

> Agent Studio asset registration and prompt routes recovered from authenticated
> Agent Studio UI traffic. These routes are not part of the public One OpenAPI
> specification; all rows remain unverified until a fresh live credential is
> available.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/ai-agents/backend/agents` | unverified | not probed this session | `one agent-assets agents list` | object: Agent Studio list response | unverified | Private-preview route recovered from authenticated Agent Studio HAR. |
| POST | `/ai-agents/backend/agents` | unverified | not probed this session | `one agent-assets agents create` | object: agent result / dry-run shape | unverified | Mutating; private-preview route recovered from the create form bundle. |
| GET | `/ai-agents/backend/agents/{id}` | unverified | not probed this session | `one agent-assets agents detail` | object: agent detail response | unverified | Private-preview route recovered from authenticated Agent Studio traffic. |
| POST | `/copilot/v2/conversations` | unverified | not probed this session | `one agent-assets agents prompt` | object: conversation response | unverified | Starts a Copilot conversation for an Agent Studio agent; applying a prompt may invoke tools. |
| POST | `/copilot/v2/chats` | unverified | not probed this session | `one agent-assets agents prompt` | object: chat response | unverified | Posts the prompt text to the newly-created conversation; the CLI uses the non-streaming path. |
| PATCH | `/ai-agents/backend/agents/{id}` | unverified | not probed this session | `one agent-assets agents update` | object: agent result / dry-run shape | unverified | Mutating; private-preview route recovered from the create form bundle. |
| DELETE | `/ai-agents/backend/agents/{id}` | unverified | not probed this session | `one agent-assets agents delete` | object: mutation result / dry-run shape | unverified | Mutating; private-preview route recovered from authenticated Agent Studio traffic. |
| GET | `/ai-agents/backend/agents/ayx-datasets` | unverified | not probed this session | `one agent-assets datasets list`<br>`one agent-assets datasets set` | object: dataset registration list | unverified | Dataset lookup used before the MCP-enabled PATCH. |
| PATCH | `/ai-agents/backend/agents/ayx-datasets/{id}/mcp-enabled` | unverified | not probed this session | `one agent-assets datasets set` | object: mutation result / dry-run shape | unverified | Mutating; toggles the Agent Studio Insights/MCP registration state. |
| GET | `/ai-agents/backend/agentyx/workflows` | unverified | not probed this session | `one agent-assets workflows list` | object: workflow list | unverified | Private-preview workflow registration route. |
| GET | `/ai-agents/backend/agentyx/tools` | unverified | not probed this session | `one agent-assets workflows list`<br>`one agent-assets workflows disable` | object: workflow shortcut list | unverified | Used to report and remove Apps shortcuts. |
| GET | `/ai-agents/backend/agentyx/toolCreations` | unverified | not probed this session | `one agent-assets workflows list` | object: asynchronous creation-job list | unverified | Used to report shortcut-registration jobs. |
| POST | `/ai-agents/backend/agentyx/toolCreations` | unverified | not probed this session | `one agent-assets workflows enable` | object: asynchronous creation job / dry-run shape | unverified | Mutating; registration returns a job that the CLI polls. |
| GET | `/ai-agents/backend/agentyx/toolCreations/{id}` | unverified | not probed this session | `one agent-assets workflows enable` | object: asynchronous creation-job status | unverified | Polled until completion or timeout. |
| DELETE | `/ai-agents/backend/agentyx/tools/{id}` | unverified | not probed this session | `one agent-assets workflows disable` | object: mutation result / dry-run shape | unverified | Mutating; removes an Apps shortcut. |
| POST | `/v4/importedDatasets` | unverified | not probed this session | `one datasets create` | object: imported-dataset reference / dry-run shape | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Creates a URI-backed imported-dataset reference; local file staging remains UI-oriented. |
| POST | `/svc-workflow/api/v1/workflows` | unverified | not probed this session | `one workflows upload` | object: workflow upload result / dry-run shape | unverified | Uploads a cloud-native workflow package; mutating and preview-first. |

### flow (implemented)

> Flow lifecycle, package, parameters, library, folder, and permission commands are wired.

> The One surface does not expose arbitrary workflow authoring through this family.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| POST | `/v4/flows` | unverified | not probed this session | `one flows create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/flows` | live 200 (0 items) | 2026-08-14T16:09Z | `one flows list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Phase 2 confirmed a genuine 200 with an empty page (`page_envelopes[0].status_code: 200`), the current zero-flow baseline for the blocked Phase 5a cycle. |
| GET | `/v4/flows/count` | live 200 | 2026-07-27T00:55Z | `one flows count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one flows count`. |
| GET | `/v4/flowsLibrary` | live 200 | 2026-07-27T00:55Z | `one flows library list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one flows library list`. |
| GET | `/v4/flowsLibrary/count` | live 200 | 2026-07-27T00:55Z | `one flows library count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one flows library count`. |
| GET | `/v4/folders` | live 200 (0 items) | 2026-08-14T16:09Z | `one flows folders list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Phase 2 confirmed a genuine 200 with `response: {"data": []}` and no folder fixtures. The Phase 1 pagination test's `pages_fetched` assertion does not tolerate this valid empty-result shape. |
| GET | `/v4/folders/count` | live 200 | 2026-07-27T00:55Z | `one flows folders count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one flows folders count`. |
| GET | `/v4/folders/{id}` | unverified | not probed this session | `one flows folders detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No folder id available (list is empty). |
| POST | `/v4/folders` | unverified | not probed this session | `one flows folders create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| PATCH | `/v4/folders/{id}` | unverified | not probed this session | `one flows folders update` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed; also no folder id. |
| DELETE | `/v4/folders/{id}` | unverified | not probed this session | `one flows folders delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed; also no folder id. |
| GET | `/v4/folders/{id}/flows` | unverified | not probed this session | `one flows folders flows list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No folder id available. |
| GET | `/v4/folders/{id}/flows/count` | unverified | not probed this session | `one flows folders flows count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No folder id available. |
| GET | `/v4/flows/{id}` | unverified | not probed this session | `one flows detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No flow id available (list is empty). |
| PATCH | `/v4/flows/{id}` | unverified | not probed this session | `one flows update` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| DELETE | `/v4/flows/{id}` | unverified | not probed this session | `one flows delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| POST | `/v4/flows/{id}/copy` | unverified | not probed this session | `one flows copy` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| POST | `/v4/flows/{id}/run` | unverified | not probed this session | `one flows run` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/flows/{id}/validate` | unverified | not probed this session | `one flows validate` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No flow id available. |
| GET | `/v4/flows/{id}/recipeParameters` | live 403 permission_denied | 2026-08-18T17:55Z | `one flows parameters` | object: raw API resource body, JSON-passthrough | json:AccessControlException | Fake flow id `999999` reached the authenticated route and returned 403 `permission_denied`; dynamic endpoint appends `?outputObjectType=`. |
| GET | `/v4/flows/{id}/inputs` | unverified | not probed this session | `one flows inputs` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No flow id available. |
| GET | `/v4/flows/{id}/outputs` | unverified | not probed this session | `one flows outputs` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No flow id available. |
| POST | `/v4/flows/{id}/permissions` | unverified | not probed this session | `one flows permissions` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/flows/{id}/permissions` | live 403 permission_denied | 2026-08-18T17:55Z | `one flows permissions-get` | object: raw API resource body, JSON-passthrough | json:AccessControlException | Fake flow id `999999` reached the authenticated route and returned 403 `permission_denied`. |
| POST | `/v4/flows/{id}/move` | unverified | not probed this session | `one flows move` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| PATCH | `/v4/flows/{id}/replaceDataset` | unverified | not probed this session | `one flows replace-dataset` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| POST | `/v4/flows/package` | unverified | not probed this session | `one flows import` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating, requires a `.yxzp` file input — not probed. |
| POST | `/v4/flows/package/dryRun` | unverified | not probed this session | `one flows import-dry-run` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Requires a `.yxzp` file input — not probed. |
| GET | `/v4/flows/{id}/package` | unverified | not probed this session | `one flows export` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No flow id available. |
| GET | `/v4/flows/{id}/package/dryRun` | unverified | not probed this session | `one flows export-dry-run` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No flow id available. |

### scheduling (implemented)

> Managed scheduling surface.

The lifecycle commands use the live OpenAPI routes for schedule create, update, and delete. Every applied schedule mutation is confirmation-gated; non-interactive runs must add `--yes`. Destructive live validation is reserved for disposable schedules.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| POST | `/v4/schedules` | live 201 | 2026-08-18T20:24Z | `one scheduling create` | object / dry-run envelope | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Created and later deleted a disposable schedule; live gateway accepted a daily trigger and rejected the published `oneTime` variant. |
| GET | `/v4/schedules` | live 200 | 2026-08-18T17:55Z | `one scheduling list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Repointed from `/scheduling/v1/schedules`; current live probe returned 1 schedule. |
| GET | `/v4/schedules/{id}` | live 200 | 2026-08-18T17:55Z | `one scheduling detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | A pre-existing schedule returned 200 after the `/v4` repoint. |
| PUT | `/v4/schedules/{id}` | live 200 | 2026-08-18T20:24Z | `one scheduling update` | object / dry-run envelope | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Renamed a disposable schedule; uses the spec-documented PUT route. |
| POST | `/v4/schedules/{id}/enable` | live 200 | 2026-08-18T20:24Z | `one scheduling enable` | object / dry-run envelope | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Live route exercised by the create baseline (`enabled: true`). |
| POST | `/v4/schedules/{id}/disable` | live 200 | 2026-08-18T20:24Z | `one scheduling disable` | object / dry-run envelope | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Disabled the disposable schedule; detail confirmed `enabled: false`. |
| DELETE | `/v4/schedules/{id}` | live 204 | 2026-08-18T20:24Z | `one scheduling delete` | object / dry-run envelope | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Deleted the disposable schedule; list returned to the original one-schedule baseline. Per-ID detail remained eventually consistent and served the deleted record. |
| GET | `/v4/schedules/count` | live 200 | 2026-08-18T17:55Z | `one scheduling count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Repointed from `/scheduling/v1/schedules/count`; current live probe returned 200. |

## Partial surfaces

Endpoints where only some of the surface's API is wired; the rest stays documented-only until the
CLI needs it (`inventory.rs` `PARTIAL_SURFACES`).

### connection (partial)

> Connection lifecycle, dry-run, status, and permissions commands are wired.

> Connector metadata defaults, current values, and overrides are wired for JDBC behavior control.

> Credential-backend specifics remain encoded in the API payloads rather than a local domain model.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/connections` | live 200 | 2026-09-01T21:35Z | `one connections list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Non-legacy canary captured the connection baseline and confirmed the final list returned exactly to it. |
| GET | `/v4/connections/count` | live 200 | 2026-07-27T00:55Z | `one connections count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one connections count`. |
| POST | `/v4/connections` | live 201 | 2026-09-01T21:35Z | `one connections create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Disposable BigQuery canary connection ID `46938` created after a successful CLI dry-run, using a temporary service-account fixture. |
| POST | `/v4/connections/dryRun` | live 403 permission_denied | 2026-09-01T21:35Z | `one connections dry-run` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:AccessControlException | The current profile reaches the server dry-run route but is scope-blocked with HTTP 403; the CLI's local dry-run completed before the live create. |
| GET | `/v4/connections/{id}` | live 200 | 2026-09-01T21:35Z | `one connections detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Detail for disposable connection ID `46938` returned 200. |
| GET | `/v4/connections/{id}/status` | live 200 | 2026-09-01T21:35Z | `one connections status` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Disposable connection ID `46938` returned status result `SUCCESS`. |
| PATCH | `/v4/connections/{id}` | unverified | not probed this session | `one connections update` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| DELETE | `/v4/connections/{id}` | live 204 | 2026-09-01T21:35Z | `one connections delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Deleted disposable connection ID `46938`; final connection IDs matched the baseline. |
| GET | `/v4/connections/{id}/permissions/sharedSubjects` | live 200 | 2026-08-14T16:15Z | `one connections permissions`<br>`one connections permissions detail` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | A disposable connection returned 200, not `RouteNotFoundException`; confirms the repaired `/permissions/sharedSubjects` route. |
| POST | `/v4/connections/share` | live 201 | 2026-08-18T20:26Z | `one connections permissions create` | object / dry-run envelope | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Shared a disposable connection with two disposable people, verified, then revoked. Empty subject buckets are omitted because the live API rejects them. |
| DELETE | `/v4/connections/share` | live 204 | 2026-08-18T20:26Z | `one connections permissions delete` | object / dry-run envelope | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Revoked both disposable shares; final list returned to the original four entries. |
| GET | `/v4/connectorMetadata/{connector}/defaults` | live 200 | 2026-07-27T00:55Z | `one connections connector-metadata defaults`<br>`one connections connector-metadata template` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one connections connector-metadata defaults bigquery` (connector id taken from the `vendor` field of a live connection, since the connections-list payload has no literal `connectorId` field). Also backs `connector-metadata template`. |
| GET | `/v4/connectorMetadata/{connector}` | live 200 | 2026-07-27T00:55Z | `one connections connector-metadata detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one connections connector-metadata detail bigquery`. |
| GET | `/v4/connectorMetadata/{connector}/publish/info` | live 403 permission_denied | 2026-07-27T00:55Z | `one connections connector-metadata publish-info` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one connections connector-metadata publish-info bigquery` → 403 `AccessControlException`. Route exists and is reached authenticated; this profile's role just isn't authorized for it. |
| GET | `/v4/connectorMetadata/{connector}/overrides` | live 200 | 2026-07-27T00:55Z | `one connections connector-metadata overrides list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one connections connector-metadata overrides list bigquery`. |
| POST | `/v4/connectorMetadata/{connector}/overrides` | unverified | not probed this session | `one connections connector-metadata overrides create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| DELETE | `/v4/connectorMetadata/{connector}/overrides` | unverified | not probed this session | `one connections connector-metadata overrides delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |

### dataset (partial)

> Dataset library list/count plus wrangled and imported dataset detail reads are wired.

> Mutating dataset lifecycle operations remain documented-only in this first cut.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/datasetLibrary` | live 400 ApiValidationFailed — CLI wiring gap | 2026-08-14T16:09Z | `one datasets list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Phase 2 reconfirmed 400 `ApiValidationFailed` before the CLI fix: `datasetsFilter` must not be null. This branch now defaults `datasetsFilter=all` and accepts repeated/comma-delimited filters. |
| GET | `/v4/datasetLibrary/count` | live 200 | 2026-07-27T00:55Z | `one datasets count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one datasets count`. |
| GET | `/v4/wrangledDatasets` | live 200 (0 items) | 2026-08-14T16:09Z | `one datasets wrangled list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Phase 2 returned a genuine 200 with 0 items. |
| GET | `/v4/wrangledDatasets/count` | live 200 | 2026-07-27T00:55Z | `one datasets wrangled count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one datasets wrangled count`. |
| GET | `/v4/wrangledDatasets/{id}` | unverified | not probed this session | `one datasets wrangled detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No wrangled dataset id available (list is empty). |
| GET | `/v4/importedDatasets/{id}` | unverified | not probed this session | `one datasets imported detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No imported dataset id available. |

### jobGroup (partial)

> Job-group execution, publish, and inspection commands are wired.

> PDF/log artifact downloads and other deeper job-library paths remain documented-only.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/jobLibrary` | live 200 | 2026-08-14T16:11Z | `one job-groups list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Phase 2 returned 25 items. |
| GET | `/v4/jobLibrary/count` | live 200 | 2026-07-27T00:55Z | `one job-groups count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one job-groups count`. |
| POST | `/v4/jobGroups` | unverified | not probed this session | `one job-groups run` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating (`one job-groups run`) — not probed. |
| PUT | `/v4/jobGroups/{id}/publish` | unverified | not probed this session | `one job-groups publish` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/jobGroups/{id}` | live 200 | 2026-08-14T16:11Z | `one job-groups detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Phase 2 detail of a disposable job group returned 200. |
| POST | `/v4/jobGroups/{id}/cancel` | unverified | not probed this session | `one job-groups cancel` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/jobGroups/{id}/status` | live 200 | 2026-07-27T00:55Z | `one job-groups status` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Disposable job-group status returned 200. |
| GET | `/v4/jobGroups/{id}/inputs` | live 400 DataServiceInvalidRequest | 2026-07-27T00:55Z | `one job-groups inputs` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | A disposable job group's inputs call returned 400, `"Illegal Argument: Only Jdbc sources have connect String"`. Route exists; this is a data-shape-specific validation error, not a CLI wiring bug. |
| GET | `/v4/jobGroups/{id}/pdfResults` | live 400 ProfilingDataNotFoundException | 2026-07-27T00:55Z | `one job-groups pdf-results` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | The disposable job group returned 400 because it has no profiling data. Route exists; the fixture simply has no profiling artifact. |
| GET | `/v4/jobGroups/{id}/outputs` | live 200 | 2026-07-27T00:55Z | `one job-groups outputs` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Disposable job-group outputs returned 200. |
| GET | `/v4/jobGroups/{id}/jobs` | live 200 | 2026-07-27T00:55Z | `one job-groups jobs` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Disposable job-group jobs returned 200. |
| GET | `/v4/jobGroups/{id}/publications` | live 200 | 2026-07-27T00:55Z | `one job-groups publications` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Disposable job-group publications returned 200. |
| GET | `/v4/jobGroups/{id}/profile` | live 400 ProfilingDataNotFoundException | 2026-07-27T00:55Z | `one job-groups profile` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | The disposable job group returned 400 for the same "no profiling data" reason as `pdfResults`. |
| GET | `/v4/jobGroups/{id}/profileResults` | live 400 ProfilingDataNotFoundException | 2026-07-27T00:55Z | `one job-groups profile-results` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | The disposable job group returned 400 for the same "no profiling data" reason. |

### outputObject (partial)

> Output object lifecycle and wrangle-to-python commands are wired.

> Additional nested resources stay documented-only until the CLI needs them.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/outputObjects` | live 200 (0 items) | 2026-08-14T16:12Z | `one output-objects list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Phase 2 returned a genuine 200 with 0 items. |
| GET | `/v4/outputObjects/count` | live 200 | 2026-07-27T00:55Z | `one output-objects count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one output-objects count`. |
| POST | `/v4/outputObjects` | unverified | not probed this session | `one output-objects create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/outputObjects/{id}` | unverified | not probed this session | `one output-objects detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No output object id available (list is empty). |
| PATCH | `/v4/outputObjects/{id}` | unverified | not probed this session | `one output-objects update` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| DELETE | `/v4/outputObjects/{id}` | unverified | not probed this session | `one output-objects delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/outputObjects/{id}/inputs` | unverified | not probed this session | `one output-objects inputs` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No output object id available. |
| POST | `/v4/outputObjects/{id}/wrangleToPython` | validated dry-run (no network) | 2026-08-14T16:15Z | `one output-objects wrangle-to-python` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Phase 3 with fake id `999999` and no `--body`/`--apply` returned `{dry_run: true, mutating: true, would_send: null}`; the apply gate short-circuited before any network call. |

### webhookFlowTask (partial)

> Webhook task create/read/delete plus webhook test are wired.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| POST | `/v4/webhookFlowTasks` | unverified | not probed this session | `one webhook-flow-tasks create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/webhookFlowTasks/{id}` | unverified | not probed this session | `one webhook-flow-tasks detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No read-only discovery command exists for this surface (create/detail/delete/test only, no `list`) — cannot resolve a live id without first creating one, which is mutating. |
| DELETE | `/v4/webhookFlowTasks/{id}` | unverified | not probed this session | `one webhook-flow-tasks delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| POST | `/v4/webhooks/test` | unverified | not probed this session | `one webhook-flow-tasks test` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating (fires a real webhook test) — not probed. |

### writeSetting (partial)

> Write-setting CRUD is wired.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/writeSettings` | live 200 (0 items) | 2026-08-14T16:12Z | `one write-settings list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Phase 2 returned a genuine 200 with 0 items. |
| GET | `/v4/writeSettings/count` | live 200 | 2026-07-27T00:55Z | `one write-settings count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one write-settings count`. |
| POST | `/v4/writeSettings` | unverified | not probed this session | `one write-settings create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/writeSettings/{id}` | unverified | not probed this session | `one write-settings detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No write setting id available (list is empty). |
| PATCH | `/v4/writeSettings/{id}` | unverified | not probed this session | `one write-settings update` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| DELETE | `/v4/writeSettings/{id}` | unverified | not probed this session | `one write-settings delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |

### apiAccessTokens (partial)

> One API access-token CRUD is wired; additional token administration endpoints remain documented-only.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/apiAccessTokens` | live 200 | 2026-09-01T21:47Z | `one auth diagnose`<br>`one auth status`<br>`one doctor auth`<br>`one token` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Ephemeral canary list was read before and after create/delete; final token IDs matched the baseline. |
| POST | `/v4/apiAccessTokens` | live 201 | 2026-09-01T21:36Z | `one token create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Created an ephemeral token with the documented name, description, and 86400-second lifetime after a successful CLI dry-run; ID was captured, while the secret was never printed or recorded. |
| GET | `/v4/apiAccessTokens/{tokenId}` | live 200 | 2026-09-01T21:47Z | `one token detail` | object: raw API resource body, JSON-passthrough | json:DataNotFoundException | Detail for the captured ephemeral token ID returned 200. |
| DELETE | `/v4/apiAccessTokens/{tokenId}` | live 204 | 2026-09-01T21:48Z | `one token delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Deleted the ephemeral token by ID; it no longer appeared and the final token list matched the baseline. |

### person (partial)

> Current lookup plus person list/count/detail/create/update/patch/delete/password workflows are wired; remaining person families stay documented-only.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/people/current` | live 200 | 2026-08-14T16:07Z | `one person current`<br>`one whoami` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Phase 2 `one person current` returned 200. |
| GET | `/v4/people` | live 200 | 2026-08-18T17:55Z | `one person list`<br>`one workspace people` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Current live probe returned 18 people. Both commands use the header-scoped route with `x-alteryx-workspace-gid`; the old `/v4/workspaces/{id}/people` route returns 404. |
| GET | `/v4/people/current` | live 200 | 2026-08-14T16:07Z | `one person current`<br>`one whoami` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Same Phase 2 evidence as the row above; this duplicate mirrors the inventory faithfully. |
| GET | `/v4/people/count` | live 410 gone | 2026-08-18T17:55Z | `one person count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:GoneException | Vendor scream-test removal: live body reported `error_code: gone`, `GoneException`, and `IAM_SCREAM_PEOPLE`. There is no replacement count endpoint; use `one person list` for enumeration. |
| GET | `/v4/people/{id}` | live 200 | 2026-07-27T00:55Z | `one person detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one person detail <id>` against a real person id from `one person list`. |
| POST | `/v4/people` | unverified | not probed this session | `one person create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| PUT | `/v4/people/{id}` | unverified | not probed this session | `one person update` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| PATCH | `/v4/people/{id}` | unverified | not probed this session | `one person patch` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| DELETE | `/v4/people/{id}` | unverified | not probed this session | `one person delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| PATCH | `/v4/people/current/updatePassword` | unverified | not probed this session | `one person update-password` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| POST | `/v4/passwordresetrequest` | unverified | not probed this session | `one person password-reset-request` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating and side-effecting (sends a real email) — not probed. |

### workspace (partial)

> Workspace lifecycle, groups, configuration, people, transfer, and cloud-config endpoints are wired; two published workspace routes remain known-dead on this tenant.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/workspaces` | live 200 | 2026-08-14T16:06Z | `one workspace list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Phase 2 returned 15 workspaces. |
| GET | `/v4/workspaces/{id}/configuration` | live 200 | 2026-07-27T00:55Z | `one workspace configuration`<br>`one workspace configuration-v4` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Probed both as `one workspace configuration-v4 <workspace-id>` and `one workspace configuration <workspace-id>`. |
| PATCH | `/v4/workspaces/current/transfer` | unverified | not probed this session | `one workspace transfer-assets` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/workspaces/current/configuration` | live 200 | 2026-07-27T00:55Z | `one workspace current-configuration` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one workspace current-configuration`. |
| PATCH | `/v4/workspaces/current/configuration` | unverified | not probed this session | `one workspace save-current-configuration` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| PATCH | `/v4/workspaces/{id}/configuration` | unverified | not probed this session | `one workspace save-configuration-v4` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed (`save-configuration-v4`). |
| GET | `/v4/workspaces/{id}/configuration-schema` | live 200 | 2026-07-27T00:55Z | `one workspace configuration-schema` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one workspace configuration-schema <workspace-id>`. |
| GET | `/v4/workspaces/current/configuration-schema` | live 200 | 2026-07-27T00:55Z | `one workspace current-configuration-schema` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one workspace current-configuration-schema`. |
| POST | `/v4/workspaces/current/delete-configuration` | unverified | not probed this session | `one workspace delete-current-configuration` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating and destructive — not probed under any circumstance. |
| POST | `/v4/workspaces/{id}/delete-configuration` | unverified | not probed this session | `one workspace delete-configuration` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating and destructive — not probed under any circumstance. |
| POST | `/v4/workspaces` | unverified | not probed this session | `one workspace create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — requires a JSON body and creates a workspace. |
| DELETE | `/v4/workspaces/{id}` | unverified | not probed this session | `one workspace delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Destructive — not probed. |
| POST | `/v4/workspaces/{id}/groups` | live 200 | 2026-08-18T20:23Z | `one workspace create-group` | object / dry-run envelope | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Created a disposable group; body requires `name` and `members`. |
| DELETE | `/v4/workspaces/{id}/groups/{groupId}` | live 201 | 2026-08-18T20:23Z | `one workspace delete-group` | object / dry-run envelope | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Deleted the disposable group after removing both users; workspace returned to the original one-group baseline. |
| PUT | `/v4/workspaces/{id}/groups/{groupId}` | unverified | not probed this session | `one workspace update-group` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — JSON body requires `name`. |
| PUT | `/v4/workspaces/{id}/groups/{groupId}/roles` | live 2xx | 2026-09-01T21:44Z | `one workspace set-group-roles` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | End-to-end canary assigned Viewer policy `25704008` to a temporary group and removed it again. Every mutation had a successful CLI dry-run first. |
| POST | `/v4/workspaces/{id}/groups/{groupId}/users` | live 201 | 2026-08-18T20:23Z | `one workspace add-group-users` | object / dry-run envelope | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Added two disposable users using repeated `userIds` query parameters. |
| DELETE | `/v4/workspaces/{id}/groups/{groupId}/users` | live 201 | 2026-08-18T20:23Z | `one workspace remove-group-users` | object / dry-run envelope | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException | Removed both disposable memberships before group deletion. |
| POST | `/v4/workspaces/{id}/people` | unverified | not probed this session | `one workspace invite` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — JSON body requires `email`; target person must be selected before live testing. |
| PATCH | `/v4/workspaces/{id}/people/batch` | unverified | not probed this session | `one workspace reinvite-users` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — JSON body requires `personIds`. |
| PUT | `/v4/workspaces/{id}/people/{personId}/suspended` | unverified | not probed this session | `one workspace suspend-user` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — target person must be selected before live testing. |
| POST | `/v4/workspaces/{workspaceId}/cloudConfigs/{cloudProvider}` | unverified | not probed this session | `one workspace create-cloud-config` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — JSON body is provider-specific. |
| PATCH | `/v4/workspaces/{workspaceId}/cloudConfigs/{cloudProvider}` | unverified | not probed this session | `one workspace update-cloud-config` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — JSON body is provider-specific. |
| PATCH | `/v4/workspaces/{workspaceId}/people/{id}` | unverified | not probed this session | `one workspace patch-user` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — target person must be selected before live testing. |
| PUT | `/v4/workspaces/{workspaceId}/people/{id}` | unverified | not probed this session | `one workspace update-user` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — target person must be selected before live testing. |

## Contracts

Request bodies below are **not** documented in the official spec (`GET /v4/open-api-spec`) or in
`docs/command-surface.md`. They were recovered from the services' own schema-validation errors —
i.e. by sending an intentionally-empty or wrong-shaped body and reading back the `400
ApiValidationFailed` response, which names the missing/invalid fields. That is the only place these
shapes are recorded; treat this section as load-bearing, not decorative.

### `POST /svc-workflow/api/v2/workflows/{id}/share`

Wired to `one workflows share`. The shape below is not in any published spec — it was recovered from
the service's own schema-validation errors. Every one of `includeDependencies`, `privileges`, and
`sendEmail` is required even when its value is `false` or empty-looking, and `additionalInfoMsg` must
be *omitted* rather than sent as `null` when there is no message. `toPersonIds`/`toGroupIds` are
arrays of **strings**, not numbers — live-verified 2026-08-31: sending them as JSON numbers gets
HTTP 400 `SchemaValidationError` (`"Invalid input: expected string, received number"`, `"Missing
field toPersonIds.0"`).

```
{
  "includeDependencies": bool,
  "privileges": [ "create" | "delete" | "execute" | "read" | "share" | "update" ],  // >= 1 entry
  "sendEmail": bool,
  "toPersonIds": [string],
  "toGroupIds": [string],
  "additionalInfoMsg": string   // optional
}
```

### `POST /svc-workflow/api/v2/workflows/{id}/duplicate`

Wired: `one workflows copy`.

```
{
  "name": string,
  "version": number
}
```

### `POST /v4/connections/share`

Wired: `one connections permissions create`.

```
{
  "connectionId": <id>,
  "policy": <string>,
  "subjects": { "person": ["<person-id>"] }
}
```

Use only non-empty subject buckets; the live API rejects an empty `group` or `person` array.
The CLI also verifies that a raw body's `connectionId` matches the positional connection id.

### `DELETE /v4/connections/share?connectionId=&subjectId=&subjectType=person|group`

Wired: `one connections permissions delete`. Query-string request, not a body — `subjectType` is
either `person` or `group`.

## Methodology

**GET-probing cannot detect POST-only routes.** `GET /v4/connections/share` returns the exact same
`RouteNotFoundException` shape as a genuinely nonexistent path — a GET probe cannot distinguish "this
route doesn't exist" from "this route exists but only accepts POST." The reliable existence test is
**POST with an intentionally invalid body**:

- `400 ApiValidationFailed` → the route exists; the API parsed the request far enough to validate
  the body and reject it. This is also how the Contracts section above was recovered.
- `404 RouteNotFoundException` → the route does not exist at this path.

**Two different classes of "404."** This distinction is the reason this doc exists at all, so it is
worth stating precisely rather than as a single blanket rule:

- **Route-level 404** — the path itself is not registered on the server at all. On the `/v4`
  gateway and the `/billing/v1` managed service, this comes back as a JSON body:
  `{"exception":{"name":"RouteNotFoundException","message":"This route does not exist",
  "details":"Cannot GET <path>"}}`. The plans, scheduling, and workspace suspend/unsuspend rows in
  this repo were repointed to `/v4`; the old `/plans/v1`, `/scheduling/v1`, and `/iam/v1`
  requests were path bugs, not entitlement gaps. On `/svc-workflow`, prior investigation (the
  finding that motivated writing this doc) found this comes back as an **Express default HTML 404
  page** instead of JSON — `html:express` in the Error-body flavor column. This session did not
  re-probe a genuinely unrouted `/svc-workflow` path (doing so safely would mean guessing at
  unrouted paths rather than using a real `ayx` command, which is out of scope for a read-only
  doc-verification pass) — treat the `html:express` rows as carried over, not freshly reproduced
  today.
- **Application-level 404** — the route exists and parsed the request, but the specific resource id
  does not. On `/svc-workflow` this session, this came back as clean JSON: `GET
  /svc-workflow/api/v1/assets/{id}/dependencies` for a well-formed but nonexistent ULID returned
  `{"kind":"NotFoundError","message":"Asset with ID: ... does not exist","errors":[],"params":{}}`
  — no HTML anywhere. So "svc-workflow returns HTML 404s" is **not** universally true of every
  not-found response on that surface; it specifically describes the route-level case. Conflating the
  two would have meant "fixing" the wrong thing.

**`one X list` reporting `"ok": true` does not prove the underlying route returned 200.** Every
`list`-shaped command goes through the shared `one_api_list_request` helper
(`ayx-one-api/src/lib.rs`), which fetches each page via `one_api_live_request`, pulls `response` off
whatever envelope comes back, and extracts `items` from it — without first checking whether that
page's HTTP call actually succeeded. A page that 404s produces an envelope with no recognizable
`items`/`data`/`assets` key, `extract_items` returns an empty `Vec`, and the aggregate result reports
`"ok": true` with 0 items — indistinguishable, at the top level, from "this tenant genuinely has zero
of this resource." This was found live in this session: `one plans list` and `one scheduling list`
both print `"ok": true` while their `page_envelopes[0].status_code` is `404`. The single-shot
`count`/`detail`/`current`-style commands (`one_api_live_request` called directly, not through the
list helper) do **not** have this problem — they surface the real status/error code. When
re-verifying a `list` row, always check `page_envelopes[].status_code`, or cross-check against the
matching `count` row (or `one doctor <surface>`, which calls the raw endpoints directly and does
surface the true status) rather than trusting `ok: true` alone.

## 2026-09-01 non-legacy live canary notes

- Target: profile `local-dev`, workspace `alteryx-fde` (`91946`); all mutations used a unique canary name and completed a successful CLI dry-run first.
- Connection: created disposable BigQuery connection ID `46938`, verified detail and `SUCCESS` status, then deleted it. The temporary GCP service-account key was held only in the OS temp directory; no test dataset was created, and the key, service-account IAM bindings, and service account were removed afterward. The server-side connection dry-run route returned scope-blocked HTTP 403 for the current profile.
- Roles: assigned and removed the least-privileged `Viewer` role (`policyId 25704008`) on a temporary group. Assignment-list verification returned HTTP 403 and is classified as `blocked_by_scope`, not a CLI failure; the group was then deleted.
- Token: created, listed, detailed, and deleted one ephemeral API token. Its secret was held only in the protected local process and never printed, committed, or recorded.
- Cloud-native workflows: inspected a real `/svc-workflow` ULID through list/detail/dependencies/engines/tools, then copied and deleted one disposable asset by ID. No cloud-native ULID was sent to `/v4/jobGroups`, `/v4/outputObjects`, or `wrangledDataset`.
- Cloud-native execution: a disposable copy of `ayx-rs-build (rc-demo)` was run through `/svc-workflow/api/v1/workflows/{id}/run`, returning provider job `4039768` and job group `4398357`; the run completed and the copy was deleted. Cancellation uses `/svc-workflow/api/v1/jobs/{id}/cancel` with the returned job id. The route is present, but this workspace returned `WFS Jobs is not enabled in this environment`, so live cancellation is capability-blocked here.
- Cleanup: connection, group, token, and workflow ID sets matched their baselines after reverse-order cleanup; no canary residue remained. Legacy recipe execution/profiling/write-settings, output-object CRUD, and `one job-groups run` remain intentionally excluded. No cloud-native ULID was sent to a legacy endpoint.

## Known-unwired services

These are real, live Alteryx One services with no `ayx` command surface at all today — not rows that
belong in the tables above (which are strictly the endpoints in `inventory.rs`), but services worth
knowing about when deciding what to wire next. Alteryx One is roughly a dozen backend services;
`ayx` currently speaks `/v4` (the API gateway) plus `/svc-workflow` (cloud-native workflows).

- **`/dataset-service/v1`** — `POST /v1/permissions` shares datasets.
- **`/external/v1`** — CCS (Centralized Connection Service) connections.
- **`/ingestion/v1/entities`**
- **`/datahub/api/v1/graphql`** — GraphQL; `shareAyxFolder` mutation.
- **`/v5/vfs`** — virtual file system.
- **`/orchestrator`**
- **`/lq-service/v0`**
- **`/optimizer-service/v1`**
- **`/transformation-service/api/v1`**
- **`/fp/api/v1`**

None of these were probed this session — they are listed for completeness per the design brief for
this doc, not freshly verified. `ayx one ui *` (the `session`/`workflow`/`data`/`library`/
`schedules`/`jobs` subtree under `one ... ui`) is **not** a path to any of these: it is unwired
scaffolding behind an off-by-default `ui` Cargo feature that no release or CI build enables, and
every leaf returns a canned `"...scaffolded"` envelope with zero live network calls. Do not reach for
it and do not document it as a usable substitute anywhere in this file.

## Caveats

- **The official spec is incomplete.** `GET /v4/open-api-spec` returned a populated live spec
  with 233 operations in this session — but `GET /v4/workflows` (the cloud-native workflow
  listing route `one workflows list` depends on) is live and working while absent from that spec.
  Neither the spec nor `inventory.rs` is trustworthy alone; probe live.
- **The coverage matcher accepts both path-template styles.** The live spec uses colon parameters
  in some paths while the inventory uses braces; `one api coverage` canonicalizes both forms before
  comparing them. The current result is 63.1% coverage, with 16 stale and 86 missing operations.
- **Live evidence in this doc comes from one disposable validation workspace** at
  `https://<region>.alteryxcloud.com`, probed 2026-08-20. The old `/plans/v1/*`,
  `/scheduling/v1/*`, and `/iam/v1/*` entries were repointed to `/v4` and should no longer be read
  as tier evidence. `/billing/v1/*` is a different case, settled rather than open: `GET
  /v4/open-api-spec` contains no billing, usage,
  credit, license, or quota route at all, and the spec is demonstrably not entitlement-filtered —
  this tenant lacks the Plans entitlement, yet 22 `/v4/plan*` paths still appear in its own spec
  response. A route the spec never describes at all is a different fact than a route the spec
  describes but this tenant can't reach; the `not_found` here was the former, and the commands
  were removed rather than left as permanently-failing. Re-verify managed-service routes generally
  against an entitled tenant before concluding a route does not exist — but that caveat no longer
  applies to billing specifically.
- **This tenant has no fixtures for several resources** — flows, folders, wrangled/imported
  datasets, output objects, write settings, and API access tokens all listed as empty (genuine `200`
  with 0 items, confirmed via `page_envelopes[].status_code`, not the masked-404 case above). Rows
  needing a real id from one of these collections could not be live-verified this session; they are
  marked `unverified` rather than guessed.
