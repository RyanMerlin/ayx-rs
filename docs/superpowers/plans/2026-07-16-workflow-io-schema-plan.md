# Action and Workflow I/O Schema Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every bundled action and workflow an explicit, machine-readable contract for the parameters it accepts and the `Envelope.data` record it returns, so an agent can discover a recipe, construct a valid invocation before it runs, and parse the result without reverse-engineering `<placeholder>` tokens or individual command envelopes.

**Architecture:** Add optional `input_schema` and `output_schema` fields to the current v2 `Action` and `Workflow` YAML shape. They are YAML-encoded JSON Schema documents held as `serde_json::Value`, constrained and validated by a small in-tree JSON Schema subset validator—no general JSON-Schema dependency and no second schema DSL. The input schema describes the existing string-valued `--param` / `--param-file` map; the output schema describes the existing action/workflow run record inside `Envelope.data` (`ActionRun` or `WorkflowRun`), not the outer `Envelope` and not arbitrary data from a child command. Load-time checks make declarations internally coherent; run-time checks validate actual inputs before any step starts and validate successful run output after it is assembled. Legacy files remain schema-v2 and work unchanged through an explicitly marked inferred string-input contract.

**Tech Stack:** Rust 2024, `serde`, `serde_yaml`, `serde_json`, `thiserror`, existing `ayx-registry` loader/executor, and the shared `ayx_core::envelope::Envelope` JSON output contract. No new crate dependency.

## Current-State Facts This Plan Preserves

- The current noun and wire format are **action/actions**, not tactic/tactics. `CURRENT_ACTION_SCHEMA` is `2`; `*.action.yaml` / `*.action.yml` load, whereas legacy `*.tactic.yaml` / `*.tactic.yml` are warned about and deliberately skipped. Do not add an alias, dual-read path, or a schema-v1 compatibility layer.
- `Action` currently owns `id`, `title`, `summary`, `safety`, `trigger` (`task_keywords` and `tags`), ordered `steps`, `validations`, and `rollback`. `Workflow` owns `id`, `title`, `summary`, `safety`, `tags`, ordered `actions`, and `success_criteria`. The roadmap's “sequence” is concretely `Action.steps` and `Workflow.actions`.
- Today `ayx actions run` and `ayx actions workflows run` accept a single `BTreeMap<String, String>`. Required keys are inferred by recursively scanning `<name>` placeholders in command templates; workflow execution passes that entire map to every referenced action.
- `ActionRun` and `WorkflowRun` already form the actual `Envelope.data` payload. Command-step envelopes are nested below `ActionRun.steps[*].envelope`; they are intentionally heterogeneous and must not be advertised as one action-specific result object.
- The capability catalog already publishes JSON-Schema-shaped `input_schema` and `output_schema` values as `serde_json::Value` in `ayx-rs/src/capability.rs`. The registry contract must reuse that familiar wire format, while adding stricter registry-specific validation so an action/workflow schema is reliable enough to drive execution.

## Contract Decisions

### Schema representation and supported subset

Use JSON Schema-shaped YAML stored directly in `Option<serde_json::Value>` fields named `input_schema` and `output_schema`.

This is preferable to a Rust/YAML-specific typed enum because Claude Code and other callers already receive JSON, the capability catalog has established exactly these field names and shape, and JSON Schema has familiar `type`, `properties`, `required`, `items`, `enum`, and `description` vocabulary. Keeping the field as JSON retains forward-compatible wire data without a conversion layer. A full Draft 2020-12 implementation would add a large dependency and deceptively imply support for references, combinators, conditional schemas, and remote resolution that this CLI cannot safely honor. Therefore the registry accepts only this documented, recursively validated subset:

| Keyword / location | Supported behavior |
| --- | --- |
| Every schema node | `type` (one of `object`, `array`, `string`, `integer`, `number`, `boolean`, `null`), non-empty `description`, `const`, and `enum` |
| Object nodes | `properties`, `required`, `additionalProperties` (boolean only) |
| Array nodes | `items` and `minItems` |
| String nodes | `minLength` |

Reject all other keywords, type unions, `$ref`, `$defs`, `allOf`/`anyOf`/`oneOf`/`not`, `pattern`, format assertions, and YAML values that cannot be represented as JSON. Enforce that both root schemas are `type: object`. Require explicit input schemas to set `additionalProperties: false`; their property names and non-empty descriptions are the complete published parameter contract. Output schemas may leave `additionalProperties` true so normal additive changes to run/audit metadata do not turn into a false failed operation.

