# One API Coverage Tool — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `ayx one api coverage`, which diffs the live Alteryx One OpenAPI spec against the wired-command inventory and reports covered/missing/stale endpoints.

**Architecture:** A pure diff core in `ayx-one-api` (`coverage(&spec) -> CoverageReport`) consumes the existing `inventory` catalog and an OpenAPI JSON document. A thin `ayx-rs` command fetches the spec live (or from `--spec <file>`), runs the core, renders an envelope/table, and applies `--check`. The API-introspection group is promoted to its post-reorg home `one api`, with `one platform api` kept as a hidden deprecated alias.

**Tech Stack:** Rust, clap (derive), serde_json, anyhow, the `Envelope` type, `cargo nextest`.

## Global Constraints

- **Alteryx One only.** Never read, import, or reference `ayx-server-api` (Alteryx Server is a separate product).
- CI runs `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `cargo run -q -p xtask -- refresh-command-surface --check`, and `cargo nextest run --workspace` on ubuntu/macos/windows. All must pass.
- Every new command carries a clap `about` description and a `CommandSpec` catalog card (no undescribed commands).
- Run `cargo fmt --all` as part of each commit step.
- `cli_smoke` tests run on all platforms (Windows spawn works since #59) — new spawn tests must be Windows-safe (no path-in-JSON, no hardcoded `ayx ` bin name).

---

### Task 1: Coverage diff core in `ayx-one-api`

**Files:**
- Create: `ayx-one-api/src/coverage.rs`
- Modify: `ayx-one-api/src/inventory.rs` (add `inventory_endpoints_full()`)
- Modify: `ayx-one-api/src/lib.rs` (`mod coverage;` + `pub use`)
- Test: inline `#[cfg(test)]` module in `ayx-one-api/src/coverage.rs`

**Interfaces:**
- Consumes: `inventory::SURFACES` (private) via a new accessor.
- Produces:
  - `pub fn inventory_endpoints_full() -> Vec<(&'static str, &'static str, &'static str)>` — `(method, path, command)` across all surface buckets.
  - `pub fn canonical_op(method: &str, full_path: &str) -> Option<(String, String)>` — `(UPPER_METHOD, canonical_path)`; `None` if the path has no `/v4` anchor.
  - `pub struct MissingEndpoint { method: String, path: String, resource: String, summary: Option<String>, operation_id: Option<String> }`
  - `pub struct StaleEndpoint { method: String, path: String, command: String }`
  - `pub struct CoverageReport { coverage_pct: f64, spec_operations: usize, inventory_operations: usize, covered: usize, missing: Vec<MissingEndpoint>, stale: Vec<StaleEndpoint>, unmatched_spec_paths: Vec<String> }`
  - `pub fn coverage(spec: &serde_json::Value) -> CoverageReport`

- [ ] **Step 1: Add the full-endpoint accessor to `inventory.rs`**

After `inventory_endpoints()` (around line 982), add:

```rust
/// Like [`inventory_endpoints`] but also returns the wired command name.
pub fn inventory_endpoints_full() -> Vec<(&'static str, &'static str, &'static str)> {
    SURFACES
        .iter()
        .chain(PARTIAL_SURFACES.iter())
        .chain(DOCUMENTED_ONLY_SURFACES.iter())
        .chain(DEFERRED_SURFACES.iter())
        .flat_map(|s| s.endpoints.iter().map(|e| (e.method, e.path, e.command)))
        .collect()
}
```

- [ ] **Step 2: Write the failing tests in `ayx-one-api/src/coverage.rs`**

Create the file with the test module first:

