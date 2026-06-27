# ayx TUI Rearchitecture — Phase 1 (Browser Core) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the Phase-0 single-asset spine into a real k9s-style browser: generalize the `ResourceKind` registry past `Kind::Flow` to all five assets (Flows, Connections, Jobs, People, Workspaces), add resource switching, async drill-down to a scrollable detail view (killing the legacy 18-line truncation and the render-thread freeze), and an in-list `/` filter — all behind the existing `AYX_TUI_V2` gate.

**Architecture:** The Elm Architecture (TEA) established in Phase 0. `Event → map_key(state) → Action → update(&mut AppState) → [Effect] → worker → Action`. The render loop never blocks on I/O. Two structural upgrades over Phase 0: (1) the reducer now *emits* fetch effects (on kind-switch and drill-down), so stale-result dropping moves out of the entry loop and into the pure reducer via a monotonic generation token held in state; (2) `nav.top()` selects the body view (list vs. detail), making the reducer and `map_key` context-sensitive.

**Tech Stack:** Rust (edition 2024), ratatui 0.30, crossterm 0.29, serde_json, anyhow. Backend calls reuse `crate::one_api_live_request` (the worker already calls it). **No new crates in Phase 1** — the `/` filter uses a plain `String` with append/backspace (full cursor editing via `tui-input` is a Phase-3 deliverable per the spec's dependency table; `nucleo-matcher`/palette is Phase 3; throbbers are Phase 5).

## Global Constraints

Copied verbatim from the design spec and Phase-0 conventions — every task implicitly includes these:

- **Spec:** `.superpowers/specs/2026-06-26-ayx-tui-rearchitecture-design.md` (commit `8eaa9dd`). **Phase-0 plan** (the spine this builds on): `.superpowers/plans/2026-06-26-ayx-tui-phase0-foundations.md`, merged in PR #62.
- **Render loop must never block on I/O.** All list *and* detail fetches go through the worker thread; the UI thread only drains results and draws.
- **No backend/API changes.** Phase 1 calls `crate::one_api_live_request` only; it does not modify `ayx-one-api`, `ayx-core`, or any endpoint. Every endpoint path/surface/operation below is copied from an existing, working CLI command (cited per task).
- **Legacy TUI stays untouched.** Do not modify or delete `tui/app.rs`, `tui/mod.rs` (beyond the env gate already added in Phase 0), `tui/store.rs`, `tui/forms.rs`, `tui/render_helpers.rs`, or their tests. All Phase-1 work is inside `tui/v2/`.
- **Reuse the theme.** All colors via `crate::tui::theme` (`theme::ACCENT`, `theme::ok()`, `theme::warn()`, `theme::danger()`, `theme::border(bool)`, `theme::muted()`, `theme::dim()`, `theme::accent()`, `theme::accent_bold()`, `theme::panel()`, `theme::selected()`, `theme::field_label()`, `theme::field_value()`, `theme::app()`). No new hardcoded colors.
- **Status colors:** green (`theme::ok()`) = ok/succeeded; yellow (`theme::warn()`) = pending/running/disabled/suspended-warning; red (`theme::danger()`) = failed/error/suspended. Always paired with a status *word*, never color alone (`StatusTone` carries the tone; the cell text carries the word).
- **Validation gate per task:** `cargo nextest run -p ayx-rs --no-fail-fast` (the crate is `ayx-rs`), then before any commit `cargo fmt --all && cargo clippy -p ayx-rs --all-targets -- -D warnings`. `cargo fmt --all` runs as part of the commit step, not after.
- **Commits:** concise, conventional. Co-author trailer:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`

---

## Scope mapping (spec phases → this plan)

The owner framed Phase 1 as *"generalize the `ResourceKind` registry past `Kind::Flow` and wire Connections/Jobs/People."* That merges the spec's **Phase 1 (Browser core)** — the k9s engine, generic list + reactive detail, scrollable detail, async drill, `/` filter, contextual footer — with the **asset impls** from the spec's **Phase 2 (All assets)**. This plan delivers both, ordered engine-first so each half is independently shippable.

**In scope:** `Kind` expanded to all five; shared row-mapping helpers; per-kind `detail_endpoint`; resource switching (tabs + number keys + Tab cycle); generation-token staleness; async drill-down to a scrollable detail view; in-list `/` filter; per-view contextual footer; `ResourceKind` impls for Connection, Job, Person, Workspace with status tones.

**Explicitly deferred (later phases, NOT this plan):**
- **Cross-asset drill** (flow→runs, job→flow) — spec Phase 2 tail. The `NavStack` supports it; no `children()` trait method is added until then.
- **`Ctrl+K` command palette, `?` help overlay, `tui-input` editing** — spec Phase 3.
- **Workspace *switching*** (ready-vs-needs-login, inline OTP) — spec Phase 4. Phase 1 browses workspaces read-only; it does not change the active workspace.
- **Actions** (run flow, cancel job, enable/disable) — spec Phase 5. No `actions()` trait method yet.

YAGNI: the `ResourceKind` trait gains exactly one new method this phase (`detail_endpoint`). `aliases()`, `actions()`, `children()` from the spec's full trait arrive in the phase that consumes them.

---

## File Structure

| File | Phase-1 change |
|------|----------------|
| `v2/resource/mod.rs` | Expand `Kind` to 5 variants (+ `singular()`); hoist shared helpers (`items_array`, `str_field`, `date_only`) from `flow.rs`; add `DetailEndpoint` + `detail_endpoint()` to the trait; expand the registry. |
| `v2/resource/flow.rs` | Use the hoisted helpers; implement `detail_endpoint`. |
| `v2/resource/connection.rs` | **New.** `ConnectionKind` impl. |
| `v2/resource/job.rs` | **New.** `JobKind` impl (status tone). |
| `v2/resource/person.rs` | **New.** `PersonKind` impl (suspended/admin tone). |
| `v2/resource/workspace.rs` | **New.** `WorkspaceKind` impl (`detail_endpoint` = `None`). |
| `v2/effect.rs` | Add token to `FetchList`; add `FetchDetail`. |
| `v2/state.rs` | `ListView` gains `filter`/`filtering`/`token` + `visible()`; new `DetailView`; `AppState` gains `detail: Option<DetailView>` + `req_seq`. |
| `v2/action.rs` | New actions (`SwitchKind`, `Open`, filter actions, token-carrying load results); context-sensitive reducer; token minting. |
| `v2/worker.rs` | Drop the per-job `RequestId` (staleness moves to the reducer); thread the token through; add the `FetchDetail` arm + `detail_payload_to_action`. |
| `v2/entry.rs` | `map_key(state, key)`; drop `list_request` tracking; `dispatch_effects` just submits. |
| `v2/view/mod.rs` | Dispatch body on `nav.top()` (list vs. detail). |
| `v2/view/header.rs` | Render the resource-tab strip on list views; breadcrumb on detail views. |
| `v2/view/detail.rs` | **New.** Scrollable full-object detail view. |
| `v2/view/footer.rs` | Per-view contextual hints (list / detail / filter). |
| `v2/view/list.rs` | Render filtered rows; show the active filter term. |

---

### Task 1: Shared row-mapping helpers + `detail_endpoint` on the trait

Hoist the three private helpers from `flow.rs` into `resource/mod.rs` so all five impls share them (DRY), and add the detail-endpoint surface to the trait. Flows must stay green.

**Files:**
- Modify: `ayx-rs/src/tui/v2/resource/mod.rs`
- Modify: `ayx-rs/src/tui/v2/resource/flow.rs`
- Test: inline `#[cfg(test)]` in `resource/mod.rs`

**Interfaces:**
- Consumes: existing `Cell`, `Column`, `Row`, `ListEndpoint`, `ResourceKind`.
- Produces:
  - `pub(crate) fn items_array(payload: &serde_json::Value) -> Vec<serde_json::Value>` — tries `["data","items","results"]` then a bare array.
  - `pub(crate) fn str_field<'a>(item: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str>`
  - `pub(crate) fn date_only(ts: &str) -> String`
  - `pub struct DetailEndpoint { surface: &'static str, operation: &'static str, path: &'static str }` (path contains `{id}`).
  - New trait method `fn detail_endpoint(&self) -> Option<DetailEndpoint>` (no default — each impl is explicit).

- [ ] **Step 1: Write the failing test for the shared helpers**

In `ayx-rs/src/tui/v2/resource/mod.rs`, extend the existing `#[cfg(test)] mod tests` with:

```rust
    #[test]
    fn items_array_reads_each_wrapper_key() {
        use serde_json::json;
        assert_eq!(items_array(&json!({ "data": [ {"a":1} ] })).len(), 1);
        assert_eq!(items_array(&json!({ "items": [ {"a":1}, {"b":2} ] })).len(), 2);
        assert_eq!(items_array(&json!({ "results": [ {"a":1} ] })).len(), 1);
        assert_eq!(items_array(&json!([ {"a":1}, {"b":2} ])).len(), 2);
        assert_eq!(items_array(&json!({ "nope": 1 })).len(), 0);
    }

    #[test]
    fn str_field_first_present_wins() {
        use serde_json::json;
        let v = json!({ "displayName": "Bob", "name": "Robert" });
        assert_eq!(str_field(&v, &["name", "displayName"]), Some("Robert"));
        assert_eq!(str_field(&v, &["missing", "displayName"]), Some("Bob"));
        assert_eq!(str_field(&v, &["missing"]), None);
    }

    #[test]
    fn date_only_strips_time() {
        assert_eq!(date_only("2026-06-20T10:00:00Z"), "2026-06-20");
        assert_eq!(date_only("not-a-date"), "not-a-date");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::resource 2>&1 | tail -20`
Expected: FAIL — `items_array`/`str_field`/`date_only` not found in `resource` (they currently live in `flow.rs`).

- [ ] **Step 3: Add the helpers + `DetailEndpoint` + trait method to `resource/mod.rs`**

In `ayx-rs/src/tui/v2/resource/mod.rs`, add the new modules to the `pub mod flow;` block:

```rust
pub mod connection;
pub mod flow;
pub mod job;
pub mod person;
pub mod workspace;
```

Add the `DetailEndpoint` struct next to `ListEndpoint`:

```rust
#[derive(Debug, Clone, Copy)]
pub struct DetailEndpoint {
    pub surface: &'static str,
    pub operation: &'static str,
    /// Path template with an `{id}` placeholder, interpolated by the worker via
    /// `&[("id", id)]` (the same convention the CLI detail commands use).
    pub path: &'static str,
}
```

Add the new method to the `ResourceKind` trait (after `fn list_endpoint`):

```rust
    /// The single-item endpoint for drill-down, or `None` if the asset has no
    /// per-id detail endpoint (e.g. Workspaces, whose detail is the switcher's
    /// job in a later phase).
    fn detail_endpoint(&self) -> Option<DetailEndpoint>;
```

Add the three shared helpers at the bottom of `resource/mod.rs` (above the test module):

```rust
/// One API list payloads wrap items under one of these keys depending on the
/// endpoint/version. Try them in order, then fall back to a bare array.
pub(crate) fn items_array(payload: &Value) -> Vec<Value> {
    for key in ["data", "items", "results"] {
        if let Some(arr) = payload.get(key).and_then(Value::as_array) {
            return arr.clone();
        }
    }
    if let Some(arr) = payload.as_array() {
        return arr.clone();
    }
    Vec::new()
}

/// First present string field among `keys`.
pub(crate) fn str_field<'a>(item: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| item.get(*k).and_then(Value::as_str))
}

/// "2026-06-20T10:00:00Z" -> "2026-06-20"; passthrough if not a timestamp.
pub(crate) fn date_only(ts: &str) -> String {
    ts.split('T').next().unwrap_or(ts).to_string()
}
```

The registry stays single-arm for now (the four new arms land in Task 2):

```rust
pub fn kind_impl(kind: Kind) -> &'static dyn ResourceKind {
    match kind {
        Kind::Flow => &flow::FlowKind,
    }
}
```

- [ ] **Step 4: Update `flow.rs` to use the shared helpers + implement `detail_endpoint`**

Replace the body of `ayx-rs/src/tui/v2/resource/flow.rs` (keep its `#[cfg(test)] mod tests`):

```rust
//! Flow ResourceKind — maps `/v4/flows` list items to display rows.
use super::{Cell, Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, date_only, str_field, items_array};
use serde_json::Value;

pub struct FlowKind;

const FLOW_COLUMNS: &[Column] = &[
    Column { title: "NAME", width: 40 },
    Column { title: "UPDATED", width: 12 },
    Column { title: "ID", width: 24 },
];

impl ResourceKind for FlowKind {
    fn columns(&self) -> &'static [Column] {
        FLOW_COLUMNS
    }

    fn extract_items(&self, payload: &Value) -> Vec<Value> {
        items_array(payload)
    }

    fn row(&self, item: &Value) -> Row {
        let id = str_field(item, &["id", "flowId"]).unwrap_or_default().to_string();
        let name = str_field(item, &["name", "displayName"])
            .unwrap_or("(unnamed)")
            .to_string();
        let updated = str_field(item, &["updatedAt", "updated_at", "modifiedAt"])
            .map(date_only)
            .unwrap_or_default();
        Row {
            id: id.clone(),
            cells: vec![Cell::plain(name), Cell::plain(updated), Cell::plain(id)],
        }
    }

    fn list_endpoint(&self) -> ListEndpoint {
        ListEndpoint { surface: "flow", operation: "tui-flow-list", path: "/v4/flows" }
    }

    // Detail path verified against ayx-rs/src/cmd/one_flows.rs:277 ("/v4/flows/{id}").
    fn detail_endpoint(&self) -> Option<DetailEndpoint> {
        Some(DetailEndpoint { surface: "flow", operation: "tui-flow-detail", path: "/v4/flows/{id}" })
    }
}
```

Note: this references `connection`/`job`/`person`/`workspace` modules declared in Step 3. Create them now as one-line stubs so the crate compiles — Task 2 fills them: each of `ayx-rs/src/tui/v2/resource/{connection,job,person,workspace}.rs` gets:

```rust
// filled in Task 2
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ayx-rs --lib tui::v2::resource 2>&1 | tail -20`
Expected: PASS — the three new helper tests plus the existing flow tests (`extract_items_reads_data_array`, `row_maps_name_updated_id`, `row_handles_missing_name`, `columns_are_three`, `kind_name_and_all`, `cell_constructors_carry_tone`).

- [ ] **Step 6: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
git add ayx-rs/src/tui/v2/resource
git commit -m "feat(tui-v2): hoist shared row helpers + add detail_endpoint to ResourceKind

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Expand `Kind` to five variants + wire the registry

Expand the `Kind` enum and registry. The four new impls get real bodies in Tasks 3–6; here they are minimal compiling stubs so the registry resolves and `Kind::all()` drives the tab strip.

**Files:**
- Modify: `ayx-rs/src/tui/v2/resource/mod.rs`
- Modify: `ayx-rs/src/tui/v2/resource/{connection,job,person,workspace}.rs` (replace the Task-1 stubs)
- Test: inline `#[cfg(test)]` in `resource/mod.rs`

**Interfaces:**
- Produces: `Kind::{Flow,Connection,Job,Person,Workspace}`; `Kind::name()` (plural, e.g. `"connections"`); `Kind::singular()` (e.g. `"connection"`); `Kind::all()` returns all five in display order; registry arms for all five.

- [ ] **Step 1: Write the failing test**

In `resource/mod.rs` tests:

```rust
    #[test]
    fn all_five_kinds_present_and_named() {
        let all = Kind::all();
        assert_eq!(all.len(), 5);
        assert_eq!(Kind::Flow.name(), "flows");
        assert_eq!(Kind::Connection.name(), "connections");
        assert_eq!(Kind::Job.name(), "jobs");
        assert_eq!(Kind::Person.name(), "people");
        assert_eq!(Kind::Workspace.name(), "workspaces");
        assert_eq!(Kind::Person.singular(), "person");
        assert_eq!(Kind::Workspace.singular(), "workspace");
    }

    #[test]
    fn registry_resolves_every_kind() {
        for &k in Kind::all() {
            // Each impl must expose a non-empty column set.
            assert!(!kind_impl(k).columns().is_empty(), "{k:?} has no columns");
        }
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::resource 2>&1 | tail -20`
Expected: FAIL — `Kind::Connection` etc. not found.

- [ ] **Step 3: Expand `Kind` + the registry**

Replace the `Kind` enum + impl + `kind_impl` in `resource/mod.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Flow,
    Connection,
    Job,
    Person,
    Workspace,
}

impl Kind {
    /// Plural display name (list title, tab label, breadcrumb root).
    pub fn name(self) -> &'static str {
        match self {
            Kind::Flow => "flows",
            Kind::Connection => "connections",
            Kind::Job => "jobs",
            Kind::Person => "people",
            Kind::Workspace => "workspaces",
        }
    }

    /// Singular form (detail breadcrumb / titles).
    pub fn singular(self) -> &'static str {
        match self {
            Kind::Flow => "flow",
            Kind::Connection => "connection",
            Kind::Job => "job",
            Kind::Person => "person",
            Kind::Workspace => "workspace",
        }
    }

    /// All kinds in tab/display order.
    pub fn all() -> &'static [Kind] {
        &[Kind::Flow, Kind::Connection, Kind::Job, Kind::Person, Kind::Workspace]
    }
}
```

```rust
pub fn kind_impl(kind: Kind) -> &'static dyn ResourceKind {
    match kind {
        Kind::Flow => &flow::FlowKind,
        Kind::Connection => &connection::ConnectionKind,
        Kind::Job => &job::JobKind,
        Kind::Person => &person::PersonKind,
        Kind::Workspace => &workspace::WorkspaceKind,
    }
}
```

- [ ] **Step 4: Write minimal compiling impls (real bodies land in Tasks 3–6)**

Each stub must expose at least one column (the registry test asserts non-empty) and a list endpoint. Replace each file:

`ayx-rs/src/tui/v2/resource/connection.rs`:
```rust
//! Connection ResourceKind. Real impl lands in Task 3.
use super::{Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, items_array};
use serde_json::Value;

pub struct ConnectionKind;
const COLS: &[Column] = &[Column { title: "NAME", width: 30 }];

impl ResourceKind for ConnectionKind {
    fn columns(&self) -> &'static [Column] { COLS }
    fn extract_items(&self, payload: &Value) -> Vec<Value> { items_array(payload) }
    fn row(&self, _item: &Value) -> Row { Row { id: String::new(), cells: Vec::new() } }
    fn list_endpoint(&self) -> ListEndpoint {
        ListEndpoint { surface: "connection", operation: "tui-connection-list", path: "/v4/connections" }
    }
    fn detail_endpoint(&self) -> Option<DetailEndpoint> {
        Some(DetailEndpoint { surface: "connection", operation: "tui-connection-detail", path: "/v4/connections/{id}" })
    }
}
```

`ayx-rs/src/tui/v2/resource/job.rs`:
```rust
//! Job ResourceKind. Real impl lands in Task 4.
use super::{Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, items_array};
use serde_json::Value;

pub struct JobKind;
const COLS: &[Column] = &[Column { title: "STATUS", width: 12 }];

impl ResourceKind for JobKind {
    fn columns(&self) -> &'static [Column] { COLS }
    fn extract_items(&self, payload: &Value) -> Vec<Value> { items_array(payload) }
    fn row(&self, _item: &Value) -> Row { Row { id: String::new(), cells: Vec::new() } }
    fn list_endpoint(&self) -> ListEndpoint {
        ListEndpoint { surface: "jobGroup", operation: "tui-job-list", path: "/v4/jobLibrary" }
    }
    fn detail_endpoint(&self) -> Option<DetailEndpoint> {
        Some(DetailEndpoint { surface: "jobGroup", operation: "tui-job-detail", path: "/v4/jobGroups/{id}" })
    }
}
```

`ayx-rs/src/tui/v2/resource/person.rs`:
```rust
//! Person ResourceKind. Real impl lands in Task 5.
use super::{Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, items_array};
use serde_json::Value;

pub struct PersonKind;
const COLS: &[Column] = &[Column { title: "NAME", width: 28 }];

impl ResourceKind for PersonKind {
    fn columns(&self) -> &'static [Column] { COLS }
    fn extract_items(&self, payload: &Value) -> Vec<Value> { items_array(payload) }
    fn row(&self, _item: &Value) -> Row { Row { id: String::new(), cells: Vec::new() } }
    fn list_endpoint(&self) -> ListEndpoint {
        ListEndpoint { surface: "platform", operation: "tui-person-list", path: "/v4/people" }
    }
    fn detail_endpoint(&self) -> Option<DetailEndpoint> {
        Some(DetailEndpoint { surface: "platform", operation: "tui-person-detail", path: "/v4/people/{id}" })
    }
}
```

`ayx-rs/src/tui/v2/resource/workspace.rs`:
```rust
//! Workspace ResourceKind. Real impl lands in Task 6.
//! No per-id detail endpoint is wired (the only proven endpoint is
//! `/v4/workspaces/current`); workspace detail is the switcher's job in a later
//! phase, so `detail_endpoint` is `None`.
use super::{Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, items_array};
use serde_json::Value;

pub struct WorkspaceKind;
const COLS: &[Column] = &[Column { title: "NAME", width: 30 }];

impl ResourceKind for WorkspaceKind {
    fn columns(&self) -> &'static [Column] { COLS }
    fn extract_items(&self, payload: &Value) -> Vec<Value> { items_array(payload) }
    fn row(&self, _item: &Value) -> Row { Row { id: String::new(), cells: Vec::new() } }
    fn list_endpoint(&self) -> ListEndpoint {
        ListEndpoint { surface: "platform", operation: "tui-workspace-list", path: "/v4/workspaces" }
    }
    fn detail_endpoint(&self) -> Option<DetailEndpoint> { None }
}
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p ayx-rs --lib tui::v2::resource 2>&1 | tail -20`
Expected: PASS (registry + all-kinds tests + existing).

- [ ] **Step 6: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
git add ayx-rs/src/tui/v2/resource
git commit -m "feat(tui-v2): expand Kind to all five assets + registry (impl stubs)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: `ConnectionKind` implementation

**Files:**
- Modify: `ayx-rs/src/tui/v2/resource/connection.rs`
- Test: inline `#[cfg(test)]`

**Interfaces:**
- Consumes: shared helpers from `resource/mod.rs`.
- Produces: `ConnectionKind` with columns `["NAME","CONNECTOR","UPDATED","ID"]`. Source of truth: `ayx-one-api/src/types.rs:153-168` (`ConnectionSummary` — fields `id`, `name`, `connector_id`/`connectorId`, `updated_at`/`updatedAt`) and `ayx-rs/src/cmd/one_connections.rs:136,193`. No status field → all cells `Neutral`.

- [ ] **Step 1: Write the failing test**

Replace the file's stub body but write the test first; in `connection.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::ResourceKind;
    use serde_json::json;

    #[test]
    fn columns_are_four() {
        assert_eq!(ConnectionKind.columns().len(), 4);
        assert_eq!(ConnectionKind.columns()[0].title, "NAME");
        assert_eq!(ConnectionKind.columns()[1].title, "CONNECTOR");
    }

    #[test]
    fn row_maps_name_connector_updated_id() {
        let item = json!({
            "id": "cn_1", "name": "Prod Snowflake",
            "connectorId": "snowflake", "updatedAt": "2026-06-18T08:30:00Z"
        });
        let row = ConnectionKind.row(&item);
        assert_eq!(row.id, "cn_1");
        assert_eq!(row.cells[0].text, "Prod Snowflake");
        assert_eq!(row.cells[1].text, "snowflake");
        assert_eq!(row.cells[2].text, "2026-06-18");
        assert_eq!(row.cells[3].text, "cn_1");
    }

    #[test]
    fn row_handles_missing_fields() {
        let row = ConnectionKind.row(&json!({ "id": "cn_x" }));
        assert_eq!(row.cells[0].text, "(unnamed)");
        assert_eq!(row.cells[1].text, "");
        assert_eq!(row.id, "cn_x");
    }

    #[test]
    fn list_endpoint_is_v4_connections() {
        assert_eq!(ConnectionKind.list_endpoint().path, "/v4/connections");
        assert_eq!(ConnectionKind.detail_endpoint().unwrap().path, "/v4/connections/{id}");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::resource::connection 2>&1 | tail -20`
Expected: FAIL — stub `columns().len()==1`, `row` returns empty.

- [ ] **Step 3: Implement `ConnectionKind`**

Replace the body above the test module:

```rust
//! Connection ResourceKind — maps `/v4/connections` items to rows.
//! Fields per ayx-one-api/src/types.rs:153-168 (ConnectionSummary).
use super::{Cell, Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, date_only, items_array, str_field};
use serde_json::Value;

pub struct ConnectionKind;

const COLS: &[Column] = &[
    Column { title: "NAME", width: 36 },
    Column { title: "CONNECTOR", width: 16 },
    Column { title: "UPDATED", width: 12 },
    Column { title: "ID", width: 24 },
];

impl ResourceKind for ConnectionKind {
    fn columns(&self) -> &'static [Column] {
        COLS
    }

    fn extract_items(&self, payload: &Value) -> Vec<Value> {
        items_array(payload)
    }

    fn row(&self, item: &Value) -> Row {
        let id = str_field(item, &["id"]).unwrap_or_default().to_string();
        let name = str_field(item, &["name"]).unwrap_or("(unnamed)").to_string();
        let connector = str_field(item, &["connectorId", "connector_id"]).unwrap_or_default().to_string();
        let updated = str_field(item, &["updatedAt", "updated_at"]).map(date_only).unwrap_or_default();
        Row {
            id: id.clone(),
            cells: vec![Cell::plain(name), Cell::plain(connector), Cell::plain(updated), Cell::plain(id)],
        }
    }

    fn list_endpoint(&self) -> ListEndpoint {
        ListEndpoint { surface: "connection", operation: "tui-connection-list", path: "/v4/connections" }
    }

    fn detail_endpoint(&self) -> Option<DetailEndpoint> {
        Some(DetailEndpoint { surface: "connection", operation: "tui-connection-detail", path: "/v4/connections/{id}" })
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ayx-rs --lib tui::v2::resource::connection 2>&1 | tail -20`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
git add ayx-rs/src/tui/v2/resource/connection.rs
git commit -m "feat(tui-v2): ConnectionKind impl

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `JobKind` implementation (status tone)

**Files:**
- Modify: `ayx-rs/src/tui/v2/resource/job.rs`
- Test: inline `#[cfg(test)]`

**Interfaces:**
- Produces: `JobKind` with columns `["FLOW","STATUS","STARTED","ID"]`. Source of truth: `ayx-one-api/src/types.rs:319-349` (`JobGroupSummary` — `id`, `flow_name`/`flowName`, `flow_id`/`flowId`, `status` ∈ {Queued,Running,Succeeded,Failed,Cancelled}, `started_at`/`startedAt`) and `ayx-rs/src/cmd/one_job_groups.rs:31,110`. The `status` cell carries a `StatusTone`.
- Status → tone map: `Succeeded` → `Ok`; `Running`/`Queued` → `Warn`; `Failed`/`Cancelled` → `Danger`; anything else → `Neutral`. Case-insensitive.

- [ ] **Step 1: Write the failing test**

In `job.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::{ResourceKind, StatusTone};
    use serde_json::json;

    #[test]
    fn columns_are_four() {
        assert_eq!(JobKind.columns().len(), 4);
        assert_eq!(JobKind.columns()[0].title, "FLOW");
        assert_eq!(JobKind.columns()[1].title, "STATUS");
    }

    #[test]
    fn row_maps_flow_status_started_id_with_tone() {
        let item = json!({
            "id": "jg_1", "flowName": "Daily ETL", "status": "Succeeded",
            "startedAt": "2026-06-21T02:00:00Z"
        });
        let row = JobKind.row(&item);
        assert_eq!(row.id, "jg_1");
        assert_eq!(row.cells[0].text, "Daily ETL");
        assert_eq!(row.cells[1].text, "Succeeded");
        assert_eq!(row.cells[1].tone, StatusTone::Ok);
        assert_eq!(row.cells[2].text, "2026-06-21");
        assert_eq!(row.cells[3].text, "jg_1");
    }

    #[test]
    fn status_tone_mapping() {
        assert_eq!(status_tone("Succeeded"), StatusTone::Ok);
        assert_eq!(status_tone("running"), StatusTone::Warn);
        assert_eq!(status_tone("Queued"), StatusTone::Warn);
        assert_eq!(status_tone("Failed"), StatusTone::Danger);
        assert_eq!(status_tone("Cancelled"), StatusTone::Danger);
        assert_eq!(status_tone("weird"), StatusTone::Neutral);
    }

    #[test]
    fn row_falls_back_to_flow_id_then_placeholder() {
        let by_id = JobKind.row(&json!({ "id": "jg_2", "flowId": "fl_9", "status": "Running" }));
        assert_eq!(by_id.cells[0].text, "fl_9");
        let none = JobKind.row(&json!({ "id": "jg_3" }));
        assert_eq!(none.cells[0].text, "(unknown flow)");
        assert_eq!(none.cells[1].text, "—");
        assert_eq!(none.cells[1].tone, StatusTone::Neutral);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::resource::job 2>&1 | tail -20`
Expected: FAIL — `status_tone` not found, stub columns/rows.

- [ ] **Step 3: Implement `JobKind`**

```rust
//! Job ResourceKind — maps `/v4/jobLibrary` job-group rows to display rows.
//! Fields per ayx-one-api/src/types.rs:319-349 (JobGroupSummary).
use super::{Cell, Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, StatusTone, date_only, items_array, str_field};
use serde_json::Value;

pub struct JobKind;

const COLS: &[Column] = &[
    Column { title: "FLOW", width: 32 },
    Column { title: "STATUS", width: 12 },
    Column { title: "STARTED", width: 12 },
    Column { title: "ID", width: 22 },
];

/// Map a One job-group status string to a tone. Case-insensitive.
pub(crate) fn status_tone(status: &str) -> StatusTone {
    match status.to_ascii_lowercase().as_str() {
        "succeeded" => StatusTone::Ok,
        "running" | "queued" => StatusTone::Warn,
        "failed" | "cancelled" | "canceled" => StatusTone::Danger,
        _ => StatusTone::Neutral,
    }
}

impl ResourceKind for JobKind {
    fn columns(&self) -> &'static [Column] {
        COLS
    }

    fn extract_items(&self, payload: &Value) -> Vec<Value> {
        items_array(payload)
    }

    fn row(&self, item: &Value) -> Row {
        let id = str_field(item, &["id"]).unwrap_or_default().to_string();
        let flow = str_field(item, &["flowName", "flow_name", "flowId", "flow_id"])
            .unwrap_or("(unknown flow)")
            .to_string();
        let status_text = str_field(item, &["status"]).unwrap_or("—").to_string();
        let tone = status_tone(&status_text);
        let started = str_field(item, &["startedAt", "started_at", "createdAt", "created_at"])
            .map(date_only)
            .unwrap_or_default();
        Row {
            id: id.clone(),
            cells: vec![
                Cell::plain(flow),
                Cell::toned(status_text, tone),
                Cell::plain(started),
                Cell::plain(id),
            ],
        }
    }

    fn list_endpoint(&self) -> ListEndpoint {
        ListEndpoint { surface: "jobGroup", operation: "tui-job-list", path: "/v4/jobLibrary" }
    }

    fn detail_endpoint(&self) -> Option<DetailEndpoint> {
        Some(DetailEndpoint { surface: "jobGroup", operation: "tui-job-detail", path: "/v4/jobGroups/{id}" })
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ayx-rs --lib tui::v2::resource::job 2>&1 | tail -20`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
git add ayx-rs/src/tui/v2/resource/job.rs
git commit -m "feat(tui-v2): JobKind impl with status tone mapping

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: `PersonKind` implementation (suspended/admin tone)

**Files:**
- Modify: `ayx-rs/src/tui/v2/resource/person.rs`
- Test: inline `#[cfg(test)]`

**Interfaces:**
- Produces: `PersonKind` with columns `["NAME","EMAIL","STATUS","ID"]`. Source of truth: `ayx-one-api/src/types.rs:202-222` (`PersonSummary` — `id`, `email`, `full_name`/`fullName`, `is_admin`/`isAdmin`, `is_suspended`/`isSuspended`) and `ayx-rs/src/cmd/one_platform/person.rs:27,84`.
- STATUS cell: if `is_suspended` true → text `"suspended"`, tone `Danger`; else if `is_admin` true → text `"admin"`, tone `Ok`; else → text `"active"`, tone `Neutral`.

- [ ] **Step 1: Write the failing test**

In `person.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::{ResourceKind, StatusTone};
    use serde_json::json;

    #[test]
    fn columns_are_four() {
        assert_eq!(PersonKind.columns().len(), 4);
        assert_eq!(PersonKind.columns()[0].title, "NAME");
        assert_eq!(PersonKind.columns()[1].title, "EMAIL");
    }

    #[test]
    fn row_uses_full_name_and_email() {
        let item = json!({ "id": "u_1", "fullName": "Ryan Merlin", "email": "ryan@alteryx.com" });
        let row = PersonKind.row(&item);
        assert_eq!(row.id, "u_1");
        assert_eq!(row.cells[0].text, "Ryan Merlin");
        assert_eq!(row.cells[1].text, "ryan@alteryx.com");
        assert_eq!(row.cells[2].text, "active");
        assert_eq!(row.cells[2].tone, StatusTone::Neutral);
        assert_eq!(row.cells[3].text, "u_1");
    }

    #[test]
    fn suspended_takes_priority_over_admin() {
        let row = PersonKind.row(&json!({ "id": "u_2", "isAdmin": true, "isSuspended": true }));
        assert_eq!(row.cells[2].text, "suspended");
        assert_eq!(row.cells[2].tone, StatusTone::Danger);
    }

    #[test]
    fn admin_when_not_suspended() {
        let row = PersonKind.row(&json!({ "id": "u_3", "isAdmin": true }));
        assert_eq!(row.cells[2].text, "admin");
        assert_eq!(row.cells[2].tone, StatusTone::Ok);
    }

    #[test]
    fn name_falls_back_to_email_then_placeholder() {
        let by_email = PersonKind.row(&json!({ "id": "u_4", "email": "x@y.com" }));
        assert_eq!(by_email.cells[0].text, "x@y.com");
        let none = PersonKind.row(&json!({ "id": "u_5" }));
        assert_eq!(none.cells[0].text, "(unnamed)");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::resource::person 2>&1 | tail -20`
Expected: FAIL — stub columns/rows.

- [ ] **Step 3: Implement `PersonKind`**

```rust
//! Person ResourceKind — maps `/v4/people` items to rows.
//! Fields per ayx-one-api/src/types.rs:202-222 (PersonSummary).
use super::{Cell, Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, StatusTone, items_array, str_field};
use serde_json::Value;

pub struct PersonKind;

const COLS: &[Column] = &[
    Column { title: "NAME", width: 28 },
    Column { title: "EMAIL", width: 30 },
    Column { title: "STATUS", width: 12 },
    Column { title: "ID", width: 22 },
];

fn bool_field(item: &Value, keys: &[&str]) -> bool {
    keys.iter().any(|k| item.get(*k).and_then(Value::as_bool).unwrap_or(false))
}

impl ResourceKind for PersonKind {
    fn columns(&self) -> &'static [Column] {
        COLS
    }

    fn extract_items(&self, payload: &Value) -> Vec<Value> {
        items_array(payload)
    }

    fn row(&self, item: &Value) -> Row {
        let id = str_field(item, &["id"]).unwrap_or_default().to_string();
        let email = str_field(item, &["email"]).unwrap_or_default().to_string();
        let name = str_field(item, &["fullName", "full_name"])
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| (!email.is_empty()).then(|| email.clone()))
            .unwrap_or_else(|| "(unnamed)".to_string());

        let (status_text, tone) = if bool_field(item, &["isSuspended", "is_suspended"]) {
            ("suspended", StatusTone::Danger)
        } else if bool_field(item, &["isAdmin", "is_admin"]) {
            ("admin", StatusTone::Ok)
        } else {
            ("active", StatusTone::Neutral)
        };

        Row {
            id: id.clone(),
            cells: vec![
                Cell::plain(name),
                Cell::plain(email),
                Cell::toned(status_text, tone),
                Cell::plain(id),
            ],
        }
    }

    fn list_endpoint(&self) -> ListEndpoint {
        ListEndpoint { surface: "platform", operation: "tui-person-list", path: "/v4/people" }
    }

    fn detail_endpoint(&self) -> Option<DetailEndpoint> {
        Some(DetailEndpoint { surface: "platform", operation: "tui-person-detail", path: "/v4/people/{id}" })
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ayx-rs --lib tui::v2::resource::person 2>&1 | tail -20`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
git add ayx-rs/src/tui/v2/resource/person.rs
git commit -m "feat(tui-v2): PersonKind impl with suspended/admin status tone

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: `WorkspaceKind` implementation (read-only, no detail endpoint)

**Files:**
- Modify: `ayx-rs/src/tui/v2/resource/workspace.rs`
- Test: inline `#[cfg(test)]`

**Interfaces:**
- Produces: `WorkspaceKind` with columns `["NAME","OWNER","STATUS","ID"]`. Source of truth: `ayx-one-api/src/types.rs:256-281` (`WorkspaceSummary` — `id`/`workspaceId`, `name`/`workspaceName`, `owner_email`/`ownerEmail`, `status`) and `ayx-rs/src/cmd/one_platform/workspace.rs:74`. `detail_endpoint()` returns `None` (only `/v4/workspaces/current` is wired; per-id detail and switching are later phases).
- STATUS cell tone: case-insensitive — `active`/`ready` → `Ok`; `suspended`/`disabled` → `Danger`; else `Neutral`. Always shows the word.

- [ ] **Step 1: Write the failing test**

In `workspace.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::{ResourceKind, StatusTone};
    use serde_json::json;

    #[test]
    fn columns_are_four() {
        assert_eq!(WorkspaceKind.columns().len(), 4);
        assert_eq!(WorkspaceKind.columns()[0].title, "NAME");
    }

    #[test]
    fn row_prefers_id_then_workspace_id() {
        let item = json!({
            "workspaceId": "w_1", "workspaceName": "Marketing",
            "ownerEmail": "ops@alteryx.com", "status": "active"
        });
        let row = WorkspaceKind.row(&item);
        assert_eq!(row.id, "w_1");
        assert_eq!(row.cells[0].text, "Marketing");
        assert_eq!(row.cells[1].text, "ops@alteryx.com");
        assert_eq!(row.cells[2].text, "active");
        assert_eq!(row.cells[2].tone, StatusTone::Ok);
        assert_eq!(row.cells[3].text, "w_1");
    }

    #[test]
    fn no_detail_endpoint() {
        assert!(WorkspaceKind.detail_endpoint().is_none());
        assert_eq!(WorkspaceKind.list_endpoint().path, "/v4/workspaces");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::resource::workspace 2>&1 | tail -20`
Expected: FAIL — stub columns/rows.

- [ ] **Step 3: Implement `WorkspaceKind`**

```rust
//! Workspace ResourceKind — maps `/v4/workspaces` items to rows (read-only
//! browse). No per-id detail endpoint is wired; workspace *switching* is a
//! later phase. Fields per ayx-one-api/src/types.rs:256-281 (WorkspaceSummary).
use super::{Cell, Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, StatusTone, items_array, str_field};
use serde_json::Value;

pub struct WorkspaceKind;

const COLS: &[Column] = &[
    Column { title: "NAME", width: 30 },
    Column { title: "OWNER", width: 28 },
    Column { title: "STATUS", width: 12 },
    Column { title: "ID", width: 22 },
];

fn status_tone(status: &str) -> StatusTone {
    match status.to_ascii_lowercase().as_str() {
        "active" | "ready" => StatusTone::Ok,
        "suspended" | "disabled" => StatusTone::Danger,
        _ => StatusTone::Neutral,
    }
}

impl ResourceKind for WorkspaceKind {
    fn columns(&self) -> &'static [Column] {
        COLS
    }

    fn extract_items(&self, payload: &Value) -> Vec<Value> {
        items_array(payload)
    }

    fn row(&self, item: &Value) -> Row {
        let id = str_field(item, &["id", "workspaceId", "workspace_id"]).unwrap_or_default().to_string();
        let name = str_field(item, &["name", "workspaceName", "workspace_name"])
            .unwrap_or("(unnamed)")
            .to_string();
        let owner = str_field(item, &["ownerEmail", "owner_email"]).unwrap_or_default().to_string();
        let status_text = str_field(item, &["status"]).unwrap_or("—").to_string();
        let tone = status_tone(&status_text);
        Row {
            id: id.clone(),
            cells: vec![Cell::plain(name), Cell::plain(owner), Cell::toned(status_text, tone), Cell::plain(id)],
        }
    }

    fn list_endpoint(&self) -> ListEndpoint {
        ListEndpoint { surface: "platform", operation: "tui-workspace-list", path: "/v4/workspaces" }
    }

    fn detail_endpoint(&self) -> Option<DetailEndpoint> {
        None
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ayx-rs --lib tui::v2::resource::workspace 2>&1 | tail -20`
Expected: PASS (3 tests).

- [ ] **Step 5: Full resource-layer check + commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo test -p ayx-rs --lib tui::v2::resource 2>&1 | tail -20   # all five impls green
git add ayx-rs/src/tui/v2/resource/workspace.rs
git commit -m "feat(tui-v2): WorkspaceKind impl (read-only, no detail endpoint)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Generation-token staleness model

Replace the entry-loop `RequestId` matching (the Phase-0 `dispatch_effects` `debug_assert!(effects.len() <= 1)` follow-up) with a monotonic token held in state. The reducer mints a token whenever it issues a fetch and records it on the target view; results carry their token and the reducer applies them only if the token still matches. This makes staleness **pure and unit-testable** and lets `update()` emit fetch effects freely. Flows must still browse end-to-end.

**Files:**
- Modify: `ayx-rs/src/tui/v2/effect.rs`, `state.rs`, `action.rs`, `worker.rs`, `entry.rs`
- Test: inline `#[cfg(test)]` in `action.rs` and `worker.rs`

**Interfaces:**
- `effect.rs`: `enum Effect { FetchList { kind: Kind, token: u64 } }` (FetchDetail added in Task 9).
- `state.rs`: `ListView` gains `pub token: u64` (default 0); `AppState` gains `pub req_seq: u64` (default 0). `AppState::new` sets `req_seq: 0` and `list.token: 0`.
- `action.rs`:
  - `ListLoaded { token: u64, rows: Vec<Row> }`, `ListFailed { token: u64, error: String }` (drop the `kind` field — token uniquely identifies the fetch).
  - `pub(crate) fn mint_token(state: &mut AppState) -> u64` (increments `req_seq`, returns it).
  - `initial_load_effect(state: &mut AppState) -> Effect` now takes `&mut` and mints the token (sets `state.list.token`).
- `worker.rs`: `Worker::submit(effect, config)` (no id); `Outcome { action }` (no id); remove `next_request_id`/`RequestId` public API; `list_payload_to_action(kind, token, payload)`.
- `entry.rs`: drop `list_request`; loop applies every outcome via `update`.

- [ ] **Step 1: Implement `effect.rs`**

```rust
//! Effects: side-effect requests emitted by `update`, executed by the worker.
//! Each fetch carries a monotonic `token`; the reducer drops results whose
//! token no longer matches the target view (stale-result protection).
use crate::tui::v2::resource::Kind;

#[derive(Debug, Clone)]
pub enum Effect {
    FetchList { kind: Kind, token: u64 },
}
```

- [ ] **Step 2: Update `state.rs`**

In `ListView`, add the field and initialize it in `new`:

```rust
#[derive(Debug, Clone)]
pub struct ListView {
    pub kind: Kind,
    pub rows: Vec<Row>,
    pub cursor: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub token: u64,
}

impl ListView {
    pub fn new(kind: Kind) -> Self {
        Self { kind, rows: Vec::new(), cursor: 0, loading: true, error: None, token: 0 }
    }
    // select_down / select_up / selected unchanged (Task 11 revises them for filtering)
```

In `AppState`, add `req_seq` and initialize:

```rust
#[derive(Debug, Clone)]
pub struct AppState {
    pub context: Context,
    pub nav: NavStack,
    pub list: ListView,
    pub should_quit: bool,
    pub req_seq: u64,
}

impl AppState {
    pub fn new(context: Context) -> Self {
        Self {
            context,
            nav: NavStack::new(View::ResourceList { kind: Kind::Flow }),
            list: ListView::new(Kind::Flow),
            should_quit: false,
            req_seq: 0,
        }
    }
}
```

- [ ] **Step 3: Update the reducer + token minting in `action.rs`**

Replace the `Action` enum's load variants and the reducer arms. New `Action` (CursorDown/CursorUp/Back/Quit unchanged for now):

```rust
#[derive(Debug, Clone)]
pub enum Action {
    CursorDown,
    CursorUp,
    Back,
    Quit,
    ListLoaded { token: u64, rows: Vec<Row> },
    ListFailed { token: u64, error: String },
}
```

Token minting + the new load arms:

```rust
/// Mint the next monotonic request token.
pub(crate) fn mint_token(state: &mut AppState) -> u64 {
    state.req_seq += 1;
    state.req_seq
}
```

```rust
        Action::ListLoaded { token, rows } => {
            if token == state.list.token {
                state.list.rows = rows;
                state.list.loading = false;
                state.list.error = None;
                if state.list.cursor >= state.list.rows.len() {
                    state.list.cursor = state.list.rows.len().saturating_sub(1);
                }
            }
            Vec::new()
        }
        Action::ListFailed { token, error } => {
            if token == state.list.token {
                state.list.loading = false;
                state.list.error = Some(error);
            }
            Vec::new()
        }
```

Replace `initial_load_effect` to mint the token:

```rust
/// Effect to fetch the current list view's data; mints + records a fresh token.
pub fn initial_load_effect(state: &mut AppState) -> Effect {
    let token = mint_token(state);
    state.list.token = token;
    state.list.loading = true;
    Effect::FetchList { kind: state.list.kind, token }
}
```

Update the existing `action.rs` tests: `ListLoaded`/`ListFailed` now take `token` not `kind`. Replace the affected tests with token-aware versions:

```rust
    #[test]
    fn list_loaded_with_matching_token_applies() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s); // sets list.token = 1
        let tok = s.list.token;
        let effects = update(&mut s, Action::ListLoaded { token: tok, rows: rows(3) });
        assert!(!s.list.loading);
        assert_eq!(s.list.rows.len(), 3);
        assert!(effects.is_empty());
    }

    #[test]
    fn list_loaded_with_stale_token_is_dropped() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s); // token = 1
        update(&mut s, Action::ListLoaded { token: 999, rows: rows(3) });
        assert!(s.list.loading, "stale result must not clear loading");
        assert_eq!(s.list.rows.len(), 0);
    }

    #[test]
    fn list_failed_with_matching_token_sets_error() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let tok = s.list.token;
        update(&mut s, Action::ListFailed { token: tok, error: "boom".into() });
        assert!(!s.list.loading);
        assert_eq!(s.list.error.as_deref(), Some("boom"));
    }
```

Keep `cursor_moves_within_bounds`, `quit_sets_flag`, `back_at_root_quits` but update any `ListLoaded { kind, .. }` usages inside them to `ListLoaded { token: { let t = mint_token(&mut s); s.list.token = t; t }, .. }` — simplest: call `initial_load_effect(&mut s)` first then use `s.list.token`. For `cursor_moves_within_bounds`:

```rust
    #[test]
    fn cursor_moves_within_bounds() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let tok = s.list.token;
        update(&mut s, Action::ListLoaded { token: tok, rows: rows(2) });
        update(&mut s, Action::CursorDown);
        assert_eq!(s.list.cursor, 1);
        update(&mut s, Action::CursorDown);
        assert_eq!(s.list.cursor, 1);
        update(&mut s, Action::CursorUp);
        assert_eq!(s.list.cursor, 0);
    }
```

Delete `cursor_clamps_when_rows_shrink` or rewrite it with two `initial_load_effect` calls (each mints a new token); rewrite:

```rust
    #[test]
    fn cursor_clamps_when_rows_shrink() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let t1 = s.list.token;
        update(&mut s, Action::ListLoaded { token: t1, rows: rows(3) });
        update(&mut s, Action::CursorDown);
        update(&mut s, Action::CursorDown);
        assert_eq!(s.list.cursor, 2);
        let _ = initial_load_effect(&mut s);
        let t2 = s.list.token;
        update(&mut s, Action::ListLoaded { token: t2, rows: rows(1) });
        assert_eq!(s.list.cursor, 0);
        assert_eq!(s.list.rows.len(), 1);
    }
```

- [ ] **Step 4: Update `worker.rs` (drop RequestId, thread token, simplify Outcome)**

Replace the worker's id machinery and the list mapper:

```rust
//! v2 worker: a single background thread that runs Effects off the UI thread
//! and returns Actions. Staleness is handled by the reducer via tokens carried
//! on each Action, so the worker no longer tracks request ids.
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::{self, JoinHandle};

use ayx_core::profile::Config;
use serde_json::Value;

use crate::tui::v2::action::Action;
use crate::tui::v2::effect::Effect;
use crate::tui::v2::resource::{Kind, Row, kind_impl};

struct Job {
    effect: Effect,
    config: Config,
}

pub struct Outcome {
    pub action: Action,
}

pub struct Worker {
    tx: Sender<Job>,
    rx: Receiver<Outcome>,
    _handle: JoinHandle<()>,
}

impl Worker {
    pub fn spawn() -> Self {
        let (job_tx, job_rx) = channel::<Job>();
        let (out_tx, out_rx) = channel::<Outcome>();
        let handle = thread::Builder::new()
            .name("ayx-tui-v2-worker".into())
            .spawn(move || worker_loop(job_rx, out_tx))
            .expect("v2 worker thread should spawn");
        Self { tx: job_tx, rx: out_rx, _handle: handle }
    }

    pub fn submit(&self, effect: Effect, config: Config) {
        let _ = self.tx.send(Job { effect, config });
    }

    pub fn try_recv(&self) -> Result<Outcome, TryRecvError> {
        self.rx.try_recv()
    }
}

fn worker_loop(rx: Receiver<Job>, tx: Sender<Outcome>) {
    while let Ok(job) = rx.recv() {
        let action = match job.effect {
            Effect::FetchList { kind, token } => {
                let endpoint = kind_impl(kind).list_endpoint();
                let payload = crate::one_api_live_request(
                    &job.config,
                    endpoint.surface,
                    endpoint.operation,
                    "GET",
                    endpoint.path,
                    false,
                    &[],
                )
                .map(|env| env.data)
                .map_err(|e| e.to_string());
                list_payload_to_action(kind, token, payload)
            }
        };
        let _ = tx.send(Outcome { action });
    }
}

/// Pure mapping from a raw list payload (or error) to an Action. Unit-tested.
pub fn list_payload_to_action(kind: Kind, token: u64, payload: Result<Value, String>) -> Action {
    match payload {
        Ok(value) => {
            let imp = kind_impl(kind);
            let rows: Vec<Row> = imp.extract_items(&value).iter().map(|i| imp.row(i)).collect();
            Action::ListLoaded { token, rows }
        }
        Err(error) => Action::ListFailed { token, error },
    }
}
```

Update the worker tests to pass a token and match on it:

```rust
    #[test]
    fn ok_payload_maps_to_list_loaded_with_rows() {
        let payload = Ok(json!({ "data": [ { "id": "fl_1", "name": "ETL" }, { "id": "fl_2", "name": "Roll" } ] }));
        match list_payload_to_action(Kind::Flow, 7, payload) {
            Action::ListLoaded { token, rows } => {
                assert_eq!(token, 7);
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].cells[0].text, "ETL");
            }
            other => panic!("expected ListLoaded, got {other:?}"),
        }
    }

    #[test]
    fn err_payload_maps_to_list_failed() {
        match list_payload_to_action(Kind::Flow, 7, Err("401 unauthorized".into())) {
            Action::ListFailed { token, error } => {
                assert_eq!(token, 7);
                assert!(error.contains("401"));
            }
            other => panic!("expected ListFailed, got {other:?}"),
        }
    }
```

- [ ] **Step 5: Update `entry.rs` (drop `list_request`, simplify dispatch)**

The loop no longer filters by id; the reducer drops stale results. Replace the relevant parts of `entry.rs`:

```rust
use crate::tui::v2::action::{Action, initial_load_effect, update};
// ...
    let mut state = AppState::new(context);
    let worker = Worker::spawn();

    // Kick the initial flow-list fetch (mints + records the token on state).
    let first = initial_load_effect(&mut state);
    worker.submit(first, config.clone());
```

Remove the `let mut list_request = 0;` line and all `&mut list_request` arguments. `main_loop` signature drops `list_request`:

```rust
fn main_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
    worker: &Worker,
    config: &Config,
) -> Result<()> {
    loop {
        while let Ok(outcome) = worker.try_recv() {
            let effects = update(state, outcome.action);
            dispatch_effects(effects, worker, config);
        }

        terminal.draw(|frame| view::render(frame, state))?;

        if event::poll(Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(action) = map_key(key)
        {
            let effects = update(state, action);
            dispatch_effects(effects, worker, config);
        }

        if state.should_quit {
            break;
        }
    }
    Ok(())
}

fn dispatch_effects(effects: Vec<Effect>, worker: &Worker, config: &Config) {
    for effect in effects {
        worker.submit(effect, config.clone());
    }
}
```

Update the `run()` call site: `let result = main_loop(&mut terminal, &mut state, &worker, &config);`. Remove the `RequestId` import (`use crate::tui::v2::worker::Worker;`). `map_key` stays `map_key(key)` until Task 10 makes it context-sensitive.

- [ ] **Step 6: Run the affected tests + build**

Run: `cargo test -p ayx-rs --lib tui::v2 2>&1 | tail -30`
Expected: PASS — action, worker, entry, view tests all green under the new token model.

Run: `cargo build -p ayx-rs 2>&1 | tail -20`
Expected: compiles clean (no dead `RequestId`).

- [ ] **Step 7: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/effect.rs ayx-rs/src/tui/v2/state.rs ayx-rs/src/tui/v2/action.rs ayx-rs/src/tui/v2/worker.rs ayx-rs/src/tui/v2/entry.rs
git commit -m "refactor(tui-v2): generation-token staleness in the reducer (drop entry-loop RequestId)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 8: Resource switching — `SwitchKind` action + tab strip + keys

Let the user change which asset is browsed: number keys `1`–`5`, `Tab`/`BackTab` to cycle, and a resource-tab strip rendered below the context header. Switching resets the list to that kind and fires a fresh fetch (new token).

**Files:**
- Modify: `ayx-rs/src/tui/v2/action.rs` (add `SwitchKind`)
- Modify: `ayx-rs/src/tui/v2/view/header.rs` (tab strip)
- Modify: `ayx-rs/src/tui/v2/entry.rs` (`map_key` for `1`–`5` / Tab)
- Test: inline `#[cfg(test)]` in `action.rs` and `view/header.rs`

**Interfaces:**
- Consumes: `Kind`, `mint_token`, `Effect::FetchList`.
- Produces: `Action::SwitchKind(Kind)`. Reducer: if already on that kind at the list root, no-op (returns `vec![]`); else reset `nav` to `ResourceList { kind }`, replace `list` with a fresh `ListView::new(kind)`, mint+record a token, return `vec![Effect::FetchList { kind, token }]`.
- `Kind::index(self) -> usize` and `Kind::from_index(usize) -> Option<Kind>` helper for tab/number mapping (add to `resource/mod.rs`).

- [ ] **Step 1: Add `Kind::index` / `from_index` (failing test first)**

In `resource/mod.rs` tests:

```rust
    #[test]
    fn kind_index_roundtrip() {
        for (i, &k) in Kind::all().iter().enumerate() {
            assert_eq!(k.index(), i);
            assert_eq!(Kind::from_index(i), Some(k));
        }
        assert_eq!(Kind::from_index(99), None);
    }
```

Implement in `impl Kind`:

```rust
    pub fn index(self) -> usize {
        Kind::all().iter().position(|&k| k == self).expect("kind is in all()")
    }
    pub fn from_index(i: usize) -> Option<Kind> {
        Kind::all().get(i).copied()
    }
```

Run: `cargo test -p ayx-rs --lib tui::v2::resource 2>&1 | tail -20` → PASS.

- [ ] **Step 2: Write the failing reducer test for `SwitchKind`**

In `action.rs` tests:

```rust
    #[test]
    fn switch_kind_resets_list_and_emits_fetch() {
        use crate::tui::v2::resource::Kind;
        let mut s = test_state(); // starts on Flow
        let effects = update(&mut s, Action::SwitchKind(Kind::Job));
        assert_eq!(s.list.kind, Kind::Job);
        assert!(s.list.loading);
        assert!(s.list.rows.is_empty());
        assert!(matches!(s.nav.top(), crate::tui::v2::nav::View::ResourceList { kind: Kind::Job }));
        match effects.as_slice() {
            [Effect::FetchList { kind: Kind::Job, token }] => assert_eq!(*token, s.list.token),
            other => panic!("expected one FetchList(Job), got {other:?}"),
        }
    }

    #[test]
    fn switch_to_current_kind_is_noop() {
        use crate::tui::v2::resource::Kind;
        let mut s = test_state(); // Flow
        let effects = update(&mut s, Action::SwitchKind(Kind::Flow));
        assert!(effects.is_empty());
    }
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::action 2>&1 | tail -20`
Expected: FAIL — `Action::SwitchKind` not found.

- [ ] **Step 4: Implement `SwitchKind`**

Add the variant to `Action`:

```rust
    SwitchKind(Kind),
```

Add the reducer arm (uses `ListView::new` + `NavStack::new` + `View`):

```rust
        Action::SwitchKind(kind) => {
            // No-op if already browsing that kind at the list root.
            if state.list.kind == kind
                && matches!(state.nav.top(), crate::tui::v2::nav::View::ResourceList { .. })
            {
                return Vec::new();
            }
            state.nav = crate::tui::v2::nav::NavStack::new(
                crate::tui::v2::nav::View::ResourceList { kind },
            );
            state.list = crate::tui::v2::state::ListView::new(kind);
            let token = mint_token(state);
            state.list.token = token;
            vec![Effect::FetchList { kind, token }]
        }
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p ayx-rs --lib tui::v2::action 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 6: Render the tab strip in `view/header.rs`**

The header currently renders two lines: context (line 0) + breadcrumb (line 1). Change line 1 to a **resource-tab strip** when the top view is a list, and keep the breadcrumb when drilled into detail. Replace `header.rs`:

```rust
//! Context header + resource tabs / breadcrumb. Always visible — the guard
//! against acting in the wrong workspace.
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::theme;
use crate::tui::v2::nav::View;
use crate::tui::v2::resource::Kind;
use crate::tui::v2::state::AppState;

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);

    let ctx = &state.context;
    let header = Line::from(vec![
        Span::styled(" Profile: ", theme::muted()),
        Span::styled(ctx.profile.clone(), theme::accent_bold()),
        Span::styled("  ·  Workspace: ", theme::muted()),
        Span::styled(ctx.workspace.clone(), theme::accent_bold()),
        Span::styled("  ·  ", theme::muted()),
        Span::styled(ctx.user.clone(), theme::dim()),
    ]);
    frame.render_widget(Paragraph::new(header).style(theme::panel()), rows[0]);

    // List view → tabs; detail view → breadcrumb.
    match state.nav.top() {
        View::ResourceList { .. } => frame.render_widget(Paragraph::new(tabs_line(state.list.kind)), rows[1]),
        View::ResourceDetail { .. } => {
            let crumb = Line::from(vec![
                Span::styled(" ", theme::dim()),
                Span::styled(state.nav.breadcrumb(), theme::dim()),
            ]);
            frame.render_widget(Paragraph::new(crumb), rows[1]);
        }
    }
}

/// `[1 flows] [2 connections] [3 jobs] [4 people] [5 workspaces]` with the
/// active kind highlighted.
fn tabs_line(active: Kind) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for &k in Kind::all() {
        let label = format!(" {} {} ", k.index() + 1, k.name());
        let style = if k == active { theme::selected() } else { theme::dim() };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}
```

- [ ] **Step 7: Write a header test (tabs render + highlight)**

In `view/header.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::context::Context;
    use crate::tui::v2::resource::Kind;
    use crate::tui::v2::state::AppState;
    use ratatui::{Terminal, backend::TestBackend};

    fn text_for(state: &AppState) -> String {
        let backend = TestBackend::new(120, 2);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, state, f.area())).unwrap();
        terminal.backend().buffer().clone().content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn tabs_show_all_kinds_on_list_view() {
        let ctx = Context { profile: "wyatt".into(), workspace: "w".into(), user: "u".into() };
        let s = AppState::new(ctx); // root = flows list
        let txt = text_for(&s);
        assert!(txt.contains("flows"));
        assert!(txt.contains("connections"));
        assert!(txt.contains("jobs"));
        assert!(txt.contains("people"));
        assert!(txt.contains("workspaces"));
        let _ = Kind::Flow; // active highlight is style-only; content asserted above
    }
}
```

- [ ] **Step 8: Map keys in `entry.rs`**

Extend `map_key` (still `map_key(key)` here; Task 10 adds the state param):

```rust
fn map_key(key: KeyEvent) -> Option<Action> {
    use crate::tui::v2::resource::Kind;
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => Some(Action::CursorDown),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::CursorUp),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char(c @ '1'..='5') => {
            Kind::from_index((c as u8 - b'1') as usize).map(Action::SwitchKind)
        }
        KeyCode::Tab => Some(Action::SwitchKind(
            Kind::from_index((/*current*/ 0 + 1) % Kind::all().len()).unwrap(),
        )),
        _ => None,
    }
}
```

Note: `Tab` needs the *current* kind to cycle, which `map_key(key)` cannot see yet. Defer Tab/BackTab cycling to Task 10 (where `map_key` gains `&AppState`); for this task implement only `1`–`5` number switching and drop the `Tab` arm above. Final `map_key` for Task 8:

```rust
fn map_key(key: KeyEvent) -> Option<Action> {
    use crate::tui::v2::resource::Kind;
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => Some(Action::CursorDown),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::CursorUp),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char(c @ '1'..='5') => {
            Kind::from_index((c as u8 - b'1') as usize).map(Action::SwitchKind)
        }
        _ => None,
    }
}
```

Add an entry test:

```rust
    #[test]
    fn number_keys_switch_kind() {
        use crate::tui::v2::resource::Kind;
        assert!(matches!(map_key(k(KeyCode::Char('1'))), Some(Action::SwitchKind(Kind::Flow))));
        assert!(matches!(map_key(k(KeyCode::Char('3'))), Some(Action::SwitchKind(Kind::Job))));
        assert!(matches!(map_key(k(KeyCode::Char('5'))), Some(Action::SwitchKind(Kind::Workspace))));
        assert!(map_key(k(KeyCode::Char('6'))).is_none());
    }
