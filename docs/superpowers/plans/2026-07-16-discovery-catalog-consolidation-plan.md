# Discovery And Catalog Consolidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the visible `clap` command tree the single canonical inventory of `ayx` commands, then make `ayx catalog` a compatibility-friendly projection of that inventory rather than a second hand-maintained command model. Keep catalog's capability behavior intact, preserve every existing annotated catalog entry and its metadata, and make CI reject stale catalog annotations and generated command-surface documentation.

**Architecture:** Add a small shared command-surface module that builds `Cli::command()` and walks its visible subcommands once into canonical command records. `ayx discover` continues to render the rich tree (arguments, options, aliases, and nested nodes) from that live clap tree, while `catalog` flattens the same canonical records, deriving `name`, slash-delimited `path`, and `summary` from clap. Replace the 209-entry `COMMAND_SPECS` command registry with a thin `CATALOG_METADATA` overlay keyed only by canonical path; it retains genuinely catalog-specific semantics (`output`, safety/mutation classification, prerequisites, notes), but no longer decides which commands exist. `catalog list` gains an explicit scope: the default `all` scope lists every visible clap node, and `curated` is the legacy-compatible view containing only annotated records. Capabilities remain independently sourced from `capability.rs` exactly as they are today.

**Tech Stack:** Rust, clap `CommandFactory`/`Command` introspection, `anyhow`, `serde_json`, existing `ayx_core::Envelope`, `cargo nextest`, the existing `xtask` generator, and GitHub Actions.

## Decision Record

Choose option **(b): collapse to one canonical source**, not a parity-only check.

Today there are two independently authored descriptions of command identity:

- `ayx-rs/src/cmd/discover.rs:62-151` constructs `Cli::command()`, resolves an optional whitespace-split path with `find_subcommand`, and recursively serializes non-hidden clap arguments and subcommands. `--deep` makes the recursion unbounded; the default stops after one subcommand level. `main.rs:5651` is its sole CLI dispatcher call site.
- `ayx-rs/src/main.rs:2855-5118` separately defines `CommandSpec` and `COMMAND_SPECS`. It holds 209 source entries, of which 206 are present in the default feature set and three are gated by `ui`. Each entry repeats the command's whitespace `name`, slash `path`, and a human summary in addition to catalog-only metadata. The current entries' names are mechanically equal to their paths with `/` replaced by a space.
- `ayx-rs/src/cmd/catalog.rs:42-81` lists that static array and appends capabilities from `capability::list_capabilities`; `catalog_describe_envelope` looks up the static array after trying a capability id. `ayx-rs/src/cmd/registry.rs:23-30` also uses the same array to validate action command references.
- `xtask/src/main.rs:36-95` invokes `ayx --output json catalog list --format full`, renders `docs/command-surface.md`, and only compares that generated file after stripping the timestamp. `.github/workflows/ci.yml:20-26` runs that check in the `fmt` job. It detects stale documentation, but it cannot detect a clap addition, rename, or removal that was not reflected in `COMMAND_SPECS`.

The `discover` method in `ayx-rs/src/capability.rs:64-81` is intentionally **not** the `ayx discover` implementation. It is `CloudCapabilityAdapter::discover`, a parser for an optional `AYX_CLOUD_CAPABILITIES_FILE`; `from_env` calls it at line 100. Catalog capability list/describe/run call `CloudCapabilityAdapter::from_env` through `capability::{list_capabilities,describe,run}` (`capability.rs:339-403`). This consolidation must not merge, rename, or otherwise change that capability registry or its availability behavior.

A parity check would catch missed edits, but would perpetuate the requirement to add every command twice and would still make reviewers decide which model was authoritative after a failure. Deriving command identity and summary from clap removes that category of divergence. The remaining overlay contains information clap does not model; it is validated against the canonical tree, not treated as an alternate inventory.

## Public Contract And Compatibility Rules

