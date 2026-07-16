# Mongo Mutation Execution With Preview Approval and Audit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a real, bounded, and reversible `ayx mongo` mutation path. An operator must first create and inspect a database-backed diff artifact, then explicitly approve that exact artifact before a named, reviewed template can modify Mongo. Both the preview and every terminal execution outcome must be durable audit artifacts; a guarded undo path must be available for the limited update semantics this release supports.

**Architecture:** Split the current mixed `queries.yaml` model into read-only support-query and mutation-template registries. Resolve only a named mutation template into a typed, parameter-bound `$set` operation; run a read-only preview that returns a per-document field diff and an approval digest; then use a structured `mongosh` invocation (not a shell-display string) to revalidate and execute the approved diff inside a Mongo transaction. A mutation is permitted only when all of these are present: a fresh successful backup audit, preview-artifact path, matching approval digest, `--apply`, `--accept-mutation-risk`, and interactive confirmation or global `--yes`. Audit persistence is a precondition to execution, and the same artifact is updated from `prepared` to `applied` or `failed`.

**Tech Stack:** Rust workspace crates (`ayx-rs`, `ayx-server`, `ayx-core`), existing `mongosh` subprocess integration, `serde`/`serde_json`/`serde_yaml`, existing `sha2` dependency, `chrono`, and the sensitive-file audit helpers. No MongoDB Rust-driver dependency is needed: the only live database transport remains `mongosh`.

## Current-State Facts This Plan Preserves or Corrects

- `docs/roadmap/mongo-registry-and-remediation.md` correctly identifies the delivery gap: mutation templates are preview-first, execution is deliberately disabled, and there is no mutation audit-artifact path.
- The present registry is `ayx-server/knowledge/mongo/queries.yaml`. Its sole writable entry, `user_email_domain_migration`, is still a `MongoQueryTemplate` with `read_only: false`; read-only diagnostics and remediation definitions are therefore not structurally separate yet.
- `ayx-rs/src/main.rs` exposes `mongo mutate` as free-form `--database`, `--collection`, `--filter`, `--update`, and optional `--template` arguments. `ayx-rs/src/cmd/mongo.rs` passes those values to `ayx_server::mongo::mutate_envelope`.
- `ayx-server/src/mongo.rs::mutate_envelope` returns a static plan for `--print` or no `--apply`, enforces `--accept-mutation-risk` only after `--apply`, and then always errors with `mongo mutate execution is not yet enabled; preview only`.
- Backup and restore already call `ayx_core::audit::write_audit_artifact` on both dry-runs and applied calls. Their payloads contain `command`, `timestamp_utc`, `profile`, `dry_run`, `applied`, operation-specific paths, execution data, and a `safety_gate`; their envelopes return `audit_artifact`.
- `Cli::yes` is already a global flag and `cmd::confirm::require_tty_confirmation` already implements the project confirmation policy. The top-level Mongo dispatch currently calls `cmd::mongo::execute(environment, command)` without passing `cli.yes`, so mutation cannot use that policy until the explicit dispatch boundary is widened.
- Do **not** build a live mutation executor on `build_mongosh_mutation_eval` or `execute_query_plan`. The former returns a copy/paste command string and drops the temporary password file created while composing it; the latter wraps that display string in another `mongosh --eval` call and does not carry connection arguments. Live execution needs an owned, structured process invocation with the password-file lifetime spanning `Command::output`.
- `COMMAND_SPECS` currently catalogs `mongo status`, `inventory`, `backup`, `restore`, `query`, and `doctor`, but has no `mongo mutate` entry. Registry validation and catalog discovery will otherwise continue to misrepresent this command.

## Global Constraints

