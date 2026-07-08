# Command Surface Coverage And Gaps

Status: mostly delivered

## Current Scope

- Every live `ayx` command should carry a clear one-line description and be
  reachable intuitively.
- Core Alteryx One API primitives (flow, folder, dataset, connection, plan,
  schedule, output, job, webhook) must be first-class, not buried inside other
  nouns or experimental buckets.

Source of this audit: a live-tree run on `main` (`ayx discover one --deep`,
`ayx 0.11.2`, 2026-07-05), cross-checked against the code, not the generated
`docs/command-surface.md`, which had drifted stale.

## Delivered

- `one dataset` is a real `/v4` primitive, not a stub under `one ui data`.
- The `one ui` subtree is feature-gated behind the `ui` cargo feature and
  stays default-off.
- Every command named in the old gap list now carries a description via the
  in-binary `COMMAND_SPECS` registry.

## Next Steps

- A few top-level `one` commands still lack a clap `#[command(about=...)]`
  help line (e.g. `status`, `inventory`, `auto-insights`, `desktop-exec`) even
  though the registry has summaries. Fold this into the in-flight
  primitive-first `one` hierarchy rework rather than editing the current tree.
- Dataset coverage is a primitive now but not the full dataset API surface
  (e.g. `POST /v4/importedDatasets` is still uncovered).
- Add shell-completion freshness automation alongside the existing
  `docs/command-surface.md` `xtask refresh-command-surface --check` gate.

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
