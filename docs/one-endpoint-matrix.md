# Alteryx One endpoint matrix

A per-endpoint probe ledger for the Alteryx One surface (`ayx one ...`), with live evidence — not
just what the CLI is *supposed* to call, but what a real tenant actually returned when asked.

Most of the other One docs (`docs/command-surface.md`, `docs/one-backend-inventory.md`, and the
`one inventory` / `one api coverage` commands themselves) describe *what is wired*, not what a live
call returned. `docs/one-live-validation.md` and `docs/one-api-surface-audit.md` come closer — both
already track family-level live results, and `one-api-surface-audit.md`'s Phase 4 ("Dead Routes")
already flagged `billing`/`plans`/`scheduling` as 404 on its test workspace back on 2026-06-22,
open item: "Validate against an enterprise workspace before deciding whether these are bugs... or
genuinely tier-gated." This session's live sweep reproduced the identical `RouteNotFoundException`
404 on a **second, different tenant** (`alteryx-fde`, workspace `91946`, tier `platform_packaging`)
— evidence toward "genuinely tier-gated," not toward a wrong endpoint pattern, though still not a
close on that open item since neither tenant tested is confirmed enterprise-tier. What none of the
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
# One-time: authenticate a profile (skip if already logged in)
ayx one login --profile <profile>

# Spot-check a representative command per surface (mirrors the live sweep this doc was built from)
ayx --output json one workspace current
ayx --output json one workspace list
ayx --output json one person current
ayx --output json one person list
ayx --output json one token
ayx --output json one doctor discover
ayx --output json one doctor plans
ayx --output json one doctor scheduling
ayx --output json one doctor billing
ayx --output json one plans list
ayx --output json one plans count
ayx --output json one flows list
ayx --output json one flows folders list
ayx --output json one datasets list
ayx --output json one datasets wrangled list
ayx --output json one connections list
ayx --output json one connections detail <connection_id>
ayx --output json one workflows list
ayx --output json one workflows count
ayx --output json one workflows tools
ayx --output json one job-groups list
ayx --output json one job-groups detail <job_group_id>
ayx --output json one output-objects list
ayx --output json one write-settings list
ayx --output json one scheduling list
ayx --output json one billing current-account
ayx --output json one api open-api-spec
ayx --output json one api coverage
```

**Reading list-command output**: `ok: true` alone does not prove the underlying route returned
`200`. Check `data.page_envelopes[].status_code` (or, for the single-shot `detail`/`status`/`count`
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
| GET | `/v4/workspaces/current` | live 200 | 2026-07-27T00:55Z | `one workspace current` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one workspace current`. |
| GET | `/v4/workspaces/{id}/configuration` | live 200 | 2026-07-27T00:55Z | `one workspace configuration`<br>`one workspace configuration-v4` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Probed both as `one workspace configuration-v4 91946` and `one workspace configuration 91946`. |
| GET | `/v4/people` | live 200 | 2026-07-27T00:55Z | `one person list`<br>`one workspace people` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Probed via `one person list` (17 items) and `one workspace people`. |
| GET | `/v4/people?role=admin` | live 200 | 2026-07-27T00:55Z | `one workspace admins` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one workspace admins`. |
| POST | `/v4/workspaces/{id}/people/batch` | unverified | not probed this session | `one workspace invite-users` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed (would invite a real user). |
| DELETE | `/v4/workspaces/{workspaceId}/people/{id}` | unverified | not probed this session | `one workspace remove-user` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| POST | `/iam/v1/workspaces/{id}/people/suspend` | unverified | not probed this session | `one workspace suspend-users` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| POST | `/iam/v1/workspaces/{id}/people/unsuspend` | unverified | not probed this session | `one workspace unsuspend-users` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| POST | `/v4/workspaces/{id}/transfer` | unverified | not probed this session | `one workspace transfer` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/authorization/roles/{id}/people` | unverified | not probed this session | `one role list-assignments` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Read-only in principle, but needs a role id this session did not resolve. |
| POST | `/v4/authorization/roles/{id}/people/{subjectId}` | unverified | not probed this session | `one role assign` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| DELETE | `/v4/authorization/roles/{id}/people/{subjectId}` | unverified | not probed this session | `one role unassign` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |

### misc (implemented)

