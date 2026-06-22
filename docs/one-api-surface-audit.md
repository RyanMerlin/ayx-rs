# Alteryx One API Surface Audit — `alteryx-fde`

**Date:** 2026-06-22  
**Tested with:** `ayx` v0.9.11, PAT via pure-HTTP OTP login, workspace `alteryx-fde` (id=91946, tier=platform_packaging)

Full test notes in `docs/HANDOFF-pure-http-auth.md`.

---

## Phase 1 — UX Papercuts (no API changes, pure CLI fixes)

These are all in ayx-rs Rust code only. Fast, low-risk.

- [ ] **`--body` flag: clarify it takes a file path**  
  Help text says `--body <BODY>` with no explanation. Passing inline JSON (`--body '{"name":"..."}`) silently fails with "No such file or directory". Fix: change the help string to `--body <FILE>` / "path to JSON body file" and/or accept `-` for stdin piping.  
  Affected commands: `flows create/update/run/copy`, `connections create/update`, `output-objects create/update`, `write-settings create/update`, `webhook-flow-tasks create`, `plans create/update`.

- [ ] **`ayx one status` and `ayx one inventory` wrong surface routing**  
  On an Alteryx One-only profile, both commands error: "config missing api/server_api section". They are routing to the Server API transport. Fix: detect that the profile has only `alteryx_one` configured, show a One-specific status summary (auth posture, workspace info), or emit a clean "not applicable for One profile — use `ayx one doctor platform`" message instead of an internal config error.

- [ ] **`platform workspace invite-users` should default `--workspace-id` to current workspace**  
  The flag is required but the profile already knows the workspace (via `workspace_gid`). Default it from profile config so single-workspace users don't need to pass it explicitly. Same applies to `remove-user`, `suspend-users`, `unsuspend-users`, `transfer`, `transfer-assets`.

- [ ] **`ayx one doctor` — surface tier information in output**  
  `doctor billing` and `doctor scheduling` silently show 404s that aren't explained. Add a note to the doctor output when a surface returns 404 across all endpoints: "This surface may not be available at your subscription tier (platform_packaging). Billing and Plans APIs are observed as enterprise-tier only."

---

## Phase 2 — Missing Read Surfaces

These require new subcommands or corrected endpoint targets.

- [ ] **`connections connector-metadata list` — discover available connector slugs**  
  `connector-metadata defaults --connector <name>` works, but there is no way to list valid connector names. `jdbc`, `remotefile`, `google_bigquery`, `bigquery` were all tried; only specific known slugs succeed. Need to find the correct API endpoint for connector enumeration and add a `connector-metadata list` subcommand.  
  Acceptance: `ayx one connections connector-metadata list` returns all available connector type names/slugs that can be passed to `--connector`.

- [ ] **`flows permissions` — add a read command**  
  Currently `flows permissions --body <FILE>` is a POST (write permissions). There is no read surface. Add `flows permissions get --flow-id <ID>` that hits `GET /v4/flows/{id}/permissions` (or equivalent) and returns the current permission set.

- [ ] **`platform workspace people/admins` — find and fix correct endpoint**  
  Both `--workspace-id 01KMGF85...` variants return 404:  
  `GET /v4/workspaces/{id}/people` → `RouteNotFoundException`  
  `GET /v4/workspaces/{id}/admins` → `RouteNotFoundException`  
  Action: find the actual v4 endpoint for workspace member listing (possibly `/v4/people` with a workspace filter, or a different path) and update the endpoint template. Until found, document the gap.

- [ ] **`job-groups` — `name=None` on all 25 entries**  
  Job-groups created from flow runs have no explicit name. The `flowRun.flowId` field is the only association back to the originating flow. `job-groups list` output is currently unintelligible for users (25 rows, all `name=None`). Fix: in the text output formatter, synthesize a display name like `flow-{flowId} run at {createdAt}` when `name` is null. Or surface `flowRun.id` and `ranfrom` fields as default columns.

---

## Phase 3 — Mutations: Body Schema Discovery

Connection create is broken in practice because the required body schema is undiscoverable.

- [ ] **`connections create` — document and validate body schema**  
  The API requires at minimum: `name`, `type`, `credentialType`, `vendor`, `vendorName`, `params` (connector-specific). Passing only `name/type/credentialType` gives sequential 400s revealing each missing field one at a time. Options (pick one or both):
  - [ ] Add `connections dry-run --connector <slug>` that calls `connector-metadata defaults` and emits a filled-in JSON template the user can edit and pass to `create --body`.
  - [ ] Add body schema validation in the CLI before sending: check that `vendor`, `vendorName`, `params` are present and emit a helpful error with the template if not.

- [ ] **`connections create` — end-to-end test with full body**  
  Complete a working `connections create --apply` with all required fields (`vendor`, `vendorName`, `params`) using a known-good connector type (BigQuery or Google Sheets from existing connections as reference). Add this as a fixture/example in `docs/`.

- [ ] **`flows update` — investigate 403 on PUT `/v4/flows/{id}`**  
  `flows create` (POST) and `flows delete` (DELETE) both succeed with the current PAT. `flows update` (PUT) returns 403 "User is not authorised to access this API." This is a token scope gap or permission model difference. Actions:
  - [ ] Check what OAuth scopes the UI-minted token carries vs the `local-auth-workspace` accessToken we mint.
  - [ ] Check if `PUT /v4/flows/{id}` requires the flow to be in a specific state (e.g. draft vs published).
  - [ ] Check if `PATCH /v4/flows/{id}` is the correct method instead of PUT.
  - [ ] If it's a scope gap: update `email_otp_login_pure_http` to request broader scopes at the PAT mint step (`POST /v4/apiAccessTokens`).

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