```rust
//! Alteryx One API coverage diff: live OpenAPI spec vs. wired inventory.
//! Alteryx One only — this module never references Alteryx Server.

use serde_json::Value;

use crate::inventory::inventory_endpoints_full;

// ... (types + fns from Steps 3-5 go here) ...

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec_with(paths: Value) -> Value {
        json!({ "openapi": "3.0.0", "servers": [{ "url": "https://x/" }], "paths": paths })
    }

    #[test]
    fn canonical_collapses_params_and_case() {
        assert_eq!(
            canonical_op("get", "/v4/flows/{flowId}"),
            Some(("GET".into(), "/v4/flows/{}".into()))
        );
    }

    #[test]
    fn param_name_drift_is_covered_not_missing() {
        // Inventory has GET /v4/flows/{id}; spec exposes GET /v4/flows/{flowId}.
        let spec = spec_with(json!({ "/v4/flows/{flowId}": { "get": { "summary": "Get flow" } } }));
        let r = coverage(&spec);
        assert!(r.missing.iter().all(|m| m.path != "/v4/flows/{flowId}"));
    }

    #[test]
    fn spec_only_op_is_missing() {
        let spec = spec_with(json!({
            "/v4/importedDatasets": { "post": { "summary": "Upload", "operationId": "createImported" } }
        }));
        let r = coverage(&spec);
        let m = r.missing.iter().find(|m| m.path == "/v4/importedDatasets" && m.method == "POST")
            .expect("should be missing");
        assert_eq!(m.resource, "importedDatasets");
        assert_eq!(m.summary.as_deref(), Some("Upload"));
        assert_eq!(m.operation_id.as_deref(), Some("createImported"));
    }

    #[test]
    fn inventory_only_op_is_stale() {
        // A spec that exposes nothing the inventory has -> everything wired is stale.
        let spec = spec_with(json!({ "/v4/nonexistent": { "get": {} } }));
        let r = coverage(&spec);
        assert!(!r.stale.is_empty(), "wired endpoints absent from spec must be stale");
        assert!(r.stale.iter().all(|s| !s.command.is_empty()));
    }

    #[test]
    fn base_path_relative_spec_is_anchored() {
        // servers URL carries /v4; paths are relative.
        let spec = json!({
            "servers": [{ "url": "https://host/v4" }],
            "paths": { "/flows/{id}": { "get": {} } }
        });
        let r = coverage(&spec);
        assert!(r.unmatched_spec_paths.is_empty(), "relative /flows must anchor to /v4/flows");
        assert!(r.missing.iter().all(|m| m.path != "/flows/{id}"));
    }

    #[test]
    fn non_v4_path_is_unmatched_not_dropped() {
        let spec = json!({ "servers": [{ "url": "https://host" }], "paths": { "/health": { "get": {} } } });
        let r = coverage(&spec);
        assert!(r.unmatched_spec_paths.iter().any(|p| p.contains("/health")));
    }

    #[test]
    fn inventory_has_no_duplicate_canonical_keys() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for (m, p, _) in inventory_endpoints_full() {
            let key = canonical_op(m, p).expect("inventory paths are /v4-anchored");
            assert!(seen.insert(key.clone()), "duplicate canonical inventory key: {key:?}");
        }
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p ayx-one-api coverage:: 2>&1 | tail -20`
Expected: FAIL — `canonical_op`, `coverage`, types not found.