The existing CLI accepts lexical parameter strings, and command substitution needs lexical strings. Thus version one permits only `type: string` properties below an `input_schema`; constraints are `required`, `minLength`, `enum`, and `const`. It intentionally does **not** silently coerce `--param` values into booleans, numbers, arrays, or objects. Output schemas support the complete subset above because `ActionRun` / `WorkflowRun` serialize to arbitrary JSON values. This is honest about the current CLI while still providing the parameter meaning, allowed values, and result shape an agent needs. Typed JSON input is a separate future CLI-interface change, not something this schema work should imply.

### Exact field semantics

`input_schema` validates the object assembled from the runner's supplied parameter map. For a direct action it contains all placeholders in that action **and in recursively composed `kind: action` steps**. For a workflow it contains the union of every referenced action's effective input contract. A workflow does not receive data from one action and feed it into another; it continues to accept one caller-supplied object up front.

`output_schema` validates the value in the successful outer envelope's `data` field:

- For `ayx actions run <id>`, the instance is the serialized `ActionRun` object (`action_id`, `title`, `safety`, `apply`, `mode`, `params`, `steps`, `validations`, and optional `rollback`).
- For `ayx actions workflows run <id>`, the instance is the serialized `WorkflowRun` object (`workflow_id`, `title`, `safety`, `apply`, `mode`, and ordered `actions`).

It is not a contract for the outer envelope fields (`ok`, `message`, `timestamp_utc`, `error_code`) and it is not a promise that a particular child command's heterogeneous `envelope.data` can be projected into a named workflow value. Existing `validations` and `success_criteria` remain the semantic success statements; schemas make the invocation and returned run-report shape machine-checkable. Named step outputs, output extraction, and output-to-input dataflow are deliberately out of scope for this additive change.

Representative authored YAML (no `schema_version` bump):

```yaml
# actions/mongo-backup-restore.action.yaml
input_schema:
  type: object
  description: Parameters for the Mongo backup/restore playbook.
  additionalProperties: false
  required: [profile, ts]
  properties:
    profile:
      type: string
      description: Named ayx profile whose Mongo deployment is backed up.
      minLength: 1
    ts:
      type: string
      description: Timestamp or unique label used as the backup-directory suffix.
      minLength: 1
output_schema:
  type: object
  description: Structured action-run report for the Mongo backup/restore playbook.
  required: [action_id, title, safety, apply, mode, params, steps, validations]
  properties:
    action_id:
      type: string
      description: Stable id of the action that produced this report.
      const: mongo.backup-restore
    mode:
      type: string
      description: Whether the run was planned or executed.
      enum: [plan, execute]
    steps:
      type: array
      description: Ordered command, composed-action, and note outcomes.
      items:
        type: object
        description: One recorded step outcome.
    validations:
      type: array
      description: Validation descriptions supplied by the action.
      items:
        type: string
        description: One validation description.
```

The final YAML uses the same complete run-report property set for each bundled action/workflow; the snippet is shortened only to make the design readable. Give every declared property a description and use a per-recipe `const` for `action_id` / `workflow_id` so a caller can detect a wrong recipe result.

### Compatibility, loading, and execution rules