- `ayx discover`, all existing command spellings, aliases, hidden-command behavior, and its envelope schema remain unchanged. The root `ayx` node is not a catalog command; every non-hidden node beneath it is.
- `ayx catalog list`, `catalog describe`, and `catalog run` remain available. Do not deprecate or rename the `catalog` command: roadmap and README material explicitly establish it as the compatibility registry for commands plus capabilities.
- Add `--scope all|curated` to `catalog list`, using clap's value enum. `all` is the default and is the canonical full command index. `curated` returns only records with a `CATALOG_METADATA` annotation and is the migration path for callers that require the old compact curated set (206 command records in the default build, 209 with `ui`). Capability records and `--tag` filtering retain their current behavior in both scopes.
- Existing annotated command records keep their current `kind`, `name`, `path`, `summary`, `output`, `safety`, `mutating`, `prerequisites`, and `notes` values. Preserve the current JSON types for these records. The legacy whitespace name and slash path forms both continue to resolve in `catalog describe`.
- New commands visible only through `--scope all` receive clap-derived `name`, `path`, and `summary`, plus `metadata_status: "unclassified"`, `output: null`, `safety: "unclassified"`, `mutating: null`, and empty full-format `prerequisites`/`notes`. This is deliberately explicit: absent catalog metadata must never be represented as read-only or safe by default. Annotated records use `metadata_status: "curated"`.
- Add `command_schema_version: 2` and `scope` to the list envelope's `data` object. These are additive fields. Document that `command_count` and list ordering vary by scope and are not a stable substitute for selecting `--scope curated`; sort all-scope command records by canonical slash path for deterministic automation.
- Derive every catalog summary from clap's one-line `about`, not from static metadata. During migration, move the current `COMMAND_SPECS.summary` wording into the appropriate clap command attribute wherever it is better or missing, so existing curated entries keep their text and every all-scope record has a non-empty one-line summary.
- The capability array, capability ids, schemas, providers, cloud availability, tag filter, and `catalog run` behavior are outside this change. The two arrays share an envelope for discoverability, but only the command array is being consolidated.
- The public `docs/command-surface.md` becomes the all-scope projection. It will grow, including group nodes that are currently absent from the curated document. The generated page remains a command index; `ayx discover [path] [--deep]` remains the source for flags, positional arguments, aliases, and nested progressive disclosure.

## Global Constraints

- Do not construct catalog command identity by parsing help text, spawning the `ayx` binary, scraping `docs/command-surface.md`, or maintaining a second generated source file. Construct it in-process from `Cli::command()`.
- Use the exact discover visibility rule: omit any command for which `Command::is_hide_set()` is true. Do not silently add hidden/internal commands to catalog, and do not make a hidden metadata row valid.
- Canonical command identity is `path.join(" ")` for `name` and `path.join("/")` for `path`, using `Command::get_name()` values, not aliases. Aliases remain discover data, not new catalog ids.
- Do not infer safety or mutation from verbs, HTTP method names, or the global `--apply` flag. Only a migrated `CATALOG_METADATA` row supplies those semantic fields; missing metadata must remain visibly unclassified.
- `CATALOG_METADATA` must be a keyed semantic overlay only: no `name`, no duplicate summary, no independent command list, and no entries that exist solely to make a command appear in catalog. Each row must reference exactly one live, non-hidden canonical slash path; duplicate, unknown, and hidden keys are implementation errors.
- Keep `catalog list --format compact|full` behavior for annotated records. `prerequisites` and `notes` remain full-format-only. New all-scope rows still have the required fields with the null/empty behavior defined above.
- Keep the existing `fmt` job's `cargo run -q -p xtask -- refresh-command-surface --check` gate. It remains the documentation freshness check; source-of-truth and overlay tests are added to the test gate rather than hidden inside xtask.
- Do not change the current capability implementation in `ayx-rs/src/capability.rs` except, if needed, comments that distinguish cloud capability discovery from CLI discovery. In particular, do not alter `CloudCapabilityAdapter::discover`, `list_capabilities`, `describe`, `run`, their environment-variable input, or the capability JSON schema.
- Preserve the existing working tree. The implementation may modify only files named by the tasks below plus the regenerated generated-document and changelog files explicitly called out there.

---

### Task 1: Establish a shared canonical live-command index and make discovery consume its visibility policy

**Files:**

