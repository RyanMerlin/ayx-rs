# Wave 0: TUI Removal And Agent Hygiene — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `0.20.0` with no bundled TUI and agent-native defaults: output auto-detection, remediation-bearing envelopes, `--jq`, `one open`, and TTY-gated pickers.

**Architecture:** Every change hangs off existing seams. The envelope grows three optional fields in `ayx-core`; `main()` resolves the output mode and enriches error envelopes once, at the single point where every command already passes; pickers are one helper (`cmd/select.rs`) called from the affected dispatch arms; `one open` is a new leaf under `one`. The TUI is deleted after its only exclusive endpoint gets a real command.

**Tech Stack:** Rust 2024 edition (`rust-version 1.97.1`), clap 4 derive, serde/serde_json, `jaq-core 3.1` + `jaq-std 3.0` + `jaq-json 2.0` (jq), `inquire 0.9` (picker), `open 5.4` (browser), `jsonschema 0.53` (dev only).

**Spec:** `docs/superpowers/specs/2026-09-04-wave0-tui-removal-and-agent-hygiene-design.md` (with ADR 0004 `docs/adr/0004-no-bundled-tui.md`).

## Global Constraints

- Baseline is `origin/main` at `ddc625c` (`v0.19.1`). Line numbers below are from that commit; re-resolve them before editing.
- Work in a dedicated worktree under `~/code/.worktrees/` (never a sibling directory) on branch `feat/wave0-tui-removal`. Always run cargo with an explicit per-worktree `CARGO_TARGET_DIR` (the global config redirects to a shared `/workspace/cargo-target`, which lets sibling worktrees' test binaries run instead of yours).
- Gates before every commit: `cargo fmt --all`, then `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run --workspace --locked`. After any change to a clap `about`/command: `cargo run -q -p xtask -- refresh-command-surface` and commit `docs/command-surface.md`.
- Alteryx One and Alteryx Server are separate products. Nothing in this plan touches `ayx server`.
- `docs/one-endpoint-matrix.md` rows for unprobed endpoints say `unverified`; never write `live 200` without a real probe.
- Do not edit historical release notes under `docs/releases/` or `site/src/content/docs/releases/`.
- Commit messages: `type(scope): summary`, and end with
  `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- The three owner decisions in the spec were approved 2026-09-04: non-TTY stdout defaults to `json`; a hidden `tui` stub ships for one cycle; `inquire` is the picker crate.
- `--watch` is out of scope (moved to Wave 1).

---

## File map

| File | Responsibility after Wave 0 |
| --- | --- |
| `ayx-core/src/envelope.rs` | `Envelope` gains `remediation`, `retryable`, `next`; `Remediation` struct; `ErrorCode::retryable()` |
| `docs/cli-schema.json` | Full-envelope schema, now including `error_code` and the three new fields |
| `ayx-rs/src/output.rs` | `OutputMode` resolution (`resolve_output_mode`, agent markers), `CompactEnvelope` mirror of new fields, `pagination_next_command` |
| `ayx-rs/src/jq.rs` (new) | `--jq` filter execution over the rendered JSON |
| `ayx-rs/src/cmd/select.rs` (new) | TTY-gated selector resolution, `MissingSelector`, `SelectionCancelled`, `items_from_envelope` |
| `ayx-rs/src/cmd/one_open.rs` (new) | `one open <kind> [id] [--print]` |
| `ayx-rs/src/main.rs` | Global flags (`--output` optional, `--jq`, `--raw-output`), `main()` resolution + enrichment, hidden `tui` stub, `OneWorkspaceCommand::Detail`, optional ids on curated leaves |
| `ayx-rs/src/cmd/one_platform/workspace.rs` | `detail` arm |
| `ayx-rs/src/cmd/one.rs` | descriptor rows for `workspace detail` and `open` |
| `ayx-rs/src/cmd/one_workflows.rs` and sibling `one_*.rs` | picker integration at the dispatch arms |
| `ayx-rs/src/cmd/catalog.rs` | `CATALOG_METADATA` rows for the new leaves |
| `ayx-one-api/src/inventory.rs` | `/v4/workspaces/{workspaceId}` row |
| `ayx-rs/tests/one_inventory_drift.rs` | drop the TUI allowlists |
| `ayx-rs/tests/cli_smoke.rs` | stub test, auto-detect tests, picker non-interactive test |
| `ayx-rs/src/tui/` | deleted |

---

### Task 1: `ayx one workspace detail <id>`

The only endpoint reachable solely from the TUI. Lands first so the removal loses nothing.

**Files:**
- Modify: `ayx-rs/src/main.rs:2114-2141` (`OneWorkspaceCommand`, add after `Current`)
- Modify: `ayx-rs/src/cmd/one_platform/workspace.rs:16-22` (const), `:246-263` (add arm after `Current`)
- Modify: `ayx-rs/src/cmd/one.rs:124-130` (descriptor)
- Modify: `ayx-one-api/src/inventory.rs:22-27` (`IAM_ENDPOINTS`)
- Modify: `ayx-rs/tests/one_inventory_drift.rs:114-125` (`NON_ONE_SURFACE_ENDPOINTS`)
- Modify: `ayx-rs/src/cmd/catalog.rs` (`CATALOG_METADATA`, near the `profile/list` row at `:80`)
- Modify: `docs/one-endpoint-matrix.md:107-109` (`platform.iam` table)
- Test: `ayx-rs/tests/cli_smoke.rs`

**Interfaces:**
- Produces: `OneWorkspaceCommand::Detail { id: String }`; endpoint const `WORKSPACE_DETAIL_ENDPOINT = "/v4/workspaces/{workspaceId}"`.

- [ ] **Step 1: Write the failing smoke test**

Append to `ayx-rs/tests/cli_smoke.rs`:

```rust
#[test]
fn one_workspace_detail_help_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "workspace", "detail", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Inspect a One workspace by numeric id"));
    assert!(stdout.contains("<ID>"));
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo nextest run -p ayx-rs --test cli_smoke one_workspace_detail_help_renders`
Expected: FAIL (clap reports `unrecognized subcommand 'detail'`, exit non-zero).

- [ ] **Step 3: Add the clap variant**

In `ayx-rs/src/main.rs`, inside `pub(crate) enum OneWorkspaceCommand`, directly after the `Current,` variant (line 2141):

```rust
    /// Inspect a One workspace by numeric id (`GET /v4/workspaces/{workspaceId}`).
    Detail {
        #[arg(value_name = "ID")]
        id: String,
    },
```

- [ ] **Step 4: Add the endpoint const and dispatch arm**

In `ayx-rs/src/cmd/one_platform/workspace.rs`, after `WORKSPACE_ADMINS_ENDPOINT` (line 22):

```rust
/// `GET /v4/workspaces/{workspaceId}` — same numeric-id contract as `admins`.
/// Previously dispatched only by the removed `ayx tui` One browser.
const WORKSPACE_DETAIL_ENDPOINT: &str = "/v4/workspaces/{workspaceId}";
```

Inside `execute`, after the `OneWorkspaceCommand::Current => { ... }` arm (ends line 263):

```rust
        OneWorkspaceCommand::Detail { id } => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "workspace",
                "workspace-detail",
                "GET",
                WORKSPACE_DETAIL_ENDPOINT,
                false,
                &[("workspaceId", &id)],
            )?
        }
```

Do **not** route the id through `resolve_workspace_path_id` — that helper rejects any id that is not the current workspace (`workspace.rs:61-74`), and `detail` exists precisely to inspect another one.

- [ ] **Step 5: Descriptor, inventory, drift carve-out, catalog**

`ayx-rs/src/cmd/one.rs`, after `OneWorkspaceCommand::Current => detail("one.workspace.current"),` (line 130):

```rust
        OneWorkspaceCommand::Detail { .. } => detail("one.workspace.detail"),
```

`ayx-one-api/src/inventory.rs`, in `IAM_ENDPOINTS` directly after the `/v4/workspaces/current` row (line 27):

```rust
    EndpointSpec {
        method: "GET",
        path: "/v4/workspaces/{workspaceId}",
        commands: &["one workspace detail"],
    },
