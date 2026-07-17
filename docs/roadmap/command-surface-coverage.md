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
- Every command named in the old gap list now carries a description sourced
  live from its clap `#[command(about = ...)]`, not a hand-maintained
  registry.

## Next Steps

- Dataset coverage is a primitive now but not the full dataset API surface
  (e.g. `POST /v4/importedDatasets` is still uncovered).
- Add shell-completion freshness automation alongside the existing
  `docs/command-surface.md` `xtask refresh-command-surface --check` gate.

## Exit Criteria

- Live clap `about` text supplies every visible command's summary — no
  command lacks a description, and none is duplicated by hand in a separate
  registry.
- Generated docs (`docs/command-surface.md`) are derived from the full live
  index (`ayx catalog list --scope all`), not a hand-curated subset.
- Datasets are a first-class `/v4` primitive, not a stub under `ui`.
- The experimental `ui` surface is either implemented or feature-gated out of
  default builds.

## Related

- Broader `ayx one` hierarchy rework (dissolve `platform`, primitive-first
  top-level, `ayx one login`, positional IDs, flags-over-JSON) is a separate
  in-flight design.
- CLI ergonomics framework (clap-native styled help, color, help groupings,
  error suggestions, completions) — see `discovery-and-catalog.md`.