> The OpenAPI spec is now exposed through the CLI.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/open-api-spec` | live 200 | 2026-07-27T00:55Z | `one api coverage`<br>`one api open-api-spec` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | 170 `paths` entries returned live. `one api coverage` on this same spec reported `spec_operations: 0` in this session (see Caveats) — an anomaly worth re-checking, not a route-existence problem. |

### plan (implemented)

> Only the /v4 plan endpoints the CLI actually dispatches are listed. Read paths (list/count/run/permissions/package/runParameters/schedules) go through the /plans/v1 service instead — see the `plans` surface.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| POST | `/v4/plans` | unverified | not probed this session | `one plans create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| POST | `/v4/plans/{id}/permissions` | unverified | not probed this session | `one plans share` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/plans/{id}/full` | unverified | not probed this session | `one plans full` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No live plan id was available this session — see `plans` surface below: `/plans/v1/plans` itself 404s on this tenant. |
| PATCH | `/v4/plans/{id}` | unverified | not probed this session | `one plans update` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| DELETE | `/v4/plans/{id}` | unverified | not probed this session | `one plans delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |

### plans (implemented)

> Managed plans surface.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/plans/v1/plans` | live 404 RouteNotFoundException | 2026-07-27T00:55Z | `one plans list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | **CLI list-helper caveat**: `one plans list` itself reports `"ok": true` with 0 items — `one_api_list_request` does not check the inner page's status before treating it as an empty page (see Methodology). The true status only shows in `page_envelopes[].status_code` (404 here) or via `one doctor plans`, which surfaced the raw 404 directly. This tenant (`alteryx-fde`, tier `platform_packaging`) most likely lacks a Plans entitlement; the route may well exist and 200 on an entitled tenant. |
| GET | `/plans/v1/plans/{id}` | unverified | not probed this session | `one plans detail` | object: raw API resource body, JSON-passthrough | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | No live plan id available (list is empty/404 on this tenant). |
| POST | `/plans/v1/plans/{id}/run` | unverified | not probed this session | `one plans run` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | Mutating — not probed. |
| GET | `/plans/v1/plans/count` | live 404 RouteNotFoundException | 2026-07-27T00:55Z | `one plans count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | Direct `one plans count` AND `one doctor plans` both show `"error_code": "not_found"`, status 404, `RouteNotFoundException`, `Cannot GET /plans/v1/plans/count`. This is the known pre-existing failure `one_plans_count_live` in `one_live_smoke.rs` — its `fail` allowlist only covers `permission_denied`, not `not_found`, so it currently reds out. Root cause is the same tenant-entitlement gap as `/plans/v1/plans` above, not a CLI bug in this endpoint's wiring. |
| GET | `/plans/v1/plans/{id}/runParameters` | unverified | not probed this session | `one plans run-parameters` | object: raw API resource body, JSON-passthrough | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | No live plan id available. |
| GET | `/plans/v1/plans/{id}/schedules` | unverified | not probed this session | `one plans schedules` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | No live plan id available. |
| GET | `/plans/v1/plans/{id}/package` | unverified | not probed this session | `one plans export` | object: raw API resource body, JSON-passthrough | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | No live plan id available. |
| POST | `/plans/v1/plans/package` | unverified | not probed this session | `one plans import` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | Mutating — not probed. |
| GET | `/plans/v1/plans/{id}/permissions` | unverified | not probed this session | `one plans permissions` | object: raw API resource body, JSON-passthrough | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | No live plan id available. |
| DELETE | `/plans/v1/plans/{id}/permissions/{subjectId}` | unverified | not probed this session | `one plans permissions` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | Mutating — not probed. |

### workflow (implemented)

> Alteryx One cloud-native (canvas) workflows, ULID-keyed, served by /svc-workflow.

> Distinct from the `flow` surface, which is Designer Cloud /v4/flows keyed by integer ids.

> detail and count are synthesized client-side; the API exposes no per-id or count route.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/workflows` | live 200 | 2026-07-27T00:55Z | `one workflows list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one workflows list` — 25 items/page, 87 total (server total, see count row). |
| GET | `/v4/workflows?limit=1` | live 200 | 2026-07-27T00:55Z | `one workflows count` | object: `{ count, count_source: "server" }` (client-synthesized total) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one workflows count` returned `count: 87`, `count_source: "server"` — confirms the total comes from the server envelope, not `len(page)`. |
| GET | `/svc-workflow/api/v1/assets` | live 200 | 2026-07-27T00:55Z | `one workflows assets`<br>`one workflows detail` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:NotFoundError (`{ kind: "NotFoundError", message, errors: [], params: {} }`) for a valid route / unknown resource id | Backs both `one workflows assets` (probed directly, 200) and `one workflows detail` (which fetches the full asset list client-side and filters — confirmed working against a real id). |
| GET | `/svc-workflow/api/v1/assets/{id}/dependencies` | live 200 (real id) / live 404 json:NotFoundError (bad id) | 2026-07-27T00:55Z | `one workflows dependencies` | object: raw API resource body, JSON-passthrough | json:NotFoundError (`{ kind: "NotFoundError", message, errors: [], params: {} }`) for a valid route / unknown resource id | `one workflows dependencies <real-id>` → 200. `one workflows dependencies 01AAAAAAAAAAAAAAAAAAAAAAAA` (well-formed ULID, no such asset) → 404 with JSON body `{"kind":"NotFoundError","message":"Asset with ID: ... does not exist","errors":[],"params":{}}`, `request_id: null` (svc-workflow does not stamp a request id the way `/v4` does). This is the clean **application-level** not-found case — a valid route, unknown resource. It is a different failure mode from the route-level Express HTML 404 in the Methodology section below (hitting a URL svc-workflow does not route at all), which this session did not re-probe. |
| GET | `/svc-workflow/api/v0/workflows/{id}/availableEngines` | live 200 | 2026-07-27T00:55Z | `one workflows engines` | object: raw API resource body, JSON-passthrough | json:NotFoundError (`{ kind: "NotFoundError", message, errors: [], params: {} }`) for a valid route / unknown resource id | `one workflows engines <real-id>`. |
| GET | `/svc-workflow/api/v1/tools` | live 200 | 2026-07-27T00:55Z | `one workflows tools` | object: raw API resource body, JSON-passthrough | json:NotFoundError (`{ kind: "NotFoundError", message, errors: [], params: {} }`) for a valid route / unknown resource id | `one workflows tools`. |
| POST | `/svc-workflow/api/v2/workflows/{id}/duplicate` | not re-probed | 2026-07-26 (prior session, see inventory.rs comments) | `one workflows copy` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:NotFoundError (`{ kind: "NotFoundError", message, errors: [], params: {} }`) for a valid route / unknown resource id | Mutating; not run this session to avoid any live side effect. `inventory.rs` records this row as "Live-verified 2026-07-26" (the session prior to this one). `one workflows copy` gates it behind `--apply` and resolves `--version` before the dry-run body is shown. |

### flow (implemented)

> Flow lifecycle, package, parameters, library, folder, and permission commands are wired.

> The One surface does not expose arbitrary workflow authoring through this family.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| POST | `/v4/flows` | unverified | not probed this session | `one flows create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/flows` | live 200 (0 items — no flow fixtures on this tenant) | 2026-07-27T00:55Z | `one flows list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Genuine 200 with an empty page (`page_envelopes[].status_code: 200`) — distinct from the plans/scheduling 404-masked-as-empty case above. |
| GET | `/v4/flows/count` | live 200 | 2026-07-27T00:55Z | `one flows count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one flows count`. |
| GET | `/v4/flowsLibrary` | live 200 | 2026-07-27T00:55Z | `one flows library list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one flows library list`. |
| GET | `/v4/flowsLibrary/count` | live 200 | 2026-07-27T00:55Z | `one flows library count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one flows library count`. |
| GET | `/v4/folders` | live 200 (0 items) | 2026-07-27T00:55Z | `one flows folders list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one flows folders list` — genuine 200, no folder fixtures. |
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
| GET | `/v4/flows/{id}/recipeParameters` | unverified | not probed this session | `one flows parameters` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No flow id available. Dynamic endpoint (appends `?outputObjectType=`). |
| GET | `/v4/flows/{id}/inputs` | unverified | not probed this session | `one flows inputs` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No flow id available. |
| GET | `/v4/flows/{id}/outputs` | unverified | not probed this session | `one flows outputs` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No flow id available. |
| POST | `/v4/flows/{id}/permissions` | unverified | not probed this session | `one flows permissions` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/flows/{id}/permissions` | unverified | not probed this session | `one flows permissions-get` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one flows permissions-get` — no flow id available. |
| POST | `/v4/flows/{id}/move` | unverified | not probed this session | `one flows move` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| PATCH | `/v4/flows/{id}/replaceDataset` | unverified | not probed this session | `one flows replace-dataset` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| POST | `/v4/flows/package` | unverified | not probed this session | `one flows import` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating, requires a `.yxzp` file input — not probed. |
| POST | `/v4/flows/package/dryRun` | unverified | not probed this session | `one flows import-dry-run` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Requires a `.yxzp` file input — not probed. |
| GET | `/v4/flows/{id}/package` | unverified | not probed this session | `one flows export` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No flow id available. |
| GET | `/v4/flows/{id}/package/dryRun` | unverified | not probed this session | `one flows export-dry-run` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No flow id available. |

