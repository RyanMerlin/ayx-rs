# CLI Modularization Plan

Captured from a Codex review (2026-05-11) of the `ayx-rs` binary crate. The
workspace-level architecture is fine — the work below is scoped entirely to the
CLI binary crate at `ayx-rs/`.

## Status Checkpoint (2026-05-29)

This note is now partly historical. Several items from the original review have
already moved:

- `catalog` dispatch is no longer root-only; the binary now has
  `ayx-rs/src/cmd/catalog.rs`.
- `ayx dashboard` also lives under `ayx-rs/src/cmd/dashboard/`.
- `ayx-rs/src/main.rs` is still large, but it is now **5,389 lines** rather
  than the original 5,815 cited below.

What remains useful here is the direction of travel: keep shrinking the root
dispatcher, keep moving command-local helpers beside their surfaces, and keep
tests from depending on specific source-file layouts.

## Diagnosis

The repo is **not** a monolithic `main.rs` at the workspace level. Domain
crates are sensibly split:

- `ayx-core/`
- `ayx-one-api/`
- `ayx-server/`
- `ayx-workflow/`
- `ayx-registry/`

The concentration of debt is in the binary crate:

- `ayx-rs/src/main.rs` is still **5,389 lines** and still owns a large share of
  clap definitions plus profile / doctor / self-update helpers.
- `ayx-rs/src/cmd/one.rs` is **2,119 lines** and was extracted verbatim — it
  still imports a large set of root enums and helpers from `main.rs`, so
  ownership is root-centric rather than local.
- `ayx-rs/tests/cli_smoke.rs` still regexes HTTP method/path literals out of
  `src/main.rs`, making `main.rs` structurally load-bearing for endpoint
  inventory drift detection.

Top-level dispatch is already shallow (`main.rs:4112`), so the bones are good.
The remaining work is finishing the extraction, not redesigning.

## Goals

- Shrink `main.rs` to: module decls, `Cli::parse()`, global transport /
  bootstrap setup, final render / error handling.
- Decouple `cmd/one.rs` from root-defined enums and helpers.
- Stop using `main.rs` source text as a test fixture.

## Plan (in execution order)

Order matters — Codex listed these flat, but #1 and #3 unblock #4.

### 1. Extract clap enum definitions into a `cli/` tree

Mechanical, low-risk. Do this first so subsequent extractions have a place to
land their command types.

```
src/cli/mod.rs
src/cli/one.rs
src/cli/server.rs
src/cli/workflow.rs
src/cli/profile.rs
```

Open design question: parallel `cli/<name>.rs` + `cmd/<name>.rs` trees vs.
single `cmd/<name>/{types,dispatch}.rs` tree. Parallel trees are more common
in larger CLIs; single tree is less ceremony. Pick one and apply consistently.

### 2. Replace the `cli_smoke` source-scraping test

Currently `tests/cli_smoke.rs` reads `src/main.rs` and regexes endpoint
literals. Replace with runtime introspection of the clap `Command` tree via
`Cli::command()`.

This is interim — same coverage, no `main.rs` coupling, no new abstraction.
A full registry/descriptor API (Codex's original suggestion) is a larger
design decision; defer until there's a second consumer that needs it.

Do this **before** step 4 so the extraction churn doesn't keep breaking a
fragile test.

### 3. Move remaining helper families out of `main.rs`

- profile helpers
- doctor helpers
- catalog helpers
- self-update

Each becomes its own module under `src/` (or `src/cmd/`, depending on the
tree decision in step 1). This is what currently forces `cmd/one.rs` to
import from root.

### 4. Break `cmd/one.rs` into submodules by surface

```
cmd/one/platform.rs
cmd/one/plans.rs
cmd/one/flows.rs
cmd/one/connections.rs
cmd/one/doctor.rs
```

This is the high-value step — it's where coupling actually drops. Doing it
after #1 and #3 means the submodules can own their types and helpers
locally instead of re-importing from root.

### 5. Final shape of `main.rs`

After the above, `main.rs` should contain only:

- `mod` declarations
- `Cli::parse()`
- global transport / bootstrap setup
- final render / error handling

Target: well under 500 lines.

## Sequencing notes

- Recent commits (Phase 1 / 2 / 3 telemetry) suggest mid-feature work. **Do
  not interleave** this modularization with telemetry — finish telemetry
  first, then land the CLI cleanup as its own series of small PRs.
- Each step above should be its own PR. They're independent enough to review
  separately and revert individually if needed.
- Resist the urge to introduce new abstractions (descriptor APIs, command
  registries, trait-based dispatch) during the move. Extract first, abstract
  later only if a second consumer appears.

## Non-goals

- No new crates. The crate-level split is already correct.
- No rewrite of command logic. Pure structural moves.
- No behavior changes. `cli_smoke` and the existing test suite should pass
  unmodified after each step (except step 2, which rewrites that one test).