- Create: `ayx-rs/src/cmd/command_surface.rs`
- Modify: `ayx-rs/src/cmd/mod.rs`
- Modify: `ayx-rs/src/cmd/discover.rs:1-151`
- Modify: `ayx-rs/src/main.rs` (only the clap `about` attributes required by the summary contract; do not touch the static catalog yet)

**Interfaces:**

- Produces a crate-private, owned live record such as:

  ```rust
  pub(crate) struct LiveCommand {
      pub name: String,
      pub path: String,
      pub summary: String,
  }

  pub(crate) fn root_command() -> clap::Command;
  pub(crate) fn visible_subcommands(command: &clap::Command) -> impl Iterator<Item = &clap::Command>;
  pub(crate) fn visible_commands() -> Vec<LiveCommand>;
  pub(crate) fn visible_command_paths() -> BTreeSet<String>;
  ```

  Exact names may vary, but the types must be owned where they outlive clap borrows and must expose the canonical whitespace and slash identity separately.

- `visible_commands()` walks the complete `Cli::command()` tree, excludes only the root and hidden nodes, and returns a stable lexicographically path-sorted list. It includes visible branch nodes as well as leaves: both are invocable/help-visible command surfaces, and this is the only definition that exactly matches a flattened `discover --deep` tree.
- The module owns the shared `!command.is_hide_set()` predicate. `discover` calls it instead of maintaining a second hidden-node filter.
- A non-empty clap `about` is mandatory for every visible node. `LiveCommand.summary` is that `about` converted to `String`; do not derive a summary from `long_about`, documentation prose, or `COMMAND_SPECS` at runtime.

- [ ] **Step 1: Add the shared walker with no catalog dependency**

  In `cmd/command_surface.rs`, import `clap::CommandFactory` and `crate::Cli`, construct a fresh root with `Cli::command()`, and implement a recursive collector that carries a vector of canonical `get_name()` tokens. For each visible child:

  1. append `child.get_name()` to the token vector;
  2. emit its `name` (`join(" ")`), `path` (`join("/")`), and required one-line `about`;
  3. recurse into the same child, regardless of whether it also has a default action; and
  4. sort the final records by `path` and reject duplicate paths defensively.

  Do not use `find_subcommand` or aliases for flattening; those belong to path resolution for `discover`, not canonical id generation.

- [ ] **Step 2: Refactor `cmd::discover` to use the shared root and child-visibility rule**

  Replace its direct `Cli::command()` construction with `command_surface::root_command()`. Keep its current path-token behavior exactly: split each positional `PATH` segment on whitespace, resolve with `find_subcommand`, produce the same unknown-path error, use `usize::MAX` for `--deep`, and serialize arguments/options/aliases as it does now.

  Change `build_node` to obtain subcommands through `command_surface::visible_subcommands`. Keep argument filtering (`arg.is_hide_set()`), `ArgAction` handling, `hidden` serialization, envelope messages, schema version, and depth behavior unchanged. This makes catalog and discovery share the command-hidden policy without changing discovery's public payload.

- [ ] **Step 3: Make every live summary clap-owned**

  Before removing static summaries, compare each existing `COMMAND_SPECS.summary` with the matching command's clap `about` text. Move the catalog wording into the `#[command(about = ...)]` attribute where clap lacks an about line or the existing help text is materially less precise. Preserve existing good clap wording only if it is semantically equivalent; otherwise prefer the established catalog wording so the curated response remains compatible.

  Include currently known gaps called out in `docs/roadmap/command-surface-coverage.md` (notably some `one` root commands). Audit all visible paths, not just the old 206 catalog paths. Add one-line attributes in the enum/module that owns the clap variant; do not use an external summary map as a fallback.

- [ ] **Step 4: Add source-of-truth tests before changing catalog**

  Add unit tests in `command_surface.rs` and `discover.rs` that:

  - flatten a deep discovery tree and assert that its canonical node path set exactly equals `visible_commands()`;
  - assert the root itself is not returned, no returned record is hidden, no path/name is duplicated, and `name.replace(' ', "/") == path` for every record;
  - assert every visible record has a non-empty, single-line summary; and
  - use a small synthetic clap tree with a hidden child and an alias to pin the intended behavior: hidden child omitted, canonical name retained, alias not treated as a second path.

  Expose only the narrowly scoped test helper needed to flatten `DiscoverNode`; do not make discover's JSON structs part of a new public library API.