```

- [ ] **Step 9: Run + commit**

Run: `cargo test -p ayx-rs --lib tui::v2 2>&1 | tail -30` → PASS.

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/resource/mod.rs ayx-rs/src/tui/v2/action.rs ayx-rs/src/tui/v2/view/header.rs ayx-rs/src/tui/v2/entry.rs
git commit -m "feat(tui-v2): resource switching — SwitchKind, tab strip, number keys

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 9: Drill-down — `Open`/`Back`, `DetailView` state, `FetchDetail` effect + worker

Wire `Enter` to drill into the selected row's full detail via the async worker (no render-thread freeze), and `Esc`/`Enter` to come back. State + reducer + effect + worker only; the scrollable *render* is Task 10.

**Files:**
- Modify: `ayx-rs/src/tui/v2/state.rs` (`DetailView`, `AppState.detail`)
- Modify: `ayx-rs/src/tui/v2/effect.rs` (`FetchDetail`)
- Modify: `ayx-rs/src/tui/v2/action.rs` (`Open`, `DetailLoaded`, `DetailFailed`; revise `Back`)
- Modify: `ayx-rs/src/tui/v2/worker.rs` (`FetchDetail` arm + `detail_payload_to_action`)
- Test: inline `#[cfg(test)]` in `action.rs`, `worker.rs`