```

`ayx-rs/tests/one_inventory_drift.rs:114-125`: replace the whole `NON_ONE_SURFACE_ENDPOINTS` block with

```rust
/// `(method, endpoint)` pairs dispatched only by a non-`one`-namespace
/// surface, so they cannot be added to `ayx-one-api/src/inventory.rs` (its
/// `commands` field is contractually `ayx one ...`-only). Empty since 0.20.0:
/// `ayx one workspace detail` made `GET /v4/workspaces/{workspaceId}` a normal
/// inventory row and the TUI that used to carve it out is gone.
const NON_ONE_SURFACE_ENDPOINTS: &[(&str, &str)] = &[];
```

`ayx-rs/src/cmd/catalog.rs`, add to `CATALOG_METADATA` (alphabetical placement is not enforced; put it beside the other `one/workspace/*` rows if any exist, else after `profile/use` at line 103):

```rust
    CatalogMetadata {
        path: "one/workspace/detail",
        output: "workspace resource envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["One PAT for the target workspace"],
        notes: &["Numeric workspace id, not the GID; mirrors `one workspace admins`."],
    },
```

`docs/one-endpoint-matrix.md`, add a row after the `/v4/workspaces/current` row (line 109):

```markdown
| GET | `/v4/workspaces/{workspaceId}` | unverified | not probed | `one workspace detail` | object: raw API resource body, JSON-passthrough | json:ApiValidationFailed / json:RouteNotFoundException / json:AccessControlException (Alteryx One `/v4` gateway shape) | Added in 0.20.0; previously reachable only from the removed `ayx tui` One browser. `workspaceId` is the numeric id, as for `admins`. |
```

- [ ] **Step 6: Run the tests and the surface gate**

Run: `cargo nextest run -p ayx-rs --test cli_smoke --test one_inventory_drift` then `cargo run -q -p xtask -- refresh-command-surface`
Expected: both test files PASS; `docs/command-surface.md` gains a `one workspace detail` row.

- [ ] **Step 7: Commit**

```bash
git add ayx-rs/src/main.rs ayx-rs/src/cmd/one_platform/workspace.rs ayx-rs/src/cmd/one.rs \
  ayx-one-api/src/inventory.rs ayx-rs/tests/one_inventory_drift.rs ayx-rs/src/cmd/catalog.rs \
  docs/one-endpoint-matrix.md docs/command-surface.md ayx-rs/tests/cli_smoke.rs
git commit -m "feat(one): add workspace detail <id>, retiring the TUI-only endpoint carve-out"
```

---

### Task 2: Envelope `remediation`, `retryable`, `next` + schema fix

**Files:**
- Modify: `ayx-core/src/envelope.rs:10-186`
- Modify: `docs/cli-schema.json`
- Modify: `ayx-core/Cargo.toml` (`[dev-dependencies]` add `jsonschema = "0.53"`; add `jsonschema = "0.53"` to the root `[workspace.dependencies]` if you prefer `workspace = true`)
- Modify: `ayx-rs/src/output.rs:85-94` (`CompactEnvelope`), `:109-121` and `:431` (`Envelope` struct literals), `:139-165` (`compact_envelope`)
- Modify: every other `Envelope {` struct literal in the workspace (find with `rg -n "Envelope \{" -g '*.rs' --glob '!target'`)

**Interfaces:**
- Produces: `pub struct Remediation { pub summary: String, pub commands: Vec<String> }`; `ErrorCode::retryable(self) -> bool`; `Envelope::{with_remediation(summary, commands), with_retryable(bool), with_next(Vec<String>), finalize_retryable()}`; fields `remediation: Option<Remediation>`, `retryable: Option<bool>`, `next: Option<Vec<String>>`.

- [ ] **Step 1: Write the failing unit tests**

Append to the `mod tests` in `ayx-core/src/envelope.rs`:

```rust
    #[test]
    fn retryable_is_derived_from_error_code() {
        for code in [
            ErrorCode::RateLimited,
            ErrorCode::Network,
            ErrorCode::Upstream,
            ErrorCode::Incomplete,
        ] {
            assert!(code.retryable(), "{code:?} must be retryable");
        }
        for code in [
            ErrorCode::ConfigMissing,
            ErrorCode::AuthFailed,
            ErrorCode::PermissionDenied,
            ErrorCode::NotFound,
            ErrorCode::Gone,
            ErrorCode::Validation,
            ErrorCode::Conflict,
            ErrorCode::WorkspaceMismatch,
            ErrorCode::OutputClassification,
            ErrorCode::Internal,
        ] {
            assert!(!code.retryable(), "{code:?} must not be retryable");
        }
    }

    #[test]
    fn optional_fields_are_omitted_when_unset_and_present_when_set() {
        let ok = serde_json::to_value(Envelope::ok("fine")).unwrap();
        assert!(ok.get("remediation").is_none());
        assert!(ok.get("retryable").is_none());
        assert!(ok.get("next").is_none());

        let err = Envelope::err_coded(ErrorCode::AuthFailed, "expired", Value::Null)
            .with_remediation("Log in again", vec!["ayx one login".to_string()])
            .finalize_retryable();
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["retryable"], Value::Bool(false));
        assert_eq!(v["remediation"]["summary"], "Log in again");
        assert_eq!(v["remediation"]["commands"][0], "ayx one login");

        let next = serde_json::to_value(
            Envelope::ok("page").with_next(vec!["ayx one flows list --page-token abc".to_string()]),
        )
        .unwrap();
        assert_eq!(next["next"][0], "ayx one flows list --page-token abc");
    }

    #[test]
    fn finalize_retryable_does_not_override_an_explicit_value() {
        let e = Envelope::err_coded(ErrorCode::Network, "flaky", Value::Null)
            .with_retryable(false)
            .finalize_retryable();
        assert_eq!(e.retryable, Some(false));
        let ok = Envelope::ok("fine").finalize_retryable();
        assert_eq!(ok.retryable, None, "success envelopes never carry retryable");
    }

    #[test]
    fn envelopes_validate_against_the_published_schema() {
        let schema_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/cli-schema.json");
        let schema: Value =
            serde_json::from_str(&std::fs::read_to_string(schema_path).unwrap()).unwrap();
        let validator = jsonschema::validator_for(&schema).expect("schema compiles");

        let ok = serde_json::to_value(Envelope::ok_with_data(
            "fine",
            serde_json::json!({ "n": 1 }),
        ))
        .unwrap();
        assert!(validator.is_valid(&ok), "success envelope must validate");

        let err = serde_json::to_value(
            Envelope::err_coded(ErrorCode::NotFound, "missing", Value::Null)
                .with_remediation(
                    "List first",
                    vec!["ayx one workflows list --output json".to_string()],
                )
                .finalize_retryable(),
        )
        .unwrap();
        let problems: Vec<String> = validator.iter_errors(&err).map(|e| e.to_string()).collect();
        assert!(problems.is_empty(), "error envelope must validate: {problems:?}");
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo nextest run -p ayx-core envelope`
Expected: compile errors (`retryable`, `with_remediation`, `finalize_retryable`, `with_next` not found; `jsonschema` unresolved).

- [ ] **Step 3: Add the dev-dependency**

`ayx-core/Cargo.toml`, under `[dev-dependencies]` (create the section if absent):

```toml
jsonschema = "0.53"
```

- [ ] **Step 4: Implement in `ayx-core/src/envelope.rs`**

After the `impl ErrorCode { ... }` block's `from_http_status` (ends line 122), add inside the same `impl`:

```rust
    /// Whether re-running the identical command can reasonably succeed without
    /// the caller changing anything. Transport and upstream classes qualify;
    /// everything the caller controls (input, auth, config, permissions) does
    /// not. `Incomplete` qualifies because the pagination that stalled may
    /// finish on a retry.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorCode::RateLimited
                | ErrorCode::Network
                | ErrorCode::Upstream
                | ErrorCode::Incomplete
        )
    }
```

Before `pub struct Envelope` (line 125):

```rust
/// Machine-readable next step attached to an error envelope so an agent can
/// branch on structure instead of parsing prose.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Remediation {
    /// One human-readable sentence.
    pub summary: String,
    /// Zero or more exact commands to run next, in order.
    pub commands: Vec<String>,
}
```

Extend the struct:

```rust
#[derive(Debug, Serialize)]
pub struct Envelope {
    pub ok: bool,
    pub message: String,
    pub timestamp_utc: DateTime<Utc>,
    pub data: Value,
    /// Machine-readable error classification. Absent on success envelopes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ErrorCode>,
    /// Suggested next step. Errors only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<Remediation>,
    /// Whether an identical retry may succeed. Errors only; filled from the
    /// error code by `finalize_retryable` when a command did not set it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    /// Suggested follow-up commands. Successes only; keep to three or fewer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<Vec<String>>,
}
```

In each of the four constructors (`ok`, `ok_with_data`, `err_with_data`, `err_coded`) add the three fields set to `None`. Then add builders to `impl Envelope`:

```rust
    pub fn with_remediation(mut self, summary: impl Into<String>, commands: Vec<String>) -> Self {
        self.remediation = Some(Remediation {
            summary: summary.into(),
            commands,
        });
        self
    }

    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = Some(retryable);
        self
    }

    pub fn with_next(mut self, next: Vec<String>) -> Self {
        self.next = Some(next);
        self
    }

    /// Fill `retryable` from the error code on failure envelopes that did not
    /// set it explicitly. No-op on success envelopes.
    pub fn finalize_retryable(mut self) -> Self {
        if !self.ok && self.retryable.is_none() {
            self.retryable = Some(self.error_code.unwrap_or(ErrorCode::Internal).retryable());
        }
        self
    }
```

- [ ] **Step 5: Replace `docs/cli-schema.json`**

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "AYX-RS Envelope",
  "description": "The full (`--output json-full`) envelope. The compact `ayx.output.v1` view adds `schema_version` and `command` on top of these fields.",
  "type": "object",
  "required": ["ok", "message", "timestamp_utc", "data"],
  "properties": {
    "ok": { "type": "boolean" },
    "message": { "type": "string" },
    "timestamp_utc": { "type": "string", "format": "date-time" },
    "data": {},
    "error_code": {
      "type": "string",
      "enum": [
        "config_missing", "auth_failed", "permission_denied", "not_found", "gone",
        "validation", "conflict", "rate_limited", "network", "upstream",
        "workspace_mismatch", "incomplete", "output_classification", "internal"
      ]
    },
    "remediation": {
      "type": "object",
      "required": ["summary", "commands"],
      "properties": {
        "summary": { "type": "string" },
        "commands": { "type": "array", "items": { "type": "string" } }
      },
      "additionalProperties": false
    },
    "retryable": { "type": "boolean" },
    "next": { "type": "array", "items": { "type": "string" } }
  },
  "additionalProperties": false
}
```

- [ ] **Step 6: Fix every `Envelope { ... }` struct literal**

Run: `rg -n "Envelope \{" -g '*.rs' --glob '!target'`

For each literal that lists the five old fields, add the three new ones. In `ayx-rs/src/output.rs:109-121` (the projected copy inside `render_envelope`) that is:

```rust
                let projected = Envelope {
                    ok: clean.ok,
                    message: clean.message.clone(),
                    timestamp_utc: clean.timestamp_utc,
                    data: compact_data(
                        &clean.data,
                        descriptor.kind,
                        descriptor.fields,
                        descriptor.collection_keys,
                        output_limit,
                        false,
                    ),
                    error_code: clean.error_code,
                    remediation: clean.remediation.clone(),
                    retryable: clean.retryable,
                    next: clean.next.clone(),
                };
```

Apply the same three lines to the literal in `redacted_envelope` (`output.rs:431` onward) and any other hit (copy the source envelope's fields through unchanged).

- [ ] **Step 7: Mirror the fields in `CompactEnvelope`**

`ayx-rs/src/output.rs:85-94`:

```rust
#[derive(Serialize)]
struct CompactEnvelope {
    schema_version: &'static str,
    command: String,
    ok: bool,
    message: String,
    timestamp_utc: chrono::DateTime<chrono::Utc>,
    error_code: Option<ayx_core::envelope::ErrorCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remediation: Option<ayx_core::envelope::Remediation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<Vec<String>>,
    data: Value,
}
```

and in `compact_envelope` (`:139-165`) populate them:

```rust
        error_code: envelope.error_code,
        remediation: envelope.remediation.clone(),
        retryable: envelope.retryable,
        next: envelope.next.clone(),
```

- [ ] **Step 8: Run the tests**

Run: `cargo nextest run -p ayx-core envelope` then `cargo nextest run --workspace --locked`
Expected: PASS (the workspace run catches any struct literal you missed as a compile error).

- [ ] **Step 9: Commit**

```bash
git add ayx-core/src/envelope.rs ayx-core/Cargo.toml Cargo.lock docs/cli-schema.json ayx-rs/src/output.rs
git commit -m "feat(envelope): add remediation, retryable, and next; fix cli-schema.json to admit error_code"
```

---

### Task 3: Enrich error envelopes in `main()`

**Files:**
- Modify: `ayx-rs/src/main.rs:6132-6211` (`main`), and the existing `hint_for_error_code` (find with `rg -n "fn hint_for_error_code" ayx-rs/src/main.rs`)

**Interfaces:**
- Consumes: Task 2's `Envelope::{finalize_retryable, with_remediation}`.
- Produces: `fn remediation_for_error_code(code: ErrorCode, command: &str) -> Option<(String, Vec<String>)>`.

- [ ] **Step 1: Write the failing unit test**

In `ayx-rs/src/main.rs`'s existing `#[cfg(test)] mod tests` (find with `rg -n "mod tests" ayx-rs/src/main.rs`; if the file has none, add one at the end):

```rust
    #[test]
    fn remediation_for_error_code_names_the_next_command() {
        use ayx_core::envelope::ErrorCode;
        let (summary, commands) =
            remediation_for_error_code(ErrorCode::AuthFailed, "one.flows.list").unwrap();
        assert!(summary.contains("log in"));
        assert_eq!(commands[0], "ayx one login");

        let (_, commands) =
            remediation_for_error_code(ErrorCode::ConfigMissing, "profile.list").unwrap();
        assert_eq!(commands, vec!["ayx onboard", "ayx profile list --output json"]);

        // Server commands must not be told to run a One login.
        let (_, commands) =
            remediation_for_error_code(ErrorCode::AuthFailed, "server").unwrap();
        assert!(commands.is_empty());

        assert!(remediation_for_error_code(ErrorCode::Internal, "one.flows.list").is_none());
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo nextest run -p ayx-rs remediation_for_error_code_names_the_next_command`
Expected: FAIL to compile (`remediation_for_error_code` not found).

- [ ] **Step 3: Implement the mapping**

Next to `hint_for_error_code` in `ayx-rs/src/main.rs`:

```rust
/// Structured remediation for dispatcher-classified failures. `command` is the
/// descriptor's dotted command id (e.g. `one.flows.list`) so product-specific
/// advice is only given to the product it applies to.
fn remediation_for_error_code(
    code: ayx_core::envelope::ErrorCode,
    command: &str,
) -> Option<(String, Vec<String>)> {
    use ayx_core::envelope::ErrorCode::*;
    let is_one = command == "one" || command.starts_with("one.");
    let cmds = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Some(match code {
        ConfigMissing => (
            "No usable profile was found; onboard or select an existing one.".to_string(),
            cmds(&["ayx onboard", "ayx profile list --output json"]),
        ),
        AuthFailed if is_one => (
            "The stored One credential was rejected; log in again.".to_string(),
            cmds(&["ayx one login", "ayx one auth status --output json"]),
        ),
        AuthFailed => (
            "The stored credential was rejected; refresh it for this product.".to_string(),
            Vec::new(),
        ),
        WorkspaceMismatch => (
            "The token belongs to a different workspace than the profile expects.".to_string(),
            cmds(&["ayx one workspace current --output json", "ayx one workspace switch"]),
        ),
        _ => return None,
    })
}
```

- [ ] **Step 4: Wire it into `main()`**

In the `Ok(envelope)` branch, before rendering:

```rust
        Ok(envelope) => {
            let mut envelope = envelope.finalize_retryable();
            if !envelope.ok && envelope.remediation.is_none() {
                if let Some(code) = envelope.error_code
                    && let Some((summary, commands)) =
                        remediation_for_error_code(code, descriptor.command)
                {
                    envelope = envelope.with_remediation(summary, commands);
                }
            }
            let rendered = format_envelope(&envelope, output, descriptor, output_limit)?;
```

In the `Err(err)` branch, replace `let err_env = Envelope::err_coded(code, "command failed", data);` with:

```rust
            let mut err_env = Envelope::err_coded(code, "command failed", data).finalize_retryable();
            if let Some((summary, commands)) = remediation_for_error_code(code, descriptor.command) {
                err_env = err_env.with_remediation(summary, commands);
            }
```

Keep the existing `data.hint` population; `remediation` is additive.

- [ ] **Step 5: Run tests and gates**

Run: `cargo nextest run -p ayx-rs` and `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add ayx-rs/src/main.rs
git commit -m "feat(cli): attach retryable and remediation to dispatcher-classified errors"
```

---

### Task 4: Remove the TUI, ship the hidden stub

**Files:**
- Delete: `ayx-rs/src/tui/` (30 files)
- Modify: `ayx-rs/src/main.rs:44` (`mod tui;`), `:373` (descriptor), `:473-476` (variant), `:4382` (dispatch)
- Modify: `Cargo.toml:39-40,60`; `ayx-rs/Cargo.toml:25-27`; `Cargo.lock`
- Modify: `ayx-rs/tests/cli_smoke.rs:186-198`; `ayx-rs/tests/one_inventory_drift.rs:87-100`
- Modify: `README.md:115,310`; `docs/output-format.md:36,41`; `docs/runtime-config-contract.md:13,15`; `docs/cli-spec.md:138`; `docs/command-surface.md` (regenerated)

**Interfaces:**
- Consumes: Task 2's `with_remediation`.

- [ ] **Step 1: Tag the last TUI commit and delete the stale branch**

From the main checkout (`~/code/ayx-rs`, not the worktree):

```bash
git fetch origin
git tag -a tui-final origin/main -m "Last commit containing ayx-rs/src/tui; removed in 0.20.0 per docs/adr/0004-no-bundled-tui.md"
git push origin tui-final
git branch -D feat/tui-v2-phase2-cross-asset-drill
```

(`tui-final` points at the base of this branch; the removal commit below is its child.)

- [ ] **Step 2: Replace the `tui_help_renders` test**

`ayx-rs/tests/cli_smoke.rs:186-198` becomes:

```rust
#[test]
fn tui_stub_returns_remediation_and_is_hidden() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["tui", "--output", "json-full"])
        .output()
        .expect("ayx binary should run");

    assert_eq!(output.status.code(), Some(2), "removed command is a validation error");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let envelope: serde_json::Value =
        serde_json::from_str(stderr.trim()).expect("stderr is one JSON envelope");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error_code"], "validation");
    assert_eq!(envelope["retryable"], false);
    assert_eq!(envelope["remediation"]["commands"][0], "ayx onboard");

    let help = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["--help"])
        .output()
        .expect("ayx binary should run");
    assert!(
        !String::from_utf8_lossy(&help.stdout).contains("tui"),
        "hidden stub must not appear in --help"
    );
}
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo nextest run -p ayx-rs --test cli_smoke tui_stub_returns_remediation_and_is_hidden`
Expected: FAIL (`tui` currently launches the TUI / exits 0 or hangs waiting for a terminal — if it hangs, kill it; the failure mode is fine).

- [ ] **Step 4: Delete the TUI and rewire the four `main.rs` sites**

```bash
git rm -r -q ayx-rs/src/tui
```

`ayx-rs/src/main.rs:44` — delete `mod tui;`.

`:473-476` — replace with:

```rust
    /// Removed in 0.20.0 (ADR 0004). Hidden stub for one release cycle so
    /// muscle-memory invocations get a remediation envelope instead of a clap
    /// "unrecognized subcommand". Delete in 0.21.0.
    #[command(hide = true)]
    Tui,
```

`:373` — `Command::Tui => OutputDescriptor::new("tui", ViewKind::Result),`

`:4382` — replace `Command::Tui => return tui::run(),` with an arm that yields an envelope in the same shape as its neighbours:

```rust
        Command::Tui => Envelope::err_coded(
            ayx_core::envelope::ErrorCode::Validation,
            "ayx tui was removed in 0.20.0",
            json!({
                "removed_in": "0.20.0",
                "adr": "docs/adr/0004-no-bundled-tui.md",
            }),
        )
        .with_remediation(
            "Use the targeted commands that replaced the TUI",
            vec![
                "ayx onboard".to_string(),
                "ayx one login".to_string(),
                "ayx profile list".to_string(),
                "ayx doctor".to_string(),
            ],
        ),
```

If the surrounding match arms end in `?` / are wrapped by `Ok(match ...)`, match that form; the point is an `Envelope` value, not an early `return`.

- [ ] **Step 5: Dependencies**

`Cargo.toml`: delete the `nucleo-matcher = "0.3"`, `ratatui = "0.30"`, and `tui-input = { ... }` lines. Keep `crossterm = "0.29"` (used by `ayx-rs/src/cmd/one_platform/auth.rs:1058-1066` on Windows).

`ayx-rs/Cargo.toml:25-27`: delete `ratatui.workspace = true`, `nucleo-matcher.workspace = true`, `tui-input.workspace = true`. Keep `crossterm.workspace = true`.

Run: `cargo check --workspace` (updates `Cargo.lock`), then record the delta:

```bash
cargo tree -p ayx-rs -e normal --prefix none | sort -u | wc -l
```

Baseline at `ddc625c` is 326 crates; expect roughly 290. Put both numbers in the commit body.

- [ ] **Step 6: Drift test**

`ayx-rs/tests/one_inventory_drift.rs:87-100`: delete the comment block and the nine `("tui/v2/worker.rs", ...)` rows.

- [ ] **Step 7: Docs**

`README.md:115`: replace the sentence "…the TUI and onboarding flows are the only places that intentionally operate on explicit file paths." with "…onboarding and migration flows are the only places that intentionally operate on explicit file paths."

`README.md:310`: delete the `- \`tui\` — …` bullet.

`docs/output-format.md:36`: replace with `- \`completions\` and onboarding-style flows still perform direct terminal I/O in places, so they are not pure envelope commands.`

`docs/output-format.md:41`: replace with `- Interactive onboarding/authentication and shell completion scripts are direct-terminal workflows; structured modes return an envelope summary.`

`docs/runtime-config-contract.md:13`: replace with `- Onboarding and migration flows may open or edit explicit files and workspaces.`

`docs/runtime-config-contract.md:15`: replace with `- Any path-based helper used by onboarding or migration must stay visibly separate from the central runtime loader.`

`docs/cli-spec.md:138`: delete the `- \`tui\`` line.

Then: `cargo run -q -p xtask -- refresh-command-surface`. If a site sync script exists (`rg -l "command-surface" scripts site/package.json`), run it; otherwise copy `docs/command-surface.md` over `site/src/content/docs/reference/command-surface.md` preserving that file's frontmatter block.

- [ ] **Step 8: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace --locked && cargo run -q -p xtask -- refresh-command-surface --check`
Expected: all PASS; `rg -n "tui" ayx-rs/src docs/output-format.md docs/runtime-config-contract.md docs/cli-spec.md README.md` returns only the hidden stub and ADR/changelog references.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat(cli)!: remove the bundled TUI; hidden tui stub returns remediation (ADR 0004)

Deletes ayx-rs/src/tui (legacy + v2, 9,868 lines) and the ratatui, tui-input,
and nucleo-matcher dependencies. crossterm stays for the Windows console reset
in one_platform/auth.rs. Dependency graph: <before> -> <after> normal crates."
```

---

### Task 5: Output-mode auto-detection

**Files:**
- Modify: `ayx-rs/src/output.rs` (add resolution API after `OutputMode`'s `Display` impl, line 41)
- Modify: `ayx-rs/src/main.rs:270-278` (`Cli.output`), `:6132-6144` (`main` head)
- Modify: `docs/output-format.md` (resolution order)
- Test: `ayx-rs/src/output.rs` unit tests; `ayx-rs/tests/cli_smoke.rs`

**Interfaces:**
- Produces: `pub enum OutputModeSource { Explicit, Env, AutoAgentMarker, AutoNonTty, Default }`; `pub const AGENT_MARKER_VARS: &[&str]`; `pub fn agent_marker_present(get: impl Fn(&str) -> Option<String>) -> bool`; `pub fn resolve_output_mode(explicit: Option<OutputMode>, env_value: Option<&str>, stdout_is_terminal: bool, agent_marker: bool) -> Result<(OutputMode, OutputModeSource), String>`.

- [ ] **Step 1: Write the failing unit tests**

Append to `ayx-rs/src/output.rs`'s `mod tests`:

```rust
    #[test]
    fn explicit_output_always_wins() {
        let (mode, src) =
            resolve_output_mode(Some(OutputMode::Yaml), Some("json"), false, true).unwrap();
        assert_eq!((mode, src), (OutputMode::Yaml, OutputModeSource::Explicit));
    }

    #[test]
    fn env_beats_auto_detection_and_rejects_garbage() {
        let (mode, src) = resolve_output_mode(None, Some("text"), false, true).unwrap();
        assert_eq!((mode, src), (OutputMode::Text, OutputModeSource::Env));
        let (mode, _) = resolve_output_mode(None, Some("JSON-FULL"), true, false).unwrap();
        assert_eq!(mode, OutputMode::JsonFull, "env value is case-insensitive");
        let err = resolve_output_mode(None, Some("xml"), true, false).unwrap_err();
        assert!(err.contains("AYX_OUTPUT"));
    }

    #[test]
    fn agent_marker_then_non_tty_then_text() {
        assert_eq!(
            resolve_output_mode(None, None, true, true).unwrap(),
            (OutputMode::Json, OutputModeSource::AutoAgentMarker)
        );
        assert_eq!(
            resolve_output_mode(None, None, false, false).unwrap(),
            (OutputMode::Json, OutputModeSource::AutoNonTty)
        );
        assert_eq!(
            resolve_output_mode(None, None, true, false).unwrap(),
            (OutputMode::Text, OutputModeSource::Default)
        );
    }

    #[test]
    fn agent_marker_ignores_empty_and_zero_values() {
        let env = |k: &str| match k {
            "CLAUDECODE" => Some("0".to_string()),
            "AI_AGENT" => Some(String::new()),
            _ => None,
        };
        assert!(!agent_marker_present(env));
        let env = |k: &str| (k == "AYX_AGENT").then(|| "1".to_string());
        assert!(agent_marker_present(env));
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo nextest run -p ayx-rs output::tests`
Expected: compile failure (`resolve_output_mode`, `OutputModeSource`, `agent_marker_present` undefined).

- [ ] **Step 3: Implement in `ayx-rs/src/output.rs`**

After the `impl std::fmt::Display for OutputMode` block (line 41):

```rust
/// Where the effective output mode came from. Logged under `--debug`; never
/// part of the envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputModeSource {
    Explicit,
    Env,
    AutoAgentMarker,
    AutoNonTty,
    Default,
}

/// Environment variables that identify an agent host. Every entry was observed
/// on a live host before being added (`CLAUDECODE` and `AI_AGENT` inside Claude
/// Code, 2026-09-04). Extend only after observing a variable on the host
/// itself; do not guess.
pub const AGENT_MARKER_VARS: &[&str] = &["AYX_AGENT", "CLAUDECODE", "AI_AGENT"];

/// True when any agent marker is set to a non-empty value other than `0`.
pub fn agent_marker_present(get: impl Fn(&str) -> Option<String>) -> bool {
    AGENT_MARKER_VARS
        .iter()
        .any(|key| get(key).is_some_and(|v| !v.is_empty() && v != "0"))
}

/// Resolve the effective output mode. Order: explicit `--output`, then
/// `AYX_OUTPUT`, then `json` for agent hosts or a non-terminal stdout, else
/// `text`. The error carries a human sentence for a bad `AYX_OUTPUT` value.
pub fn resolve_output_mode(
    explicit: Option<OutputMode>,
    env_value: Option<&str>,
    stdout_is_terminal: bool,
    agent_marker: bool,
) -> Result<(OutputMode, OutputModeSource), String> {
    if let Some(mode) = explicit {
        return Ok((mode, OutputModeSource::Explicit));
    }
    if let Some(raw) = env_value {
        let mode = <OutputMode as clap::ValueEnum>::from_str(raw, true).map_err(|_| {
            format!("AYX_OUTPUT={raw:?} is not one of: text, json, json-full, yaml, table")
        })?;
        return Ok((mode, OutputModeSource::Env));
    }
    if agent_marker {
        return Ok((OutputMode::Json, OutputModeSource::AutoAgentMarker));
    }
    if !stdout_is_terminal {
        return Ok((OutputMode::Json, OutputModeSource::AutoNonTty));
    }
    Ok((OutputMode::Text, OutputModeSource::Default))
}
```

- [ ] **Step 4: Make `--output` optional and resolve it in `main()`**

`ayx-rs/src/main.rs:270-278`:

```rust
struct Cli {
    /// Output format. Defaults to `text` on a terminal and `json` when stdout
    /// is not a terminal or an agent host is detected (`AYX_AGENT`,
    /// `CLAUDECODE`, `AI_AGENT`). `AYX_OUTPUT=<mode>` overrides the automatic
    /// choice; this flag overrides everything. Put it after the complete
    /// command path, for example: `ayx one flows list --output json`.
    #[arg(long, global = true)]
    output: Option<output::OutputMode>,
```

`main()` head (`:6135-6138`) — replace `let output = cli.output;` with:

```rust
    let (output, output_source) = match output::resolve_output_mode(
        cli.output,
        std::env::var("AYX_OUTPUT").ok().as_deref(),
        io::stdout().is_terminal(),
        output::agent_marker_present(|key| std::env::var(key).ok()),
    ) {
        Ok(resolved) => resolved,
        Err(message) => {
            let err_env = Envelope::err_coded(
                ayx_core::envelope::ErrorCode::Validation,
                message,
                json!({ "env": "AYX_OUTPUT" }),
            )
            .finalize_retryable();
            eprintln!("{}", serde_json::to_string_pretty(&err_env).unwrap_or_default());
            std::process::exit(exit_code_for_envelope(&err_env));
        }
    };
    if cli.debug {
        eprintln!("[ayx-debug] output mode {output} ({output_source:?})");
    }
```

Add `use std::io::IsTerminal;` to `main.rs` imports if absent. Error rendering already follows `output` when `--error-format` is `text` (`main.rs:6188-6196`), so no change is needed there.

- [ ] **Step 5: Smoke tests**

Append to `ayx-rs/tests/cli_smoke.rs`:

```rust
#[test]
fn piped_stdout_defaults_to_compact_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["catalog", "list"])
        .env_remove("AYX_OUTPUT")
        .env_remove("AYX_AGENT")
        .env_remove("CLAUDECODE")
        .env_remove("AI_AGENT")
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let v: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("piped stdout is compact JSON");
    assert_eq!(v["schema_version"], "ayx.output.v1");
}

#[test]
fn ayx_output_env_overrides_auto_detection() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["catalog", "list"])
        .env("AYX_OUTPUT", "text")
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    assert!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).is_err(),
        "AYX_OUTPUT=text must produce the text renderer, not JSON"
    );
}

#[test]
fn bad_ayx_output_is_a_validation_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["catalog", "list"])
        .env("AYX_OUTPUT", "xml")
        .output()
        .expect("ayx binary should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("AYX_OUTPUT"));
}
```

- [ ] **Step 6: Run the whole suite and fix text-asserting tests**

Run: `cargo nextest run --workspace --locked`
Expected: any pre-existing integration test that spawned the binary without `--output` and asserted on *text* output now receives JSON. For each such failure add `"--output", "text"` to that test's args (they were testing text rendering, not the default). Re-run until green.

- [ ] **Step 7: Document**

In `docs/output-format.md`, after the paragraph that ends "Leading placement remains accepted for backwards compatibility." (line 25) insert:

```markdown
Resolution order for the effective mode:

1. An explicit `--output <mode>` always wins.
2. `AYX_OUTPUT=<mode>` (case-insensitive; an unknown value is a `validation`
   error, exit 2).
3. `json` when an agent host is detected (`AYX_AGENT`, `CLAUDECODE`, or
   `AI_AGENT` set to a non-empty value other than `0`) or when stdout is not a
   terminal.
4. Otherwise `text`.

Piping `ayx … | less` therefore shows JSON since 0.20.0; set `AYX_OUTPUT=text`
in your shell profile if you prefer the text renderer in pipes.
```

- [ ] **Step 8: Commit**

```bash
git add ayx-rs/src/output.rs ayx-rs/src/main.rs ayx-rs/tests docs/output-format.md
git commit -m "feat(cli)!: default to json for agent hosts and non-terminal stdout; add AYX_OUTPUT"
```

---

### Task 6: `--jq <filter>` and `--raw-output`

**Files:**
- Create: `ayx-rs/src/jq.rs`
- Modify: `Cargo.toml` (`[workspace.dependencies]`), `ayx-rs/Cargo.toml` (`[dependencies]`)
- Modify: `ayx-rs/src/main.rs` (module decl beside `mod output;`, `Cli` flags, `main()` emission)
- Modify: `docs/output-format.md`, `docs/cli-spec.md`

**Interfaces:**
- Produces: `pub fn jq::apply(filter_src: &str, json_document: &str, raw_output: bool) -> anyhow::Result<Vec<String>>` — one output line per filter result. Parse/compile/runtime errors are `anyhow!("validation: …")`.

- [ ] **Step 1: Add dependencies**

`Cargo.toml` `[workspace.dependencies]`:

```toml
jaq-core = "3.1"
jaq-std = "3.0"
jaq-json = { version = "2.0", features = ["serde"] }
```

`ayx-rs/Cargo.toml` `[dependencies]`:

```toml
jaq-core.workspace = true
jaq-std.workspace = true
jaq-json.workspace = true
```

- [ ] **Step 2: Write the failing unit tests**

Create `ayx-rs/src/jq.rs` with only the tests first:

```rust
//! `--jq` post-processing: run a jq filter over the rendered JSON envelope.
//!
//! The filter sees exactly what the user would have seen (redaction and
//! `--output-limit` already applied), so it cannot widen the output.

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"{"ok":true,"message":"hello","data":{"items":[{"id":"a"},{"id":"b"}]}}"#;

    #[test]
    fn selects_a_field() {
        assert_eq!(apply(".ok", DOC, false).unwrap(), vec!["true"]);
    }

    #[test]
    fn raw_output_unquotes_strings_only() {
        assert_eq!(apply(".message", DOC, false).unwrap(), vec!["\"hello\""]);
        assert_eq!(apply(".message", DOC, true).unwrap(), vec!["hello"]);
        assert_eq!(apply(".ok", DOC, true).unwrap(), vec!["true"]);
    }

    #[test]
    fn iterates_one_line_per_result() {
        assert_eq!(
            apply(".data.items[].id", DOC, true).unwrap(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn std_functions_are_available() {
        assert_eq!(apply(".data.items | length", DOC, false).unwrap(), vec!["2"]);
    }

    #[test]
    fn bad_filter_is_a_validation_error() {
        let err = apply(".[", DOC, false).unwrap_err().to_string();
        assert!(err.starts_with("validation:"), "{err}");
    }
}
```

- [ ] **Step 3: Run them to verify they fail**

Add `mod jq;` to `ayx-rs/src/main.rs` beside `mod output;`. Run: `cargo nextest run -p ayx-rs jq::tests`
Expected: compile failure (`apply` undefined).

- [ ] **Step 4: Implement**

Above the tests in `ayx-rs/src/jq.rs`:

```rust
use anyhow::{Context, Result, anyhow};
use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, Vars, data, unwrap_valr};
use jaq_json::{Val, read};

/// Run `filter_src` over `json_document`; return one line per output value.
/// With `raw_output`, string results print without quotes (like `jq -r`).
pub fn apply(filter_src: &str, json_document: &str, raw_output: bool) -> Result<Vec<String>> {
    let input: Val = read::parse_single(json_document.as_bytes())
        .map_err(|e| anyhow!("internal: rendered envelope is not valid JSON: {e:?}"))?;

    let program = File { code: filter_src, path: () };
    let defs = jaq_core::defs().chain(jaq_std::defs()).chain(jaq_json::defs());
    let funs = jaq_core::funs().chain(jaq_std::funs()).chain(jaq_json::funs());

    let loader = Loader::new(defs);
    let arena = Arena::default();
    let modules = loader
        .load(&arena, program)
        .map_err(|errs| anyhow!("validation: --jq filter failed to parse: {errs:?}"))?;
    let filter = Compiler::default()
        .with_funs(funs)
        .compile(modules)
        .map_err(|errs| anyhow!("validation: --jq filter failed to compile: {errs:?}"))?;

    let ctx = Ctx::<data::JustLut<Val>>::new(&filter.lut, Vars::new([]));
    let mut lines = Vec::new();
    for out in filter.id.run((ctx, input)).map(unwrap_valr) {
        let val = out.map_err(|e| anyhow!("validation: --jq filter error: {e:?}"))?;
        let value: serde_json::Value =
            serde_json::to_value(&val).context("convert jq value to JSON")?;
        lines.push(match (&value, raw_output) {
            (serde_json::Value::String(s), true) => s.clone(),
            _ => serde_json::to_string(&value)?,
        });
    }
    Ok(lines)
}
```

This mirrors the crate-level example in `jaq-core`'s `lib.rs`. If the `3.1` release names `data::JustLut` or `Vars::new` differently, follow the example in `~/.cargo/registry/src/*/jaq-core-3.1.*/src/lib.rs` — the shape (load → compile → `Ctx` → `run`) is stable.

- [ ] **Step 5: Run the unit tests**

Run: `cargo nextest run -p ayx-rs jq::tests`
Expected: PASS.

- [ ] **Step 6: Global flags and emission in `main()`**

Check for a short-flag collision first: `rg -n "short = 'r'" ayx-rs/src/main.rs`. If there is one, omit `short = 'r'` below.

Add to `Cli` after `output_limit` (`main.rs:296`):

```rust
    /// Apply a jq filter to the JSON result and print one value per line.
    /// Forces `--output json` unless `--output json-full` is given.
    #[arg(long, global = true, value_name = "FILTER")]
    jq: Option<String>,
    /// With --jq, print string results without quotes (like `jq -r`).
    #[arg(long, short = 'r', global = true, requires = "jq")]
    raw_output: bool,