- This release executes **only named mutation templates**. Free-form `--database`/`--collection`/`--filter`/`--update` input may remain available for a non-executing `--print` compatibility plan, but must be rejected whenever the caller asks for a database preview or supplies `--apply`. Never turn arbitrary JSON pasted at the command line into a production write path.
- V1 executable templates support one non-empty `$set` document only. They may not write `_id`, use pipeline updates, `$where`, `$expr`, JavaScript, array positional operators, or any other update operator. This intentionally narrow contract makes the preview diff and guarded inverse deterministic. Unsupported existing or future templates remain `preview_only` until a separately designed semantics/rollback implementation exists.
- Require a non-empty template filter and a template-owned `max_affected` cap. Enforce a global hard maximum of 1,000 documents in code even if a template asks for more. The preview must request one extra document and fail before approval when the cap would be exceeded.
- Parameter substitution is structural, not string interpolation. A template placeholder must occupy an entire JSON string (`"${parameter_name}"`); resolve it by walking the parsed JSON tree and replacing it with the parameter's declared typed JSON value. Reject unknown, missing, duplicate, malformed, or inline placeholders. Never concatenate a value into JavaScript supplied to `mongosh`.
- Treat preview, execution, and undo data as sensitive local artifacts. They may contain document identifiers, prior field values, and PII. Continue to use the owner-only directory/file posture from `ayx_core::audit`; do not record raw passwords, unredacted Mongo URIs, or temporary credential-file contents/paths.
- A successful applied `mongo backup` audit artifact is a mandatory execution prerequisite. A dry-run backup, a restore artifact, a backup for another profile, a missing output directory, or an artifact older than the template's `max_backup_age_minutes` must fail closed. This is a recovery prerequisite, not proof that a backup is restorable; the runbook still requires a tested restore.
- Require a transaction-capable Mongo deployment for `--apply` and undo. The `mongosh` program must use `session.withTransaction`; if the server is standalone or otherwise cannot start a transaction, return a clear no-write error directing the operator to use backup/restore or a topology that supports transactions. Do not silently fall back to a partial `updateMany`.
- The write path must re-read the candidate identities and the fields affected by `$set` inside that transaction and compare them with the approved preview snapshot. Any mismatch, count change, template revision/source-digest mismatch, or post-update verification mismatch aborts the transaction. There is no `--force`, stale-preview override, or automatic retry for mutations.
- A mutation is never retried after `mongosh` starts. A transport/process failure may be ambiguous about commit status; record it as `failed_or_unknown`, tell the operator to inspect the artifact and query the target, and do not issue a second write automatically.
- The existing backup/restore artifact format is a useful baseline, not a sufficient mutation format. Preserve its common fields and add explicit template identity, approval, diff, backup provenance, lifecycle status, and undo data described below.
- Keep `cargo fmt --all` clean. The final implementation must pass `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run --workspace --locked`, and the command-level tests in this plan.

## CLI Contract

### Preview (default, read-only)

```sh
ayx mongo mutate \
  --profile prod \
  --template user_email_domain_migration \
  --param new_email='admin@companyB.com' \
  --audit-dir /var/lib/ayx/audits
```

This runs a read-only candidate query, produces a capped field-level diff, writes a `mongo-mutate-preview-*.json` artifact, and returns an envelope containing `preview`, `approval_artifact`, and `approval_digest`. It does not call `updateMany`, does not require `--accept-mutation-risk`, and has `dry_run: true`, `applied: false`.

`--print` remains a no-connection static rendering mode for compatibility. It prints the resolved template and a redacted `mongosh` display command, reports `preview_available: false`, and writes no approval artifact because it has not inspected live candidate documents. It is mutually exclusive with `--apply`, `--approval-artifact`, `--approve`, and `--backup-audit-artifact`.

### Approved execution

```sh
ayx --yes mongo mutate \
  --profile prod \
  --template user_email_domain_migration \
  --param new_email='admin@companyB.com' \
  --backup-audit-artifact /var/lib/ayx/audits/mongo-backup-20260716T120000.000Z.json \
  --approval-artifact /var/lib/ayx/audits/mongo-mutate-preview-20260716T121500.000Z.json \
  --approve sha256:<digest-returned-by-preview> \
  --accept-mutation-risk \
  --apply \
  --audit-dir /var/lib/ayx/audits
```

For a TTY caller, omitting global `--yes` displays the existing confirmation prompt after all read-only validation succeeds. The prompt must name the profile, `database.collection`, template id/revision, planned count, and short approval digest. A non-TTY caller without `--yes` is refused by `require_tty_confirmation` before the transaction starts.

The executor must validate these gates in this order, without any write before the last step:

1. Parse and resolve the named template plus typed `--param key=value` values; reject arbitrary live mutation input and unsafe template semantics.
2. Require the complete apply tuple: `--apply`, `--accept-mutation-risk`, `--backup-audit-artifact`, `--approval-artifact`, and `--approve`. Explain every missing item in one validation error.
3. Load and validate the backup artifact, then load the preview artifact. Recompute the canonical approved payload digest and require it to equal both the artifact's recorded digest and `--approve`. Require profile, template id/revision/source digest, resolved parameter digest, database, collection, filter, update, cap, and audit schema version to match the current invocation.
4. Create the execution audit artifact in `prepared` state; inability to create or atomically write it aborts before any Mongo command.
5. Run the established TTY/`--yes` confirmation.
6. Invoke the transaction program, which revalidates the approved candidates, applies the bounded update, verifies affected fields, and commits only on a complete match. Atomically update the execution artifact to `applied`, `failed`, or `failed_or_unknown` before returning.

### Undo

Add a sibling command rather than changing the currently flat `mongo mutate` syntax:

```sh
ayx mongo undo --mutation-audit-artifact <applied-mutation-artifact>
```

`mongo undo` follows the same preview/approval/apply contract and accepts the same `--approval-artifact`, `--approve`, `--accept-mutation-risk`, `--apply`, `--audit-dir`, and global `--yes` flags. Its default operation is a read-only undo diff. Applied undo uses a transaction and only restores fields that still equal the mutation artifact's recorded post-value; it refuses stale or partially changed documents rather than overwriting later operator changes. It writes `mongo-undo-preview-*` and `mongo-undo-*` artifacts linked to the original mutation.

Undo is deliberately not an automatic rollback. If the mutation committed but the process result is ambiguous, or if an undo preview is stale, the operator must investigate using the audit and use the required prior backup/restore process if needed. A failed/unknown audit may never be replayed blindly.

## Mutation Audit Artifact Contract

Use a versioned JSON schema, `schema_version: 1`, and stable `operation_id` embedded in both the filename and payload. Extend `ayx_core::audit` with a small create/update API that generates the path once and atomically rewrites that same sensitive file as operation state changes. Existing `write_audit_artifact` remains the compatibility helper used by backup/restore.

Every mutation preview artifact must contain:

```json
{
  "schema_version": 1,
  "operation_id": "mongo-mutate-...",
  "kind": "mongo_mutation_preview",
  "status": "previewed",
  "command": "mongo mutate",
  "created_at_utc": "...",
  "profile": "prod",
  "connection": { "mode": "managed", "url": "mongodb://***:***@..." },
  "dry_run": true,
  "applied": false,
  "template": {
    "id": "user_email_domain_migration",
    "revision": 1,
    "source_sha256": "sha256:...",
    "purpose": "...",
    "kba_refs": ["..."]
  },
  "target": { "database": "AlteryxGallery", "collection": "users" },
  "resolved_mutation": {
    "filter": { "...": "..." },
    "update": { "$set": { "...": "..." } },
    "parameter_digest": "sha256:..."
  },
  "preflight": {
    "max_affected": 25,
    "matched_count": 3,
    "candidate_digest": "sha256:...",
    "field_diffs": ["..."]
  },
  "approval": {
    "approval_digest": "sha256:...",
    "expires_at_utc": "..."
  },
  "rollback": { "supported": true, "strategy": "guarded_set_inverse" }
}
```

The execution artifact starts with the same immutable plan/preview/approval data plus `backup` provenance (`artifact_path`, backup operation id/timestamp/output directory), `safety_gate` (`apply`, `accept_mutation_risk`, `confirmation`), and `status: "prepared"`. Its final state adds `started_at_utc`, `finished_at_utc`, sanitized command result, transaction outcome, matched/modified counts, post-update verification, any safe error text, and `undo` information. In particular:

- Preserve the backup/restore common fields (`command`, timestamp, profile, `dry_run`, `applied`, connection where available, execution, and `safety_gate`) so `ayx audit` users see a consistent baseline.
- Store a per-document Extended JSON `_id` and, for every `$set` path, whether the prior field was present plus its prior and post values. This is the minimum data needed for a guarded inverse; do not store a whole document when only selected fields are changed.
- Store an immutable canonical payload hash over schema version, template identity, target, parameter digest, resolved filter/update, cap, candidate snapshot, and expiry. The caller's `--approve` must match it exactly.
- Never store raw URI credentials, raw `mongosh` argv, the password config file, secret parameter literals, or an unredacted connection string. Render only `sanitize_args`/`resolve_connection_detail`-style redacted data. Reject templates that declare a parameter as secret for v1; a secret-bearing remediation requires a later design.
- On a subprocess error after `prepared` is written, update `status` to `failed_or_unknown`, include the sanitized error and artifact path in the returned error, and do not claim that no database write happened. On a known transaction abort, use `failed`/`aborted` and record `applied: false`.