**Interfaces:**
- `state.rs`:
  - `struct DetailView { kind: Kind, id: String, title: String, loading: bool, json: Option<serde_json::Value>, error: Option<String>, scroll: u16, token: u64 }` + `DetailView::new(kind, id, title, token) -> Self` (loading=true, scroll=0).
  - `AppState` gains `pub detail: Option<DetailView>` (default `None` in `new`).
- `effect.rs`: `Effect::FetchDetail { kind: Kind, id: String, token: u64 }`.
- `action.rs`:
  - `Action::Open` — drill into the selected list row (no-op if the kind has no `detail_endpoint`).
  - `Action::DetailLoaded { token: u64, json: serde_json::Value }`, `Action::DetailFailed { token: u64, error: String }`.
  - `Back` now: if `state.detail` is some, clear it and `nav.pop()`; returns `vec![]`.
- `worker.rs`: `detail_payload_to_action(token, payload) -> Action`; `FetchDetail` arm interpolates `&[("id", id)]`.

- [ ] **Step 1: Update `state.rs`**

Add `DetailView` and the `AppState.detail` field:

```rust
use serde_json::Value;
// ... existing imports

#[derive(Debug, Clone)]
pub struct DetailView {
    pub kind: Kind,
    pub id: String,
    pub title: String,
    pub loading: bool,
    pub json: Option<Value>,
    pub error: Option<String>,
    pub scroll: u16,
    pub token: u64,
}

impl DetailView {
    pub fn new(kind: Kind, id: String, title: String, token: u64) -> Self {
        Self { kind, id, title, loading: true, json: None, error: None, scroll: 0, token }
    }
    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }
    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }
}
```