```

In `main()`, right after the output-mode resolution block:

```rust
    let jq_filter = cli.jq.clone();
    let raw_output = cli.raw_output;
    let output = match (&jq_filter, output) {
        (Some(_), output::OutputMode::JsonFull) => output::OutputMode::JsonFull,
        (Some(_), _) => output::OutputMode::Json,
        (None, mode) => mode,
    };
```

Add a helper next to `main()`:

```rust
/// Apply `--jq` to a rendered JSON document, or pass it through. A filter
/// failure becomes a validation envelope so the caller sees the standard shape.
fn apply_jq_or_passthrough(
    rendered: String,
    jq_filter: Option<&str>,
    raw_output: bool,
) -> Result<String, Envelope> {
    let Some(filter) = jq_filter else {
        return Ok(rendered);
    };
    match jq::apply(filter, &rendered, raw_output) {
        Ok(lines) => Ok(lines.join("\n")),
        Err(err) => Err(Envelope::err_coded(
            ayx_core::envelope::ErrorCode::Validation,
            err.to_string(),
            json!({ "jq": filter }),
        )
        .finalize_retryable()),
    }
}
```

Then in the `Ok(envelope)` branch, replace `let rendered = format_envelope(&envelope, output, descriptor, output_limit)?;` with:

```rust
            let rendered = format_envelope(&envelope, output, descriptor, output_limit)?;
            let rendered = match apply_jq_or_passthrough(rendered, jq_filter.as_deref(), raw_output) {
                Ok(text) => text,
                Err(err_env) => {
                    eprintln!("{}", serde_json::to_string_pretty(&err_env).unwrap_or_default());
                    std::process::exit(exit_code_for_envelope(&err_env));
                }
            };