- These fields are additive. Leave `CURRENT_ACTION_SCHEMA` at `2`, retain the existing default of `2` when `schema_version` is omitted, and keep all current v2 action/workflow extensions and `kind: action` composition unchanged.
- A legacy current-format file with no `input_schema` still has an **effective inferred input schema**: a root object whose required string properties are the recursively discovered placeholders and whose `additionalProperties` is true. It preserves current handling of extra `--param` keys and the existing `MissingParams` error. A missing `output_schema` means “not declared / not output-validated,” not an invented guarantee.
- An explicitly declared action schema must list every recursive placeholder as a required property and must use the same property definition as each referenced action. An explicit workflow schema must list exactly the union of the referenced actions' effective properties, with matching definitions. Reject disagreement and action-composition cycles during final registry validation instead of letting a caller find a missing key halfway through a run.
- Parse and schema-grammar errors are load-time errors with source path, owner kind/id, and JSON-pointer-like location. Cross-action/workflow contract checks run only after all override directories and bundled resources have loaded, because references can resolve across those sources. Keep command/capability catalog drift in `ayx actions validate`; it remains an out-of-band lint and must not make a registry fail to load.
- Before planning or spawning any subprocess, validate the direct action/workflow's effective input. For an explicit schema, reject missing, empty/too-short, enum/const-invalid, and unknown keys as a structured validation error. For a workflow or a composed action, filter the global map to each child action's declared property set before validating/running that child; otherwise a strict action would incorrectly reject a parameter belonging to a later action. Legacy inferred actions retain permissive extra-key behavior.
- After a successful action/workflow run is assembled, serialize its `ActionRun`/`WorkflowRun` and validate it against its declared output schema. An output mismatch is an `ExecutorError::OutputContractViolation` that names every failing JSON path and says explicitly that commands may already have run; do not retry, invent a rollback, or mask the failure as a successful contract. Per-step audit artifacts remain the evidence available to the operator.

## Global Constraints

- Preserve the post-rename public surface exactly: `ayx actions`, `ayx actions workflows`, `Action`, `Workflow`, `Step::Action`, `actions`, `actions_resolved`, `actions_missing`, and `*.action.yaml` / `*.action.yml`. Do not use stale tactic terminology anywhere new.
- Do not change the CLI's repeated `--param key=value` or YAML-map `--param-file` interface in this work. Explicit schemas describe its string inputs; no coercion or new `--json-input` route belongs here.
- Do not make a mutating/destructive action less safe. Schema validation must happen before `run_steps`; the established `--apply` gate and maximum workflow safety propagation must remain intact.
- Do not auto-run `Validation.check_cmd`, `Workflow.success_criteria`, rollback text, or a new schema-derived command. They remain declarative/operator-visible as they are today.
- Do not add a generic JSON Schema crate or accept unsupported JSON Schema keywords. The supported subset must be documented in Rustdoc and in command output so an agent never assumes a keyword is enforced when it is not.
- Keep the current `ayx actions validate` behavior for unknown commands/capabilities and dangling refs. Contract/load failures are hard errors because the runner could not safely know what to accept; command catalog findings continue to be reportable diagnostics.
- Run `cargo fmt --all` before each commit, then `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo nextest run --workspace --locked` before handoff.

---

### Task 1: Add the in-tree JSON Schema subset and stable error model

**Files:**

- Create: `ayx-registry/src/io_schema.rs`
- Modify: `ayx-registry/src/lib.rs:1-85` (module export and registry errors)
- Modify: `ayx-registry/Cargo.toml` only if a currently direct dependency is needed; the intended implementation uses already-present `serde_json` and adds no dependency.

**Interfaces:**

- Produces a crate-private schema module with functions equivalent to:

  ```rust
  pub(crate) fn validate_schema(schema: &Value, role: SchemaRole) -> Result<(), Vec<SchemaViolation>>;
  pub(crate) fn validate_instance(schema: &Value, instance: &Value) -> Vec<SchemaViolation>;
  pub(crate) fn inferred_string_object(required: BTreeSet<String>) -> Value;
  pub(crate) fn object_property_names(schema: &Value) -> BTreeSet<String>;
  ```

- `SchemaViolation` contains a JSON-pointer-style `path` (for example `/properties/profile/minLength`) and a concise reason. It is serializable only where needed for executor errors; do not expose Rust parser internals.
- `RegistryError` gains a schema/contract variant containing `path`, `owner_kind`, `owner_id`, `location`, and `message`. `ExecutorError` (Task 3) gets separate input and output contract variants rather than reusing a parse error.

- [ ] **Step 1: Define the subset once, with no permissive fallback**

  Implement recursive grammar validation over `serde_json::Value`. Check that every node is an object, that only the table's keywords occur, each keyword's value has the correct JSON type, and constraints are meaningful for the declared type (`properties` only for object, `items` only for array, `minLength` only for string, and so on). Validate each `required` name exists in `properties`; validate `enum`/`const` values against the declared type; reject negative `minItems` / `minLength`; reject a non-boolean `additionalProperties`.

  Require an object root in both roles. For `SchemaRole::Input`, require `additionalProperties: false`, require each property to have a non-empty `description`, and reject every property type other than `string`. For `SchemaRole::Output`, permit all supported types and retain JSON Schema's permissive additional-properties default. Do not add `$schema`: the existing capability entries omit it, and the registry's documented subset identifier is Rustdoc/CLI documentation, not a claim of full draft conformance.