- [ ] **Step 4: Implement the core (paste above the test module in `coverage.rs`)**

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct MissingEndpoint {
    pub method: String,
    pub path: String,
    pub resource: String,
    pub summary: Option<String>,
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StaleEndpoint {
    pub method: String,
    pub path: String,
    pub command: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CoverageReport {
    pub coverage_pct: f64,
    pub spec_operations: usize,
    pub inventory_operations: usize,
    pub covered: usize,
    pub missing: Vec<MissingEndpoint>,
    pub stale: Vec<StaleEndpoint>,
    pub unmatched_spec_paths: Vec<String>,
}

const HTTP_METHODS: &[&str] = &["get", "put", "post", "delete", "patch", "head", "options", "trace"];

/// Canonicalize an operation to `(UPPER_METHOD, canonical_path)`.
/// Anchors the path at `/v4`, drops query/fragment and trailing slash, and
/// replaces every `{param}` segment with `{}`. Returns `None` if no `/v4`.
pub fn canonical_op(method: &str, full_path: &str) -> Option<(String, String)> {
    let no_q = full_path.split(['?', '#']).next().unwrap_or(full_path);
    let trimmed = no_q.trim_end_matches('/');
    let idx = trimmed.find("/v4/").or_else(|| if trimmed.ends_with("/v4") { Some(trimmed.len() - 3) } else { None })?;
    let from_v4 = &trimmed[idx..];
    let canon = from_v4
        .split('/')
        .map(|seg| if seg.starts_with('{') && seg.ends_with('}') && seg.len() >= 2 { "{}" } else { seg })
        .collect::<Vec<_>>()
        .join("/");
    Some((method.to_ascii_uppercase(), canon))
}

/// Base path component from the spec's first `servers[].url` (or empty).
fn spec_base_path(spec: &Value) -> String {
    let url = spec.get("servers")
        .and_then(|s| s.as_array())
        .and_then(|a| a.first())
        .and_then(|s| s.get("url"))
        .and_then(Value::as_str)
        .unwrap_or("");
    // Strip scheme://host, keep path. Cheap parse: take substring after the 3rd '/'.
    if let Some(rest) = url.split_once("://").map(|(_, r)| r) {
        match rest.find('/') { Some(i) => rest[i..].trim_end_matches('/').to_string(), None => String::new() }
    } else {
        url.trim_end_matches('/').to_string()
    }
}

fn resource_of(canonical_path: &str) -> String {
    // /v4/<resource>/... -> <resource>
    canonical_path.strip_prefix("/v4/").unwrap_or(canonical_path)
        .split('/').next().unwrap_or("").to_string()
}

pub fn coverage(spec: &Value) -> CoverageReport {
    use std::collections::{HashMap, HashSet};

    let base = spec_base_path(spec);

    // Inventory canonical set + a lookup back to (method, path, command) for stale.
    let inv_full = inventory_endpoints_full();
    let mut inv_keys: HashSet<(String, String)> = HashSet::new();
    let mut inv_meta: HashMap<(String, String), (&'static str, &'static str, &'static str)> = HashMap::new();
    for (m, p, c) in &inv_full {
        if let Some(key) = canonical_op(m, p) {
            inv_keys.insert(key.clone());
            inv_meta.insert(key, (m, p, c));
        }
    }

    let mut spec_keys: HashSet<(String, String)> = HashSet::new();
    let mut missing: Vec<MissingEndpoint> = Vec::new();
    let mut unmatched: Vec<String> = Vec::new();
    let mut spec_ops = 0usize;

    if let Some(paths) = spec.get("paths").and_then(Value::as_object) {
        for (path_key, item) in paths {
            let full = format!("{}{}", base, path_key);
            let Some(methods) = item.as_object() else { continue };
            for (verb, op) in methods {
                if !HTTP_METHODS.contains(&verb.to_ascii_lowercase().as_str()) { continue; }
                spec_ops += 1;
                let Some((m, canon)) = canonical_op(verb, &full) else {
                    unmatched.push(format!("{} {}", verb.to_ascii_uppercase(), path_key));
                    continue;
                };
                let key = (m.clone(), canon.clone());
                spec_keys.insert(key.clone());
                if !inv_keys.contains(&key) {
                    missing.push(MissingEndpoint {
                        method: m,
                        path: path_key.clone(),
                        resource: resource_of(&canon),
                        summary: op.get("summary").and_then(Value::as_str).map(str::to_string),
                        operation_id: op.get("operationId").and_then(Value::as_str).map(str::to_string),
                    });
                }
            }
        }
    }

    let mut stale: Vec<StaleEndpoint> = inv_keys.difference(&spec_keys)
        .filter_map(|key| inv_meta.get(key))
        .map(|(m, p, c)| StaleEndpoint { method: (*m).to_string(), path: (*p).to_string(), command: (*c).to_string() })
        .collect();

    // Deterministic ordering for stable output + tests.
    missing.sort_by(|a, b| (&a.resource, &a.path, &a.method).cmp(&(&b.resource, &b.path, &b.method)));
    stale.sort_by(|a, b| (&a.path, &a.method).cmp(&(&b.path, &b.method)));
    unmatched.sort();

    let covered = spec_keys.intersection(&inv_keys).count();
    let coverage_pct = if spec_keys.is_empty() { 100.0 } else {
        (covered as f64 / spec_keys.len() as f64 * 1000.0).round() / 10.0
    };

    CoverageReport {
        coverage_pct,
        spec_operations: spec_ops,
        inventory_operations: inv_keys.len(),
        covered,
        missing,
        stale,
        unmatched_spec_paths: unmatched,
    }
}
```

- [ ] **Step 5: Wire the module in `lib.rs`**

Add near the other `mod` lines (~line 24): `mod coverage;`
Add to the `pub use` block (~line 426):
```rust
pub use coverage::{coverage, CoverageReport, MissingEndpoint, StaleEndpoint};
pub use inventory::inventory_endpoints_full;
```

- [ ] **Step 6: Run tests + clippy to verify pass**

Run: `cargo test -p ayx-one-api coverage:: 2>&1 | tail -20`
Expected: PASS (7 tests).
Run: `cargo clippy -p ayx-one-api --all-targets -- -D warnings 2>&1 | tail -3`
Expected: `Finished`, no warnings.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add ayx-one-api/src/coverage.rs ayx-one-api/src/inventory.rs ayx-one-api/src/lib.rs
git commit -m "feat(one-api): coverage diff core (spec vs inventory)"
```

---

### Task 2: Promote the `api` group to `one api` (+ hidden `one platform api` alias)

**Files:**
- Modify: `ayx-rs/src/main.rs` (rename enum, add `OneCommand::Api`, hide `OnePlatformCommand::Api`)
- Create: `ayx-rs/src/cmd/one_api/mod.rs`
- Modify: `ayx-rs/src/cmd/mod.rs` (declare `pub(crate) mod one_api;`)
- Modify: `ayx-rs/src/cmd/one_platform/api.rs` (delegate to shared logic) or move it
- Modify: `ayx-rs/src/cmd/one_platform/mod.rs` (route hidden alias)
- Test: `ayx-rs/tests/cli_smoke.rs`

**Interfaces:**
- Consumes: `OneApiCommand` (renamed from `OnePlatformApiCommand`).
- Produces: `cmd::one_api::execute(runtime, command: OneApiCommand) -> Result<Envelope>`; top-level `one api` group.

- [ ] **Step 1: Rename `OnePlatformApiCommand` -> `OneApiCommand` and reuse it top-level**

In `main.rs`: rename the enum at line ~1413 to `OneApiCommand` (keep `Status`, `Diagnose`, `OpenApiSpec`). Update its single use in `OnePlatformCommand::Api { command: OneApiCommand }` (~line 1182). Add a top-level variant to `OneCommand` (the enum at ~1104):

```rust
    /// Alteryx One API introspection (spec + coverage).
    Api {
        #[command(subcommand)]
        command: Option<OneApiCommand>,
    },
```

Mark the old platform path hidden — on the `OnePlatformCommand::Api` variant (~line 1180), add `#[command(hide = true)]` above it.

- [ ] **Step 2: Create `cmd/one_api/mod.rs` with the shared handler**

```rust
//! Alteryx One API introspection commands (`one api`). One only.
use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one::{api_diagnose_envelope, api_status_envelope};
use ayx_one_api::one_api_live_request;

use crate::{OneApiCommand, cmd::RuntimeCtx};

pub(crate) mod coverage;

pub(crate) fn execute(runtime: &RuntimeCtx<'_>, command: OneApiCommand) -> Result<Envelope> {
    Ok(match command {
        OneApiCommand::Status { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            api_status_envelope(&config, "one")?
        }
        OneApiCommand::Diagnose { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            api_diagnose_envelope(&config, "one")?
        }
        OneApiCommand::OpenApiSpec { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(&config, "api", "open-api-spec", "GET", "/v4/open-api-spec", false, &[])?
        }
    })
}
```

- [ ] **Step 3: Route `one api` and delegate the hidden alias**

In `cmd/mod.rs` add `pub(crate) mod one_api;`.
In `main.rs`'s `execute()` dispatch (find `OneCommand::Platform` arm), add an arm:
```rust
        OneCommand::Api { command } => match command {
            Some(cmd) => crate::cmd::one_api::execute(&runtime, cmd)?,
            None => /* mirror how other group-with-optional-subcommand arms render help/summary */,
        },
```
(Match the exact `runtime`/help-fallback pattern used by the neighboring `OneCommand` arms — copy their shape.)
In `cmd/one_platform/mod.rs:25`, change the hidden alias to delegate:
```rust
        Some(OnePlatformCommand::Api { command }) => crate::cmd::one_api::execute(runtime, command)?,
```
Delete `cmd/one_platform/api.rs` (logic now lives in `cmd/one_api`). Remove its `mod api;` declaration from `cmd/one_platform/mod.rs`.

- [ ] **Step 4: Write the failing cli_smoke tests**

Add to `ayx-rs/tests/cli_smoke.rs`:
```rust
#[test]
fn one_api_group_help_lists_spec_and_coverage() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "api", "--help"]).output().expect("ayx runs");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("open-api-spec"));
    assert!(stdout.contains("coverage"));
}

#[test]
fn hidden_platform_api_alias_still_parses() {
    // Deprecated path must still work (help renders, exit 0).
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "platform", "api", "open-api-spec", "--help"]).output().expect("ayx runs");
    assert!(output.status.success());
}
```
(The `coverage` assertion will pass only after Task 3 adds the subcommand; if executing strictly task-by-task, split it — assert `open-api-spec` here, add the `coverage` assert in Task 3.)

- [ ] **Step 5: Build, run, verify**

Run: `cargo build -p ayx-rs 2>&1 | tail -3` → `Finished`.
Run: `cargo nextest run -p ayx-rs --test cli_smoke -E 'test(one_api_group_help_lists_spec_and_coverage) + test(hidden_platform_api_alias_still_parses)' 2>&1 | tail -8`
Expected: PASS (the `coverage` assert deferred to Task 3 if split).
Run: `cargo clippy -p ayx-rs --all-targets -- -D warnings 2>&1 | tail -3` → clean.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add -A
git commit -m "feat(one): promote api group to \`one api\`; hide \`one platform api\` alias"
```

---

### Task 3: `one api coverage` subcommand + handler

**Files:**
- Modify: `ayx-rs/src/main.rs` (add `Coverage` variant to `OneApiCommand`)
- Create: `ayx-rs/src/cmd/one_api/coverage.rs`
- Modify: `ayx-rs/src/cmd/one_api/mod.rs` (route `Coverage`)
- Create: `ayx-rs/tests/fixtures/one-openapi-min.json` (spec fixture)
- Test: `ayx-rs/tests/cli_smoke.rs`

**Interfaces:**
- Consumes: `ayx_one_api::{coverage, CoverageReport}`, `one_api_live_request`.
- Produces: `cmd::one_api::coverage::execute(runtime, profile, spec, check) -> Result<Envelope>`.

- [ ] **Step 1: Add the `Coverage` variant to `OneApiCommand` (main.rs)**

```rust
    /// Diff the live One OpenAPI spec against the wired-command inventory.
    Coverage {
        #[arg(long)]
        profile: Option<String>,
        /// Diff a saved OpenAPI spec JSON file instead of fetching live.
        #[arg(long, value_name = "FILE")]
        spec: Option<std::path::PathBuf>,
        /// Exit non-zero if any endpoint is missing (CI regression gate).
        #[arg(long)]
        check: bool,
    },
```

- [ ] **Step 2: Create the fixture `ayx-rs/tests/fixtures/one-openapi-min.json`**

```json
{
  "openapi": "3.0.0",
  "servers": [{ "url": "https://example/" }],
  "paths": {
    "/v4/flows/{flowId}": { "get": { "summary": "Get a flow" } },
    "/v4/importedDatasets": { "post": { "summary": "Upload an imported dataset", "operationId": "createImportedDataset" } }
  }
}
```
(`GET /v4/flows/{id}` is in the inventory → covered; `POST /v4/importedDatasets` is not → missing.)

- [ ] **Step 3: Write the failing cli_smoke tests**

```rust
#[test]
fn coverage_from_spec_file_reports_missing() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/one-openapi-min.json");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["--output", "json", "one", "api", "coverage", "--spec", fixture])
        .output().expect("ayx runs");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    let missing = json["data"]["missing"].as_array().expect("missing array");
    assert!(missing.iter().any(|m| m["path"] == "/v4/importedDatasets" && m["method"] == "POST"));
}

#[test]
fn coverage_check_flag_exits_nonzero_when_missing() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/one-openapi-min.json");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "api", "coverage", "--spec", fixture, "--check"])
        .output().expect("ayx runs");
    assert!(!output.status.success(), "--check must fail when endpoints are missing");
}
```

- [ ] **Step 4: Run to verify they fail**

Run: `cargo nextest run -p ayx-rs --test cli_smoke -E 'test(coverage_from_spec_file_reports_missing) + test(coverage_check_flag_exits_nonzero_when_missing)' 2>&1 | tail -10`
Expected: FAIL (unknown subcommand `coverage` / no handler).

- [ ] **Step 5: Implement `cmd/one_api/coverage.rs`**

```rust
//! `ayx one api coverage` — diff the live One OpenAPI spec vs. wired inventory.
//! Alteryx One only.
use std::path::PathBuf;

use anyhow::{Context, Result};
use ayx_core::envelope::{Envelope, ErrorCode};
use ayx_one_api::{coverage, one_api_live_request};

use crate::cmd::RuntimeCtx;

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    profile: Option<String>,
    spec: Option<PathBuf>,
    check: bool,
) -> Result<Envelope> {
    // Obtain the OpenAPI document: from --spec file, or live.
    let spec_json: serde_json::Value = match spec {
        Some(path) => {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading spec file {}", path.display()))?;
            serde_json::from_str(&text).context("parsing spec file as JSON")?
        }
        None => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let env = one_api_live_request(
                &config, "api", "open-api-spec", "GET", "/v4/open-api-spec", false, &[],
            )?;
            // Propagate an auth/network failure as-is rather than diffing garbage.
            if !env.ok {
                return Ok(env);
            }
            env.data.clone()
        }
    };

    let report = coverage(&spec_json);
    let missing_n = report.missing.len();
    let data = serde_json::to_value(&report).context("serializing coverage report")?;

    // `err_coded` yields ok=false, which `exit_code_for_envelope` maps to exit 1
    // (verified in ayx-rs/src/main.rs:6439). That is the `--check` CI gate.
    if check && missing_n > 0 {
        Ok(Envelope::err_coded(
            ErrorCode::Validation,
            format!("coverage incomplete: {missing_n} endpoint(s) missing"),
            data,
        ))
    } else {
        Ok(Envelope::ok_with_data("one api coverage", data))
    }
}
```

**Verified against source:** `Envelope` has fields `ok/message/timestamp_utc/data/error_code` and constructors `ok_with_data(msg, data)` / `err_coded(code, msg, data)` (`ayx-core/src/envelope.rs`); there is no `meta` field, so per-report counts live in `data`. `ErrorCode::Validation` is a real variant. A non-ok envelope exits 1 via `exit_code_for_envelope` (`main.rs:6439`).

- [ ] **Step 6: Route `Coverage` in `cmd/one_api/mod.rs`**

Add to the `match command` in `execute`:
```rust
        OneApiCommand::Coverage { profile, spec, check } => {
            coverage::execute(runtime, profile, spec, check)?
        }