```

Leave the `Err(err)` branch untouched: `--jq` applies to command results, and an error envelope on stderr keeps its full shape so the failure stays diagnosable.

- [ ] **Step 7: Smoke test**

Append to `ayx-rs/tests/cli_smoke.rs`:

```rust
#[test]
fn jq_filters_the_compact_envelope() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["catalog", "list", "--jq", ".schema_version", "--raw-output"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ayx.output.v1");

    let bad = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["catalog", "list", "--jq", ".["])
        .output()
        .expect("ayx binary should run");
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("validation"));
}
```

- [ ] **Step 8: Measure binary size, document, gate**

Run: `cargo build --release -p ayx-rs && ls -l "$CARGO_TARGET_DIR/release/ayx"` before and after this task (stash the number from a `git stash`-free approach: build once on the parent commit in a second worktree, or read the size from the last release asset). Record both in the commit body.

`docs/output-format.md`, append a section:

```markdown
## `--jq`

`--jq <FILTER>` runs a jq filter (pure-Rust `jaq`; jq 1.7 syntax and the
standard library) over the rendered JSON and prints one value per line.
`--raw-output` / `-r` prints string results without quotes. `--jq` forces
`--output json` unless `--output json-full` is given, and it runs after
redaction and `--output-limit`, so it cannot reveal anything the plain output
would not. A filter that fails to parse, compile, or run is a `validation`
error (exit 2).
```

`docs/cli-spec.md`: add `--jq` and `--raw-output` to the global-flag list and note exit code 2 for filter errors.

Run all gates. Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock ayx-rs/Cargo.toml ayx-rs/src/jq.rs ayx-rs/src/main.rs ayx-rs/tests/cli_smoke.rs docs/output-format.md docs/cli-spec.md
git commit -m "feat(cli): add --jq and --raw-output backed by jaq

Release binary size: <before> -> <after> bytes."
```