In `AppState`, add `pub detail: Option<DetailView>` and set `detail: None` in `new`.

- [ ] **Step 2: Update `effect.rs`**

```rust
#[derive(Debug, Clone)]
pub enum Effect {
    FetchList { kind: Kind, token: u64 },
    FetchDetail { kind: Kind, id: String, token: u64 },
}
```

- [ ] **Step 3: Write the failing reducer tests**

In `action.rs` tests:

```rust
    #[test]
    fn open_drills_into_selected_row_and_emits_fetch_detail() {
        use crate::tui::v2::resource::Kind;
        let mut s = test_state(); // Flow has a detail endpoint
        let _ = initial_load_effect(&mut s);
        let tok = s.list.token;
        update(&mut s, Action::ListLoaded { token: tok, rows: rows(2) });
        let effects = update(&mut s, Action::Open);
        let d = s.detail.as_ref().expect("detail view created");
        assert!(d.loading);
        assert_eq!(d.id, "fl_0");
        assert!(matches!(s.nav.top(), crate::tui::v2::nav::View::ResourceDetail { .. }));
        match effects.as_slice() {
            [Effect::FetchDetail { kind: Kind::Flow, id, token }] => {
                assert_eq!(id, "fl_0");
                assert_eq!(*token, d.token);
            }
            other => panic!("expected FetchDetail, got {other:?}"),
        }
    }

    #[test]
    fn open_on_kind_without_detail_is_noop() {
        use crate::tui::v2::resource::Kind;
        let mut s = test_state();
        update(&mut s, Action::SwitchKind(Kind::Workspace)); // no detail endpoint
        let tok = s.list.token;
        update(&mut s, Action::ListLoaded { token: tok, rows: rows(1) });
        let effects = update(&mut s, Action::Open);
        assert!(s.detail.is_none());
        assert!(effects.is_empty());
    }

    #[test]
    fn back_clears_detail_and_pops() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let tok = s.list.token;
        update(&mut s, Action::ListLoaded { token: tok, rows: rows(1) });
        update(&mut s, Action::Open);
        assert!(s.detail.is_some());
        update(&mut s, Action::Back);
        assert!(s.detail.is_none());
        assert!(matches!(s.nav.top(), crate::tui::v2::nav::View::ResourceList { .. }));
    }

    #[test]
    fn detail_loaded_with_matching_token_applies() {
        use serde_json::json;
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let lt = s.list.token;
        update(&mut s, Action::ListLoaded { token: lt, rows: rows(1) });
        update(&mut s, Action::Open);
        let dt = s.detail.as_ref().unwrap().token;
        update(&mut s, Action::DetailLoaded { token: dt, json: json!({ "id": "fl_0" }) });
        let d = s.detail.as_ref().unwrap();
        assert!(!d.loading);
        assert!(d.json.is_some());
    }

    #[test]
    fn detail_loaded_with_stale_token_is_dropped() {
        use serde_json::json;
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let lt = s.list.token;
        update(&mut s, Action::ListLoaded { token: lt, rows: rows(1) });
        update(&mut s, Action::Open);
        update(&mut s, Action::DetailLoaded { token: 9999, json: json!({}) });
        assert!(s.detail.as_ref().unwrap().loading, "stale detail must not clear loading");
    }
```

