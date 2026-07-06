# Command Surface Coverage And Gaps

Status: active

## Current Scope

- Every live `ayx` command should carry a clear one-line description and be
  reachable intuitively.
- Core Alteryx One API primitives (flow, folder, dataset, connection, plan,
  schedule, output, job, webhook) must be first-class, not buried inside other
  nouns or experimental buckets.

Source of the gaps below: a live-tree audit on `main` (`ayx discover one
--deep`, `ayx 0.11.2`, 2026-07-05), cross-checked against the code — not the
generated `docs/command-surface.md`, which had drifted stale.

## Priority Gaps

### P1 — Missing `dataset` API primitive

Datasets exist in the CLI only under the stubbed `one ui data` subtree; there
is **no real `/v4` dataset command** in the API surface. Build first-class
`one dataset` commands (list/show/…) against the One API. A core building
block is currently unreachable through the real (non-stub) surface.

### P1 — Description backfill (~25 commands have no `about`/registry text)

World-class help requires every command to carry a description. Commands with
no description in code today:

- `scheduling`: detail, enable, disable, count
- `plans`: detail, count, export, import, permissions, run-parameters, schedules
- `platform workspace`: switch, invite-users, remove-user, suspend-users,
  unsuspend-users, transfer, transfer-assets
- `platform role`: assign, unassign
- `platform token`: list
- `flows`: permissions-get
- `connections permissions`: list
- `billing`: usage-export
- top-level `one`: status, inventory, auto-insights, desktop-exec
- `webhook-flow-tasks`: test

### P2 — `one ui` subtree is all stubs

Every `one ui` leaf (session/workflow/data/library/schedules/jobs) returns a
hardcoded placeholder envelope; there is no browser automation wired
(`grep -rl playwright` = 0 matches). Decide the disposition: implement it, or
**feature-gate it behind a cargo feature (default-off)** so default builds
exclude the experimental surface and any future browser-automation deps live
behind that same feature.

## Next Steps

- Land P1 dataset commands and the description backfill first; then resolve P2
  (implement vs feature-gate the `ui` subtree).
- Regenerate `docs/command-surface.md` and shell completions after each surface
  change so the generated references stay in sync with the live tree.

## Exit Criteria

- No live command lacks a description.
- Datasets are a first-class `/v4` primitive, not a stub under `ui`.
- The experimental `ui` surface is either implemented or feature-gated out of
  default builds.

## Related

- Broader `ayx one` hierarchy rework (dissolve `platform`, primitive-first
  top-level, `ayx one login`, positional IDs, flags-over-JSON) is a separate
  in-flight design. See the Wyatt vault note
  `Aria/Wyatt/projects/2026-07-05-ayx-one-cli-tree-current-proposed-cleanup.md`.
- CLI ergonomics framework (clap-native styled help, color, help groupings,
  error suggestions, completions) — see `discovery-and-catalog.md`.