---

### Task 7: `next` hint for paginated lists

**Files:**
- Modify: `ayx-rs/src/output.rs` (add `pagination_next_command` + tests)
- Modify: `ayx-rs/src/main.rs` (`Ok(envelope)` branch)

**Interfaces:**
- Produces: `pub fn pagination_next_command(argv: &[String], token: &str) -> String`.

- [ ] **Step 1: Write the failing unit tests**

In `ayx-rs/src/output.rs` `mod tests`:

```rust
    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn next_command_appends_the_token() {
        let cmd = pagination_next_command(
            &argv(&["/usr/bin/ayx", "one", "flows", "list", "--output", "json"]),
            "tok123",
        );
        assert_eq!(cmd, "ayx one flows list --output json --page-token tok123");
    }

    #[test]
    fn next_command_replaces_an_existing_token_in_either_form() {
        let cmd = pagination_next_command(
            &argv(&["ayx", "one", "flows", "list", "--page-token", "old"]),
            "new",
        );
        assert_eq!(cmd, "ayx one flows list --page-token new");
        let cmd = pagination_next_command(
            &argv(&["ayx", "one", "flows", "list", "--page-token=old"]),
            "new",
        );
        assert_eq!(cmd, "ayx one flows list --page-token new");
    }

    #[test]
    fn next_command_quotes_arguments_with_spaces() {
        let cmd = pagination_next_command(
            &argv(&["ayx", "one", "flows", "list", "--profile", "my profile"]),
            "t",
        );
        assert_eq!(cmd, "ayx one flows list --profile 'my profile' --page-token t");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo nextest run -p ayx-rs output::tests::next_command`
Expected: compile failure.

- [ ] **Step 3: Implement**

In `ayx-rs/src/output.rs`:

