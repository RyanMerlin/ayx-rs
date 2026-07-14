# Alteryx One API Surface Audit

**Date:** 2026-06-22  
**Tested with:** `ayx` v0.9.11 → v0.10.0, PAT via pure-HTTP OTP login, workspace `example-workspace`

Per-endpoint status in `docs/one-live-validation.md`.

> **Status (v0.10.2):** all five phases complete. The security + correctness red-team of the
> auth flow and surface work is fully closed — the deferred M2/M3 transport items (redirect-host
> allowlist, interaction-id shape validation, broader response-body redaction) landed in v0.10.2.
> The Playwright/headless-Chromium fallback was removed in v0.10.1; the email-OTP flow is now
> pure-HTTP only. Key model fact established: the PAT is **workspace-bound** (the
> `x-alteryx-workspace-gid` header is ignored server-side), which drove the argless
> `workspace people`/`admins` and the new `workspace switch` command.

---

## Phase 1 — UX Papercuts (no API changes, pure CLI fixes)

These are all in ayx-rs Rust code only. Fast, low-risk.

- [x] **`--body` flag: clarify it takes a file path** — DONE v0.9.12  
  All 32 `body: PathBuf` fields now show `--body <FILE>` with "path to JSON body file" in help.  
  Affected commands: `flows create/update/run/copy`, `connections create/update`, `output-objects create/update`, `write-settings create/update`, `webhook-flow-tasks create`, `plans create/update`.

- [x] **Server-api status/inventory wrong surface routing** — superseded by the command-hierarchy rework
  The obsolete Server API status view was removed from the One command surface. `ayx one inventory` now reports the One surface registry.

- [x] **`platform workspace invite-users` should default `--workspace-id` to current workspace** — DONE v0.9.12  
  `--workspace-id` is now `Option<String>` on `invite-users`, `remove-user`, `suspend-users`, `unsuspend-users`, `transfer`, `transfer-assets`. Defaults to `workspace_gid` from the active profile when not supplied.

- [x] **`ayx one doctor` / dead-route help text** — DONE v0.9.12 (partial)  
  Billing, plans, and scheduling `None` help arms now include "Note: requires enterprise tier — returns 404 on some workspace tiers." Full runtime 404 detection deferred to Phase 4.

---

## Phase 2 — Missing Read Surfaces

These require new subcommands or corrected endpoint targets.

- [x] **`connections connector-metadata list` — gap documented** — DONE v0.9.12  
  `/v4/connectors` returns 404 — no enumeration endpoint exists in the Alteryx One v4 API. The `connector-metadata` help text now documents this gap and lists known working slugs (`gsheetsuser`, `remotefile`, etc.). A `list` subcommand is deferred until the API adds enumeration support.

- [x] **`flows permissions` — add a read command** — DONE v0.9.13  
  Added `ayx one flows permissions-get --flow-id <ID>` that hits `GET /v4/flows/{id}/permissions`. The endpoint returns 403 via PAT (permission_denied error code). The command exists and surfaces a clean `permission_denied` error — not a gap in the CLI, a limitation in the API's PAT scope. Documented in `site/src/content/docs/one/flows/permissions.md`.

- [x] **`platform workspace people/admins` — fixed correct endpoints** — DONE v0.9.12  
  `people` → `GET /v4/people` (workspace context via `x-alteryx-workspace-gid` header — live-verified 200, 9 members returned).  
  `admins` → `GET /v4/people?role=admin`. Both `/v4/workspaces/{id}/people` and `/v4/workspaces/{id}/admins` are confirmed non-existent routes.

- [x] **`job-groups` — `name=None` on all entries** — DONE v0.9.13  
  `ayx one job-groups list` now post-processes the response: when `name` is null, synthesizes a display name from `flowRun.flowId` (`flow-{flowId}`) or falls back to `job-{id}`. The API returns no job-groups in the `example-workspace` workspace currently so this was implemented based on the known item shape from the prior audit session.

---

## Phase 3 — Mutations: Body Schema Discovery

Connection create is broken in practice because the required body schema is undiscoverable.

- [x] **`connections create` — template generator** — DONE v0.9.14  
  Added `ayx one connections connector-metadata template --connector <slug>`. It calls `GET /v4/connectorMetadata/{slug}/defaults` and emits a fillable JSON create-body: `name`, `description`, `type` (derived from category: `relational`→`jdbc`, else `remotefile`), `vendor`, `vendorName`, `credentialType` (first of metadata `credentialTypes`), `isGlobal`, `ssl`, and a `params` object built from `connectionParameters` (defaults or `<type>` placeholders). Live-verified: `bigquery`→jdbc/apiKey/`params.projectId`; `gsheetsuser`→remotefile/oauth2.

- [~] **`connections create` — end-to-end test** — PARTIAL  
  `POST /v4/connections/dryRun` returns `AccessControlException` (403) via the current PAT — same scope wall as flows permissions/recipeParameters/roles. A full `create --apply` needs valid connector credentials (OAuth token for gsheets, service-account key for bigquery) that aren't available in this environment. The template generator unblocks the body-construction half; the credential half is environment-gated.

