# API Surface And Observability

Status: active

## Current Scope

- Server auth work should stay focused on diagnosis and simulation.
- Public API branches should remain product-scoped under `server`, `license`,
  and `one`.
- One transport and observability need to stay hardened as the surface grows.

## Next Steps

- **Configure `AYX_ONE_API_ACCESS_TOKEN` so the nightly live smoke actually
  runs.** The secret is not set, so `live-smoke.yml` skips every meaningful step
  and still reports success — 21+ consecutive green runs in which nothing was
  validated. Local `one_live_smoke` tests behave the same way, passing in
  milliseconds without a network call. This is the highest-value item in this
  file. Every One-surface defect found in the 2026-07-28..30 sweep (the dead
  connections-permissions route, the transport masking failures as empty
  successes, the coverage gate that could not fail, the `410` misclassification)
  was found by hand, because CI has never made a live call. Treat a green
  nightly as meaningless until this is set.
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
- **Settle whether `one connections share` sends person ids in the right JSON
  type.** `build_connection_share_body` sends them as strings while its sibling
  `build_workflow_share_body` sends numbers. Raised in review and never
  confirmed either way; it needs one live `--apply` probe against a real
  connection, not more code reading. If the connection form is wrong, a share
  silently grants nobody access.
- **Add `ayx one workspace detail <id>`.** `GET /v4/workspaces/{id}` is wired
  and reachable today, but only from `ayx tui`'s legacy One browser — no `one`
  command dispatches it, so it sits in `NON_ONE_SURFACE_ENDPOINTS` in the drift
  gate rather than in the inventory proper. A real command would make it a
  normal inventory row and remove the carve-out.
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

- **Expand the `one_plans_count_live` fail-allowlist to include `not_found`.** The same tenant-tier outcome currently affects the plans list/detail live tests, scheduling, and billing-shaped checks where the whole service returns 404.
- **Teach `one_flows_folders_list_page_boundary_live` to accept the genuine empty-result shape.** `GET /v4/folders?limit=1` returned 200 with `response: {"data": []}` for the zero-folder tenant.
- **Make `one_job_groups_inspection_live_real_object` tolerate data-dependent inputs behavior.** A real non-JDBC job group correctly returned `400 DataServiceInvalidRequest` for its inputs sub-call, but the test currently treats that valid outcome as a failure.
- **Implement `--output table` separately or document the alias deliberately.** The live UX pass confirmed that it is currently byte-identical to `--output text`.
- **Add workflow-aware entries to `render_object_array`'s `PREFERRED` column list.** The generic picker exposes `contentChecksum`, a truncated hash, ahead of more useful workflow fields in the default demo table.
- **Warn when `ayx one workflows list --all` under-delivers against the server total.** `/v4/workflows` is limit-only and non-cursor-paginated; the CLI can see the true `count` but currently reports only the default page without warning.
- **Render workflow `tools`, `engines`, and `dependencies` cleanly in text mode.** The live pass found long, raw, single-line JSON blobs, unlike the readable list/count/detail output.
- **Unify and document the One API base-URL configuration precedence.** `AYX_ONE_BASE_URL` and `AYX_ONE_API_BASE_URL` are similarly named but resolved in different layers, and a second `.env` lookup beside the resolved central profile can override the working-directory file.
- **Investigate and wire cloud-native workflow DELETE.** The live probes found a route-level 404 for `/svc-workflow/api/v1/assets/...` but an application-level JSON `NotFoundError` for `/svc-workflow/api/v2/workflows/...`, strongly indicating a real server capability with no `ayx one workflows delete` command.
- **Make plans, scheduling, and billing list-shaped tests tolerate tier-gated whole-surface 404s.** The outcome is already documented as expected tenant behavior, but the current tests do not consistently allow it.

## Exit Criteria

- Product-specific command trees stay cleanly separated.
- API logging is consistent across products and opt-in where appropriate.
- Transport retries, envelopes, and error reporting are handled once.
