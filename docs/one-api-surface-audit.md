# Alteryx One API Surface Audit — `alteryx-fde`

**Date:** 2026-06-22  
**Tested with:** `ayx` v0.9.11, PAT via pure-HTTP OTP login, workspace `alteryx-fde` (id=91946, tier=platform_packaging)

Full test notes in `docs/HANDOFF-pure-http-auth.md`.

---

## Phase 1 — UX Papercuts (no API changes, pure CLI fixes)

These are all in ayx-rs Rust code only. Fast, low-risk.

- [x] **`--body` flag: clarify it takes a file path** — DONE v0.9.12  
  All 32 `body: PathBuf` fields now show `--body <FILE>` with "path to JSON body file" in help.  
  Affected commands: `flows create/update/run/copy`, `connections create/update`, `output-objects create/update`, `write-settings create/update`, `webhook-flow-tasks create`, `plans create/update`.

- [x] **`ayx one status` and `ayx one inventory` wrong surface routing** — DONE v0.9.12  
  Both commands now detect One-only profiles and return a clean message pointing to `ayx one doctor platform` instead of erroring with "config missing api/server_api section".

- [x] **`platform workspace invite-users` should default `--workspace-id` to current workspace** — DONE v0.9.12  
  `--workspace-id` is now `Option<String>` on `invite-users`, `remove-user`, `suspend-users`, `unsuspend-users`, `transfer`, `transfer-assets`. Defaults to `workspace_gid` from the active profile when not supplied.

- [x] **`ayx one doctor` / dead-route help text** — DONE v0.9.12 (partial)  
  Billing, plans, and scheduling `None` help arms now include "Note: requires enterprise tier — returns 404 on platform_packaging workspaces." Full runtime 404 detection deferred to Phase 4.

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
  `ayx one job-groups list` now post-processes the response: when `name` is null, synthesizes a display name from `flowRun.flowId` (`flow-{flowId}`) or falls back to `job-{id}`. The API returns no job-groups in the `alteryx-fde` workspace currently so this was implemented based on the known item shape from the prior audit session.

---

## Phase 3 — Mutations: Body Schema Discovery

Connection create is broken in practice because the required body schema is undiscoverable.

- [ ] **`connections create` — document and validate body schema**  
  The API requires at minimum: `name`, `type`, `credentialType`, `vendor`, `vendorName`, `params` (connector-specific). Passing only `name/type/credentialType` gives sequential 400s revealing each missing field one at a time. Options (pick one or both):
  - [ ] Add `connections dry-run --connector <slug>` that calls `connector-metadata defaults` and emits a filled-in JSON template the user can edit and pass to `create --body`.
  - [ ] Add body schema validation in the CLI before sending: check that `vendor`, `vendorName`, `params` are present and emit a helpful error with the template if not.

- [ ] **`connections create` — end-to-end test with full body**  
  Complete a working `connections create --apply` with all required fields (`vendor`, `vendorName`, `params`) using a known-good connector type (BigQuery or Google Sheets from existing connections as reference). Add this as a fixture/example in `docs/`.

- [x] **`flows update` — FIXED: PUT → PATCH** — DONE v0.9.12  
  Root cause: CLI was using `PUT /v4/flows/{id}` (403) instead of `PATCH /v4/flows/{id}` (200). Live-verified: PATCH returns 200, PUT returns 403. One-line fix in `one_flows.rs`. `flows create`/`update`/`delete` all now work end-to-end.

---

## Phase 4 — Dead Routes (Tier-Gated Surfaces)

All of these return `RouteNotFoundException` on `platform_packaging` tier. Validate against an enterprise workspace before deciding whether these are bugs in ayx-rs endpoint templates or genuinely tier-gated features.

- [ ] **Billing surface — `/billing/v1/`**  
  - `billing current-account` → 404: `/billing/v1/my/billing-accounts/current`  
  - `billing usage-export` → 404: `/billing/v1/usage/export`  
  Action: confirm if these endpoints exist on enterprise tier. If tier-gated, emit a clean error: "Billing API is not available on this workspace tier (platform_packaging)." If the URL pattern is wrong, fix the endpoint template.

- [ ] **Plans surface — `/plans/v1/`**  
  - `plans list/count/create/detail/run/...` → all 404: `/plans/v1/plans`  
  Action: same as billing — verify endpoint pattern against enterprise or check API docs. The Plans surface is fully implemented in `ayx-rs` but dead against this workspace.

- [ ] **Scheduling surface — `/scheduling/v1/`**  
  - `scheduling list/count/detail/enable/disable` → all 404: `/scheduling/v1/schedules`  
  Action: same. The scheduling API may live under a different path or version for this tier.

---

## Phase 5 — Untested (Needs Fixtures or Real Content)

These require either a .yxmd workflow file or more complex setup to test fully.

- [ ] **`flows import`** — needs a `.yxmd` or `.yxzp` file. Test with a minimal valid workflow.
- [ ] **`flows validate`** — same; tests flow XML validity before run.
- [ ] **`flows library`** — unclear what this surface exposes; needs investigation.
- [ ] **`flows copy --flow-id`** — copy an existing flow to a new name; test end-to-end.
- [ ] **`job-groups run`** — needs a flow with actual content and output destinations.
- [ ] **`job-groups outputs/inputs/jobs`** — needs a completed flow run to have data.
- [ ] **`connections update/delete/status/permissions`** — test against the test connection once create is working.
- [ ] **`output-objects create`** — needs a valid flow with output.
- [ ] **`write-settings create`** — needs a writable destination configured.
- [ ] **`webhook-flow-tasks create/test`** — needs a webhook-enabled flow.
- [ ] **`platform workspace invite-users --apply`** — send a real invite to a test address.
- [ ] **`platform role list-assignments --role-id <id>`** — need to know valid role IDs from the workspace.

---

## Summary Table

| Phase | Items | Effort | Risk |
|-------|-------|--------|------|
| 1 — UX papercuts | 4 | Low | None |
| 2 — Missing reads | 5 | Medium | Low |
| 3 — Mutation schemas + `flows update` 403 | 5 | Medium–High | Medium |
| 4 — Dead routes (tier validation) | 3 | Low (investigation) | Low |
| 5 — Untested (fixtures needed) | 11 | High | Low |