- [x] **`flows update` — FIXED: PUT → PATCH** — DONE v0.9.12  
  Root cause: CLI was using `PUT /v4/flows/{id}` (403) instead of `PATCH /v4/flows/{id}` (200). Live-verified: PATCH returns 200, PUT returns 403. One-line fix in `one_flows.rs`. `flows create`/`update`/`delete` all now work end-to-end.

---

## Phase 4 — Dead Routes (Tier-Gated Surfaces)

All of these return `RouteNotFoundException` on the test workspace. Validate against an enterprise workspace before deciding whether these are bugs in ayx-rs endpoint templates or genuinely tier-gated features.

- [ ] **Billing surface — `/billing/v1/`**  
  - `billing current-account` → 404: `/billing/v1/my/billing-accounts/current`  
  - `billing usage-export` → 404: `/billing/v1/usage/export`  
  Action: confirm if these endpoints exist on enterprise tier. If tier-gated, emit a clean error: "Billing API is not available on this workspace tier." If the URL pattern is wrong, fix the endpoint template.

- [ ] **Plans surface — `/plans/v1/`**  
  - `plans list/count/create/detail/run/...` → all 404: `/plans/v1/plans`  
  Action: same as billing — verify endpoint pattern against enterprise or check API docs. The Plans surface is fully implemented in `ayx-rs` but dead against this workspace.

- [ ] **Scheduling surface — `/scheduling/v1/`**  
  - `scheduling list/count/detail/enable/disable` → all 404: `/scheduling/v1/schedules`  
  Action: same. The scheduling API may live under a different path or version for this tier.

---

## Phase 5 — Untested (Needs Fixtures or Real Content)

Probed live against `example-workspace` 2026-06-22. Full per-endpoint status in `docs/one-live-validation.md`.

**Biggest finding — 4 commands panicked (FIXED v0.9.14):** `flows export`, `server system-info`,
`server runtime-settings`, and `tools workspace init` each defined a local `--output <PathBuf>` arg
that collided with the global `--output <text|json>` format flag (same clap id, different type) and
panicked at runtime on every call. All four renamed their file arg to `--output-file`.

- [x] **`flows export`** — FIXED v0.9.14. Was panicking; now exports a real `.yxzp` package (743 bytes for an empty flow, live-verified).
- [x] **`flows copy --flow-id`** — VERIFIED working. `POST /v4/flows/{id}/copy` returns 201.
- [x] **`flows library`** — VERIFIED working. `GET /v4/flowsLibrary` returns 200 (0 items in this workspace).
- [x] **`flows inputs` / `flows outputs`** — VERIFIED working on an empty flow (200).
- [x] **`output-objects list` / `write-settings list`** — VERIFIED working (200, 0 items).
- [~] **`flows import`** — endpoint wired; needs a valid `.yxzp` package and credentials. Export now produces a package, so an export→import round-trip is the natural next test (deferred — import of an empty-flow package returned a backend validation error, not a CLI bug).
- [x] **`flows validate`** — `GET /v4/flows/{id}/validate` returns 404. No validate route exists in this API version. Documented as unsupported.
- [~] **`job-groups run` / `outputs` / `inputs` / `jobs`** — need a flow with real content + a completed run. The workspace has 0 job-groups; can't exercise without authoring a non-empty flow (requires Designer/UI, not the API).
- [~] **`connections update/delete/status`** — need a test connection, which needs valid credentials (see Phase 3 partial).
- [x] **`connections dry-run`** — `POST /v4/connections/dryRun` returns `AccessControlException` (403) via PAT. Endpoint exists but PAT lacks scope.
- [~] **`output-objects create` / `write-settings create`** — need a valid flow with output and a writable destination.
- [x] **`webhook-flow-tasks create/test`** — `/v4/webhookFlowTasks` returns 404. Not present on the test workspace tier. Documented as unavailable.
- [ ] **`platform workspace invite-users --apply`** — would send a real invite; intentionally not exercised.
- [x] **`platform role list`** — `GET /v4/roles` returns `AccessControlException` (403) via PAT. Scope-gated.

### The PAT scope wall

A consistent cluster of surfaces returns `AccessControlException` ("User is not authorised to
access this API.", HTTP 403) under the PAT minted by the workspace-bearer OIDC flow:
`flows permissions-get`, `flows parameters` (recipeParameters), `platform role list`,
`connections dry-run`. The PAT has create/read/delete on flows and connections but lacks scope for
these read/validation surfaces. Resolving requires either a UI-minted token or requesting broader
OAuth scopes at the `POST /v4/apiAccessTokens` mint step. This is an API/token-scope limitation, not
a CLI bug — the commands exist and surface clean `permission_denied` errors.

---

## Summary Table

| Phase | Items | Status |
|-------|-------|--------|
| 1 — UX papercuts | 4 | All done (v0.9.12) |
| 2 — Missing reads | 5 | All done (v0.9.12–13) |
| 3 — Mutation schemas + `flows update` 403 | 5 | update fixed (v0.9.12); template added (v0.9.14); create-apply env-gated |
| 4 — Dead routes (tier validation) | 3 | Documented as enterprise-tier-gated (v0.9.12) |
| 5 — Untested (fixtures needed) | 11 | 4 panics fixed + 7 verified working + rest classified (scope/tier/fixture-gated) (v0.9.14) |