```rust
/// Rebuild the invoking command line with `--page-token <token>` so an agent
/// can fetch the next page without reconstructing the arguments. `argv[0]` is
/// normalized to `ayx`; an existing `--page-token` (either form) is replaced.
pub fn pagination_next_command(argv: &[String], token: &str) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(argv.len() + 2);
    let mut skip_next = false;
    for (i, arg) in argv.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if i == 0 {
            parts.push("ayx".to_string());
            continue;
        }
        if arg == "--page-token" {
            skip_next = true;
            continue;
        }
        if arg.starts_with("--page-token=") {
            continue;
        }
        parts.push(shell_quote(arg));
    }
    parts.push("--page-token".to_string());
    parts.push(shell_quote(token));
    parts.join(" ")
}

fn shell_quote(arg: &str) -> String {
    if arg.is_empty()
        || arg
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '\'' | '"' | '$' | '`' | '\\' | '|' | '&' | ';'))
    {
        format!("'{}'", arg.replace('\'', "'\\''"))
    } else {
        arg.to_string()
    }
}
```

- [ ] **Step 4: Wire into `main()`**

In the `Ok(envelope)` branch, after `let mut envelope = envelope.finalize_retryable();` (Task 3):

```rust
            if envelope.ok
                && envelope.next.is_none()
                && envelope.data.get("pages_fetched").is_some()
                && let Some(token) = envelope
                    .data
                    .get("next_page_token")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
            {
                let argv: Vec<String> = std::env::args().collect();
                envelope = envelope.with_next(vec![output::pagination_next_command(&argv, token)]);
            }
```

`pages_fetched` is only emitted by `one_api_list_request` (`ayx-one-api/src/lib.rs:1058-1067`), which is exactly the set of commands that accept `--page-token`.

- [ ] **Step 5: Run tests and gates**

Run: `cargo nextest run --workspace --locked && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add ayx-rs/src/output.rs ayx-rs/src/main.rs
git commit -m "feat(cli): emit a next hint with the exact --page-token command on paginated lists"
```

---

### Task 8: Picker helper + `one workflows detail|delete`

**Files:**
- Create: `ayx-rs/src/cmd/select.rs`
- Modify: `ayx-rs/src/cmd/mod.rs` (add `pub mod select;` in the module list)
- Modify: `Cargo.toml` (`inquire = "0.9"` in `[workspace.dependencies]`), `ayx-rs/Cargo.toml` (`inquire.workspace = true`)
- Modify: `ayx-rs/src/main.rs:3188-3198` (`Detail`), `:3253-3259` (`Delete`), and `main()` `Err` branch
- Modify: `ayx-rs/src/cmd/one_workflows.rs:515-561` (`execute` arms for `Detail` and `Delete`)
- Test: `ayx-rs/tests/cli_smoke.rs`

**Interfaces:**
- Produces: `pub struct SelectItem { pub id: String, pub label: String }`; `pub struct SelectPolicy { pub no_input: bool, pub interactive_terminal: bool }` with `SelectPolicy::from_runtime(no_input: bool)` and `may_prompt(self) -> bool`; `pub struct MissingSelector { pub what: &'static str, pub list_command: &'static str }` (implements `std::error::Error`); `pub struct SelectionCancelled`; `pub fn resolve_selector(what: &'static str, list_command: &'static str, given: Option<String>, policy: SelectPolicy, fetch: impl FnOnce() -> anyhow::Result<Vec<SelectItem>>) -> anyhow::Result<String>`; `pub fn items_from_envelope(envelope: &Envelope, label_keys: &[&str]) -> anyhow::Result<Vec<SelectItem>>`.

- [ ] **Step 1: Dependencies**

`Cargo.toml` `[workspace.dependencies]`: `inquire = "0.9"`. `ayx-rs/Cargo.toml` `[dependencies]`: `inquire.workspace = true`. Run `cargo tree -i crossterm -p ayx-rs` afterwards; if `inquire` pulls a different `crossterm` major than the workspace's `0.29`, bump the workspace pin to the one `inquire` uses so a single copy compiles.

- [ ] **Step 2: Write the failing unit tests**

Create `ayx-rs/src/cmd/select.rs` starting with:

```rust
//! TTY-gated selector resolution (ADR 0004).
//!
//! When a command's required selector is omitted on an interactive terminal,
//! fetch the candidates and offer a picker. Off a terminal, or under
//! `--no-input`, fail closed with a `MissingSelector` that `main()` turns into
//! a structured remediation naming the list command.

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::envelope::Envelope;
    use serde_json::json;

    fn never_fetch() -> anyhow::Result<Vec<SelectItem>> {
        panic!("fetch must not run when a selector was given or prompting is not allowed");
    }

    #[test]
    fn a_given_selector_is_returned_untouched() {
        let policy = SelectPolicy { no_input: false, interactive_terminal: true };
        let id = resolve_selector("workflow id", "ayx one workflows list", Some("01X".into()), policy, never_fetch)
            .unwrap();
        assert_eq!(id, "01X");
    }

    #[test]
    fn missing_selector_off_tty_is_a_typed_error() {
        let policy = SelectPolicy { no_input: false, interactive_terminal: false };
        let err = resolve_selector("workflow id", "ayx one workflows list --output json", None, policy, never_fetch)
            .unwrap_err();
        let missing = err.downcast_ref::<MissingSelector>().expect("typed error");
        assert_eq!(missing.list_command, "ayx one workflows list --output json");
        assert!(err.to_string().starts_with("validation:"));
    }

    #[test]
    fn no_input_blocks_prompting_even_on_a_tty() {
        let policy = SelectPolicy { no_input: true, interactive_terminal: true };
        assert!(!policy.may_prompt());
        let err = resolve_selector("flow id", "ayx one flows list", None, policy, never_fetch).unwrap_err();
        assert!(err.downcast_ref::<MissingSelector>().is_some());
    }

    #[test]
    fn items_from_envelope_maps_ids_and_first_matching_label() {
        let env = Envelope::ok_with_data(
            "list",
            json!({ "items": [
                { "id": "a", "name": "Alpha" },
                { "id": 7, "title": "Seven" },
                { "id": "c" },
                { "name": "no id" }
            ]}),
        );
        let items = items_from_envelope(&env, &["name", "title"]).unwrap();
        assert_eq!(
            items,
            vec![
                SelectItem { id: "a".into(), label: "Alpha".into() },
                SelectItem { id: "7".into(), label: "Seven".into() },
                SelectItem { id: "c".into(), label: String::new() },
            ]
        );
        assert_eq!(items[0].to_string(), "Alpha  (a)");
        assert_eq!(items[2].to_string(), "c");
    }

    #[test]
    fn items_from_a_failed_envelope_is_an_error() {
        let env = Envelope::err_coded(ayx_core::envelope::ErrorCode::AuthFailed, "nope", json!(null));
        assert!(items_from_envelope(&env, &["name"]).is_err());
    }
}
```

- [ ] **Step 3: Run to verify they fail**

Add `pub mod select;` to `ayx-rs/src/cmd/mod.rs`. Run: `cargo nextest run -p ayx-rs select::tests`
Expected: compile failure.

- [ ] **Step 4: Implement**

Above the tests in `ayx-rs/src/cmd/select.rs`:

```rust
use std::fmt;
use std::io::IsTerminal;

use anyhow::{Result, bail};
use ayx_core::envelope::Envelope;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectItem {
    pub id: String,
    pub label: String,
}

impl fmt::Display for SelectItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.label.is_empty() || self.label == self.id {
            write!(f, "{}", self.id)
        } else {
            write!(f, "{}  ({})", self.label, self.id)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SelectPolicy {
    pub no_input: bool,
    pub interactive_terminal: bool,
}

impl SelectPolicy {
    /// Prompting needs both stdin (keys) and stdout (the list) to be terminals.
    pub fn from_runtime(no_input: bool) -> Self {
        Self {
            no_input,
            interactive_terminal: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        }
    }

    pub fn may_prompt(self) -> bool {
        !self.no_input && self.interactive_terminal
    }
}

/// A required selector was omitted and no picker may run. `main()` downcasts
/// this to attach `remediation.commands = [list_command]`.
#[derive(Debug)]
pub struct MissingSelector {
    pub what: &'static str,
    pub list_command: &'static str,
}

impl fmt::Display for MissingSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "validation: {} is required when not running interactively; run `{}` to find one",
            self.what, self.list_command
        )
    }
}

impl std::error::Error for MissingSelector {}

#[derive(Debug)]
pub struct SelectionCancelled;

impl fmt::Display for SelectionCancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("validation: selection cancelled")
    }
}

impl std::error::Error for SelectionCancelled {}

/// Return `given` when present; otherwise prompt (if allowed) or fail closed.
pub fn resolve_selector(
    what: &'static str,
    list_command: &'static str,
    given: Option<String>,
    policy: SelectPolicy,
    fetch: impl FnOnce() -> Result<Vec<SelectItem>>,
) -> Result<String> {
    if let Some(id) = given {
        return Ok(id);
    }
    if !policy.may_prompt() {
        return Err(MissingSelector { what, list_command }.into());
    }
    let items = fetch()?;
    if items.is_empty() {
        bail!("validation: no {what} candidates were returned; nothing to select");
    }
    match inquire::Select::new(&format!("Select a {what}:"), items)
        .with_page_size(12)
        .prompt()
    {
        Ok(item) => Ok(item.id),
        Err(inquire::InquireError::OperationCanceled)
        | Err(inquire::InquireError::OperationInterrupted) => Err(SelectionCancelled.into()),
        Err(other) => bail!("picker failed: {other}"),
    }
}

/// Build picker rows from a normalized list envelope (`data.items[]`, each
/// with an `id`). The label is the first present key from `label_keys`.
pub fn items_from_envelope(envelope: &Envelope, label_keys: &[&str]) -> Result<Vec<SelectItem>> {
    if !envelope.ok {
        bail!("{}", envelope.message);
    }
    let items = envelope
        .data
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(items
        .iter()
        .filter_map(|item| {
            let id = match item.get("id")? {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return None,
            };
            let label = label_keys
                .iter()
                .find_map(|key| item.get(*key).and_then(Value::as_str))
                .unwrap_or("")
                .to_string();
            Some(SelectItem { id, label })
        })
        .collect())
}
```

- [ ] **Step 5: Run the unit tests**

Run: `cargo nextest run -p ayx-rs select::tests`
Expected: PASS.

- [ ] **Step 6: Structured remediation in `main()`**

In the `Err(err)` branch of `main()`, after the Task 3 remediation block:

```rust
            if let Some(missing) = err.downcast_ref::<cmd::select::MissingSelector>() {
                err_env = err_env.with_remediation(
                    format!(
                        "Provide the {} explicitly, or run on a terminal to pick one",
                        missing.what
                    ),
                    vec![missing.list_command.to_string()],
                );
            }