- [ ] **Step 4: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::action 2>&1 | tail -20`
Expected: FAIL — `Action::Open` / `DetailLoaded` not found.

- [ ] **Step 5: Implement the reducer arms**

Add variants to `Action`:

```rust
    Open,
    DetailLoaded { token: u64, json: serde_json::Value },
    DetailFailed { token: u64, error: String },
```

Replace the `Back` arm and add the new arms (uses `kind_impl` to gate on `detail_endpoint`):

```rust
        Action::Open => {
            use crate::tui::v2::nav::View;
            use crate::tui::v2::resource::kind_impl;
            let kind = state.list.kind;
            if kind_impl(kind).detail_endpoint().is_none() {
                return Vec::new();
            }
            let Some(row) = state.list.selected() else { return Vec::new() };
            let id = row.id.clone();
            if id.is_empty() {
                return Vec::new();
            }
            let title = row.cells.first().map(|c| c.text.clone()).unwrap_or_else(|| id.clone());
            state.nav.push(View::ResourceDetail { kind, id: id.clone(), title: title.clone() });
            let token = mint_token(state);
            state.detail = Some(crate::tui::v2::state::DetailView::new(kind, id.clone(), title, token));
            vec![Effect::FetchDetail { kind, id, token }]
        }
        Action::DetailLoaded { token, json } => {
            if let Some(d) = state.detail.as_mut()
                && d.token == token
            {
                d.json = Some(json);
                d.loading = false;
                d.error = None;
            }
            Vec::new()
        }
        Action::DetailFailed { token, error } => {
            if let Some(d) = state.detail.as_mut()
                && d.token == token
            {
                d.loading = false;
                d.error = Some(error);
            }
            Vec::new()
        }