- [ ] **Step 2: Implement instance validation without coercion**

  Validate `required`, `additionalProperties`, primitive type, `const`, `enum`, string length, array length/items, and recursive object properties. Return *all* deterministic failures sorted by pointer, so an agent receives one actionable payload instead of an edit/run/fail loop. Treat absent optional object properties as valid. A parameter value stays a JSON string even if it looks like `true` or `42`.

- [ ] **Step 3: Add unit tests before wiring it into the loader**

  Cover a valid schema using every supported node kind; invalid root, unknown keyword, invalid `required`, incorrect constraint/type pairing, missing input description, input `additionalProperties: true`/absent, and non-string input property. Cover valid/invalid instances for required/missing key, unknown key, min length, enum, nested object, array item, and `const`; assert exact pointer paths and sorted ordering. Cover `inferred_string_object` producing required string properties plus `additionalProperties: true`.

- [ ] **Step 4: Verify the isolated registry crate**

  Run:

  ```bash
  cargo fmt --all
  cargo clippy -p ayx-registry --all-targets --locked -- -D warnings
  cargo nextest run -p ayx-registry
  ```

  Expected: all new subset tests pass with no external schema dependency added to `Cargo.lock`.

---

### Task 2: Extend v2 registry types and validate contracts after the full registry is assembled

**Files:**

- Modify: `ayx-registry/src/lib.rs:57-454` (`RegistryError`, `Action`, `Workflow`, loading/finalization helpers)
- Modify: `ayx-registry/src/stdlib.rs:70-90` only as needed to run the same structural validation for bundled YAML
- Modify: `ayx-registry/src/lib.rs` test module

**Interfaces:**

- `Action` and `Workflow` each gain:

  ```rust
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub input_schema: Option<serde_json::Value>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub output_schema: Option<serde_json::Value>,
  ```

- Add internal `Registry::effective_action_input_schema(&self, id: &str)` and `Registry::effective_workflow_input_schema(&self, id: &str)` helpers. They return the declared contract or an inferred string-object schema plus an `explicit`/`inferred` origin marker for the CLI.
- Add an explicit post-load finalization method, called by `Registry::load_default()` only after override directories *and* `stdlib::install_into()` complete. It validates action-composition contracts, workflow contracts, and composition cycles, then calls existing `propagate_workflow_safety()`.

- [ ] **Step 1: Add optional fields without changing schema version or serde behavior**

  Insert the fields after the human-facing metadata in `Action` and `Workflow`; preserve `source_path` as loader-owned and skipped on serialization. Leave `CURRENT_ACTION_SCHEMA == 2`, `default_schema_version()`, file discovery, and the legacy tactic warning untouched. A v2 file with no schema must deserialize identically to today, and `actions export` must not materialize absent schema fields into a user's legacy action YAML.

- [ ] **Step 2: Validate declaration grammar at parse/insert time with useful provenance**

  Once a parsed action/workflow has its `source_path`, call `io_schema::validate_schema` for each present field and translate each violation to the new `RegistryError` variant. Apply the same path-aware check to filesystem files and bundled `include_str!` resources. This makes malformed declarations fail at their actual owner/file, not later as an opaque executor error.

- [ ] **Step 3: Build effective action input contracts recursively**

  Reuse the existing placeholder extraction rules exactly (`<name>` from `Step::Command`; recurse through `Step::Action`) rather than creating a second token parser. For an undeclared action, construct the inferred permissive string object from its transitive placeholder set. For a declared action, require that every direct/transitive placeholder appears in `properties` and `required`; compare every child action's property definition byte-for-byte after canonical JSON object ordering, so a parent cannot promise a weaker/different meaning for a child input. Report the parent action, child action, and conflicting property in the error.

  Track the action-id recursion stack and reject `A → B → A` with the complete cycle path. This is necessary because the schema traversal would otherwise make an existing latent recursive composition overflow before execution.