- [ ] **Step 5: Run focused checks**

  Run:

  ```bash
  cargo fmt --all
  cargo nextest run -p ayx-rs command_surface
  cargo nextest run -p ayx-rs discover
  ```

  Expected: both flattened views agree, all visible clap nodes supply one-line summaries, and the existing discover envelope tests (if any) retain their current schema/behavior. This task intentionally leaves `COMMAND_SPECS` in place so catalog's output has not changed yet.

---

### Task 2: Replace `COMMAND_SPECS` with a metadata overlay and derive catalog command records from clap

**Files:**

- Modify: `ayx-rs/src/main.rs:2828-5118`
- Modify: `ayx-rs/src/cmd/catalog.rs:1-329`
- Modify: `ayx-rs/src/cmd/mod.rs`
- Create tests in: `ayx-rs/src/cmd/catalog.rs` or a focused `ayx-rs/src/cmd/command_surface.rs` test module

**Interfaces:**

- Remove `pub(crate) struct CommandSpec` and `pub(crate) const COMMAND_SPECS` from `main.rs` completely.
- Keep `CatalogCommand` in `main.rs`, and change its `List` variant to accept:

  ```rust
  #[arg(long, value_enum, default_value_t = CatalogScope::All)]
  scope: CatalogScope,
  ```

  Define `CatalogScope::{All, Curated}` with clap `ValueEnum`, `Debug`, `Clone`, `Copy`, `PartialEq`, and `Eq`. `all` must be the explicit default.

- In `cmd/catalog.rs`, define a private static semantic overlay that contains only:

  ```rust
  struct CatalogMetadata {
      path: &'static str,
      output: &'static str,
      safety: &'static str,
      mutating: bool,
      prerequisites: &'static [&'static str],
      notes: &'static [&'static str],
  }

  const CATALOG_METADATA: &[CatalogMetadata] = &[ /* migrated current rows */ ];
  ```

  The precise type may use an enum for safety internally, but it must serialize the existing strings unchanged. It must not contain `name`, `summary`, or an independent command-existence flag.

- Add a derived, owned internal record for JSON rendering. Its semantic fields are optional so all-scope, unannotated commands can be represented honestly:

  ```rust
  struct CatalogCommandRecord {
      name: String,
      path: String,
      summary: String,
      metadata_status: MetadataStatus, // Curated | Unclassified
      output: Option<&'static str>,
      safety: Option<&'static str>,
      mutating: Option<bool>,
      prerequisites: &'static [&'static str],
      notes: &'static [&'static str],
  }

  fn catalog_command_records(scope: CatalogScope) -> Result<Vec<CatalogCommandRecord>>;
  ```

- `catalog_command_records(All)` joins every `command_surface::visible_commands()` record with its optional metadata. `Curated` filters that already-derived vector to metadata-bearing records. It never starts from `CATALOG_METADATA` and therefore cannot omit a newly added visible command from all-scope catalog output.

- `catalog list` data gains `command_schema_version: 2`, `scope`, and the per-record `metadata_status` field. `catalog describe` resolves all live commands (not only curated records) and uses the same record serialization. Existing capability-first resolution remains unchanged.

- [ ] **Step 1: Move, do not reinterpret, the existing semantic metadata**

  Move each current `COMMAND_SPECS` entry into `CATALOG_METADATA` in the same order and behind the same `#[cfg(feature = "ui")]` guards. For each row:

  - retain the existing slash `path` as the metadata key;
  - delete the redundant `name` field;
  - remove `summary` only after Task 1 has migrated it into clap;
  - copy `output`, `safety`, `mutating`, `prerequisites`, and `notes` byte-for-byte; and
  - retain all 206 default-feature annotations and the three UI annotations.

  This is a data relocation, not an opportunity to silently reclassify existing mutations or rewrite prerequisites. Any intentional metadata correction belongs in a separately justified change.