```

Confirm the classifier maps the `validation:` prefix to `ErrorCode::Validation`: `rg -n "validation" ayx-rs/src/main.rs | rg -n "classify|starts_with"`. If it keys on a different marker, use that marker in `MissingSelector`'s and `SelectionCancelled`'s `Display` instead.

- [ ] **Step 7: Make the two workflow ids optional and resolve them**

`ayx-rs/src/main.rs:3188-3198` (`Detail`) and `:3253-3259` (`Delete`): change `id: String` to

```rust
        /// Workflow ULID. Omit on a terminal to pick from the list.
        #[arg(value_name = "ID")]
        id: Option<String>,
```

In `ayx-rs/src/cmd/one_workflows.rs::execute`, at the top of the `Detail { profile, id, include_dependencies }` arm and the `Delete { profile, id }` arm, before the existing body:

```rust
            let id = crate::cmd::select::resolve_selector(
                "workflow id",
                "ayx one workflows list --output json",
                id,
                crate::cmd::select::SelectPolicy::from_runtime(runtime.no_input),
                || {
                    let config = runtime.load_profile_lenient(profile.as_deref())?;
                    crate::cmd::select::items_from_envelope(
                        &fetch_all_assets(&config)?,
                        &["name", "title"],
                    )
                },
            )?;
```

The closure loads its own profile so the non-interactive failure happens *before* any profile load, which keeps the smoke test below config-free. The rest of each arm then uses `id: String` as before.

- [ ] **Step 8: Smoke test**

Append to `ayx-rs/tests/cli_smoke.rs`:

```rust
#[test]
fn omitted_workflow_id_off_tty_names_the_list_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "workflows", "detail", "--no-input", "--output", "json-full"])
        .output()
        .expect("ayx binary should run");

    assert_eq!(output.status.code(), Some(2), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let envelope: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stderr).trim()).unwrap();
    assert_eq!(envelope["error_code"], "validation");
    assert_eq!(
        envelope["remediation"]["commands"][0],
        "ayx one workflows list --output json"
    );
}
```

- [ ] **Step 9: Manual interactive smoke (record the result in the PR)**

On a real terminal with a logged-in profile: `ayx one workflows detail` → picker appears, typing filters, Enter opens the detail, Esc yields `validation: selection cancelled`. Do this once on Linux and once on Windows Terminal (colleagues are on Windows).

- [ ] **Step 10: Gates and commit**

Run all gates plus `cargo run -q -p xtask -- refresh-command-surface`. Expected: PASS.

```bash
git add Cargo.toml Cargo.lock ayx-rs/Cargo.toml ayx-rs/src/cmd/select.rs ayx-rs/src/cmd/mod.rs ayx-rs/src/main.rs ayx-rs/src/cmd/one_workflows.rs ayx-rs/tests/cli_smoke.rs docs/command-surface.md
git commit -m "feat(one): TTY-gated picker when a workflow id is omitted; typed remediation off-TTY"
```

---

### Task 9: Pickers for the rest of the curated set

**Files:**
- Modify: `ayx-rs/src/main.rs:2058-2064` (`one person detail`), `:2266-2272` (`one workspace switch`), `:2489-2495` (`one plans detail`), `:2624-2630` (`one flows detail`), `:2986-2992` (`one connections detail`), `:3438-3444` (`one job-groups detail`)
- Modify: the matching `execute` arms in `ayx-rs/src/cmd/one_platform/person.rs`, `one_platform/workspace.rs`, `one_plans.rs`, `one_flows.rs`, `one_connections.rs`, `one_job_groups.rs`

**Interfaces:**
- Consumes: Task 8's `resolve_selector`, `SelectPolicy::from_runtime`, `items_from_envelope`.

- [ ] **Step 1: Make each `id` optional**

For `person detail`, `plans detail`, `flows detail`, `connections detail`, `job-groups detail`: change `id: String` to

```rust
        /// Resource id. Omit on a terminal to pick from the list.
        #[arg(value_name = "ID")]
        id: Option<String>,
```

`workspace switch` (`:2266-2272`) already has `id: Option<String>`; read its arm in `workspace.rs` to see what `None` does today and keep any existing behavior that is not "error out".

- [ ] **Step 2: Resolve at each dispatch arm**

Each family's `List` arm already calls `one_api_list_request(&config, <surface>, "list", <ENDPOINT>, &[], &params)` with its own endpoint constant or literal. At the top of each `Detail { profile, id }` arm add the block below, substituting the row from the table (reuse the module's existing endpoint constant where one exists, and the same `surface` string its `List` arm passes):

| Command | `what` | `list_command` | list endpoint | `label_keys` |
| --- | --- | --- | --- | --- |
| `person detail` | `"person id"` | `"ayx one person list --output json"` | `/v4/people` | `&["email", "fullName", "full_name"]` |
| `plans detail` | `"plan id"` | `"ayx one plans list --output json"` | `/v4/plans` | `&["name"]` |
| `flows detail` | `"flow id"` | `"ayx one flows list --output json"` | `/v4/flows` | `&["name"]` |
| `connections detail` | `"connection id"` | `"ayx one connections list --output json"` | `/v4/connections` | `&["name"]` |
| `job-groups detail` | `"job group id"` | `"ayx one job-groups list --output json"` | `/v4/jobLibrary` | `&["name", "flowName", "flow_name"]` |
| `workspace switch` | `"workspace id"` | `"ayx one workspace list --output json"` | `/v4/workspaces` | `&["name"]` |

```rust
            let id = crate::cmd::select::resolve_selector(
                "flow id",
                "ayx one flows list --output json",
                id,
                crate::cmd::select::SelectPolicy::from_runtime(runtime.no_input),
                || {
                    let config = runtime.load_profile_lenient(profile.as_deref())?;
                    let params = OneListParams::new()
                        .with_limit(Some(200))
                        .with_all(true, Some(10));
                    let listed = one_api_list_request(
                        &config,
                        "flow",
                        "picker-list",
                        FLOWS_LIST_ENDPOINT,
                        &[],
                        &params,
                    )?;
                    crate::cmd::select::items_from_envelope(&listed, &["name"])
                },
            )?;