```

- [ ] **Step 7: Run tests + clippy**

Run: `cargo nextest run -p ayx-rs --test cli_smoke -E 'test(coverage_from_spec_file_reports_missing) + test(coverage_check_flag_exits_nonzero_when_missing) + test(one_api_group_help_lists_spec_and_coverage)' 2>&1 | tail -10`
Expected: PASS.
Run: `cargo clippy -p ayx-rs --all-targets -- -D warnings 2>&1 | tail -3` → clean.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
git add -A
git commit -m "feat(one): add \`one api coverage\` (live/--spec diff, --check gate)"
```

---

### Task 4: Catalog cards, descriptions, and regenerated command-surface docs

**Files:**
- Modify: `ayx-rs/src/main.rs` (`CommandSpec` catalog cards)
- Modify: `docs/command-surface.md` (regenerated)
- Modify: shell completions if the repo commits them (check `git status` after regen)
- Test: `ayx-rs/tests/cli_smoke.rs` (catalog assertions), xtask check

- [ ] **Step 1: Update/add catalog cards in `main.rs`**

Change the existing `open-api-spec` card (name `"one platform api open-api-spec"`, path `"one/platform/api/open-api-spec"`) to the new path:
```rust
    CommandSpec {
        name: "one api open-api-spec",
        path: "one/api/open-api-spec",
        summary: "Fetch the Alteryx One OpenAPI specification.",
        output: "one api open-api-spec envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &["Maps to GET /v4/open-api-spec in the One API docs."],
    },
```
Add a new card for coverage:
```rust
    CommandSpec {
        name: "one api coverage",
        path: "one/api/coverage",
        summary: "Diff the live One OpenAPI spec against wired commands (covered/missing/stale).",
        output: "one api coverage envelope",
        safety: "read-only",
        mutating: false,
        prerequisites: &["central runtime profile", "server_api"],
        notes: &[
            "Fetches GET /v4/open-api-spec (or --spec <file>) and diffs it against the ayx-one-api inventory.",
            "--check exits non-zero when endpoints are missing.",
        ],
    },
```
Also add `status`/`diagnose` cards under `one api ...` if the old `one platform api status/diagnose` had cards — grep `"one platform api"` in `main.rs` and rename each to `"one api ..."`.