- [ ] **Step 4: Build and verify workflow input contracts**

  For each workflow, union the effective contracts of its ordered `actions`. An undeclared workflow receives an inferred, permissive string object so existing workflows continue to execute. A declared workflow must expose exactly that union as required properties, with each property definition identical to the action contract that consumes it; report both action ids if two actions give the same key incompatible definitions. Do not try to merge or weaken constraints. Do not derive a workflow output schema from child outputs—there is no current output binding/dataflow model.

- [ ] **Step 5: Finalize in the one real load path**

  In `Registry::load_default`, invoke the finalizer only after operator overrides and the bundled standard library have been inserted. This ordering preserves override precedence and lets cross-file references resolve. Keep `Registry::load_dir` as parsing/insertion for focused tests and callers building a registry incrementally; document that they must call the finalizer before resolving/running. Do not move `validate.rs` command/capability checks into this path.

- [ ] **Step 6: Add loader/compatibility tests**

  Add temporary-directory tests for: a current `*.action.yaml` without either schema loading with effective inferred `profile`; a declared input/output schema loading; malformed input and output schema errors containing source/id/pointer; an action that omits a direct placeholder; an action that omits a composed child's placeholder; a composition cycle; a workflow that omits/changes a required action property; and conflicting property declarations across workflow actions. Keep the existing tests proving `*.action.yaml` loads and legacy `*.tactic.yaml` is ignored unchanged, then assert they still pass with `CURRENT_ACTION_SCHEMA == 2`.

---

### Task 3: Enforce input contracts before execution and output contracts on the actual run records

**Files:**

- Modify: `ayx-registry/src/executor.rs:39-531`
- Modify: `ayx-registry/src/executor.rs` test module

**Interfaces:**

- Add errors equivalent to:

  ```rust
  ExecutorError::InputContractViolation { id: String, violations: Vec<SchemaViolation> }
  ExecutorError::OutputContractViolation { id: String, violations: Vec<SchemaViolation> }
  ```

  Their display text must name the action/workflow, quote the failing paths, and state that an output mismatch happens after steps may have executed.

- Add helpers that convert the existing `BTreeMap<String, String>` to a JSON object, select the parameter subset applicable to an action, and validate an effective input schema. Do not alter `ExecutionConfig.params`' public type.

- [ ] **Step 1: Replace duplicate placeholder scans with the registry's effective contract**

  Before `run_action_inner` invokes `run_steps`, validate its `cfg.params` against the action's effective input schema. Keep the current `MissingParams` behavior for inferred/legacy contracts so existing callers and tests keep their error contract; use the richer `InputContractViolation` for explicit schemas (unknown parameter, empty/short string, enum/const failure). All checks must occur before a read-only command, mutating plan, or `--apply` subprocess can start.

- [ ] **Step 2: Filter parameters at action and workflow boundaries**

  `run_workflow` must validate the whole map once against the workflow schema, then build a filtered child `ExecutionConfig` for each referenced action. `run_steps` must do the same when it expands `Step::Action`. A strict action receives only its declared keys; an inferred legacy action retains the old permissive map. This prevents a valid workflow parameter intended for action two from becoming an “unknown property” error in action one, while preserving the existing global input interface and audit behavior.

- [ ] **Step 3: Validate the documented output instance, not a child-command guess**

  After `run_action_inner` builds `ActionRun`, serialize that run with `serde_json::to_value` and validate it against the action's declared `output_schema` when present. After `run_workflow` builds `WorkflowRun`, do the same against the workflow's declared schema. Validate plan-mode records too: schemas must allow `mode: plan` and planned steps, so discovering a schema error never requires `--apply`.

  An output violation returns `OutputContractViolation`; it must not retry, spawn any additional step, or trigger rollback text. Add a comment at the error construction point explaining that this is a post-execution contract-integrity failure, not proof that a mutating operation was undone.

- [ ] **Step 4: Retire only the now-redundant executor parameter collector**

  Remove or make private-unused `collect_required_params` only after all action and workflow execution uses the effective-contract path. Keep the single placeholder extractor available to `Registry` so inference exactly preserves current syntax. Do not change substitution: it still operates on the original lexical strings and leaves an unreachable unknown placeholder intact defensively.

- [ ] **Step 5: Add deterministic executor tests**

  Build small in-memory/mutable test registries with mutating command steps and `apply = false`, so no test needs a real profile or subprocess. Prove explicit missing/unknown/enum/min-length errors happen before a planned step exists; prove legacy missing keys still produce `MissingParams`; prove two strict workflow actions with disjoint parameters both plan successfully and receive only their own parameters; prove the same behavior through a nested `Step::Action`; prove a matching action/workflow output schema passes in plan mode; and prove a deliberately wrong output `const` produces `OutputContractViolation` without executing a command.