### scheduling (implemented)

> Managed scheduling surface.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/scheduling/v1/schedules` | live 404 RouteNotFoundException | 2026-07-27T00:55Z | `one scheduling list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | Same CLI list-helper caveat as `/plans/v1/plans`: `one scheduling list` shows `"ok": true`, 0 items; `page_envelopes[0].status_code` is 404. `one doctor scheduling` surfaced the raw 404/`RouteNotFoundException` directly. Tenant most likely lacks a Scheduling entitlement. |
| GET | `/scheduling/v1/schedules/{id}` | unverified | not probed this session | `one scheduling detail` | object: raw API resource body, JSON-passthrough | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | No live schedule id available. |
| POST | `/scheduling/v1/schedules/{id}/enable` | unverified | not probed this session | `one scheduling enable` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | Mutating — not probed. |
| POST | `/scheduling/v1/schedules/{id}/disable` | unverified | not probed this session | `one scheduling disable` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | Mutating — not probed. |
| GET | `/scheduling/v1/schedules/count` | live 404 RouteNotFoundException | 2026-07-27T00:55Z | `one scheduling count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | Direct `one scheduling count` AND `one doctor scheduling` both confirm 404 `Cannot GET /scheduling/v1/schedules/count`. No `one_scheduling_count_live` live-smoke case exists today, so this gap is invisible to the current test suite. |

### billing (implemented)

> Managed billing posture and usage export surface.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/billing/v1/my/billing-accounts/current` | live 404 RouteNotFoundException | 2026-07-27T00:55Z | `one billing current-account` | object: raw API resource body, JSON-passthrough | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | `one billing current-account` AND `one doctor billing` both confirm 404 `Cannot GET /billing/v1/my/billing-accounts/current`. `one_billing_current_account_live` in `one_live_smoke.rs` already allowlists `not_found` in its `fail` set, so this is a known, correctly-tolerated live gap (tenant entitlement), not a red test. |
| GET | `/billing/v1/usage/export` | live 404 RouteNotFoundException | 2026-07-27T00:55Z | `one billing usage-export` | object: raw API resource body, JSON-passthrough | json:RouteNotFoundException (same gateway error shape as `/v4`, confirmed live for plans/scheduling/billing) | `one billing usage-export` confirms the same 404 shape. |