- [ ] **Step 2: Validate the overlay before exposing it**

  Build a `BTreeMap<&'static str, &CatalogMetadata>` (or equivalent) from the overlay and return a descriptive `anyhow` error when a key is duplicated, absent from `visible_command_paths()`, or points to a hidden command. Invoke this validation from all catalog record construction, so a renamed/removal command cannot produce stale catalog documentation.

  Add a helper that accepts a supplied metadata slice and a supplied live-path set so unit tests can exercise duplicate, unknown, and hidden-key errors without mutating the real static overlay. Error messages must identify the offending slash path and explain whether it is duplicate, unknown, or hidden.

- [ ] **Step 3: Derive all command fields from the live index**

  Implement `catalog_command_records` by iterating the sorted live records. For each record, copy its clap-derived `name`, `path`, and `summary`; then merge a matching metadata row if one exists. Render records as follows:

  | Record kind | `metadata_status` | `output` | `safety` | `mutating` | Full-format arrays |
  | --- | --- | --- | --- | --- | --- |
  | Curated (migrated row) | `"curated"` | existing string | existing string | existing boolean | existing arrays |
  | Live but unannotated | `"unclassified"` | `null` | `"unclassified"` | `null` | `[]` / `[]` |

  Continue to emit the legacy keys (`kind`, `name`, `path`, `summary`, `output`, `safety`, `mutating`) for both rows. `compact` omits only `prerequisites` and `notes`, as before; status is included in both formats. Do not use `false` as the unclassified mutation default.

- [ ] **Step 4: Update list and describe routing**

  Thread `scope` through `CatalogCommand::List`, `cmd::catalog::execute`, and `catalog_list_envelope`. Add `scope` and `command_schema_version: 2` beside the existing `format`, `tag`, `count`, `command_count`, `capability_count`, `commands`, and `capabilities` fields.

  `catalog describe` has no scope restriction: after checking a capability id it should search the all-scope derived command map by canonical whitespace name or slash path. This preserves every existing successful query while allowing `catalog describe` on a formerly uncataloged visible command. Keep `catalog run` strictly capability-only.

  Sort command list output by slash path. State this ordering in the command help/docs rather than relying on declaration order from the former static array.

- [ ] **Step 5: Add catalog compatibility and derivation tests**

  Extend `cmd/catalog.rs` tests to cover all of the following:

  - all-scope `catalog_list_envelope` command paths exactly equal `command_surface::visible_command_paths()`;
  - every all-scope row has canonical matching `name`/`path`, a nonblank clap summary, and either a curated row or the explicit unclassified/null fields;
  - curated scope returns exactly the metadata-key path set, retaining all legacy representative values (including mutating Mongo/One commands, a read-only command, `catalog list`, and feature-gated rows under `ui`);
  - `catalog describe` still finds `mongo backup`, `server/api/import-swagger`, `license api diagnose`, `one auth diagnose`, and a capability id, and additionally finds a visible all-scope command that is not curated;
  - unknown/duplicate/hidden metadata keys fail validation with actionable errors; and
  - capability tag filtering, capability JSON, and `catalog run` retain their current tests unchanged.

  Do not retain a second test-only list of every old command id. The migrated overlay is checked against the real tree; compatibility is pinned through representative field assertions, the preserved overlay entries, and the explicit `curated` scope contract.

- [ ] **Step 6: Remove the legacy static model and run focused checks**

  Delete the old type and array only after all call sites compile against derived records. Verify there are no remaining references:

  ```bash
  rg -n 'COMMAND_SPECS|CommandSpec' ayx-rs ayx-registry xtask docs README.md
  cargo fmt --all
  cargo nextest run -p ayx-rs catalog
  cargo nextest run -p ayx-rs command_surface
  ```

  Expected: `rg` finds no implementation dependency on the legacy model (historical changelog/audit prose may be updated in Task 4); catalog list all-scope equals the live tree; curated preserves the old annotated records; and no stale metadata key can be emitted.

---

### Task 3: Point action/workflow validation at the canonical live tree and preserve capability behavior

**Files:**

- Modify: `ayx-rs/src/cmd/registry.rs:1-30,183-185`
- Modify: `ayx-registry/src/lib.rs:1-30`
- Modify: `ayx-registry/src/validate.rs:1-20,75-90`
- Optionally modify comments only: `ayx-rs/src/capability.rs:58-105,339-403`

