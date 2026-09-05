# API Surface And Observability

Status: active

## Current Scope

- Server auth work should stay focused on diagnosis and simulation.
- Public API branches should remain product-scoped under `server`, `license`,
  and `one`.
- One transport and observability need to stay hardened as the surface grows.

## Next Steps

- **CI still makes no live call to Alteryx One, and that stays true by deliberate choice, not
  neglect.** `live-smoke.yml` skips every meaningful step because no `AYX_ONE_API_ACCESS_TOKEN`
  secret is configured; local `one_live_smoke` tests behave the same way, passing in milliseconds
  without a network call. Commit `7702c26` resolved this differently than "configure the token":
  the nightly schedule trigger was dropped and the workflow made `workflow_dispatch`-only,
  reasoning that a long-lived real-tenant token sitting in a public repo's Actions secrets is a
  worse posture than an honest gap. Do not reinstate the schedule without re-litigating that
  call. The underlying problem this bullet used to describe is still real and still the
  highest-value gap in this file: every One-surface defect found across the 2026-07-28..30 and
  2026-08-14/17 sweeps (the dead connections-permissions route, the transport masking failures as
  empty successes, the coverage gate that could not fail, the `410` misclassification, 17
  wrong-path endpoints, a dead `billing` surface, an `/iam/v1` leak in `telemetry permissions`) was
  found by hand, because CI has never made a live call. Treat a green nightly-that-no-longer-runs
  as meaningless; treat a green `workflow_dispatch` run as meaningful only for whoever ran it
  against their own tenant. `docs/one-live-validation.md` is the runbook for doing that by hand.
- **Decide the shape of the `ayx one api coverage --check` gate before wiring
  it into CI.** It gates on `missing == 0`; the first real measurement
  (2026-07-30) is 43.8% coverage with 132 missing, so it cannot pass today.
  Either gate on a coverage threshold or scope it to a resource allowlist
  expected to be complete. See the Live Coverage Baseline in
  `docs/one-backend-inventory.md`. A gate that cannot pass is a gate nobody
  turns on.
- **Work the `missing` list by resource**, starting with `workspaces` (23
  operations, the largest single gap and admin-facing), then `plans` (9),
  `schedules` (9), and `accounts` (8).
- **RESOLVED (2026-08-31, live-verified):** `one workflows share` was sending
  `toPersonIds`/`toGroupIds` as JSON numbers; `POST
  /svc-workflow/api/v2/workflows/{id}/share` rejects that with HTTP 400
  SchemaValidationError (`"Invalid input: expected string, received
  number"`, `"Missing field toPersonIds.0"`). `build_connection_share_body`'s
  string form was correct all along. `build_workflow_share_body` in
  `ayx-rs/src/cmd/one_workflows.rs` now serializes both fields as strings,
  matching `build_connection_share_body` in `one_connections.rs`.
- **RESOLVED (Wave 0, 0.20.0):** `ayx one workspace detail <id>` now dispatches
  `GET /v4/workspaces/{id}` directly. It previously sat in
  `NON_ONE_SURFACE_ENDPOINTS` in the drift gate because the only caller was
  `ayx tui`'s legacy One browser, itself removed in the same release; the
  endpoint is a normal inventory row now and `NON_ONE_SURFACE_ENDPOINTS` is
  empty.
- Chase the remaining unverified review leads, each raised against real source
  but never confirmed with a live call:
  - `record_api_event` logs `status.is_success()` while the envelope it
    accompanies may be `ok: false`.
  - `extract_shared_subject_ids` discards the person/group bucket, so two
    distinct subjects sharing an id collapse into one.
  - The 2xx-non-JSON success path may re-open the empty-list hole that
    `one_api_list_request` was hardened against.
  - `catalog.rs` prerequisites text is stale in at least one entry.
  - `docs/one-endpoint-matrix.md` documents a paginated response shape for some
    commands that do not emit it.
- Complete shared JSONL API logging for `license` commands (they still return
  static envelopes and emit no `record_api_event`).
- Move the generic license helpers out of `ayx-one-api` so HTTP/auth helper
  placement is product-pure.

The following ten items came out of the 2026-08-14 v0.15.0 live validation pass against a private
test tenant (see `docs/one-endpoint-matrix.md` and `docs/one-live-validation.md` for the
evidence):

- ~~**Expand the `one_plans_count_live` fail-allowlist to include `not_found`.**~~ Withdrawn: this
  was written from the tier-gating misdiagnosis. `/plans/v1` and `/scheduling/v1` were simply
  wrong paths (fixed, repointed to `/v4/plans` and `/v4/schedules`), and `billing` had no `/v4`
  equivalent at all (removed, not repointed). The correct follow-up is to **re-probe** the
  repointed `/v4/plans` and `/v4/schedules` rows live and then *tighten* the live-smoke allowlist
  once real evidence exists, not widen it further on the old diagnosis.
- ~~**Teach `one_flows_folders_list_page_boundary_live` to accept the genuine empty-result shape.**~~ Delivered: the live gate now asserts the genuine bounded request shape for `GET /v4/folders?limit=1`, which returns raw `{ "data": [] }` rather than a normalized pagination envelope.
- ~~**Make `one_job_groups_inspection_live_real_object` tolerate data-dependent inputs behavior.**~~ Delivered: the live gate accepts only the documented non-JDBC `400 DataServiceInvalidRequest` for `inputs`, while retaining hard failures for every other error.
- **Implement `--output table` separately or document the alias deliberately.** The live UX pass confirmed that it is currently byte-identical to `--output text`.
- **Add workflow-aware entries to `render_object_array`'s `PREFERRED` column list.** The generic picker exposes `contentChecksum`, a truncated hash, ahead of more useful workflow fields in the default demo table.
- ~~**Warn when `ayx one workflows list --all` under-delivers against the server total.**~~
  Delivered in `2ba1abd`: `--all` now requests a generous limit, compares against the endpoint's
  own `count`, and emits `complete: true/false` plus a stderr warning when the two disagree.
- **Render workflow `tools`, `engines`, and `dependencies` cleanly in text mode.** The live pass found long, raw, single-line JSON blobs, unlike the readable list/count/detail output.
- **Unify and document the One API base-URL configuration precedence.** `AYX_ONE_BASE_URL` and `AYX_ONE_API_BASE_URL` are similarly named but resolved in different layers, and a second `.env` lookup beside the resolved central profile can override the working-directory file.
- ~~**Investigate and wire cloud-native workflow DELETE.**~~ Wired and now fully live-verified:
  `ayx one workflows delete` (`DELETE /svc-workflow/api/v2/workflows/{id}`, gated behind `--apply`
  + TTY confirmation, mirrors `one flows delete`). The residual this item used to carry — a live
  call against a real id — is satisfied: duplicated a real workflow (`201`), deleted the copy
  (`200 {}`), and confirmed removal three ways (absent from `list --all`, count dropped by one,
  `detail` → `not_found`). The unknown-id guard was also verified live, rejecting before any
  mutating request. `docs/one-endpoint-matrix.md`'s DELETE row is updated to `live 200`.
- ~~**Make plans, scheduling, and billing list-shaped tests tolerate tier-gated whole-surface 404s.**~~
  Withdrawn along with the sibling item above: this was the same misdiagnosis. Plans and scheduling
  are repointed to real `/v4` paths and need live re-verification, not a wider tolerance for a
  wrong path. Billing had no `/v4` route at all and is removed — there is no billing test left to
  make tolerant.

## Exit Criteria

- Product-specific command trees stay cleanly separated.
- API logging is consistent across products and opt-in where appropriate.
- Transport retries, envelopes, and error reporting are handled once.