## Task 1: Separate and validate the Mongo remediation registry

**Files:**

- Modify: `ayx-server/src/mongo.rs:14-66, 234-307, 670-753` (current mixed template types and query/mutation resolution)
- Modify: `ayx-server/knowledge/mongo/queries.yaml` (retain only support diagnostics)
- Add: `ayx-server/knowledge/mongo/mutations.yaml` (typed remediation definitions)
- Modify: `ayx-server/tests/mongo_smoke.rs` or add focused unit tests under `ayx-server/src/mongo.rs`

**Interfaces:**

- Produces `MongoSupportQueryTemplate`, `MongoMutationTemplate`, `MutationParameter`, `ResolvedMutation`, and `MutationTemplateMode` (`Executable` or `PreviewOnly`).
- Produces `resolve_mutation_template(name, params) -> Result<ResolvedMutation>` and a deterministic `canonical_mutation_digest(&ResolvedMutation, &CandidateSnapshot) -> String`.
- Removes `update` and `read_only` from the support-query model; no code path can accidentally treat a support template as a remediation.

- [ ] **Step 1: Define the two file formats and migrate the existing entries**

  Keep the currently read-only entries in `queries.yaml`, represented by a type that has filter/projection/sort/limit/purpose/KBA metadata but no update field. Create `mutations.yaml` with a top-level `mutations:` list. Each mutation must declare:

  ```yaml
  mutations:
    - id: user_email_domain_migration
      revision: 1
      mode: preview_only
      database: AlteryxGallery
      collection: users
      filter: { ... }
      update:
        $set: { email: "${new_email}" }
      parameters:
        new_email:
          type: string
          required: true
      max_affected: 25
      max_backup_age_minutes: 60
      purpose: ...
      kba_refs: [...]
      rollback: guarded_set_inverse
  ```

  Move the current `user_email_domain_migration` entry out of `queries.yaml`, but retain it as `preview_only` initially. Its present filter matches a domain while its update assigns one literal email to every matching document; it must not be promoted to `executable` until the remediation owner supplies an operation-specific, reviewed migration definition and a meaningful cap. The code path is real and tests use a safe fixture template marked `executable`; no shipped template is silently made live merely because it already has `read_only: false`.

- [ ] **Step 2: Add typed parameter binding and template validation before any command rendering**

  Parse repeated `--param key=value` values into a `BTreeMap`, reject `key` duplication, and validate them against the selected template's declaration. Implement a recursive JSON replacement that accepts placeholders only as an entire string value and produces a JSON string for `type: string`. Leave a clear extension point for `integer`, `boolean`, and `json` parameter types, but do not expose them until their validation tests exist.

  Validate at load/resolve time: non-empty id/database/collection/purpose; unique id; positive `revision`, `max_affected`, and backup-age; a non-empty object filter; only a non-empty `$set` object; no `_id` target or unsupported path/operator; no raw JavaScript-shaped template values; and a supported rollback strategy. Apply the global 1,000-document maximum independently of YAML.

- [ ] **Step 3: Replace the present free-form live resolver**

  Split the current `resolve_mutation_spec` behavior into:

  - a compatibility static-plan resolver for `--print`, which may describe raw arguments but marks `executable: false`; and
  - a live-preview/apply resolver that requires `--template`, binds only the typed parameters, and returns `ResolvedMutation`.

  Require an `Executable` template for preview/apply. Make every path that reaches database I/O use this resolver; do not accept a raw `--update` fallback or default `json!({})` update.

- [ ] **Step 4: Add registry and binding tests before moving on**

  Add tests for successful resolution of a test-only executable `$set` template; unknown/missing/duplicate parameter failure; inline-placeholder and unknown-placeholder rejection; empty filter/$set/_id/unsupported operator rejection; cap above 1,000 rejection; query registry containing no mutation; and current `user_email_domain_migration` resolving only as `preview_only`.

  Run: `cargo nextest run -p ayx-server mongo --no-fail-fast`

  Expected: all existing Mongo smoke tests still pass and each unsafe registry shape is rejected before a subprocess could be created.

---

## Task 2: Build a structured `mongosh` preview and transaction executor

**Files:**