## Partial surfaces

Endpoints where only some of the surface's API is wired; the rest stays documented-only until the
CLI needs it (`inventory.rs` `PARTIAL_SURFACES`).

### connection (partial)

> Connection lifecycle, dry-run, status, and permissions commands are wired.

> Connector metadata defaults, current values, and overrides are wired for JDBC behavior control.

> Credential-backend specifics remain encoded in the API payloads rather than a local domain model.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/connections` | live 200 | 2026-07-27T00:55Z | `one connections list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one connections list` — 11 items. |
| GET | `/v4/connections/count` | live 200 | 2026-07-27T00:55Z | `one connections count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one connections count`. |
| POST | `/v4/connections` | unverified | not probed this session | `one connections create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| POST | `/v4/connections/dryRun` | not re-probed — known failing live-smoke case | 2026-07-26 (prior session, see inventory.rs comments) | `one connections dry-run` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Not run this session (POST, chose not to construct a body under the read-only-only constraint). Flagged here because `one_connections_dry_run_shape_live` is one of the 4 pre-existing `AYX_ONE_LIVE_SMOKE` failures called out for this branch — worth checking whether the dry-run response shape has drifted before assuming this row is healthy. |
| GET | `/v4/connections/{id}` | live 200 | 2026-07-27T00:55Z | `one connections detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one connections detail 44865`. |
| GET | `/v4/connections/{id}/status` | live 200 | 2026-07-27T00:55Z | `one connections status` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one connections status 44865`. |
| PATCH | `/v4/connections/{id}` | unverified | not probed this session | `one connections update` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| DELETE | `/v4/connections/{id}` | unverified | not probed this session | `one connections delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/connections/{id}/permissions/sharedSubjects` | live 200 | 2026-07-27T00:55Z | `one connections permissions`<br>`one connections permissions detail` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one connections permissions list 44865`. Backs both `permissions` and `permissions detail`. |
| POST | `/v4/connections/share` | not re-probed | 2026-07-26 (prior session, see inventory.rs comments) | `one connections permissions create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed this session. `inventory.rs` records this route (not the old, broken `/v4/connections/{id}/permissions`) as "Live-verified 2026-07-26". |
| DELETE | `/v4/connections/share` | not re-probed | 2026-07-26 (prior session, see inventory.rs comments) | `one connections permissions delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed this session; same prior-session verification as the POST row above. |
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
| GET | `/v4/datasetLibrary` | live 400 ApiValidationFailed — CLI wiring gap | 2026-07-27T00:55Z | `one datasets list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | **Found live this session**: `one datasets list` sends `GET /v4/datasetLibrary` with no query params; the tenant rejects it with 400 `ApiValidationFailed`, `"'datasetsFilter' query parameter must not be null"`. The route exists (400, not 404) but the CLI does not currently supply a `datasetsFilter` value, so this command is wired-but-broken against this tenant. Not fixed here — out of scope for this doc — but worth a follow-up issue. |
| GET | `/v4/datasetLibrary/count` | live 200 | 2026-07-27T00:55Z | `one datasets count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one datasets count`. |
| GET | `/v4/wrangledDatasets` | live 200 (0 items) | 2026-07-27T00:55Z | `one datasets wrangled list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one datasets wrangled list` — genuine 200. |
| GET | `/v4/wrangledDatasets/count` | live 200 | 2026-07-27T00:55Z | `one datasets wrangled count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one datasets wrangled count`. |
| GET | `/v4/wrangledDatasets/{id}` | unverified | not probed this session | `one datasets wrangled detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No wrangled dataset id available (list is empty). |
| GET | `/v4/importedDatasets/{id}` | unverified | not probed this session | `one datasets imported detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No imported dataset id available. |

### jobGroup (partial)

> Job-group execution, publish, and inspection commands are wired.

> PDF/log artifact downloads and other deeper job-library paths remain documented-only.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/jobLibrary` | live 200 | 2026-07-27T00:55Z | `one job-groups list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one job-groups list` — 25 items. |
| GET | `/v4/jobLibrary/count` | live 200 | 2026-07-27T00:55Z | `one job-groups count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one job-groups count`. |
| POST | `/v4/jobGroups` | unverified | not probed this session | `one job-groups run` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating (`one job-groups run`) — not probed. |
| PUT | `/v4/jobGroups/{id}/publish` | unverified | not probed this session | `one job-groups publish` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/jobGroups/{id}` | live 200 | 2026-07-27T00:55Z | `one job-groups detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one job-groups detail 4087561`. |
| POST | `/v4/jobGroups/{id}/cancel` | unverified | not probed this session | `one job-groups cancel` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/jobGroups/{id}/status` | live 200 | 2026-07-27T00:55Z | `one job-groups status` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one job-groups status 4087561`. |
| GET | `/v4/jobGroups/{id}/inputs` | live 400 DataServiceInvalidRequest | 2026-07-27T00:55Z | `one job-groups inputs` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one job-groups inputs 4087561` → 400, `"Illegal Argument: Only Jdbc sources have connect String"`. Route exists; this is a real, data-shape-specific validation error for this particular job group's inputs, not a CLI wiring bug. |
| GET | `/v4/jobGroups/{id}/pdfResults` | live 400 ProfilingDataNotFoundException | 2026-07-27T00:55Z | `one job-groups pdf-results` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one job-groups pdf-results 4087561` → 400, `"Job group 4087561 does not have profiling data"`. Route exists; this job group simply has no profiling artifact. |
| GET | `/v4/jobGroups/{id}/outputs` | live 200 | 2026-07-27T00:55Z | `one job-groups outputs` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one job-groups outputs 4087561`. |
| GET | `/v4/jobGroups/{id}/jobs` | live 200 | 2026-07-27T00:55Z | `one job-groups jobs` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one job-groups jobs 4087561`. |
| GET | `/v4/jobGroups/{id}/publications` | live 200 | 2026-07-27T00:55Z | `one job-groups publications` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one job-groups publications 4087561`. |
| GET | `/v4/jobGroups/{id}/profile` | live 400 ProfilingDataNotFoundException | 2026-07-27T00:55Z | `one job-groups profile` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one job-groups profile 4087561` → 400, same "no profiling data" reason as `pdfResults`. |
| GET | `/v4/jobGroups/{id}/profileResults` | live 400 ProfilingDataNotFoundException | 2026-07-27T00:55Z | `one job-groups profile-results` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one job-groups profile-results 4087561` → 400, same "no profiling data" reason. |