**Interfaces:**

- `LiveCatalog` in `cmd/registry.rs` becomes a live-command lookup backed by `command_surface::visible_command_paths()`, ideally stored as a `BTreeSet<String>` when `actions validate` starts. It no longer imports `COMMAND_SPECS` or relies on catalog scope/metadata.
- `CatalogLookup::has_command_path` continues to receive the whitespace command path extracted from action YAML (for example, `"one flows list"`). Convert the canonical live record set to that same `name` form, or expose an additional live-name set. Do not alter `ayx-registry`'s command-line parser or validation finding schema.
- The `CatalogLookup` trait's behavior and capability lookup remain unchanged; only comments must stop claiming the adapter is over `COMMAND_SPECS`.

- [ ] **Step 1: Make `LiveCatalog` live-tree-backed**

  Replace `use crate::{ActionsCommand, COMMAND_SPECS, WorkflowsCommand};` with imports for the new shared surface module. Give `LiveCatalog` a constructor that captures the set of canonical whitespace command names from the full visible live index. Keep `has_capability` delegated to `capability::has_capability`.

  In `ActionsCommand::Validate`, construct this lookup before calling `ayx_registry::validate::validate`; propagate any new surface validation error through the existing `Result<Envelope>` path. Do not make actions validation invoke `ayx catalog list` as a subprocess and do not restrict it to curated scope—an action referencing a real live command must validate even before someone adds semantic catalog metadata.

- [ ] **Step 2: Update ownership comments without changing the registry crate's abstraction**

  In `ayx-registry/src/lib.rs` and `validate.rs`, replace statements that say capabilities/commands are produced from or queried through `COMMAND_SPECS` with language that describes the CLI's canonical command surface and capability registry. Preserve the trait boundary: `ayx-registry` must not depend on `ayx-rs`.

  Leave its command-token extraction, unknown-command finding, safety heuristic, action schema, and permissive/empty test doubles unchanged. Those are unrelated to consolidation.

- [ ] **Step 3: Add end-to-end adapter tests**

  Add `cmd/registry.rs` tests that demonstrate:

  - a known legacy action command still resolves;
  - a visible command that is available only in all-scope catalog also resolves through `LiveCatalog`; and
  - an invented command remains unknown.

  Run the existing registry-validation test suite as well. This proves registry references now track command reality, rather than an arbitrary curated catalog subset.

- [ ] **Step 4: Explicitly preserve the capability half of catalog**

  Retain the existing `cmd/catalog.rs` call sequence: `catalog list` calls `capability::list_capabilities(tag, full)`, `catalog describe` tries `capability::describe` before command lookup, and `catalog run` calls `capability::run`. Keep the `CloudCapabilityAdapter::from_env()` calls in those functions and the `AYX_CLOUD_CAPABILITIES_FILE` test coverage intact.

  If comments are clarified, state that `CloudCapabilityAdapter::discover` is cloud-capability availability parsing, not `ayx discover`. Do not combine the registries or require cloud capabilities to have a clap command path.

- [ ] **Step 5: Run focused checks**

  Run:

  ```bash
  cargo fmt --all
  cargo nextest run -p ayx-rs registry
  cargo nextest run -p ayx-registry
  cargo nextest run -p ayx-rs catalog
  ```

  Expected: bundled actions continue to validate against live command names, capability ids continue to validate/run as before, and no registry code refers to the removed static command model.

---

### Task 4: Regenerate documentation from the all-scope projection and publish the catalog migration contract

**Files:**

- Modify: `xtask/src/main.rs:18-23,75-95,134-224`
- Modify (generated): `docs/command-surface.md`
- Modify: `docs/cli-spec.md:144-153`
- Modify: `docs/roadmap/discovery-and-catalog.md`
- Modify: `docs/roadmap/command-surface-coverage.md`
- Modify: `README.md:181-211,270`
- Modify: `docs/output-format.md:1-26`
- Modify: `site/src/pages/index.astro:29`
- Modify: `CHANGELOG.md` under `## Unreleased`

**Interfaces:**

