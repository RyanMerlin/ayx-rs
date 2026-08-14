# v0.15.0 RC Live Validation — 2026-08-14

Short-lived handoff note, per `docs/roadmap/README.md`. Delete it once the demo asset below is
cleaned up and this note has been read.

## Where things stand

Full live validation pass run against tenant `alteryx-fde` (workspace `91946`) ahead of tagging
`v0.15.0`, following the new runbook at `docs/one-live-validation.md`. Summary:

- Offline suite (762 tests) and live suite (76 tests, 69 passed) both green, modulo known/filed
  gaps — see `docs/roadmap/api-surface-and-observability.md`'s newest 10 items.
- The 0.15.0 CHANGELOG's headline fixes all confirmed live: `api coverage`'s breaking shape
  change, the `connections permissions` route fix, the `wrangle-to-python` `--apply` gate, and —
  most importantly — the `410 GoneException` → `not_found` classification, confirmed via a real
  create→update→delete→read-back cycle on `one flows` (id `178993`, fully cleaned up, baseline
  restored to 0).
- `docs/one-endpoint-matrix.md` evidence refreshed for everything this pass touched.
- Also landed in this window: `chore/pin-rust-toolchain-1.97.1` (#159, Wyatt) — repo now pins
  `rust-toolchain.toml` to 1.97.1; rebuilt and re-verified clean under it (fmt/clippy/762 tests/
  release build all pass).

## Outstanding — needs cleanup

**A real, permanent demo asset was created and is NOT yet cleaned up:**

- **Cloud-native workflow copy**: `workflowId 01M00M9CRWSANK79MBCA0V9VXX`, name
  `"ayx-rs-build (rc-demo)"`, copied from `01KTZB2VPA38V4K87QJTGW25BB` (`ayx-rs-build`) via
  `ayx one workflows copy --apply`, created 2026-08-14T17:14Z, for Merlin's release demo.
- `ayx one workflows` has no `delete`/`unshare` command (see roadmap item 9 in
  `docs/roadmap/api-surface-and-observability.md`), so this can't be removed via the CLI. The
  live delete-route probe in this pass found evidence a real DELETE route likely exists at
  `/svc-workflow/api/v2/workflows/{id}` (application-level 404 on a fake id, not a route-level
  one) — cleanup is probably possible via a raw authenticated `DELETE` call even without a CLI
  command, but this was not attempted or confirmed against a real id.
- **Action needed**: once the demo is done, either delete it via the Alteryx One web UI, or (if
  wiring `ayx one workflows delete` per roadmap item 9 lands first) delete it that way and use it
  as the first live proof that the new command works.

## What is actually next

The roadmap file carries the work items; this note does not duplicate them.

- `docs/roadmap/api-surface-and-observability.md` — 10 new items from this pass (test allowlist
  gaps, `--output table` alias, the `workflows list --all` pagination gap, the
  `AYX_ONE_BASE_URL`/`AYX_ONE_API_BASE_URL` precedence footgun, the likely-unwired workflow
  DELETE, etc).

## One trap worth knowing

**Forcing a live transport failure for ad-hoc `.env`-based testing is harder than it looks.**
`.env`'s `AYX_ONE_BASE_URL` gets copied into the in-memory profile's `base_url` before host
resolution ever checks `AYX_ONE_API_BASE_URL`, and a second `.env` lookup beside the resolved
central profile can override an isolated scratch copy too. See `docs/one-live-validation.md`'s
Phase 3 item 1 for the actual working repro command, or just trust the existing non-live
regression tests in `ayx-one-api/src/lib.rs` (`list_request_json_404_is_reported_as_failure_not_an_empty_success`
and siblings) — that's what this pass did.