---

### Task 4: Expose effective contracts to callers and align interactive prompting with them

**Files:**

- Modify: `ayx-rs/src/cmd/registry.rs:33-428`
- Modify: `ayx-rs/src/main.rs:450-543` only to correct/help-text wording if needed; do not change flags
- Modify: focused unit/integration tests in `ayx-rs/src/cmd/registry.rs` or existing `ayx-rs/tests/` files

**Interfaces:**

- `ayx actions describe <id> --output json` returns the action's declared metadata plus an `input_schema` field that is either declared or effective-inferred and an `input_schema_source` value (`"declared"` or `"inferred"`). Preserve the existing fields and make this additive.
- `ayx actions workflows explain <id> --output json` exposes the equivalent workflow `input_schema`, `input_schema_source`, and declared `output_schema` within the `workflow` object, while retaining `actions_resolved` and `actions_missing` (the current post-rename names).
- `--prompt-missing` reads the effective contract's `required` list rather than independently scanning templates. Its prompt behavior remains TTY-only and string-valued.

- [ ] **Step 1: Make discovery output the agent-facing source of truth**

  Replace direct `serde_json::to_value(action)` in `ActionsCommand::Describe` with a small descriptor builder that adds the effective input schema and origin without modifying stored `Action` data. For declared schemas, the value must round-trip exactly as authored. For a legacy action, render the inferred required string properties and permissive `additionalProperties: true`, so an agent still has a concrete contract rather than scraping `steps`.

  In workflow `Explain`, produce a similarly augmented workflow descriptor, not a new tactic-shaped envelope. Include `output_schema` only when declared; never synthesize an output promise for old custom files.

- [ ] **Step 2: Use one required-key source for prompting**

  Replace `collect_action_params` / manual workflow action loops in `prompt_missing_action_params` and `prompt_missing_workflow_params` with the registry helpers from Task 2. Preserve existing sort/deduplicate, non-TTY no-op, and empty-line behavior. This ensures interactive and non-interactive callers see the same required key list, including nested action composition.

- [ ] **Step 3: Keep compact list/resolve output compact**

  Do not place full schemas in `actions list`, `actions resolve`, or `actions workflows list`; they are ranking/index endpoints and would become needlessly large. Document in help comments/tests that an agent should resolve/list, then call `describe` or `workflows explain` before constructing parameters. The existing `--output json` envelope remains the transport for both discovery and execution.

- [ ] **Step 4: Test the actual agent path**

  Add command-layer tests asserting that a declared bundled action description includes `input_schema`, `output_schema`, `required`, property descriptions, and `input_schema_source: declared`; that a temporary legacy custom action shows `input_schema_source: inferred` and no output schema; and that workflow explain exposes its own schema alongside the current `actions_resolved` key. Add a prompt-helper test for a composed action/workflow required-key set without opening stdin.

---

### Task 5: Annotate every bundled v2 action and workflow with truthful contracts

**Files:**

- Modify: all `ayx-registry/actions/*.action.yaml`
- Modify: `ayx-registry/workflows/backup-restore.workflow.yaml`
- Modify: `ayx-registry/workflows/governance-go-live.workflow.yaml`
- Modify: `ayx-registry/src/stdlib.rs` only if a test list/assertion needs updating; its `include_str!` paths and resource names stay unchanged.

**Interfaces:**

- Every bundled action receives a declared strict `input_schema` covering its transitive placeholders and a declared `output_schema` for `ActionRun`.
- Both bundled workflows receive a strict `input_schema` that is the exact union of their referenced actions' effective inputs and a declared `WorkflowRun` output schema.

- [ ] **Step 1: Inventory inputs mechanically before authoring**

  Use the shared registry helper/test to list each action's transitive placeholder set, then author only those keys. This must catch composition: `server.upgrade.preflight` inherits the backup action's `ts`; `one.workspace-migrate` inherits `one.flow.promote` inputs. Do not infer inputs from prose or from stale pre-rename examples. Name each property exactly as the template uses it, including hyphenated keys such as `idp-metadata-url` where present.