```

Replace the existing `Back` arm:

```rust
        Action::Back => {
            if state.detail.is_some() {
                state.detail = None;
                let _ = state.nav.pop();
            }
            Vec::new()
        }
```

Note: `selected()` borrows `state.list` immutably while we later call `mint_token(state)` (mutable). Resolve the borrow by cloning `id`/`title` out of the row *before* `mint_token` (the code above already clones into locals and ends the borrow before `mint_token` is called — keep that ordering).

- [ ] **Step 6: Implement the worker `FetchDetail` arm + mapper**

In `worker.rs`, add to the `match job.effect`:

```rust
            Effect::FetchDetail { kind, id, token } => match kind_impl(kind).detail_endpoint() {
                Some(ep) => {
                    let payload = crate::one_api_live_request(
                        &job.config,
                        ep.surface,
                        ep.operation,
                        "GET",
                        ep.path,
                        false,
                        &[("id", id.as_str())],
                    )
                    .map(|env| env.data)
                    .map_err(|e| e.to_string());
                    detail_payload_to_action(token, payload)
                }
                None => Action::DetailFailed { token, error: "no detail endpoint for this kind".into() },
            },
```

Add the mapper next to `list_payload_to_action`:

```rust
/// Pure mapping from a raw detail payload (or error) to an Action.
pub fn detail_payload_to_action(token: u64, payload: Result<Value, String>) -> Action {
    match payload {
        Ok(json) => Action::DetailLoaded { token, json },
        Err(error) => Action::DetailFailed { token, error },
    }
}
```

Add worker tests:

```rust
    #[test]
    fn detail_ok_maps_to_detail_loaded() {
        use serde_json::json;
        match detail_payload_to_action(3, Ok(json!({ "id": "x" }))) {
            Action::DetailLoaded { token, json } => {
                assert_eq!(token, 3);
                assert_eq!(json["id"], "x");
            }
            other => panic!("expected DetailLoaded, got {other:?}"),
        }
    }

    #[test]
    fn detail_err_maps_to_detail_failed() {
        match detail_payload_to_action(3, Err("404".into())) {
            Action::DetailFailed { token, error } => {
                assert_eq!(token, 3);
                assert!(error.contains("404"));
            }
            other => panic!("expected DetailFailed, got {other:?}"),
        }
    }
```

- [ ] **Step 7: Run to verify it passes + build**

Run: `cargo test -p ayx-rs --lib tui::v2 2>&1 | tail -30` → PASS.
Run: `cargo build -p ayx-rs 2>&1 | tail -20` → compiles.

- [ ] **Step 8: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/state.rs ayx-rs/src/tui/v2/effect.rs ayx-rs/src/tui/v2/action.rs ayx-rs/src/tui/v2/worker.rs
git commit -m "feat(tui-v2): async drill-down — Open/Back, DetailView, FetchDetail

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 10: Scrollable detail view + context-sensitive keys/render

Render the fetched detail object as a scrollable key/value panel (killing the legacy 18-line truncation), dispatch the body on `nav.top()`, and make `map_key` + the cursor/scroll actions context-sensitive (list vs. detail).

**Files:**
- Create: `ayx-rs/src/tui/v2/view/detail.rs`
- Modify: `ayx-rs/src/tui/v2/view/mod.rs` (dispatch on `nav.top()`)
- Modify: `ayx-rs/src/tui/v2/action.rs` (`CursorDown`/`CursorUp` scroll the detail when it's open)
- Modify: `ayx-rs/src/tui/v2/entry.rs` (`map_key(state, key)`; `Enter`→`Open` on list / `Back` on detail; Tab/BackTab cycle)
- Test: inline `#[cfg(test)]` in `view/detail.rs` and `entry.rs`

**Interfaces:**
- `view/detail.rs`: `pub fn render(frame: &mut Frame, state: &AppState, area: Rect)` — reads `state.detail`.
- `view/mod.rs`: body dispatches `match state.nav.top() { ResourceList => list::render, ResourceDetail => detail::render }`.
- `action.rs`: `CursorDown`/`CursorUp` check `state.detail`: if `Some`, scroll it; else move the list cursor.
- `entry.rs`: `map_key(state: &AppState, key) -> Option<Action>`.

- [ ] **Step 1: Make `CursorDown`/`CursorUp` context-sensitive (failing test)**

In `action.rs` tests:

```rust
    #[test]
    fn cursor_scrolls_detail_when_open() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let tok = s.list.token;
        update(&mut s, Action::ListLoaded { token: tok, rows: rows(3) });
        update(&mut s, Action::Open);
        update(&mut s, Action::CursorDown);
        assert_eq!(s.detail.as_ref().unwrap().scroll, 1);
        update(&mut s, Action::CursorUp);
        assert_eq!(s.detail.as_ref().unwrap().scroll, 0);
        // list cursor unaffected while detail is open
        assert_eq!(s.list.cursor, 0);
    }
```

Update the `CursorDown`/`CursorUp` arms:

```rust
        Action::CursorDown => {
            match state.detail.as_mut() {
                Some(d) => d.scroll_down(),
                None => state.list.select_down(),
            }
            Vec::new()
        }
        Action::CursorUp => {
            match state.detail.as_mut() {
                Some(d) => d.scroll_up(),
                None => state.list.select_up(),
            }
            Vec::new()
        }
```

Run: `cargo test -p ayx-rs --lib tui::v2::action 2>&1 | tail -20` → PASS.

- [ ] **Step 2: Implement `view/detail.rs`**

```rust
//! Scrollable detail view: pretty-prints the fetched object as key/value lines.
//! Fixes the legacy 18-line truncation — Paragraph scroll shows any length.
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use serde_json::Value;

use crate::tui::theme;
use crate::tui::v2::state::{AppState, DetailView};

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let Some(d) = state.detail.as_ref() else { return };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border(true))
        .style(theme::panel())
        .title(Span::styled(format!(" {} · {} ", d.kind.singular(), d.title), theme::accent()));

    if d.loading {
        frame.render_widget(Paragraph::new(" ⟳ loading… ").block(block).style(theme::dim()), area);
        return;
    }
    if let Some(err) = &d.error {
        frame.render_widget(
            Paragraph::new(format!(" error: {err} ")).block(block).style(theme::danger()),
            area,
        );
        return;
    }

    let lines = d.json.as_ref().map(json_lines).unwrap_or_default();
    frame.render_widget(Paragraph::new(lines).block(block).scroll((d.scroll, 0)), area);
}

/// Flatten a JSON object's top-level fields to `key: value` lines. Nested
/// objects/arrays are pretty-printed and indented under their key.
fn json_lines(v: &Value) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                match val {
                    Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                        out.push(Line::from(vec![
                            Span::styled(format!("{k}: "), theme::field_label()),
                            Span::styled(scalar(val), theme::field_value()),
                        ]));
                    }
                    _ => {
                        out.push(Line::from(Span::styled(format!("{k}:"), theme::field_label())));
                        let pretty = serde_json::to_string_pretty(val).unwrap_or_default();
                        for line in pretty.lines() {
                            out.push(Line::from(Span::styled(format!("  {line}"), theme::field_value())));
                        }
                    }
                }
            }
        }
        other => {
            let pretty = serde_json::to_string_pretty(other).unwrap_or_default();
            for line in pretty.lines() {
                out.push(Line::from(Span::styled(line.to_string(), theme::field_value())));
            }
        }
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled("(empty)", theme::muted())));
    }
    out
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::context::Context;
    use crate::tui::v2::resource::Kind;
    use crate::tui::v2::state::{AppState, DetailView};
    use ratatui::{Terminal, backend::TestBackend};
    use serde_json::json;

    fn text_for(state: &AppState) -> String {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, state, f.area())).unwrap();
        terminal.backend().buffer().clone().content().iter().map(|c| c.symbol()).collect()
    }

    fn state_with_detail(json: serde_json::Value, loading: bool) -> AppState {
        let ctx = Context { profile: "wyatt".into(), workspace: "w".into(), user: "u".into() };
        let mut s = AppState::new(ctx);
        let mut d = DetailView::new(Kind::Flow, "fl_1".into(), "ETL".into(), 1);
        d.loading = loading;
        d.json = (!loading).then_some(json);
        s.detail = Some(d);
        s
    }

    #[test]
    fn renders_fields() {
        let s = state_with_detail(json!({ "id": "fl_1", "name": "ETL Pipeline" }), false);
        let txt = text_for(&s);
        assert!(txt.contains("ETL Pipeline"));
        assert!(txt.contains("id"));
    }

    #[test]
    fn shows_loading() {
        let s = state_with_detail(json!({}), true);
        assert!(text_for(&s).to_lowercase().contains("loading"));
    }
}
```