- `xtask refresh-command-surface` explicitly invokes:

  ```text
  cargo run -q -p ayx-rs -- --output json catalog list --format full --scope all
  ```

  It must not rely on the catalog default, so a future compatibility adjustment cannot silently shrink public generated documentation.

- The generated document's provenance line, introduction, and non-goals accurately say it is a full, flattened, live-clap command index with catalog metadata—not a hand-curated command list. It still directs users to `discover`/help for flags and nested tree traversal.
- The command table renders unclassified values clearly: blank output/mutating cell, `unclassified` safety, and no fabricated prerequisites/notes. Update `yes_no` only as needed to preserve `null` as a blank cell.

- [ ] **Step 1: Change the generator input and wording**

  Update `run_catalog_list` in `xtask/src/main.rs` to pass `--scope all`. Update the generated provenance string, overview, and non-goals section:

  - remove the assertion that the document is a curated catalog rather than the complete command tree;
  - state that command identity and summary come from the live clap tree at generation time;
  - state that catalog metadata may be unclassified until a richer annotation is added; and
  - retain the instruction to use `ayx discover --deep` or command help for flags, positional arguments, aliases, payload schemas, and implementation detail.

  Preserve timestamp normalization exactly so `--check` remains deterministic.

- [ ] **Step 2: Regenerate the checked-in command surface**

  Run the normal write form once after implementation:

  ```bash
  cargo run -q -p xtask -- refresh-command-surface
  cargo run -q -p xtask -- refresh-command-surface --check
  ```

  Review the diff. Expected changes are expanded command coverage, clap-aligned summaries, explicit unclassified cells where metadata has not yet been curated, and updated generated prose. Do not hand-edit the timestamp or table content.

- [ ] **Step 3: Update user-facing catalog semantics**

  In README, `docs/cli-spec.md`, `docs/output-format.md`, and the site homepage copy:

  - present `discover` as the progressive, rich tree/flag discovery API;
  - present `catalog list --scope all` as the complete flattened machine-readable command-and-capability index;
  - document `catalog list --scope curated` as the compatibility view for clients that need the previous fully annotated curated records;
  - state that `catalog describe` continues to accept both legacy command name/path and capability ids; and
  - make examples use the leading global `--output json` form and explicit `--scope all` where a full command list is meant.

  Do not promise that `catalog run` runs commands; it remains capability execution only.

- [ ] **Step 4: Close the roadmap items with the actual new invariants**

  Update `docs/roadmap/discovery-and-catalog.md` so its current scope records clap as canonical, catalog as a derived compatibility/metadata view, and the resolved two-model decision. Replace the old parity-check next step with follow-up work limited to enriching `CATALOG_METADATA` for currently unclassified commands and, later, deciding whether `curated` needs a formal deprecation window.

  Update `docs/roadmap/command-surface-coverage.md` to remove the claim that descriptions come from the in-binary `COMMAND_SPECS` registry. Its exit criterion should state that live clap `about` text supplies every visible command summary and generated docs are derived from the full live index.

- [ ] **Step 5: Record the user-visible change**

  Add a `### Changed` entry under `CHANGELOG.md`'s `## Unreleased` section. State that `ayx catalog` is now derived from the live clap command tree; `catalog list` defaults to all visible commands; existing annotated entries retain their metadata; and `--scope curated` exists for consumers that need the former curated projection. Mention that `docs/command-surface.md` now reflects the all-scope live index.

- [ ] **Step 6: Verify generated docs and the documentation site**

  Run:

  ```bash
  cargo run -q -p xtask -- refresh-command-surface --check
  npm run build
  ```

  Run the latter in `site/`, matching CI. Expected: the generator accepts the checked-in document after timestamp normalization, and the site accepts the expanded table/copy without broken links or rendering errors.

---

### Task 5: Gate the source-of-truth invariant in CI, including the UI feature variant

**Files:**

- Modify: `.github/workflows/ci.yml`
- Modify as needed for stable test naming: `ayx-rs/src/cmd/command_surface.rs`, `ayx-rs/src/cmd/catalog.rs`, `ayx-rs/src/cmd/registry.rs`

**Interfaces:**

- Keep `.github/workflows/ci.yml`'s existing `fmt` job command unchanged:

  ```bash
  cargo run -q -p xtask -- refresh-command-surface --check
  ```

