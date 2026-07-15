# CLI namespace reorg: Designer/Server/One + tactics-workflows collision cleanup

**Date:** 2026-07-15
**Status:** Approved, ready for implementation planning

## Background

While investigating why `ayx one flows list` returned zero items despite the
user having real assets in their Alteryx One workspace, we confirmed (via the
live `/v4/open-api-spec` response) that Alteryx One treats `flow`, `workflow`,
and `desktopworkflow` as three distinct resource/task types, each with its own
RBAC roles (`workflow_owner`, `desktopworkflow_owner`, etc.). The user's
workspace assets were all "Workflow" type (Designer Cloud Workflow / Live
Query Workflow), not "Flow" type — so `ayx one flows` correctly reported zero,
but the CLI has **zero coverage for the `workflow` resource** at all.

Adding that coverage surfaced a naming collision: the CLI already has a
top-level `ayx workflow` (singular) command — Designer/Server-native XML/
package tooling for `.yxmd`/`.yxmc`/`.yxzp`/`.yxdb` files, entirely unrelated
to the Alteryx One cloud resource, but confusingly similar in name. A new
`one workflows` command sitting next to it would repeat the exact "near-
identical, unclear naming" mistake just fixed in `one flows` vs
`one flows library` (0.13.2, PR #120).

Further, a second pre-existing collision was found: a top-level `ayx
workflows` (plural) command already exists, and it's an internal automation/
playbook composition registry ("higher-order skills composing tactics") —
completely unrelated to any Alteryx product concept. Between the existing
`ayx workflow` and `ayx workflows`, and the new `ayx one workflows`, there
would be three similarly-named, semantically distinct top-level surfaces.

## Decisions

### 1. Namespace moves (this spec, "sub-project A")

| Before | After |
|---|---|
| `ayx workflow` (Designer/Server XML/package tooling) | `ayx designer workflow` |
| `ayx tactics` (atomic, safety-classified operational playbooks) | `ayx actions` |
| `ayx workflows` (composed sequences of tactics) | `ayx actions workflows` |

- `ayx designer workflow`'s 11 existing subcommands move **verbatim**:
  `inspect`, `unpack`, `validate`, `replace`, `repackage`, `recurse`, `scan`,
  `convert-cloud`, `publish`, `migrate`, `yxdb`. `publish` (which republishes
  through the Server API) stays with the group rather than moving to
  `ayx server` — like `git push`, it targets a remote but is still an
  artifact-lifecycle action.
- `ayx actions`'s existing subcommands (`list`, `describe`, `resolve`, `run`,
  `validate`) are unchanged.
- `ayx actions workflows`'s existing subcommands (`list`, `explain`, `run`)
  are unchanged.
- Grammar: `one workflows` (sub-project B, plural — matches its actual
  siblings `flows`/`plans`/`connections`/`datasets`/`job-groups`) and
  `designer workflow` (singular — matches its own tool-belt style; no
  sibling under the new `designer` group to be consistent with yet) are
  **not** forced to match each other. The existing `one` subtree already
  mixes singular (`workspace`, `role`, `token`, `person` — each has a
  "current/self" concept) and plural (pure enumerable collections), so
  there's no single existing rule to standardize against, and these two
  renamed groups aren't peers of each other anyway.
- Registry rename choice: `actions` over `playbooks`/`runbooks`, because it
  mirrors GitHub Actions' well-known pairing (actions = atomic operations,
  workflows = composed sequences of actions) — a mental model that needs no
  new vocabulary.