- Modify: `ayx-server/src/mongo.rs:184-232, 490-642, 678-731, 857-975` (mutation path and existing display-string helpers)
- Modify: `ayx-server/tests/mongo_smoke.rs`
- Add: a narrow test helper in `ayx-server/src/mongo.rs` or `ayx-server/tests/` for scripted `mongosh` output

**Interfaces:**

- Produces an owned `MongoshInvocation`/`PreparedMongoshInvocation` that carries process args and keeps `MongoPasswordFile` alive until after execution.
- Produces `MutationPreview`, `CandidateSnapshot`, `FieldDiff`, and `MutationExecutionResult` parsed from a single, versioned `mongosh` JSON result.
- Replaces mutation use of full command strings with `render_redacted_mongosh(&invocation)` for human/audit display and `run_mongosh(&invocation)` for actual execution.

- [ ] **Step 1: Separate display rendering from executable process construction**

  Refactor the connection-argument code so it builds an argument vector plus an owned optional password-file guard. The executor receives those exact args; the display renderer receives a sanitized clone and never exposes an unredacted URI or temporary config path. Keep existing query behavior stable in this task, but do not call `build_mongosh_mutation_eval` or `execute_query_plan` from any mutation code.

  Preserve `ensure_tool_available` and `run_command_capture` error conventions, but give the mutation runner a testable boundary (for example a small `MongoshRunner` trait with a production `Command` implementation). Tests must be able to provide a JSON result without relying on a host `mongosh` binary or modifying `PATH` globally.