- [ ] **Step 3: Dispatch the body in `view/mod.rs`**

Add `mod detail;` and switch the body render on `nav.top()`:

```rust
mod detail;
mod footer;
mod header;
mod list;

use crate::tui::v2::nav::View;

pub fn render(frame: &mut Frame, state: &AppState) {
    frame.render_widget(Block::default().style(theme::app()), frame.area());
    let chunks = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(frame.area());

    header::render(frame, state, chunks[0]);
    match state.nav.top() {
        View::ResourceList { .. } => list::render(frame, state, chunks[1]),
        View::ResourceDetail { .. } => detail::render(frame, state, chunks[1]),
    }
    footer::render(frame, state, chunks[2]);
}
```

- [ ] **Step 4: Make `map_key` context-sensitive in `entry.rs`**

`Enter` means `Open` on a list and `Back` on a detail; Tab/BackTab cycle kinds (needs current kind). Replace `map_key` and its call sites:

```rust
fn map_key(state: &AppState, key: KeyEvent) -> Option<Action> {
    use crate::tui::v2::nav::View;
    use crate::tui::v2::resource::Kind;

    let on_detail = matches!(state.nav.top(), View::ResourceDetail { .. });

    match key.code {
        KeyCode::Down | KeyCode::Char('j') => Some(Action::CursorDown),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::CursorUp),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Enter => Some(if on_detail { Action::Back } else { Action::Open }),
        KeyCode::Char(c @ '1'..='5') => {
            Kind::from_index((c as u8 - b'1') as usize).map(Action::SwitchKind)
        }
        KeyCode::Tab => {
            let n = Kind::all().len();
            let next = (state.list.kind.index() + 1) % n;
            Kind::from_index(next).map(Action::SwitchKind)
        }
        KeyCode::BackTab => {
            let n = Kind::all().len();
            let prev = (state.list.kind.index() + n - 1) % n;
            Kind::from_index(prev).map(Action::SwitchKind)
        }
        _ => None,
    }
}
```

Update both call sites in `main_loop` to `map_key(state, key)`. Update the entry tests to pass a state — add a helper and revise:

```rust
    fn list_state() -> crate::tui::v2::state::AppState {
        let ctx = crate::tui::v2::context::Context { profile: "w".into(), workspace: "w".into(), user: "u".into() };
        crate::tui::v2::state::AppState::new(ctx) // root = flows list
    }

    #[test]
    fn enter_opens_on_list() {
        let s = list_state();
        assert!(matches!(map_key(&s, k(KeyCode::Enter)), Some(Action::Open)));
    }

    #[test]
    fn tab_cycles_kind() {
        use crate::tui::v2::resource::Kind;
        let s = list_state(); // Flow (index 0)
        assert!(matches!(map_key(&s, k(KeyCode::Tab)), Some(Action::SwitchKind(Kind::Connection))));
        assert!(matches!(map_key(&s, k(KeyCode::BackTab)), Some(Action::SwitchKind(Kind::Workspace))));
    }
```

Revise the pre-existing entry tests (`arrows_and_vim_keys_map_to_cursor`, `q_quits_esc_is_back`, `unmapped_key_is_none`, `number_keys_switch_kind`) to thread `&list_state()` as the first arg.

- [ ] **Step 5: Run + build**

Run: `cargo test -p ayx-rs --lib tui::v2 2>&1 | tail -30` → PASS.
Run: `cargo build -p ayx-rs 2>&1 | tail -20` → compiles.

- [ ] **Step 6: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/view/detail.rs ayx-rs/src/tui/v2/view/mod.rs ayx-rs/src/tui/v2/action.rs ayx-rs/src/tui/v2/entry.rs
git commit -m "feat(tui-v2): scrollable detail view + context-sensitive keys (Enter/Tab)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 11: In-list `/` filter

Add a client-side filter: `/` enters filter mode, typing narrows the list by case-insensitive substring on the first column, `Enter` applies (keeps the term, exits input), `Esc` clears it. No new crate — a plain `String` (full cursor editing arrives with `tui-input` in Phase 3).

**Files:**
- Modify: `ayx-rs/src/tui/v2/state.rs` (`ListView.filter`, `filtering`, `visible()`, cursor over visible)
- Modify: `ayx-rs/src/tui/v2/action.rs` (filter actions)
- Modify: `ayx-rs/src/tui/v2/entry.rs` (`map_key` routes keys to the filter while filtering)
- Modify: `ayx-rs/src/tui/v2/view/list.rs` (render filtered rows; show the term)
- Test: inline `#[cfg(test)]` in `state.rs`, `action.rs`

**Interfaces:**
- `state.rs`: `ListView` gains `pub filter: String` (empty = no filter) and `pub filtering: bool`. New `visible(&self) -> Vec<&Row>` (rows whose `cells[0].text` contains `filter`, case-insensitive; all rows when filter empty). `selected()` returns `visible().get(cursor)`. `select_down`/`select_up` clamp to `visible().len()`.
- `action.rs`: `FilterStart`, `FilterInput(char)`, `FilterBackspace`, `FilterApply` (Enter), `FilterClear` (Esc while filtering). Each resets `cursor` to 0 when the predicate changes.

- [ ] **Step 1: Update `ListView` (failing test first)**

In `state.rs` tests (add a `#[cfg(test)] mod tests`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::{Cell, Kind, Row};

    fn lv_with(names: &[&str]) -> ListView {
        let mut lv = ListView::new(Kind::Flow);
        lv.loading = false;
        lv.rows = names.iter().map(|n| Row { id: n.to_string(), cells: vec![Cell::plain(*n)] }).collect();
        lv
    }

    #[test]
    fn visible_is_all_when_no_filter() {
        let lv = lv_with(&["alpha", "beta"]);
        assert_eq!(lv.visible().len(), 2);
    }

    #[test]
    fn visible_filters_case_insensitive_on_first_cell() {
        let mut lv = lv_with(&["Daily ETL", "Sales Rollup", "daily report"]);
        lv.filter = "daily".to_string();
        let vis = lv.visible();
        assert_eq!(vis.len(), 2);
        assert_eq!(vis[0].cells[0].text, "Daily ETL");
    }

    #[test]
    fn selected_indexes_into_visible() {
        let mut lv = lv_with(&["aaa", "bbb", "abc"]);
        lv.filter = "a".to_string(); // matches "aaa","abc"
        lv.cursor = 1;
        assert_eq!(lv.selected().unwrap().cells[0].text, "abc");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::state 2>&1 | tail -20`
Expected: FAIL — `filter`/`visible` not found.

- [ ] **Step 3: Implement filter on `ListView`**

Add fields + initialize in `new`:

```rust
    pub filter: String,
    pub filtering: bool,
```

```rust
    pub fn new(kind: Kind) -> Self {
        Self { kind, rows: Vec::new(), cursor: 0, loading: true, error: None, token: 0, filter: String::new(), filtering: false }
    }
```

Replace `select_down`/`select_up`/`selected` to operate over visible rows:

```rust
    pub fn visible(&self) -> Vec<&Row> {
        if self.filter.is_empty() {
            return self.rows.iter().collect();
        }
        let needle = self.filter.to_ascii_lowercase();
        self.rows
            .iter()
            .filter(|r| {
                r.cells
                    .first()
                    .map(|c| c.text.to_ascii_lowercase().contains(&needle))
                    .unwrap_or(false)
            })
            .collect()
    }

    pub fn select_down(&mut self) {
        let len = self.visible().len();
        if len > 0 && self.cursor + 1 < len {
            self.cursor += 1;
        }
    }

    pub fn select_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn selected(&self) -> Option<&Row> {
        self.visible().get(self.cursor).copied()
    }
```

Note: `ListLoaded` clamps `cursor` against `rows.len()`; keep that, and additionally clamp against `visible().len()` in the filter actions below (when the term changes the visible set shrinks). The simplest invariant: reset `cursor = 0` whenever the filter string changes.

- [ ] **Step 4: Filter actions (failing reducer test)**

In `action.rs` tests:

```rust
    #[test]
    fn filter_flow_narrows_and_resets_cursor() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let tok = s.list.token;
        // rows with names a0..a4 via helper that sets cells[0]
        s.list.rows = (0..5)
            .map(|i| crate::tui::v2::resource::Row {
                id: format!("fl_{i}"),
                cells: vec![crate::tui::v2::resource::Cell::plain(format!("name {i}"))],
            })
            .collect();
        s.list.loading = false;
        let _ = tok;
        update(&mut s, Action::CursorDown);
        update(&mut s, Action::FilterStart);
        assert!(s.list.filtering);
        update(&mut s, Action::FilterInput('3'));
        assert_eq!(s.list.filter, "3");
        assert_eq!(s.list.cursor, 0, "cursor resets when filter changes");
        assert_eq!(s.list.visible().len(), 1);
        update(&mut s, Action::FilterApply);
        assert!(!s.list.filtering);
        assert_eq!(s.list.filter, "3", "apply keeps the term");
        update(&mut s, Action::FilterClear);
        assert!(s.list.filter.is_empty());
        assert!(!s.list.filtering);
    }
```

- [ ] **Step 5: Implement the filter actions**

Add to `Action`:

```rust
    FilterStart,
    FilterInput(char),
    FilterBackspace,
    FilterApply,
    FilterClear,
```

Reducer arms:

```rust
        Action::FilterStart => {
            state.list.filtering = true;
            Vec::new()
        }
        Action::FilterInput(c) => {
            state.list.filter.push(c);
            state.list.cursor = 0;
            Vec::new()
        }
        Action::FilterBackspace => {
            state.list.filter.pop();
            state.list.cursor = 0;
            Vec::new()
        }
        Action::FilterApply => {
            state.list.filtering = false;
            Vec::new()
        }
        Action::FilterClear => {
            state.list.filter.clear();
            state.list.filtering = false;
            state.list.cursor = 0;
            Vec::new()
        }
```

- [ ] **Step 6: Route keys while filtering in `entry.rs`**

At the top of `map_key`, before the normal match, capture keystrokes when the list is in filter-input mode (only on a list view):

```rust
    // While the filter input is active, keystrokes edit the term.
    if state.list.filtering && !on_detail {
        return match key.code {
            KeyCode::Char(c) => Some(Action::FilterInput(c)),
            KeyCode::Backspace => Some(Action::FilterBackspace),
            KeyCode::Enter => Some(Action::FilterApply),
            KeyCode::Esc => Some(Action::FilterClear),
            _ => None,
        };
    }
```

And add `/` to the normal list bindings:

```rust
        KeyCode::Char('/') if !on_detail => Some(Action::FilterStart),
```

(Place the `'/'` arm before the `'1'..='5'` arm so it is not shadowed; `'/'` is not in that range anyway.)

Add an entry test:

```rust
    #[test]
    fn slash_starts_filter_then_typing_feeds_it() {
        let mut s = list_state();
        assert!(matches!(map_key(&s, k(KeyCode::Char('/'))), Some(Action::FilterStart)));
        s.list.filtering = true;
        assert!(matches!(map_key(&s, k(KeyCode::Char('x'))), Some(Action::FilterInput('x'))));
        assert!(matches!(map_key(&s, k(KeyCode::Enter)), Some(Action::FilterApply)));
    }
```

- [ ] **Step 7: Render filtered rows + the term in `view/list.rs`**

In `render_table`, build the table from `state.list.visible()` instead of `state.list.rows`, and reflect the filter in the title. Replace the rows-building + title lines:

```rust
    let visible = state.list.visible();
    let title = if state.list.filter.is_empty() {
        format!(" {} · {} ", state.list.kind.name(), state.list.rows.len())
    } else {
        format!(
            " {} · {}/{}  /{}{} ",
            state.list.kind.name(),
            visible.len(),
            state.list.rows.len(),
            state.list.filter,
            if state.list.filtering { "▏" } else { "" }
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border(true))
        .style(theme::panel())
        .title(Span::styled(title, theme::accent()));
```

and:

```rust
    if visible.is_empty() {
        frame.render_widget(Paragraph::new(" no matches ").block(block).style(theme::muted()), area);
        return;
    }
    // header unchanged ...
    let rows: Vec<TRow> = visible
        .iter()
        .map(|r| TRow::new(r.cells.iter().map(render_cell).collect::<Vec<_>>()))
        .collect();
```

(The `loading`/`error` early-returns stay above this, unchanged. `render_detail`, the reactive split panel, already reads `state.list.selected()` which now indexes visible rows — no change needed there.)

- [ ] **Step 8: Run + commit**

Run: `cargo test -p ayx-rs --lib tui::v2 2>&1 | tail -30` → PASS.
Run: `cargo build -p ayx-rs 2>&1 | tail -20` → compiles.

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/state.rs ayx-rs/src/tui/v2/action.rs ayx-rs/src/tui/v2/entry.rs ayx-rs/src/tui/v2/view/list.rs
git commit -m "feat(tui-v2): in-list / filter (client-side, first-column substring)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 12: Per-view contextual footer

Make the footer change with the focused view: list, detail, and filter-input each get plain-language hints (the spec's "discoverability hero").

**Files:**
- Modify: `ayx-rs/src/tui/v2/view/footer.rs`
- Test: inline `#[cfg(test)]` in `view/footer.rs`

**Interfaces:**
- `footer::render(frame, state, area)` reads `state` to pick the hint set.

- [ ] **Step 1: Write the failing test**

Replace the `footer.rs` body with a state-aware version and add tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::context::Context;
    use crate::tui::v2::resource::Kind;
    use crate::tui::v2::state::{AppState, DetailView};
    use ratatui::{Terminal, backend::TestBackend};

    fn text_for(state: &AppState) -> String {
        let backend = TestBackend::new(120, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, state, f.area())).unwrap();
        terminal.backend().buffer().clone().content().iter().map(|c| c.symbol()).collect()
    }

    fn base() -> AppState {
        let ctx = Context { profile: "w".into(), workspace: "w".into(), user: "u".into() };
        AppState::new(ctx)
    }

    #[test]
    fn list_footer_has_open_and_filter() {
        let txt = text_for(&base());
        assert!(txt.contains("Open"));
        assert!(txt.contains("Filter"));
        assert!(txt.contains("Switch"));
    }

    #[test]
    fn filter_footer_when_filtering() {
        let mut s = base();
        s.list.filtering = true;
        let txt = text_for(&s);
        assert!(txt.to_lowercase().contains("filter"));
        assert!(txt.contains("Apply") || txt.contains("Cancel"));
    }

    #[test]
    fn detail_footer_has_back_and_scroll() {
        let mut s = base();
        s.nav.push(crate::tui::v2::nav::View::ResourceDetail {
            kind: Kind::Flow,
            id: "fl_1".into(),
            title: "ETL".into(),
        });
        s.detail = Some(DetailView::new(Kind::Flow, "fl_1".into(), "ETL".into(), 1));
        let txt = text_for(&s);
        assert!(txt.contains("Back"));
        assert!(txt.contains("Scroll"));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::view::footer 2>&1 | tail -20`
Expected: FAIL — current footer is fixed; detail/filter assertions fail.

- [ ] **Step 3: Implement the contextual footer**

```rust
//! Contextual footer hint bar — plain-language labels, changes per view.
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::theme;
use crate::tui::v2::nav::View;
use crate::tui::v2::state::AppState;

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let hint = if state.list.filtering && matches!(state.nav.top(), View::ResourceList { .. }) {
        Line::from(vec![
            label(" Filtering — type to narrow  "),
            key(" ↵ "),
            label("Apply  "),
            key(" ⌫ "),
            label("Delete  "),
            key(" ⎋ "),
            label("Cancel"),
        ])
    } else {
        match state.nav.top() {
            View::ResourceDetail { .. } => Line::from(vec![
                key(" ↑↓ "),
                label("Scroll  "),
                key(" ↵/⎋ "),
                label("Back  "),
                key(" q "),
                label("Quit"),
            ]),
            View::ResourceList { .. } => Line::from(vec![
                key(" ↵ "),
                label("Open  "),
                key(" / "),
                label("Filter  "),
                key(" 1-5/⇥ "),
                label("Switch  "),
                key(" q "),
                label("Quit"),
            ]),
        }
    };
    frame.render_widget(Paragraph::new(hint).style(theme::panel()), area);
}

fn key(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), theme::accent_bold())
}
fn label(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), theme::dim())
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ayx-rs --lib tui::v2::view::footer 2>&1 | tail -20`
Expected: PASS (3 tests).

Note: the Phase-0 `view/mod.rs` test `list_shows_flow_row_and_footer_hint` asserts the footer contains `"Palette"`. The list footer no longer shows "Palette" (palette is Phase 3). Update that assertion to `assert!(text.contains("Switch"));` (or `"Filter"`) in `view/mod.rs` tests.

- [ ] **Step 5: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/view/footer.rs ayx-rs/src/tui/v2/view/mod.rs
git commit -m "feat(tui-v2): per-view contextual footer (list/detail/filter)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 13: Full validation + manual smoke + status update

Final gate: whole-suite green, lint clean, and a manual smoke of every asset + drill + filter against a live workspace. Then record status.

**Files:**
- Modify: `.superpowers/plans/2026-06-27-ayx-tui-phase1-browser-core.md` (check the boxes; add a STATUS line)

- [ ] **Step 1: Full suite + lint + fmt**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all --check
cargo clippy -p ayx-rs --all-targets -- -D warnings
cargo nextest run -p ayx-rs --no-fail-fast 2>&1 | tail -30
```
Expected: fmt clean, zero clippy warnings, all tests pass (Phase-0 baseline was 248/248; this phase adds resource-impl, reducer, worker, view, and entry tests).