- Confirmed no collision with `ayx telemetry workflows` (a "top workflows by
  run count" metric) — already disambiguated by its own parent scope, not in
  scope for this reorg.

### 2. Sub-project B (separate follow-up spec, not this document)

`ayx one workflows` — Alteryx One's cloud workflow resource. Blocked on the
exact `/v4/` REST paths (pending the user's targeted grep of the live
OpenAPI spec). Starts with `list`/`count`; further CRUD coverage is
out of scope until that lands as its own spec.

The mutual cross-reference in `--help` text (`designer workflow` ↔
`one workflows`, learning from the flows/library fix) is added **when B
ships**, not as part of A — a forward reference to a command that doesn't
exist yet would be worse than no reference at all.

## Implementation touchpoints

All three renames follow the same shape in `ayx-rs/src/main.rs`'s top-level
`Command` enum — a single-field variant wrapping a subcommand enum:

```rust
Workflow  { #[command(subcommand)] command: WorkflowCommand },   // line 306
Tactics   { #[command(subcommand)] command: TacticsCommand },    // line 364
Workflows { #[command(subcommand)] command: WorkflowsCommand },  // line 369
```

The leaf subcommand enums and all handler/business logic are **untouched** —
only the top-level wrapping structure changes:

- **A1 (`workflow` → `designer workflow`):** new `Command::Designer {
  command: DesignerCommand }`; `DesignerCommand` has one variant, `Workflow
  { command: WorkflowCommand }`, reusing the existing type. Old top-level
  `Workflow` variant removed.
- **A2 (`tactics` → `actions`):** **full rename of the noun** — see the
  "A2 scope amendment" below. `TacticsCommand` → `ActionsCommand`, and the
  `tactic` domain vocabulary is renamed to `action` throughout: the
  `ayx-registry` public types, the on-disk YAML directory, the JSON envelope
  keys, and all human-readable strings.
- **A3 (`workflows` → `actions workflows`):** remove the standalone
  `Command::Workflows` variant; add `Workflows { command: WorkflowsCommand
  }` *inside* `ActionsCommand`, reusing the existing `WorkflowsCommand` type.

**Sequencing:** A2 must land before A3 (A3's `ActionsCommand` type doesn't
exist until A2 creates it). A1 is fully independent and can land in any
order relative to A2/A3.

### A2 scope amendment (2026-07-15, approved by Merlin)

Two corrections to A2 as originally written, found during implementation prep:

**1. `cmd::tactics` does not exist.** The original text prescribed renaming a
`cmd::tactics` module to `cmd::actions`. There is no such module — dispatch
lives in `cmd::registry::execute_tactics` (`ayx-rs/src/cmd/registry.rs`,
which serves both `tactics` and `workflows`). Rename the function to
`execute_actions`; the `cmd::registry` module keeps its name.

**2. The rename is the noun, not just the surface.** `tactic` is a domain
term with four layers, and the original A2 text scoped only the first:

| Layer | Renamed? |
|---|---|
| CLI word + `TacticsCommand` → `ActionsCommand` | yes |
| Human strings (`"12 tactic(s)"`, error text) | yes |
| JSON envelope keys (`tactics`, `tactic_id`, `tactic_count`) | yes |
| `ayx-registry` public API (`Tactic`, `TacticNotFound`, `Step::Tactic`) | yes |
| On-disk YAML dir (`tactics/*.yaml`) + search path | yes → `actions/*.yaml` |

**Why full, not surface-only:** this spec's own approved structure requires
it. Mapping `tactics` → `actions` *and* `workflows` → `actions workflows`
only coheres if `action` == `tactic` (actions are the leaf things; "actions
workflows" are compositions of them). A surface-only rename ships
`ayx actions list` printing `12 tactic(s)` and emitting `{"tactics": [...]}`.

**Why now:** this breaks the on-disk YAML contract and the agent-facing JSON
contract. ayx-rs is public but **not yet announced**, so the installed base
with custom tactic YAML is effectively zero. This is the cheapest this rename
will ever be; after the announce it becomes a permanent migration burden.
The 0.14.0 announce gate is precisely why the surface must be coherent.

Also touched per rename: the `CommandSpec` catalog entries in `main.rs`
(name/path fields), `docs/command-surface.md` regeneration (`cargo run -p
xtask -- refresh-command-surface`), README references if any, CHANGELOG
entries, and any test asserting the old command-name strings (`catalog.rs`,
`cli_smoke.rs`).

## Breaking change handling & versioning

Clean break, no back-compat aliases or deprecation period — matching this
project's own precedent (0.13.0's `platform` dissolution: *"BREAKING...
Pre-release, no back-compat aliases"*). Called out explicitly in the
CHANGELOG under each rename. Lands as a **minor bump (0.14.0)**, not a
patch, since this is structural/breaking rather than a fix.

**Sequencing:** three separate PRs (A1, A2, A3) — each independently
reviewable, testable, and revertible without entangling the others (A2
before A3, per the type dependency above). All three land before the 0.14.0
tag.

## Testing plan

Per rename:
- Full `cargo nextest run --workspace --locked`.
- Actually render `--help` at every affected level (not just confirm it
  compiles) — `ayx designer workflow --help`, `ayx actions --help`, `ayx
  actions workflows --help` — and read the output.
- `ayx catalog list --format full` reflects the new paths.
- `docs/command-surface.md` regenerated; diff touches only the intended
  lines.

## Delegation

Implementation routes through codex (paired with rust-reviewer for the diff
review), not solo rust-engineer, given the touchpoint count per rename —
per established project convention for non-trivial Rust changes.

## Out of scope (this spec)

- `ayx one workflows` itself (sub-project B, separate spec).
- Any further redistribution of `ayx server`/`ayx tools` subcommands beyond
  what's decided above.
- `ayx telemetry workflows` (confirmed no collision, not touched).