- [ ] **Step 2: Implement the read-only preflight/diff program**

  Generate a `mongosh --quiet --eval` program from serde-serialized values only. It must:

  1. Query the resolved non-empty filter with a deterministic `_id` sort and `limit(max_affected + 1)`.
  2. Project `_id` plus exactly the `$set` fields needed for the diff.
  3. Fail with a structured `cap_exceeded` result if it sees the extra document.
  4. Emit canonical Extended JSON for each candidate's `_id`, old field-presence/value, new `$set` value, and field-level diff.
  5. Emit one versioned sentinel object, not free-form text, so Rust can reject malformed or additional output.

  In Rust, calculate the candidate and approval digest from canonical serialized data, set an expiry (the template's backup-age window, capped at a documented maximum), and return a `MutationPreview`. Zero matches is a valid preview artifact but is not approvable for `--apply`.

- [ ] **Step 3: Implement one no-retry transactional mutation program**

  The apply program receives the expected preview snapshot as serialized Extended JSON. Inside `session.withTransaction`, it must re-query the target with the same deterministic ordering and projection, compare count, ids, field presence, and prior values with the approved snapshot, then call the bounded `$set` update. Re-query the changed fields and verify every post-value before committing. Return structured `applied`, `aborted`, or `failed_or_unknown` data, including matched/modified counts and sanitized shell diagnostics.

  Treat an unsupported transaction topology, preflight mismatch, zero-match apply, count mismatch, and post-verification mismatch as no-commit errors. Do not add a non-transactional fallback. Never retry this program once it has been spawned.

- [ ] **Step 4: Add pure and fake-runner tests**

  Cover candidate diff creation (missing field, changed field, no-op value, cap+1), stable digest output regardless of parameter order, redaction of URI/password-file data, malformed sentinel rejection, preflight count mismatch, transaction-unsupported failure, transaction abort, successful applied result, and `failed_or_unknown` classification. Assert the apply program contains `withTransaction` and never contains raw parameter interpolation or an unredacted URI.

  Run: `cargo nextest run -p ayx-server mongo --no-fail-fast`

  Expected: no live Mongo process is required; the fake runner verifies the exact safe command boundary and structured result handling.

---

## Task 3: Wire the preview/approval gates into the CLI and confirmation flow

**Files:**

- Modify: `ayx-rs/src/main.rs:223-267, 694-785, 2919-2963, 5045-5063, 5604-5610` (global state handoff, Mongo Clap args, catalog)
- Modify: `ayx-rs/src/cmd/mongo.rs:17-103`
- Modify: `ayx-server/src/mongo.rs:184-232` (replace disabled `mutate_envelope` branch)
- Add or modify: `ayx-rs/tests/cli_smoke.rs` and focused command tests

**Interfaces:**

- `cmd::mongo::execute(environment, yes, command)` receives the existing global confirmation consent.
- `MongoCommand::Mutate` gains `--param`, `--audit-dir`, `--backup-audit-artifact`, `--approval-artifact`, and `--approve`; it retains `--template`, `--print`, `--apply`, and `--accept-mutation-risk`.
- Adds `MongoCommand::Undo` with `--mutation-audit-artifact` plus the shared preview/apply approval flags.
- `mutate_envelope` accepts a request struct rather than another long positional argument list, and returns the normal `Envelope` with a stable mutation payload.

- [ ] **Step 1: Extend Clap without relying on global `--apply` precedence**

  Keep Mongo's existing per-command `--apply` field so its behavior stays explicit and independent of the global One API `--apply`. Add the new paths/options above with `PathBuf` types for artifacts and a repeatable `Vec<String>` for `--param`. Use Clap conflicts for obvious invalid combinations, then perform the complete apply-tuple validation in Rust so the error can list all missing safety gates.

  Pass `cli.yes` into the Mongo module at the top-level dispatch. On the apply path, call `cmd::confirm::require_tty_confirmation(yes, ...)` after artifact validation and before spawning `mongosh`; use `destructive_action_message` or a new narrowly named mutation warning builder so the prompt includes target/template/count/digest rather than a generic sentence.

- [ ] **Step 2: Implement preview output and apply validation order**

  Replace the current `if print_query || !apply` static return with three intentional modes:

  - `--print`: static non-audited rendering only.
  - no `--apply`: create the live read-only diff preview, persist its audit artifact, and return `dry_run: true`, `applied: false`, `preview`, `approval_artifact`, `approval_digest`, and redacted `mongosh` display data.
  - `--apply`: first reject missing complete gates, then validate backup/approval artifacts and current template binding, create the prepared execution artifact, confirm, and call the transactional executor.

  Require a non-expired preview with a positive candidate count. The apply invocation must reproduce the same template and parameter digest; it cannot use a preview of a similar query, a new profile, a different audit directory, or manual JSON editing as authorization.

- [ ] **Step 3: Add command catalog entries and registry-validation coverage**

  Add `mongo mutate` with `safety: "destructive"` (or the catalog's most conservative supported mutating classification), explicit prerequisites for `mongosh`, a current successful backup artifact, and an approved preview artifact. Add `mongo undo` as destructive with the source mutation artifact prerequisite. Update `LiveCatalog` tests so both command paths validate.

  This closes the current catalog omission; a future remediation action can reference a valid capability rather than being falsely reported as unknown by `ayx actions validate`.

- [ ] **Step 4: Add CLI-surface tests**

  Exercise Clap parsing and dispatcher behavior for: default preview; `--print` conflict combinations; raw mutation arguments rejected for preview/apply; all missing apply gates reported; `--apply --accept-mutation-risk` without approval rejected; non-TTY apply without `--yes` rejected by the established helper; and a TTY/`--yes` success route reaching the server request boundary. Assert no test reaches a real `mongosh` binary.

  Run: `cargo nextest run -p ayx-rs cli mongo --no-fail-fast`

  Expected: the old terminal `execution is not yet enabled` error is gone, and every actual write route is demonstrably behind all five safety controls.

---

## Task 4: Persist lifecycle-safe mutation audits and implement guarded undo

**Files:**

- Modify: `ayx-core/src/audit.rs:1-67` (create/update lifecycle artifact helper)
- Modify: `ayx-server/src/mongo.rs` (preview/execution artifact construction, backup-artifact validation, undo planner/executor)
- Modify: `ayx-core/tests/` or `ayx-server/tests/mongo_smoke.rs`

**Interfaces:**

- Produces an `AuditArtifactHandle { operation_id, path }` or equivalent with `create_sensitive_audit_artifact` and atomic `update_sensitive_audit_artifact` operations.
- Produces `load_and_validate_backup_audit`, `load_and_validate_preview_approval`, and `load_applied_mutation_audit` with versioned schema validation.
- Adds `undo_envelope` and an inverse plan that restores only the recorded `$set` fields with optimistic post-value checks.

- [ ] **Step 1: Add an atomic lifecycle helper without regressing backup/restore**

  Factor the sensitive directory creation and atomic file write already used by `write_audit_artifact` into a reusable helper that can create one uniquely named path, then overwrite that same path atomically. Generate an operation id with sufficient uniqueness for concurrent invocations (timestamp plus a random/UUID suffix; add a small dependency only if the workspace has no appropriate existing primitive). Keep `write_audit_artifact`'s public behavior and filename prefixes unchanged for backup/restore callers.

  An execution artifact must be created and contain `status: "prepared"` before the transaction process starts. If that first write fails, return before mutation. Always attempt a final atomic status update; if the write itself fails after a known commit, return a high-severity error that names the operation id and says the mutation may have succeeded, rather than reporting a clean failure.

- [ ] **Step 2: Implement strict backup and approval-artifact validation**

  Parse JSON rather than trusting filenames. Backup validation requires `command == "mongo backup"`, `applied == true`, `dry_run == false`, matching profile, an existing output directory, a parseable timestamp within the template limit, and a schema/payload shape produced by the current backup writer. Preview validation requires the mutation preview kind/version, `status == "previewed"`, unexpired approval, matching template source digest/revision, matching resolved mutation and parameter digest, and the caller-provided `--approve` equal to the recomputed canonical digest.

  Return field-specific validation errors without echoing sensitive artifact contents. Do not regard a backup artifact as evidence of an executed backup merely because its file exists.

- [ ] **Step 3: Persist all mutation outcomes with the full schema**

  Implement typed serializable audit structs rather than assembling an unvalidated large `json!` value in multiple branches. Use the contract above for preview, prepared execution, applied execution, aborted execution, failed execution, and failed-or-unknown execution. The returned envelope contains only a compact result plus `audit_artifact`; the artifact is the durable detailed record.

  Test that the existing backup/restore common fields remain present and verify the mutation-only fields: schema/operation id, redacted connection, template revision/source hash, approval digest, capped diff, backup linkage, safety gates, timestamps, terminal status, and undo snapshot. Add negative assertions that a managed URI password, `--config` temporary-file path, and raw `mongosh` argv cannot appear anywhere in the serialized artifact.

- [ ] **Step 4: Implement preview-first guarded undo**

  `mongo undo` loads only an `applied` schema-v1 mutation artifact with supported `guarded_set_inverse` rollback data. Its read-only preview resolves each stored `_id`, verifies every affected field still equals the recorded post-value, and shows the field changes that would restore the recorded prior presence/value. Refuse an artifact marked failed/unknown, an unknown schema/version, zero successful changes, a target/profile mismatch, or any stale candidate.

  Applied undo follows the same preview-artifact digest, `--apply`, `--accept-mutation-risk`, and confirmation gates. In one transaction, use ordered `bulkWrite` operations whose filters include `_id` and recorded post-values; restore old values with `$set` or remove previously absent paths with `$unset`; verify the prior values after writes; and commit only if every document matches. The undo artifact contains `undo_of`, the source artifact hash, its own approval/diff/execution data, and a backlink to the undo artifact in the original mutation artifact when the update succeeds.

- [ ] **Step 5: Add artifact lifecycle and undo tests**

  Add tests for owner-only lifecycle writes, terminal state replacement, failed/unknown recording, valid and invalid backup artifacts, expired/tampered/mismatched preview artifacts, and no-secret serialization. Use the fake Mongo runner to verify successful inverse, stale-field refusal, transaction abort, and unknown-result behavior. Confirm an undo never automatically runs after a failed-or-unknown mutation.

  Run: `cargo nextest run -p ayx-core -p ayx-server mongo audit --no-fail-fast`

  Expected: each preview and all terminal execution outcomes have a readable, secure, linked artifact; no database state is touched when audit preparation or any approval check fails.

---

## Task 5: Update actions, documentation, and user-facing safety guidance

**Files:**

- Modify: `README.md:255-260` (Mongo capability status)
- Modify: `site/src/content/docs/server/mongo/index.md:14-19, 99-111, 130-138`
- Modify: `ayx-registry/actions/mongo-queue-stuck.action.yaml`
- Modify: `ayx-registry/actions/mongo-backup-restore.action.yaml`
- Modify: `docs/roadmap/mongo-registry-and-remediation.md`
- Modify: `CHANGELOG.md` under `## Unreleased`

**Interfaces:**

- Documents the exact two-command preview/approve/apply workflow, redacted audit behavior, backup prerequisite, and guarded undo limitations.
- Ensures registry recipes do not claim unsupported `--dry-run` syntax or suggest that `mongo mutate` is still preview-only after this feature ships.

- [ ] **Step 1: Correct the command examples and safety model**

  Replace the current docs' direct raw mutation example with the template/parameter preview command followed by the full approved apply command. Explain that omitting `--apply` is the Mongo dry-run; no `--dry-run` flag exists on the current Mongo command tree. Show where the preview and execution artifacts are written, what `--approve` binds, why `--yes` is mandatory in automation, and why a successful backup artifact is required.

- [ ] **Step 2: Document recovery without overpromising undo**

  State that V1 undo is available only for successfully audited `$set` mutations while all affected fields remain unchanged since the mutation. It is not a substitute for backup/restore, does not repair unknown transaction outcomes, and deliberately refuses stale data. Direct operators to the backup artifact and tested restore procedure for broader recovery.

- [ ] **Step 3: Repair the bundled registry recipes**

  Update the stuck-queue action note to point to the reviewed template workflow and require a backup/preview approval before remediation. Correct `mongo-backup-restore.action.yaml`, whose current command uses unsupported `--dry-run`: use an omitted `--apply` invocation for its planned backup step. Do not add a bulk mutation action until a production template is explicitly promoted from `preview_only` with owner-reviewed semantics and a capped blast radius.

- [ ] **Step 4: Update status and release notes only after tests pass**

  Mark the roadmap's execution/audit gap complete while leaving its orphan-detection/results-correlation work active. Add an Unreleased safety-focused changelog entry: named, bounded template mutations now require a read-only diff artifact, explicit digest approval, backup evidence, risk acceptance, and confirmation; all outcomes are audited; and undo is guarded rather than automatic.

---

## Task 6: End-to-end verification and controlled live rehearsal

**Files:**

- Modify only if needed from prior tasks: tests named above; no production behavior changes in this task.

**Interfaces:**

- Verifies the final public CLI contract, audit schema, transaction behavior, and documentation agree.

- [ ] **Step 1: Run formatting, build, lint, and the full test suite**

  Run:

  ```sh
  cargo fmt --all --check
  cargo build --workspace
  cargo clippy --workspace --all-targets -- -D warnings
  cargo nextest run --workspace --locked
  ```

  Expected: clean output. In particular, the new Mongo tests must not require Mongo, `mongosh`, a TTY, or a production profile in CI.

- [ ] **Step 2: Verify representative JSON envelopes locally with a fake runner/test seam**

  Assert snapshots for these cases:

  1. Default preview: `ok`, `dry_run: true`, `applied: false`, bounded field diff, artifact path, digest, no mutation execution.
  2. Static `--print`: no live preview or audit artifact and clear `preview_available: false`.
  3. Missing/tampered/expired approval or backup artifact: non-zero result before the write boundary, no prepared execution artifact (except an explicitly documented rejected-attempt artifact if implemented).
  4. Approved success: `ok`, `applied: true`, `matched_count == modified_count == preview count`, final execution artifact with a transaction result and undo metadata.
  5. Transaction abort/unsupported topology/ambiguous process result: non-success with a final artifact whose status distinguishes known no-write from `failed_or_unknown`.
  6. Undo preview and successful guarded undo; stale post-value must refuse rather than overwrite.

- [ ] **Step 3: Run an isolated live rehearsal before production use**

  This step cannot run in CI. Use a disposable or staging Mongo replica-set profile, a purpose-built executable test template that matches one disposable document, and an isolated `AYX_CONFIG_HOME`/audit directory. Do not use the bundled `user_email_domain_migration` while it is `preview_only`.

  1. Take a real backup with `ayx mongo backup --apply` and retain its artifact.
  2. Run default `mongo mutate` preview. Inspect the candidate diff, redaction, cap, template revision, and approval digest in both envelope and artifact.
  3. Try `--apply` without one safety input at a time; confirm every case refuses before a write.
  4. Run the complete apply command interactively, answer `yes`, and independently query the disposable document to confirm the expected `$set` value.
  5. Run the default `mongo undo` preview, inspect it, then use its own approval/apply flow. Independently verify that the prior field value/presence returns.
  6. Record artifact paths, command versions, profile name, result status, and any unexpected wording in the change record. A production rollout is blocked until this rehearsal and a restore drill from the required backup both succeed.

  Expected: exactly one bounded mutation and one guarded inverse commit; four linked artifacts (mutation preview/execution and undo preview/execution) contain no credentials; the backup artifact remains the recovery source of record.