### outputObject (partial)

> Output object lifecycle and wrangle-to-python commands are wired.

> Additional nested resources stay documented-only until the CLI needs them.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/outputObjects` | live 200 (0 items) | 2026-07-27T00:55Z | `one output-objects list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one output-objects list` — genuine 200. |
| GET | `/v4/outputObjects/count` | live 200 | 2026-07-27T00:55Z | `one output-objects count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one output-objects count`. |
| POST | `/v4/outputObjects` | unverified | not probed this session | `one output-objects create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/outputObjects/{id}` | unverified | not probed this session | `one output-objects detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No output object id available (list is empty). |
| PATCH | `/v4/outputObjects/{id}` | unverified | not probed this session | `one output-objects update` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| DELETE | `/v4/outputObjects/{id}` | unverified | not probed this session | `one output-objects delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/outputObjects/{id}/inputs` | unverified | not probed this session | `one output-objects inputs` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No output object id available. |
| POST | `/v4/outputObjects/{id}/wrangleToPython` | unverified | not probed this session | `one output-objects wrangle-to-python` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating (dry-runs without `--body`, but no output object id was available to try even that) — not probed. |

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
| GET | `/v4/writeSettings` | live 200 (0 items) | 2026-07-27T00:55Z | `one write-settings list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one write-settings list` — genuine 200. |
| GET | `/v4/writeSettings/count` | live 200 | 2026-07-27T00:55Z | `one write-settings count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one write-settings count`. |
| POST | `/v4/writeSettings` | unverified | not probed this session | `one write-settings create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/writeSettings/{id}` | unverified | not probed this session | `one write-settings detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No write setting id available (list is empty). |
| PATCH | `/v4/writeSettings/{id}` | unverified | not probed this session | `one write-settings update` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| DELETE | `/v4/writeSettings/{id}` | unverified | not probed this session | `one write-settings delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |

### apiAccessTokens (partial)

> One API access-token CRUD is wired; additional token administration endpoints remain documented-only.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/apiAccessTokens` | live 200 | 2026-07-27T00:55Z | `one auth diagnose`<br>`one auth status`<br>`one doctor auth`<br>`one token` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one token`, `one auth diagnose`/`one auth status`, `one doctor auth` all confirmed 200 (0 tokens listed directly via `one token`, but `doctor auth`'s workspace probe against the same route returned real token records). |
| POST | `/v4/apiAccessTokens` | unverified | not probed this session | `one token create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating (creates a real PAT) — not probed. |
| GET | `/v4/apiAccessTokens/{tokenId}` | unverified | not probed this session | `one token detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | No token id resolved via `one token` this session. |
| DELETE | `/v4/apiAccessTokens/{tokenId}` | unverified | not probed this session | `one token delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |

### person (partial)

> Current lookup plus person list/count/detail/create/update/patch/delete/password workflows are wired; remaining person families stay documented-only.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/people/current` | live 200 | 2026-07-27T00:55Z | `one person current`<br>`one whoami` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one person current` and `one whoami`. |
| GET | `/v4/people` | live 200 | 2026-07-27T00:55Z | `one person list`<br>`one workspace people` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Probed via `one person list` (17 items) and `one workspace people`. |
| GET | `/v4/people/current` | live 200 | 2026-07-27T00:55Z | `one person current`<br>`one whoami` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Same evidence as the row above — `inventory.rs`'s `person` `PARTIAL_SURFACES` entry literally lists `GET /v4/people/current` twice; this row mirrors that duplicate faithfully rather than silently dropping it. |
| GET | `/v4/people/count` | live 403 permission_denied | 2026-07-27T00:55Z | `one person count` | object: raw API count body (`{ count }`/`{ total }`, service-specific) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one person count` → 403 `AccessControlException`, `"User is not authorised to access this API."`. Route exists and is reached authenticated; this profile's role lacks the permission. |
| GET | `/v4/people/{id}` | live 200 | 2026-07-27T00:55Z | `one person detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one person detail <id>` against a real person id from `one person list`. |
| POST | `/v4/people` | unverified | not probed this session | `one person create` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| PUT | `/v4/people/{id}` | unverified | not probed this session | `one person update` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| PATCH | `/v4/people/{id}` | unverified | not probed this session | `one person patch` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| DELETE | `/v4/people/{id}` | unverified | not probed this session | `one person delete` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| PATCH | `/v4/people/current/updatePassword` | unverified | not probed this session | `one person update-password` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| POST | `/v4/passwordresetrequest` | unverified | not probed this session | `one person password-reset-request` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating and side-effecting (sends a real email) — not probed. |

### workspace (partial)

> Workspace listing, configuration, transfer, and v4 configuration-by-id endpoints are wired; other workspace families remain documented-only.

| Method | Path | Live status | Verified (UTC) | ayx command(s) | Response shape | Error-body flavor | Notes |
|---|---|---|---|---|---|---|---|
| GET | `/v4/workspaces` | live 200 | 2026-07-27T00:55Z | `one workspace list` | paginated list: `{ items[], next_page_token, pages_fetched, page_envelopes[] }` (CLI-normalized) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one workspace list`. |
| GET | `/v4/workspaces/{id}/configuration` | live 200 | 2026-07-27T00:55Z | `one workspace configuration`<br>`one workspace configuration-v4` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Probed both as `one workspace configuration-v4 91946` and `one workspace configuration 91946`. |
| PATCH | `/v4/workspaces/current/transfer` | unverified | not probed this session | `one workspace transfer-assets` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| GET | `/v4/workspaces/current/configuration` | live 200 | 2026-07-27T00:55Z | `one workspace current-configuration` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one workspace current-configuration`. |
| PATCH | `/v4/workspaces/current/configuration` | unverified | not probed this session | `one workspace save-current-configuration` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed. |
| PATCH | `/v4/workspaces/{id}/configuration` | unverified | not probed this session | `one workspace save-configuration-v4` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating — not probed (`save-configuration-v4`). |
| GET | `/v4/workspaces/{id}/configuration-schema` | live 200 | 2026-07-27T00:55Z | `one workspace configuration-schema` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one workspace configuration-schema 91946`. |
| GET | `/v4/workspaces/current/configuration-schema` | live 200 | 2026-07-27T00:55Z | `one workspace current-configuration-schema` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | `one workspace current-configuration-schema`. |
| POST | `/v4/workspaces/current/delete-configuration` | unverified | not probed this session | `one workspace delete-current-configuration` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating and destructive — not probed under any circumstance. |
| POST | `/v4/workspaces/{id}/delete-configuration` | unverified | not probed this session | `one workspace delete-configuration` | object: mutation result / dry-run shape (`{ dry_run, mutating, would_send }` when not `--apply`) | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Mutating and destructive — not probed under any circumstance. |

## Contracts

Request bodies below are **not** documented in the official spec (`GET /v4/open-api-spec`) or in
`docs/command-surface.md`. They were recovered from the services' own schema-validation errors —
i.e. by sending an intentionally-empty or wrong-shaped body and reading back the `400
ApiValidationFailed` response, which names the missing/invalid fields. That is the only place these
shapes are recorded; treat this section as load-bearing, not decorative.

### `POST /svc-workflow/api/v2/workflows/{id}/share`

Not wired to any `ayx` command today (see Known-unwired services below — this one differs from the
rest of that list in that its shape *is* known, just not its CLI wiring).

```
{
  "includeDependencies": bool,
  "privileges": [ "create" | "delete" | "execute" | "read" | "share" | "update" ],  // >= 1 entry
  "sendEmail": bool,
  "toPersonIds": [int],
  "toGroupIds": [int],
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
  "subjects": { "group": [], "person": [] }
}
```

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

- **Route-level 404** — the path itself is not registered on the server at all. On the `/v4` gateway
  and the `/plans/v1`, `/scheduling/v1`, `/billing/v1` managed services, this comes back as a JSON
  body: `{"exception":{"name":"RouteNotFoundException","message":"This route does not exist",
  "details":"Cannot GET <path>"}}`. On `/svc-workflow`, prior investigation (the finding that
  motivated writing this doc) found this comes back as an **Express default HTML 404 page**
  instead of JSON — `html:express` in the Error-body flavor column. This session did not re-probe a
  genuinely unrouted `/svc-workflow` path (doing so safely would mean guessing at unrouted paths
  rather than using a real `ayx` command, which is out of scope for a read-only doc-verification
  pass) — treat the `html:express` rows as carried over, not freshly reproduced today.
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

- **The official spec is incomplete.** `GET /v4/open-api-spec` returned 170 `paths` entries live in
  this session, confirming the spec is populated — but `GET /v4/workflows` (the cloud-native
  workflow listing route `one workflows list` depends on) is live and working while absent from that
  spec. Neither the spec nor `inventory.rs` is trustworthy alone; probe live.
- **`one api coverage` returned `spec_operations: 0` in this session's live run**, against the same
  spec that (fetched directly via `one api open-api-spec`) has 170 paths and non-empty method
  entries under each. `coverage_pct` reported `100.0` as a result (division-by-zero guard, not a
  real 100% match) and `stale` listed all 123 canonicalized inventory operations. This looks like a
  live anomaly in how `coverage()`'s `spec_base_path()`/`canonical_op()` anchoring handled this
  tenant's spec (this tenant's custom base URL is `alteryx-fde.us1.alteryxcloud.com`, not the
  generic `us1.alteryxcloud.com` used elsewhere) rather than a real 0%-covered result. Not
  root-caused or fixed here — out of scope for this doc (`ayx-one-api/src/coverage.rs` is off
  limits) — but flagged because a `coverage_pct: 100.0` on a fresh tenant is exactly the kind of
  false-green a probe ledger like this one is meant to catch. Worth a follow-up issue.
- **Live evidence in this doc comes from one tenant**: workspace `alteryx-fde` (id `91946`, tier
  `platform_packaging`), `https://us1.alteryxcloud.com`, probed 2026-07-27 ~00:50–01:01 UTC using
  the repo's `default`-profile PAT (workspace-bound, no OTP). A `not_found`/`RouteNotFoundException`
  recorded here for `/plans/v1/*`, `/scheduling/v1/*`, and `/billing/v1/*` most likely reflects this
  tenant's entitlements (Plans/Scheduling/Billing features not provisioned on this tier), not that
  the route is absent from the product. Re-verify against an entitled tenant before concluding a
  managed-service route does not exist at all.
- **This tenant has no fixtures for several resources** — flows, folders, wrangled/imported
  datasets, output objects, write settings, and API access tokens all listed as empty (genuine `200`
  with 0 items, confirmed via `page_envelopes[].status_code`, not the masked-404 case above). Rows
  needing a real id from one of these collections could not be live-verified this session; they are
  marked `unverified` rather than guessed.
- **`docs/one-backend-inventory.md`'s `connection` partial-surface section is stale.** It still lists
  `GET/POST /v4/connections/{id}/permissions` and `GET/DELETE /v4/connections/{id}/permissions/{aid}`
  — the pre-fix paths that returned `RouteNotFoundException` (see `inventory.rs`'s comment on the
  `connection` `CONNECTION_ENDPOINTS` and commit `94c0c6d`, "repair connections permissions, which
  called a 404 route"). The corrected paths
  (`GET /v4/connections/{id}/permissions/sharedSubjects`, `POST`/`DELETE /v4/connections/share`) are
  what this doc's `connection` section and `inventory.rs` both carry today. Not fixed here — out of
  scope (that file isn't in this doc's edit scope) — flagged so it doesn't propagate further.