- [ ] **Step 2: Manual smoke against a live workspace**

```bash
AYX_TUI_V2=1 cargo run -p ayx-rs -- tui
```
Verify, against the active authed workspace:
- Header shows `Profile · Workspace · User`; the tab strip shows `1 flows  2 connections  3 jobs  4 people  5 workspaces` with flows highlighted.
- `1`–`5` and `Tab`/`Shift+Tab` switch asset; each list shows `⟳ loading…` then populates (or `error: …` if the workspace can't serve that asset — soft-fail, the shell stays usable).
- `j`/`k` move the cursor; the reactive right panel updates per row; job STATUS / person STATUS / workspace STATUS show color + word.
- `Enter` on a flow/connection/job/person drills into a **scrollable** detail (`↑`/`↓` scroll past 18 lines — the legacy truncation is gone); `Enter` on a workspace is a no-op (no detail endpoint). `Esc`/`Enter` returns to the list; the breadcrumb replaced the tab strip while in detail.
- `/` filters the current list by name; `Enter` applies, `Esc` clears; the title shows `matched/total /term`.
- `q` quits cleanly with the terminal restored. Then confirm the legacy path is intact: `cargo run -p ayx-rs -- tui` (no env var) opens the old interface.

Record the result of the smoke (pass / any soft-fails observed) in the commit message.

- [ ] **Step 3: Mark the plan complete + commit**

Add to the bottom of this plan file:

```markdown
## STATUS

**Phase 1 (Browser Core) COMPLETE — <DATE>.** All five assets browse through the
ResourceKind registry behind `AYX_TUI_V2=1 ayx tui`: switch (1-5/Tab), async
drill to scrollable detail (no freeze, no truncation), `/` filter, per-view
footer, status tones. Staleness moved to in-state generation tokens. Legacy TUI
untouched. Full suite green.

Deferred to later phases (unchanged from the scope mapping): cross-asset drill
(flow→runs), Ctrl+K palette + ? help + tui-input (Phase 3), workspace switching
+ inline OTP (Phase 4), actions run/cancel/enable (Phase 5).
```

```bash
cd /home/merlin/code/ayx-rs
git add .superpowers/plans/2026-06-27-ayx-tui-phase1-browser-core.md
git commit -m "docs(tui-v2): mark Phase 1 browser-core plan complete

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage** (spec Phase 1 "Browser core" + Phase 2 asset impls):
- `ResourceKind` trait + registry generalized past `Kind::Flow` → Tasks 1, 2. ✓
- Generic table list + reactive detail panel → already in Phase 0; preserved and fed filtered rows in Task 11. ✓
- Scrollable detail (kills 18-line truncation) → Task 10 (`Paragraph::scroll`). ✓
- Async list+detail via worker (kills the freeze) → Task 9 (FetchDetail through the worker thread). ✓
- `/` filter → Task 11. ✓
- Contextual footer → Task 12. ✓
- Status colors → Tasks 4 (jobs), 5 (people), 6 (workspaces), paired with words. ✓
- `ResourceKind` impls for Connections, Jobs, People, Workspaces → Tasks 3–6. ✓
- Resource switching ("browser-core menu" Phase-0 follow-up) → Task 8. ✓
- Phase-0 follow-up "generalize AppState root beyond hardcoded Kind::Flow" → Task 8 (SwitchKind resets nav+list). ✓
- Phase-0 follow-up "revisit list_request tracking once update() emits fetch effects" → Task 7 (generation tokens). ✓
- Deferred (documented): cross-asset drill, palette/help/tui-input, workspace switching, actions. ✓ (explicitly out of scope per the mapping)

**2. Placeholder scan:** No "TBD"/"handle errors appropriately"/"similar to Task N". Every code step shows complete code. The one deliberate note (Task 8 Step 8: drop the `Tab` arm until Task 10 gives `map_key` state access) names the exact final code, not a placeholder.

**3. Type consistency:**
- `DetailEndpoint` defined Task 1, implemented by every kind (Tasks 1–6), consumed by the worker Task 9.
- `Kind` variants/`name`/`singular`/`index`/`from_index` defined Tasks 2 & 8, used in header (Task 8), entry `map_key` (Tasks 8, 10), reducer (Tasks 8–10).
- Token model: `Effect::FetchList { kind, token }` / `FetchDetail { kind, id, token }` (Tasks 7, 9); `Action::ListLoaded/ListFailed { token, .. }` & `DetailLoaded/DetailFailed { token, .. }` (Tasks 7, 9); `mint_token` (Task 7); `ListView.token` / `DetailView.token` / `AppState.req_seq` (Tasks 7, 9). Worker `list_payload_to_action(kind, token, payload)` (Task 7) & `detail_payload_to_action(token, payload)` (Task 9) produce exactly those variants. Consistent.
- `Worker::submit(effect, config)` + `Outcome { action }` (Task 7) used by `entry.rs` `dispatch_effects` (Task 7) — the old `RequestId`/`next_request_id`/`Outcome.id` are fully removed, no dangling references.
- `ListView.visible()`/`selected()` (Task 11) consumed by `view/list.rs` (Task 11) and the reactive `render_detail` (unchanged, reads `selected()`); cursor semantics (indexes visible rows) consistent across reducer cursor arms (Task 10) and filter cursor resets (Task 11).
- `map_key` signature changes once (Task 10: `(key)` → `(&AppState, key)`); all call sites and tests updated in the same task. Tasks 8 & 11 add arms to the pre-Task-10 and post-Task-10 signatures respectively, in the correct order.

**Phasing note:** This plan delivers working, testable software on its own — `AYX_TUI_V2=1 ayx tui` browses all five assets with switch/drill/filter in the new architecture. Phases 3–5 (palette/help/tui-input, workspace switching, actions) each get their own plan.

## STATUS

**Phase 1 (Browser Core) COMPLETE — 2026-06-27.** All five assets browse through the
ResourceKind registry behind `AYX_TUI_V2=1 ayx tui`: switch (1-5/Tab/BackTab), async
drill to scrollable detail (no freeze, no truncation), `/` filter, per-view
footer, status tones. Staleness moved to in-state generation tokens. Legacy TUI
untouched. Workspace-wide suite green (421/421), clippy clean, fmt clean.

Built via subagent-driven-development: codex (gpt-5.4, high effort on the meaty
tasks) implemented all 12 tasks; Claude (opus) reviewed + committed each; the
generation-token refactor (Task 7) and the whole branch got dedicated rust-reviewer
(opus) passes (both APPROVE). Commit range: a126df9..3064a20 (13 commits off main fe6b58a).

**Manual live smoke (Task 13 Step 2): PENDING — needs an interactive TTY + an authed
workspace.** Run `AYX_TUI_V2=1 ayx tui` and exercise: tab/number switch across all
5 assets; j/k + reactive panel; Enter drill to scrollable detail (workspace = no-op);
/ filter; q quit; then `ayx tui` (no env var) = legacy path intact.

Deferred to later phases (unchanged from the scope mapping): cross-asset drill
(flow→runs), Ctrl+K palette + ? help + tui-input (Phase 3), workspace switching
+ inline OTP (Phase 4), actions run/cancel/enable (Phase 5).

**Follow-ups logged from the final review (non-blocking):**
- Worker-thread panic (e.g. OAuth-refresh OS-entropy `expect`) leaves a view in
  `⟳ loading…` forever — the always-worker model makes a worker panic worse than
  Phase 0. Harden: `catch_unwind` in `worker_loop` emitting a `*Failed`, or surface
  a terminal error on closed-channel `try_recv`.
- `Action::Open` relies on `map_key` routing Enter→Back when on a detail; add a
  defensive `if state.detail.is_some() { return; }` so the reducer is correct
  independent of the key layer (matters once the Phase-3 palette can emit `Open`).
- **Serial-worker stale-request starvation** (codex adversarial review): the single
  worker thread runs requests FIFO; switching kind / drilling while a slow request
  is in flight makes the fresh view wait up to the 60s client timeout for the
  now-irrelevant request. UI thread never blocks (no correctness bug) but it hurts
  responsiveness. Later phase: small worker pool, or drop/cancel superseded
  requests (the generation token already identifies them).

**Adversarial-review fixes applied before merge** (codex pass, commit after final
review): filter now uses Unicode `to_lowercase()` (was ASCII-only `to_ascii_*`); the
list footer omits "↵ Open" for kinds with no detail endpoint (Workspaces) so it
never advertises a dead key.