- [ ] **Step 2: Update the catalog smoke assertion**

In `cli_smoke.rs::catalog_surface_lists_core_one_commands`, if it asserts on `one platform api ...` names, update to the new `one api ...` names. Add `assert!(names.contains(&"one api coverage"));`.

- [ ] **Step 3: Regenerate the command-surface docs + completions**

Run: `cargo run -q -p xtask -- refresh-command-surface`
Then check what changed: `git status --porcelain` (expect `docs/command-surface.md` and possibly completion files).

- [ ] **Step 4: Verify the whole gate locally**

Run: `cargo run -q -p xtask -- refresh-command-surface --check` → exits 0 (in sync).
Run: `cargo fmt --all --check` → clean.
Run: `cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3` → clean.
Run: `cargo nextest run --workspace 2>&1 | tail -12` → all pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "docs(surface): catalog cards + regenerated surface for \`one api\`"
```

---

### Task 5: PR, CI, merge

- [ ] **Step 1: Push and open the PR**

```bash
git push -u origin feat/one-api-coverage
```
Open a PR (base `main`) titled `feat(one): ayx one api coverage — spec-vs-inventory diff (#59 follow-on)` summarizing: new `one api coverage`, `api` group promoted to `one api` with hidden alias, pure diff core in `ayx-one-api`, One-only.

- [ ] **Step 2: Watch CI green on all three OSes**

Confirm `Test (ubuntu-latest)`, `Test (macos-latest)`, `Test (windows-latest)`, `Clippy`, `Rustfmt`, `Docs`, `cargo-audit` all pass.

- [ ] **Step 3: Merge**

`aria gh pr merge RyanMerlin/ayx-rs <n> --method squash --delete-branch` with an accurate squash message.

---

## Self-Review

**Spec coverage:** command `one api coverage` (Tasks 3-4) ✓; `--spec`/`--check`/`--profile`/`--output` (Task 3) ✓; live + offline sources (Task 3) ✓; placement at `one api` + hidden alias (Task 2) ✓; path normalization + `/v4` anchor + unmatched bucket (Task 1) ✓; covered/missing/stale (Task 1) ✓; reusable pure core in `ayx-one-api` (Task 1) ✓; JSON envelope + human table — **envelope done (Task 3); the human `--output text` table is produced by the global output formatter that renders every `Envelope`, so no per-command table code is needed** (verify the default text rendering of the coverage envelope is readable; if not, that's a follow-up, not a blocker); unit + cli_smoke + inventory-hygiene tests (Tasks 1, 3) ✓; One-only non-goal enforced (Global Constraints, module docs) ✓.

**Placeholder scan:** one deliberate "match the neighboring arm's shape" note remains (Task 2 Step 3 — the `OneCommand::Api { command: None }` help-fallback must mirror the sibling `one`-group arms rather than be invented; it names where to copy from). The Task 3 envelope constructors were verified against `ayx-core/src/envelope.rs` and corrected. No TBD/TODO left.

**Type consistency:** `OneApiCommand` (renamed) used in Tasks 2-3; `coverage(&Value) -> CoverageReport` and field names (`missing`, `stale`, `coverage_pct`, `unmatched_spec_paths`) consistent between Task 1 types, Task 3 handler, and the fixture-based assertions. `inventory_endpoints_full()` defined in Task 1, consumed in Task 1's core only.
