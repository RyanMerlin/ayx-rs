# ADR 0004: No Bundled TUI

Status: accepted
Date: 2026-09-04

## Context

`ayx tui` shipped two full-screen terminal applications inside the default
binary: the legacy profile/config manager (`ayx-rs/src/tui/`, ~6,100 lines) and
the v2 k9s-style resource browser (`ayx-rs/src/tui/v2/`, ~3,700 lines) behind
the `AYX_TUI_V2` environment variable. Together they were 24% of the `ayx-rs`
crate and pulled 37 crates that nothing else in the workspace used.

The facts that drove this decision, as of `v0.19.1`:

- The legacy TUI had six effective tests and three known defects open since
  June (a synchronous drill that blocks the render thread, an 18-line detail
  truncation with no scroll, and per-keystroke API calls). The last intentional
  TUI change landed on 2026-06-27.
- The v2 rebuild was parked by the owner the day it was demoed. The recorded
  critique: read-only asset browsing is the least valuable, most CLI-redundant
  capability, and "the k9s analogy is a category error" for a domain with ~5
  nouns, modest counts, and zero actions.
- The only live use of the TUI was profile/auth/connectivity setup, which
  `ayx onboard` and the Wizard `ayx one login` now own end to end.
- The TUI was not in the docs sidebar, was listed as `Safety: unclassified` and
  non-mutating in the catalog despite writing profiles and OS keyring secrets,
  and the `AYX_TUI_V2` gate was presence-checked so `AYX_TUI_V2=0` still
  activated v2.
- No vendor administration CLI surveyed in 2026 bundles a TUI (Supabase,
  flyctl, doctl, Stripe, GitHub). The popular terminal UIs in adjacent domains
  (k9s, lazygit, gh-dash) are separate projects from the plumbing CLI. flyctl
  routes "visualize this" to the web console instead.
- A full-screen TUI is hostile to the CLI's fastest-growing caller. An agent
  cannot drive stdin interactively or parse a repainted screen the way it
  parses a JSON envelope.

## Decision

`ayx` has no bundled TUI. `ayx tui` (legacy and v2) is removed from the binary
in the next minor release.

The interactive moments a TUI used to provide are delivered as **TTY-gated
primitives inside ordinary commands**, each inert when stdin or stdout is not a
terminal or `--no-input` is set, so agent-mode behavior never changes:

1. **Pickers.** When a command's required selector (an id or a workspace) is
   omitted on a TTY, the command fetches the candidate list and offers a fuzzy
   picker. Off a TTY the same omission is a `validation` error with a
   remediation that names the list command to run.
2. **`--watch`.** Long-running resources (job groups, workflow runs) can be
   polled to a terminal state; a TTY gets a redrawing status line, a pipe gets
   JSON Lines events.
3. **`open`.** `ayx one open <kind> <id>` deep-links the product web console.
   On a TTY it launches the browser; off a TTY it prints the URL.

`ratatui`, `tui-input`, and `nucleo-matcher` leave the dependency graph.
`crossterm` survives only as a transitive dependency of the picker crate; its
one direct non-TUI use (`one_platform/auth.rs`) is removed.

Any future full-screen interface is a separate binary and repository, never a
feature of the plumbing CLI. The removal is recorded with a `tui-final` git tag
so the code stays recoverable.

## Alternatives considered

### Keep the legacy TUI, delete v2

Rejected. Halves the cost but keeps 6,100 lines with six tests and three known
defects, and keeps a second setup path that duplicates `onboard` and the
Wizard.

### Rebuild the TUI around governance and access (the June direction)

Rejected as a TUI. The governance questions ("who has access to what", "why
can this person see this", "what changed since last month") want tables, graphs,
snapshots, and machine-readable output, and every one of those has to exist as
a CLI primitive first so agents can use it. A TUI would be a view over
primitives that do not yet exist. Build the primitives (Wave 1 of
`docs/roadmap/agent-first-substrate.md`); revisit a separate dashboard only if
a live-view need appears that a `--watch` cannot meet.

### Keep both and defer

Rejected. Every profile or secrets refactor pays a collateral tax across 30
files nobody exercises, and shipping an undocumented second UI behind an
environment variable is a support liability.

## Consequences

- The `ayx-rs` crate shrinks by roughly a quarter; the dependency graph loses
  `ratatui`, `tui-input`, `nucleo-matcher`, and their transitive-only crates.
  Build time and binary size drop.
- `GET /v4/workspaces/{id}` was reachable only from the legacy TUI. A real
  `ayx one workspace detail <id>` must land before or with the removal so no
  endpoint is lost and the drift-gate carve-out can go.
- Removing a top-level command is a breaking change; the release is `0.20.0`.
  A hidden `tui` stub returns a `validation` envelope with remediation
  (`ayx onboard`, `ayx one login`, `ayx profile`) for one release cycle.
- README, `docs/output-format.md`, `docs/runtime-config-contract.md`,
  `docs/cli-spec.md`, and the generated command surface drop their TUI
  references. Historical release notes are left as written.
- The unmerged `feat/tui-v2-phase2-cross-asset-drill` branch is deleted after
  tagging.

Implementation: `docs/superpowers/specs/2026-09-04-wave0-tui-removal-and-agent-hygiene-design.md`.