```

(The example is `flows detail`; repeat it per row with that row's values. `OneWorkspaceCommand` variants have no `profile` field — pass `None`.) The drift test compares call-site endpoint literals against the inventory, so passing the module's existing constant keeps it green.

- [ ] **Step 3: Smoke test one more family**

Append to `ayx-rs/tests/cli_smoke.rs`:

```rust
#[test]
fn omitted_flow_id_off_tty_names_the_list_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "flows", "detail", "--no-input", "--output", "json-full"])
        .output()
        .expect("ayx binary should run");

    assert_eq!(output.status.code(), Some(2));
    let envelope: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stderr).trim()).unwrap();
    assert_eq!(envelope["remediation"]["commands"][0], "ayx one flows list --output json");
}
```

- [ ] **Step 4: Gates and commit**

Run all gates plus `refresh-command-surface`. Expected: PASS; `docs/command-surface.md` shows `<ID>` as optional (`[ID]`) for the six leaves.

```bash
git add ayx-rs/src ayx-rs/tests/cli_smoke.rs docs/command-surface.md
git commit -m "feat(one): pickers for person, plan, flow, connection, job-group detail and workspace switch"
```

---

### Task 10: `ayx one open <kind> [id] [--print]`

**Files:**
- Create: `ayx-rs/src/cmd/one_open.rs`
- Modify: `ayx-rs/src/cmd/mod.rs` (`mod one_open;`)
- Modify: `ayx-rs/src/main.rs` (`enum OneCommand` — locate with `rg -n "enum OneCommand" ayx-rs/src/main.rs`)
- Modify: `ayx-rs/src/cmd/one.rs` (descriptor + dispatch — locate the `OneCommand::Workspace` arms and add `Open` beside them)
- Modify: `Cargo.toml` (`open = "5.4"`), `ayx-rs/Cargo.toml` (`open.workspace = true`)
- Modify: `ayx-rs/src/cmd/catalog.rs` (`CATALOG_METADATA`)

**Interfaces:**
- Produces: `pub fn one_open::build_url(base: &str, kind: &str, id: &str) -> Result<String, Envelope>`; `pub fn one_open::execute(runtime: &RuntimeCtx<'_>, kind: String, id: Option<String>, print: bool) -> anyhow::Result<Envelope>`; `pub const VERIFIED_KINDS: &[&str]`.

- [ ] **Step 1: Write the failing unit tests**

Create `ayx-rs/src/cmd/one_open.rs` with tests first:

```rust
//! `one open`: deep-link the Alteryx One web console for a resource.
//!
//! Only kinds whose web path has been verified in a browser are wired. Others
//! return a validation envelope that tells the caller where to look.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_kinds_build_their_paths() {
        assert_eq!(
            build_url("https://us1.alteryxcloud.com/", "workspace", "01GID").unwrap(),
            "https://us1.alteryxcloud.com/?workspaceGid=01GID"
        );
        assert_eq!(
            build_url("https://us1.alteryxcloud.com", "workflow", "01ULID").unwrap(),
            "https://us1.alteryxcloud.com/ayx-one/cloud-native/workflows/01ULID"
        );
    }

    #[test]
    fn unverified_kind_is_a_validation_envelope_with_a_hint() {
        let env = build_url("https://us1.alteryxcloud.com", "flow", "42").unwrap_err();
        assert!(!env.ok);
        assert_eq!(env.error_code, Some(ayx_core::envelope::ErrorCode::Validation));
        let remediation = env.remediation.expect("remediation present");
        assert!(remediation.summary.contains("https://us1.alteryxcloud.com"));
        assert!(remediation.summary.contains("42"));
        assert_eq!(env.data["verified_kinds"], serde_json::json!(VERIFIED_KINDS));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Add `mod one_open;` to `ayx-rs/src/cmd/mod.rs`. Run: `cargo nextest run -p ayx-rs one_open::tests`
Expected: compile failure.

- [ ] **Step 3: Implement**

Add dependencies: `Cargo.toml` `[workspace.dependencies]` `open = "5.4"`; `ayx-rs/Cargo.toml` `open.workspace = true`.

Above the tests:

```rust
use std::io::IsTerminal;

use anyhow::{Result, anyhow};
use ayx_core::envelope::{Envelope, ErrorCode};
use serde_json::json;

use super::RuntimeCtx;

/// Kinds with a browser-verified web path. Add a kind here only after opening
/// the constructed URL in a browser against a real tenant.
pub const VERIFIED_KINDS: &[&str] = &["workspace", "workflow"];

pub fn build_url(base: &str, kind: &str, id: &str) -> Result<String, Envelope> {
    let base = base.trim_end_matches('/');
    match kind {
        "workspace" => Ok(format!("{base}/?workspaceGid={id}")),
        "workflow" => Ok(format!("{base}/ayx-one/cloud-native/workflows/{id}")),
        other => Err(Envelope::err_coded(
            ErrorCode::Validation,
            format!("no verified web path for kind `{other}`"),
            json!({ "kind": other, "id": id, "verified_kinds": VERIFIED_KINDS }),
        )
        .with_remediation(
            format!("Open {base} in a browser and search for {id}"),
            Vec::new(),
        )),
    }
}

pub fn execute(
    runtime: &RuntimeCtx<'_>,
    kind: String,
    id: Option<String>,
    print: bool,
) -> Result<Envelope> {
    let config = runtime.load_profile_lenient(None)?;
    let one = config
        .alteryx_one
        .as_ref()
        .ok_or_else(|| anyhow!("validation: `one open` requires an alteryx_one profile"))?;
    let base = one_base_url(one)?;
    let id = match (kind.as_str(), id) {
        ("workspace", None) => one.resolved_workspace_gid().ok_or_else(|| {
            anyhow!("validation: this profile has no active workspace GID; pass the GID explicitly")
        })?,
        (_, Some(id)) => id,
        (kind, None) => return Err(anyhow!("validation: `one open {kind}` requires an id")),
    };
    let url = match build_url(&base, &kind, &id) {
        Ok(url) => url,
        Err(envelope) => return Ok(envelope),
    };
    let launch = !print && !runtime.no_input && std::io::stdout().is_terminal();
    let launched = launch && open::that(&url).is_ok();
    Ok(Envelope::ok_with_data(
        if launched { format!("opened {url}") } else { url.clone() },
        json!({ "kind": kind, "id": id, "url": url, "launched": launched }),
    ))
}
```

`one_base_url`: the One base URL accessor lives in `ayx-core/src/profile.rs` — find it with `rg -n "fn .*base_url" ayx-core/src/profile.rs` (the same accessor `one_api_live_request` uses to build request URLs; follow `ayx-one-api/src/lib.rs` if the name is not obvious). Write:

```rust
fn one_base_url(one: &ayx_core::profile::AlteryxOneProfile) -> Result<String> {
    one.base_url() // adapt to the accessor's real name/return type
        .map(|u| u.to_string())
        .ok_or_else(|| anyhow!("validation: the alteryx_one profile has no base URL configured"))
}
```

`resolved_workspace_gid()` exists on the One profile (`workspace.rs:44-49` uses it); adapt if it returns `Option<&str>`.

- [ ] **Step 4: Clap variant, descriptor, dispatch, catalog**

In `enum OneCommand` (`main.rs`), add:

```rust
    /// Open a One resource in the web console; prints the URL off a terminal or with --print.
    Open {
        /// Resource kind: `workspace` or `workflow` (other kinds are not yet verified).
        #[arg(value_name = "KIND")]
        kind: String,
        /// Workspace GID for `workspace` (defaults to the profile's active
        /// workspace) or workflow ULID for `workflow`.
        #[arg(value_name = "ID")]
        id: Option<String>,
        /// Print the URL instead of launching a browser.
        #[arg(long)]
        print: bool,
    },
```

In `ayx-rs/src/cmd/one.rs`: descriptor arm

```rust
        OneCommand::Open { .. } => OutputDescriptor::new("one.open", ViewKind::Result)
            .with_fields(&["kind", "id", "url", "launched"]),
```

and dispatch arm (beside the `OneCommand::Workspace { command }` dispatch):

```rust
        OneCommand::Open { kind, id, print } => super::one_open::execute(runtime, kind, id, print)?,
```

`ayx-rs/src/cmd/catalog.rs` `CATALOG_METADATA`:

```rust
    CatalogMetadata {
        path: "one/open",
        output: "url envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["alteryx_one profile with a base URL"],
        notes: &["Launches a browser only on a terminal without --no-input; otherwise prints the URL."],
    },
```

- [ ] **Step 5: Verify the two web paths in a browser (record in the PR)**

With a logged-in profile on a terminal: `ayx one open workspace --print` and `ayx one open workflow <real ulid> --print`; paste each URL into a browser. If either does not land on the resource, fix the path in `build_url` and its test before merging. Do not add `flow`/`connection`/`plan` kinds in this wave.

- [ ] **Step 6: Gates and commit**

Run all gates plus `refresh-command-surface`. Expected: PASS.

```bash
git add Cargo.toml Cargo.lock ayx-rs/Cargo.toml ayx-rs/src/cmd/one_open.rs ayx-rs/src/cmd/mod.rs ayx-rs/src/main.rs ayx-rs/src/cmd/one.rs ayx-rs/src/cmd/catalog.rs docs/command-surface.md
git commit -m "feat(one): add one open <kind> [id] web deep-links (workspace, workflow)"
```

---

### Task 11: Changelog, agent docs, site, final gates

**Files:**
- Modify: `CHANGELOG.md:3-5` (Unreleased)
- Modify: `docs/agent-guide.md`, `skills/ayx-cli-agent/SKILL.md`
- Modify: `site/src/content/docs/guides/getting-started*.md` (locate with `rg -l "getting-started\|Getting started" site/src/content/docs`)

- [ ] **Step 1: CHANGELOG**

Under `## Unreleased` (after the comment line 5):

```markdown
### Removed

- **`ayx tui`.** The bundled terminal UI (legacy and the `AYX_TUI_V2` preview)
  is removed per ADR 0004. A hidden `ayx tui` stub returns a remediation
  envelope for this release cycle and is deleted in 0.21.0. Profile, auth, and
  connectivity setup live in `ayx onboard`, `ayx one login`, `ayx profile`,
  and `ayx doctor`.

### Changed

- **Output mode is auto-detected.** Without `--output`, `ayx` emits compact
  JSON when stdout is not a terminal or an agent host is detected
  (`AYX_AGENT`, `CLAUDECODE`, `AI_AGENT`); `AYX_OUTPUT=<mode>` overrides the
  automatic choice. Terminals still get text. Piped human use needs
  `AYX_OUTPUT=text`.
- `docs/cli-schema.json` now admits `error_code` and the new optional fields;
  every error envelope previously failed the published schema.

### Added

- Error envelopes carry `retryable` and, for dispatcher-classified failures, a
  structured `remediation { summary, commands }`. Paginated list results carry
  `next` with the exact `--page-token` continuation command.
- `--jq <FILTER>` and `--raw-output` (`-r`) run a jq filter over the JSON
  result in-binary (pure-Rust `jaq`).
- `ayx one workspace detail <id>`.
- `ayx one open <kind> [id] [--print]` deep-links the web console for
  `workspace` and `workflow`.
- Omitting the id of `one workflows detail|delete`, `one flows detail`,
  `one connections detail`, `one job-groups detail`, `one person detail`,
  `one plans detail`, or `one workspace switch` on a terminal opens a picker;
  off a terminal it is a `validation` error naming the list command.
```

- [ ] **Step 2: Agent docs**

`docs/agent-guide.md`: in "Start here", after the sentence "Always put `--output json` at the end of the command.", add: "From 0.20.0 a non-terminal stdout already defaults to compact JSON; keep the explicit flag for clarity. Use `--jq <filter>` to project fields in-binary. Error envelopes carry `error_code`, `retryable`, and, where the CLI can name the fix, `remediation.commands`; run those before retrying."

`skills/ayx-cli-agent/SKILL.md`: under "Read the standard envelope carefully", add two bullets:

```markdown
- on failure, branch on `error_code` and `retryable`; when `remediation.commands`
  is present, run those commands before re-attempting the original;
- on paginated successes, `next[0]` is the exact command for the next page.
```

- [ ] **Step 3: Site**

In the getting-started guide, add a short "Interactive helpers" subsection listing `one open` and the pickers (two sentences each, no screenshots).

- [ ] **Step 4: Full gates, surface check, and clean-tree check**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace --locked
cargo run -q -p xtask -- refresh-command-surface --check
ayx actions validate --output json
git status --short   # must be empty after the commit below
```

Expected: all PASS.

- [ ] **Step 5: Commit and open the PR**

```bash
git add CHANGELOG.md docs/agent-guide.md skills/ayx-cli-agent/SKILL.md site/src/content/docs
git commit -m "docs(release): changelog and agent guidance for 0.20.0 (TUI removal, agent hygiene)"
git push -u origin feat/wave0-tui-removal
```

Open the PR against `main` titled `feat(cli)!: Wave 0 — remove the TUI, agent-native defaults (0.20.0)`; body links ADR 0004, the spec, and this plan, and pastes the dependency-count and binary-size deltas from Tasks 4 and 6 plus the two manual smoke results (Task 8 Step 9, Task 10 Step 5). The version bump to `0.20.0` and the tag are done by the `ayx-release` flow after review, not in this branch.

---

## Self-review against the spec

- **A1 `workspace detail`** → Task 1. **A2 delete** → Task 4 (Step 4-7). **A3 stub** → Task 4 Step 4 (uses Task 2's `with_remediation`, hence the ordering). **A4 tag/branch** → Task 4 Step 1.
- **B1 auto-detect** → Task 5 (`AYX_AGENT`, `CLAUDECODE`, `AI_AGENT`; `AYX_OUTPUT`; TTY). **B2 envelope fields + schema** → Task 2; dispatcher population → Task 3; `next` → Task 7. **B3 `--jq`** → Task 6. **B4 `one open`** → Task 10 (two verified kinds; others error with a hint). **B5 pickers** → Tasks 8-9 (all seven curated leaves). **B6 `--watch`** → explicitly moved to Wave 1 (spec updated).
- Spec's "Docs to update alongside" → Tasks 4, 5, 6, 11. "Testing and verification" → each task's tests plus Task 11's gates; the dependency and size deltas are recorded in commit bodies and the PR.
- Type consistency: `Remediation { summary, commands }`, `with_remediation(summary, Vec<String>)`, `finalize_retryable()`, `resolve_output_mode(...) -> Result<(OutputMode, OutputModeSource), String>`, `jq::apply(...) -> Result<Vec<String>>`, `resolve_selector(what, list_command, given, policy, fetch)`, `items_from_envelope(&Envelope, &[&str])`, `build_url(base, kind, id) -> Result<String, Envelope>` are used with the same names and shapes in every task that references them.