- Add a dedicated Ubuntu command-surface test job, or equivalently clearly named steps in the existing test job, that executes the source-of-truth tests in both the default and `ui` feature configurations. Use `cargo nextest` and `--locked`, matching the repository's test policy.

- [ ] **Step 1: Add a targeted CI job**

  Add an Ubuntu `command-surface` job after the existing toolchain/nextest setup pattern. Run at least:

  ```bash
  cargo nextest run -p ayx-rs --locked command_surface
  cargo nextest run -p ayx-rs --locked catalog
  cargo nextest run -p ayx-rs --features ui --locked command_surface
  cargo nextest run -p ayx-rs --features ui --locked catalog
  ```

  If the repository's test names make the string filters too broad or brittle, put the contract tests in a clearly named integration test target and run it with `--test command_surface`. Do not use `--no-default-features` unless the current `ui` feature matrix requires it; the point is to validate the same default surface and the additive UI surface.

- [ ] **Step 2: Ensure failures explain the required repair**

  Contract-test assertion messages must identify whether a failure means:

  - discover and the shared index disagree about a visible path;
  - a live command lacks a clap summary;
  - `CATALOG_METADATA` has a duplicate, hidden, or unknown path;
  - all-scope catalog omitted or invented a live command; or
  - curated catalog failed to preserve an annotation.

  A maintainer adding a clap command should be able to see that the command automatically appears in all-scope catalog, then decide whether it also needs rich metadata/curated membership. A maintainer renaming/removing a command should receive an immediate metadata-key error rather than an unexplained docs diff.

- [ ] **Step 3: Run the complete pre-merge verification set**

  Run:

  ```bash
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --locked -- -D warnings
  cargo nextest run --workspace --locked
  cargo nextest run -p ayx-rs --features ui --locked command_surface
  cargo nextest run -p ayx-rs --features ui --locked catalog
  cargo run -q -p xtask -- refresh-command-surface --check
  npm run build
  git diff --check
  ```

  Run `npm run build` from `site/`. Expected: all platform-neutral source-of-truth tests pass, UI-gated metadata still resolves to UI-gated clap commands, the docs gate remains fresh, and no formatting/whitespace errors remain.

---

## Migration And Rollout Notes

1. Land Tasks 1-3 as one compatibility-preserving code change: all current catalog paths continue to resolve, and `--scope curated` is available before the default list expands. Do not release an intermediate build that deletes `COMMAND_SPECS` without the overlay and derived lookup in place.
2. The default `catalog list` expansion is additive: it may increase `command_count` and introduce unclassified records, but it does not remove any annotated command or capability. Consumers that parse the old fixed curated set should switch to `--scope curated`; consumers that want authoritative command identity should use `--scope all` (or omit scope after this release).
3. Preserve catalog list/describe/run command names and JSON envelope conventions. The only intentional schema additions are `command_schema_version`, `scope`, `metadata_status`, and the possibility of `null` semantic fields on newly all-scope-only records. Existing curated records retain their prior concrete values and types.
4. `xtask` is the first in-repo catalog consumer migrated to explicit all scope. `actions validate` intentionally bypasses catalog scope and uses the full live index, so action YAML does not become dependent on documentation curation.
5. Do not remove `--scope curated` in this implementation. Revisit it only after a documented compatibility window and after every command has the desired rich semantic annotation. Until then it is the explicit compatibility view, not a second command model.

## Completion Criteria

- No `COMMAND_SPECS`/`CommandSpec` command inventory remains in production code.
- Every command returned by all-scope catalog is a non-hidden node from `Cli::command()`, and every such live node is returned exactly once with canonical name/path/summary.
- `discover --deep` and catalog all scope flatten to the same visible command-path set while retaining their distinct payload shapes and purposes.
- Every metadata row resolves to exactly one live visible path; stale, duplicate, and hidden rows fail tests and CI.
- Existing curated catalog records, capability behavior, catalog describe compatibility, catalog run behavior, and action validation continue to work.
- `docs/command-surface.md` is generated from explicit all scope and CI confirms it is fresh.
- Default and `ui` feature configurations enforce the invariant in CI.