- [ ] **Step 2: Author strict, useful input schemas**

  Add a root `description`, `additionalProperties: false`, all required keys, and a non-empty description/minimum length for every parameter. Use `enum` only where the current action really limits values. Do not invent a type such as `path`, a secret field, an environment enum, or an optional input merely because it would be convenient for a future action; the contract must describe what the current command template accepts.

- [ ] **Step 3: Author output schemas for the current runner—not fictional business objects**

  For each action, require the stable `ActionRun` fields and give `action_id` the action's exact `const`; include appropriately typed/described `mode`, `params`, `steps`, `validations`, and `rollback` as optional. For each workflow, require the stable `WorkflowRun` fields, use the workflow id as `const`, and describe ordered `actions` as action-run report objects. Keep output `additionalProperties` open to preserve additive audit/envelope evolution. Do not claim fields such as `backup_path`, `flow_id`, or `target_workspace` exist at the top level unless the executor actually emits them.

- [ ] **Step 4: Verify contracts against the bundled registry**

  Add a table-driven test that `Registry::load_default()` finalizes every bundled action and workflow, every bundled action/workflow declares both schemas, each declared action input equals its calculated transitive placeholder set, and each declared workflow input equals the union of its actions. This makes a future action/template edit fail in CI until its published contract is updated.

- [ ] **Step 5: Preserve source and rename compatibility**

  Do not add `schema_version: 3`, rename a YAML file, alter `kind: action`, add a `tactics:` field, or change `stdlib.rs` include names. Existing custom v2 action files without the new fields must keep loading beside this fully annotated standard library.

---

### Task 6: Validate end-to-end behavior, regressions, and release documentation

**Files:**

- Modify: `CHANGELOG.md` under `## Unreleased` with an `### Added` or `### Changed` entry that calls out machine-readable action/workflow I/O contracts, v2/additive compatibility, and strict validation for newly declared schemas.
- Modify: `docs/roadmap/runtime-and-orchestration.md` to mark the explicit workflow input/output schema next step complete and state the resulting agent-facing contract.
- Modify: tests only where Tasks 1–5 identify gaps; do not create a parallel tactic fixture or a second registry model.

- [ ] **Step 1: Run focused regression suites while implementing**

  ```bash
  cargo nextest run -p ayx-registry
  cargo nextest run -p ayx-rs registry
  cargo nextest run -p ayx-rs catalog
  ```

  Expected: existing tactic-skip, `*.action.yaml` discovery, workflow-safety, executor plan-mode, catalog capability-schema, and registry command tests remain green alongside the new schema cases.

- [ ] **Step 2: Exercise the human/agent CLI sequence against the built binary**

  In JSON mode, verify this sequence (substitute a harmless declared action/workflow if a fixture is safer):

  ```bash
  ayx --output json actions describe mongo.backup-restore
  ayx --output json actions workflows explain ops.backup-restore
  ayx --output json actions run mongo.backup-restore --param profile=test --param ts=2026-07-16
  ayx --output json actions workflows run ops.backup-restore --param profile=test --param ts=2026-07-16
  ```

  Confirm discovery exposes declared schemas and origin, mutating runs remain plans without `--apply`, matching plan output passes its output contract, a missing/invalid/unknown declared parameter returns a validation-classified failure before a command runs, and the workflow passes filtered parameters to each child. Also confirm `ayx actions validate` still reports its ordinary catalog findings rather than changing into a schema loader command.

- [ ] **Step 3: Run the required full quality gate**

  ```bash
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --locked -- -D warnings
  cargo nextest run --workspace --locked
  ```

  Expected: clean format/clippy and all workspace tests pass. Inspect `git diff --check`, then inspect `git status --short` to ensure the implementation touched only the planned source, registry YAML, tests, roadmap, and changelog files—never generated audit artifacts or an accidental legacy tactic file.

- [ ] **Step 4: Commit in reviewable slices**

  Keep the schema engine + type/loading/finalization tests separate from executor/CLI behavior, then land bundled YAML/doc updates with their invariant test. Suggested messages:

  ```text
  feat(registry): add validated action and workflow I/O contracts
  feat(actions): expose and enforce registry input/output schemas
  docs(registry): publish bundled action workflow I/O contracts
  ```

  Every commit must compile and its tests must pass; never land standard-library schemas before the loader understands them.
