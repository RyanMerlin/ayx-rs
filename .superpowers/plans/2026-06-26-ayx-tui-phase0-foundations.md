# ayx TUI Rearchitecture — Phase 0 (Foundations) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the new k9s-style TUI spine — unidirectional state/action/effect architecture with a `ResourceKind` registry — and wire **Flows** end-to-end (context header → async list → reactive detail) behind an env gate, leaving the existing TUI untouched.

**Architecture:** The Elm Architecture (TEA). `Event → Action → update(&mut AppState) → [Effect] → worker → TaskResult → Action`. The render loop never blocks on I/O. A new `tui/v2/` module tree holds the spine; `tui::run()` dispatches to it only when `AYX_TUI_V2` is set (strangler-fig — the old `app.rs`/`mod.rs` path stays live and its tests stay green until later phases port and delete it).

**Tech Stack:** Rust (edition 2024), ratatui 0.30, crossterm 0.29, serde_json, anyhow. Backend calls reuse `ayx_one_api::one_api_live_request`. No new crates in Phase 0 (palette/input/throbber crates arrive in their phase-of-use per the spec).

## Global Constraints

Copied verbatim from the design spec and repo conventions — every task implicitly includes these:

- **Spec:** `.superpowers/specs/2026-06-26-ayx-tui-rearchitecture-design.md` (commit `8eaa9dd`).
- **Render loop must never block on I/O.** All list/detail fetches go through the worker thread; the UI thread only drains results.
- **No backend/API changes.** Phase 0 calls `ayx_one_api::one_api_live_request` only; it does not modify `ayx-one-api`, `ayx-core`, or any endpoint.
- **Existing tests stay green.** Do not modify or delete `tui/app.rs`, `tui/mod.rs`, `tui/store.rs`, `tui/forms.rs`, `tui/render_helpers.rs`, or their tests. The only edit to existing TUI code is adding `mod v2;` and a 2-line env gate at the top of `tui::run()` in `tui/mod.rs`.
- **Reuse the theme.** All colors via `crate::tui::theme` (`theme::ACCENT`, `theme::ok()`, `theme::warn()`, `theme::danger()`, `theme::border(bool)`, etc.). No new hardcoded colors.
- **Status colors:** green (`theme::ok()`) = ok/succeeded; yellow (`theme::warn()`) = pending/running/disabled; red (`theme::danger()`) = failed/error. Always paired with a status word, never color alone.
- **Validation gate per task:** `cargo nextest run -p ayx-rs --no-fail-fast` (the crate is `ayx-rs`), then before any commit `cargo fmt --all && cargo clippy -p ayx-rs --all-targets -- -D warnings`. `cargo fmt --all` runs as part of the commit step, not after.
- **Commits:** concise, conventional. Co-author trailer:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`

---

## File Structure

All new files live under `ayx-rs/src/tui/v2/`. Each has one responsibility:

| File | Responsibility |
|------|----------------|
| `v2/mod.rs` | Module wiring + `run()` entry: terminal setup, panic hook, main loop. |
| `v2/resource/mod.rs` | `Kind` enum, `Column`, `Cell`, `StatusTone`, `Row`, the `ResourceKind` trait, and the registry (`kind_impl`). |
| `v2/resource/flow.rs` | `FlowKind` — `ResourceKind` impl for flows (columns, item extraction, row mapping, list endpoint). |
| `v2/nav.rs` | `View` enum + `NavStack` (push/pop/breadcrumb). |
| `v2/context.rs` | `Context { profile, workspace, user }` + `Context::from_config`. |
| `v2/state.rs` | `AppState`, `ListView` (rows + cursor + loading + error), `Toast`. |
| `v2/action.rs` | `Action` enum + `update(&mut AppState, Action) -> Vec<Effect>` reducer. |
| `v2/effect.rs` | `Effect` enum. |
| `v2/worker.rs` | `Worker` — generic effect executor (FetchList/FetchDetail) over `one_api_live_request`, with `RequestId` stale-drop. |
| `v2/view/mod.rs` | `render(frame, state)` dispatcher + shared layout. |
| `v2/view/header.rs` | Context header + breadcrumb row. |
| `v2/view/list.rs` | Table list + reactive detail split panel. |
| `v2/view/footer.rs` | Contextual footer hint bar. |

The single edit to existing code: `tui/mod.rs` gains `mod v2;` and the env gate.

---

### Task 1: Resource model — `Kind`, `Row`, `ResourceKind` trait

**Files:**
- Create: `ayx-rs/src/tui/v2/mod.rs` (module declarations only, this task)
- Create: `ayx-rs/src/tui/v2/resource/mod.rs`
- Test: inline `#[cfg(test)]` in `resource/mod.rs`

**Interfaces:**
- Produces:
  - `enum Kind { Flow }` (later phases add Connection, Job, Person, Workspace). `Kind::name(&self) -> &'static str`, `Kind::all() -> &'static [Kind]`.
  - `enum StatusTone { Neutral, Ok, Warn, Danger }`
  - `struct Cell { text: String, tone: StatusTone }` with `Cell::plain(impl Into<String>)` and `Cell::toned(impl Into<String>, StatusTone)`.
  - `struct Column { title: &'static str, width: u16 }`
  - `struct Row { cells: Vec<Cell>, id: String }`
  - `trait ResourceKind { fn columns(&self)->&'static [Column]; fn extract_items(&self, payload:&serde_json::Value)->Vec<serde_json::Value>; fn row(&self, item:&serde_json::Value)->Row; fn list_endpoint(&self)->ListEndpoint; }`
  - `struct ListEndpoint { surface: &'static str, operation: &'static str, path: &'static str }`
  - `fn kind_impl(kind: Kind) -> &'static dyn ResourceKind` (the registry; Task 2 fills the Flow arm).

- [ ] **Step 1: Create the v2 module file with declarations**

Create `ayx-rs/src/tui/v2/mod.rs`:

```rust
//! Phase-0 spine of the rearchitected TUI (the "v2" surface).
//!
//! Unidirectional: Event -> Action -> update(state) -> [Effect] -> worker ->
//! Action. The render loop never blocks on I/O. Gated behind AYX_TUI_V2 so the
//! legacy `tui/app.rs` path stays live until later phases port it.
#![allow(dead_code)] // trait surface lands ahead of all callers during Phase 0

pub mod action;
pub mod context;
pub mod effect;
pub mod nav;
pub mod resource;
pub mod state;
pub mod view;
pub mod worker;

pub use entry::run;

mod entry;
```

Note: `entry` (the `run()` loop) is added in Task 8. To keep the crate compiling now, create a placeholder `ayx-rs/src/tui/v2/entry.rs`:

```rust
//! TUI v2 entry point. Real loop lands in Task 8.
use anyhow::Result;
use ayx_core::envelope::Envelope;

pub fn run() -> Result<Envelope> {
    Ok(Envelope::ok("tui v2 not yet wired"))
}
```

Also create empty placeholder files so the `mod` lines resolve — each gets real content in its task: `action.rs`, `context.rs`, `effect.rs`, `nav.rs`, `state.rs`, `worker.rs`, `resource/mod.rs`, and `view/mod.rs`. For this task, only `resource/mod.rs` gets real content (below); create the others as `// filled in Task N` one-line stubs that still compile (an empty `.rs` file compiles fine). For `view/mod.rs` create the dir `ayx-rs/src/tui/v2/view/` first.

- [ ] **Step 2: Write the failing test for `Row`/`Cell` and `Kind`**

In `ayx-rs/src/tui/v2/resource/mod.rs`, add at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_name_and_all() {
        assert_eq!(Kind::Flow.name(), "flows");
        assert!(Kind::all().contains(&Kind::Flow));
    }

    #[test]
    fn cell_constructors_carry_tone() {
        let plain = Cell::plain("hello");
        assert_eq!(plain.text, "hello");
        assert_eq!(plain.tone, StatusTone::Neutral);

        let toned = Cell::toned("failed", StatusTone::Danger);
        assert_eq!(toned.tone, StatusTone::Danger);
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::resource 2>&1 | tail -20`
Expected: FAIL — `Kind`, `Cell`, `StatusTone` not found (compile error).

- [ ] **Step 4: Implement the resource model**

At the top of `ayx-rs/src/tui/v2/resource/mod.rs` (above the test module):

```rust
//! Resource model: the k9s engine. Each browsable asset implements
//! `ResourceKind`, so the list/detail views and effect executor are written
//! once and work for every asset. Phase 0 ships `Kind::Flow` only.
use serde_json::Value;

pub mod flow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Flow,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Flow => "flows",
        }
    }

    pub fn all() -> &'static [Kind] {
        &[Kind::Flow]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Neutral,
    Ok,
    Warn,
    Danger,
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub text: String,
    pub tone: StatusTone,
}

impl Cell {
    pub fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), tone: StatusTone::Neutral }
    }
    pub fn toned(text: impl Into<String>, tone: StatusTone) -> Self {
        Self { text: text.into(), tone }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Column {
    pub title: &'static str,
    pub width: u16,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, Copy)]
pub struct ListEndpoint {
    pub surface: &'static str,
    pub operation: &'static str,
    pub path: &'static str,
}

/// Each browsable asset implements this. Pure data mapping — no I/O, no state.
pub trait ResourceKind: Sync {
    fn columns(&self) -> &'static [Column];
    /// Pull the array of item objects out of a raw list-endpoint payload.
    fn extract_items(&self, payload: &Value) -> Vec<Value>;
    /// Map one item object to a display row (cells + stable id).
    fn row(&self, item: &Value) -> Row;
    fn list_endpoint(&self) -> ListEndpoint;
}

/// Registry: map a `Kind` to its static trait object. Filled per-asset.
pub fn kind_impl(kind: Kind) -> &'static dyn ResourceKind {
    match kind {
        Kind::Flow => &flow::FlowKind,
    }
}
```

This references `flow::FlowKind` (Task 2). To keep Task 1 self-contained and compiling, create `ayx-rs/src/tui/v2/resource/flow.rs` now with a minimal stub that Task 2 replaces:

```rust
//! Flow ResourceKind. Real impl lands in Task 2.
use super::{Column, ListEndpoint, ResourceKind, Row};
use serde_json::Value;

pub struct FlowKind;

impl ResourceKind for FlowKind {
    fn columns(&self) -> &'static [Column] {
        &[]
    }
    fn extract_items(&self, _payload: &Value) -> Vec<Value> {
        Vec::new()
    }
    fn row(&self, _item: &Value) -> Row {
        Row { id: String::new(), cells: Vec::new() }
    }
    fn list_endpoint(&self) -> ListEndpoint {
        ListEndpoint { surface: "flow", operation: "tui-flow-list", path: "/v4/flows" }
    }
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p ayx-rs --lib tui::v2::resource 2>&1 | tail -20`
Expected: PASS (2 tests). If `mod v2;` is not yet declared in `tui/mod.rs`, the module won't build — add it now: in `ayx-rs/src/tui/mod.rs`, add `mod v2;` alongside the other `mod` lines (line ~25-31). This is allowed by the Global Constraints.

- [ ] **Step 6: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
git add ayx-rs/src/tui/v2 ayx-rs/src/tui/mod.rs
git commit -m "feat(tui-v2): resource model — Kind, Row, ResourceKind trait + registry

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Flow `ResourceKind` implementation

**Files:**
- Modify: `ayx-rs/src/tui/v2/resource/flow.rs` (replace the Task 1 stub)
- Test: inline `#[cfg(test)]` in `flow.rs`

**Interfaces:**
- Consumes: `Column`, `Cell`, `StatusTone`, `Row`, `ListEndpoint`, `ResourceKind` from `resource/mod.rs`.
- Produces: `FlowKind` (unit struct) with real columns `["NAME","UPDATED","ID"]`, `extract_items` that reads the `data`/`items`/`results` array key, and `row` that maps a flow object.

- [ ] **Step 1: Write the failing test**

In `ayx-rs/src/tui/v2/resource/flow.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_items_reads_data_array() {
        let payload = json!({
            "data": [
                { "id": "fl_1", "name": "ETL Pipeline", "updatedAt": "2026-06-20T10:00:00Z" },
                { "id": "fl_2", "name": "Sales Rollup", "updatedAt": "2026-06-19T09:00:00Z" }
            ]
        });
        let items = FlowKind.extract_items(&payload);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn row_maps_name_updated_id() {
        let item = json!({
            "id": "fl_1", "name": "ETL Pipeline", "updatedAt": "2026-06-20T10:00:00Z"
        });
        let row = FlowKind.row(&item);
        assert_eq!(row.id, "fl_1");
        assert_eq!(row.cells[0].text, "ETL Pipeline");
        assert_eq!(row.cells[1].text, "2026-06-20"); // date only
        assert_eq!(row.cells[2].text, "fl_1");
    }

    #[test]
    fn row_handles_missing_name() {
        let item = json!({ "id": "fl_x" });
        let row = FlowKind.row(&item);
        assert_eq!(row.cells[0].text, "(unnamed)");
        assert_eq!(row.id, "fl_x");
    }

    #[test]
    fn columns_are_three() {
        assert_eq!(FlowKind.columns().len(), 3);
        assert_eq!(FlowKind.columns()[0].title, "NAME");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::resource::flow 2>&1 | tail -20`
Expected: FAIL — stub returns empty columns/rows; assertions fail.

- [ ] **Step 3: Implement `FlowKind`**

Replace the body of `flow.rs` (keep the test module):

```rust
//! Flow ResourceKind — maps `/v4/flows` list items to display rows.
use super::{Cell, Column, ListEndpoint, ResourceKind, Row};
use serde_json::Value;

pub struct FlowKind;

const FLOW_COLUMNS: &[Column] = &[
    Column { title: "NAME", width: 40 },
    Column { title: "UPDATED", width: 12 },
    Column { title: "ID", width: 24 },
];

/// One API list payloads wrap items under one of these keys depending on the
/// endpoint/version. Try them in order (same heuristic the legacy browser uses).
fn items_array(payload: &Value) -> Vec<Value> {
    for key in ["data", "items", "results"] {
        if let Some(arr) = payload.get(key).and_then(Value::as_array) {
            return arr.clone();
        }
    }
    // Some endpoints return a bare array.
    if let Some(arr) = payload.as_array() {
        return arr.clone();
    }
    Vec::new()
}

fn str_field<'a>(item: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| item.get(*k).and_then(Value::as_str))
}

/// "2026-06-20T10:00:00Z" -> "2026-06-20"; passthrough if not a timestamp.
fn date_only(ts: &str) -> String {
    ts.split('T').next().unwrap_or(ts).to_string()
}

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
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ayx-rs --lib tui::v2::resource::flow 2>&1 | tail -20`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
git add ayx-rs/src/tui/v2/resource/flow.rs
git commit -m "feat(tui-v2): FlowKind ResourceKind impl — columns, item extraction, row mapping

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Navigation stack — `View` + `NavStack`

**Files:**
- Modify: `ayx-rs/src/tui/v2/nav.rs` (replace stub)
- Test: inline `#[cfg(test)]`

**Interfaces:**
- Consumes: `Kind` from `resource`.
- Produces:
  - `enum View { ResourceList { kind: Kind }, ResourceDetail { kind: Kind, id: String, title: String } }`
  - `struct NavStack { stack: Vec<View> }` with `new(root: View)`, `push(View)`, `pop() -> bool` (false if at root), `top(&self) -> &View`, `breadcrumb(&self) -> String`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::Kind;

    #[test]
    fn root_cannot_be_popped() {
        let mut nav = NavStack::new(View::ResourceList { kind: Kind::Flow });
        assert!(!nav.pop());
        assert!(matches!(nav.top(), View::ResourceList { kind: Kind::Flow }));
    }

    #[test]
    fn push_then_pop_returns_to_root() {
        let mut nav = NavStack::new(View::ResourceList { kind: Kind::Flow });
        nav.push(View::ResourceDetail {
            kind: Kind::Flow,
            id: "fl_1".into(),
            title: "ETL Pipeline".into(),
        });
        assert!(matches!(nav.top(), View::ResourceDetail { .. }));
        assert!(nav.pop());
        assert!(matches!(nav.top(), View::ResourceList { .. }));
    }

    #[test]
    fn breadcrumb_shows_path() {
        let mut nav = NavStack::new(View::ResourceList { kind: Kind::Flow });
        assert_eq!(nav.breadcrumb(), "flows");
        nav.push(View::ResourceDetail {
            kind: Kind::Flow,
            id: "fl_1".into(),
            title: "ETL Pipeline".into(),
        });
        assert_eq!(nav.breadcrumb(), "flows › ETL Pipeline");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::nav 2>&1 | tail -20`
Expected: FAIL — `View`/`NavStack` not found.

- [ ] **Step 3: Implement**

Top of `nav.rs`:

```rust
//! Navigation stack: drill-down is `push`, back is `pop`, breadcrumb is the
//! rendered path. The root view can never be popped.
use crate::tui::v2::resource::Kind;

#[derive(Debug, Clone)]
pub enum View {
    ResourceList { kind: Kind },
    ResourceDetail { kind: Kind, id: String, title: String },
}

impl View {
    fn crumb(&self) -> String {
        match self {
            View::ResourceList { kind } => kind.name().to_string(),
            View::ResourceDetail { title, .. } => title.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NavStack {
    stack: Vec<View>,
}

impl NavStack {
    pub fn new(root: View) -> Self {
        Self { stack: vec![root] }
    }

    pub fn push(&mut self, view: View) {
        self.stack.push(view);
    }

    /// Pop one level. Returns false (and does nothing) if already at the root.
    pub fn pop(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            true
        } else {
            false
        }
    }

    pub fn top(&self) -> &View {
        self.stack.last().expect("nav stack is never empty")
    }

    pub fn breadcrumb(&self) -> String {
        self.stack.iter().map(View::crumb).collect::<Vec<_>>().join(" › ")
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ayx-rs --lib tui::v2::nav 2>&1 | tail -20`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
git add ayx-rs/src/tui/v2/nav.rs
git commit -m "feat(tui-v2): nav stack — View enum, push/pop, breadcrumb

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Context header data — `Context`

**Files:**
- Modify: `ayx-rs/src/tui/v2/context.rs` (replace stub)
- Test: inline `#[cfg(test)]`

**Interfaces:**
- Consumes: `ayx_core::profile::Config`.
- Produces: `struct Context { profile: String, workspace: String, user: String }` + `Context::from_config(config: &Config, active_profile: Option<&str>) -> Context`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::profile::{AlteryxOneProfile, Config};

    #[test]
    fn context_reads_profile_workspace_user() {
        let mut config = Config::default();
        let mut one = AlteryxOneProfile::default();
        one.account_email = "ryan@alteryx.com".into();
        one.expected_workspace_id = Some("w_marketing".into());
        one.workspace_credentials.insert("w_marketing".into(), Default::default());
        config.alteryx_one = Some(one);

        let ctx = Context::from_config(&config, Some("wyatt"));
        assert_eq!(ctx.profile, "wyatt");
        assert_eq!(ctx.workspace, "w_marketing");
        assert_eq!(ctx.user, "ryan@alteryx.com");
    }

    #[test]
    fn context_degrades_gracefully_without_one_profile() {
        let config = Config::default();
        let ctx = Context::from_config(&config, None);
        assert_eq!(ctx.profile, "(none)");
        assert_eq!(ctx.workspace, "(no workspace)");
        assert_eq!(ctx.user, "(no identity)");
    }
}
```

Note: confirm `Config` and `AlteryxOneProfile` derive `Default` (they have `Default::default()`-friendly construction in `profile.rs`; `WorkspaceCredential: Default` is used by the test). If any does not derive `Default`, build the struct with its existing constructor instead — check `profile.rs` and adjust the test setup, keeping the three assertions identical.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::context 2>&1 | tail -20`
Expected: FAIL — `Context` not found.

- [ ] **Step 3: Implement**

```rust
//! Context header data: the always-visible "where am I" strip
//! (Profile · Workspace · User). Derived from the loaded Config.
use ayx_core::profile::Config;

#[derive(Debug, Clone)]
pub struct Context {
    pub profile: String,
    pub workspace: String,
    pub user: String,
}

impl Context {
    pub fn from_config(config: &Config, active_profile: Option<&str>) -> Self {
        let one = config.alteryx_one.as_ref();
        let workspace = one
            .and_then(|o| o.active_workspace_id())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "(no workspace)".to_string());
        let user = one
            .map(|o| o.account_email.clone())
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| "(no identity)".to_string());
        Self {
            profile: active_profile.unwrap_or("(none)").to_string(),
            workspace,
            user,
        }
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ayx-rs --lib tui::v2::context 2>&1 | tail -20`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
git add ayx-rs/src/tui/v2/context.rs
git commit -m "feat(tui-v2): Context header data from Config (profile/workspace/user)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: State, Effect, Action + the `update` reducer

**Files:**
- Modify: `ayx-rs/src/tui/v2/effect.rs`, `ayx-rs/src/tui/v2/state.rs`, `ayx-rs/src/tui/v2/action.rs` (replace stubs)
- Test: inline `#[cfg(test)]` in `action.rs`

**Interfaces:**
- Consumes: `Kind`, `Row`, `kind_impl` from `resource`; `View`, `NavStack` from `nav`; `Context` from `context`.
- Produces:
  - `effect.rs`: `enum Effect { FetchList { kind: Kind } }`
  - `state.rs`:
    - `struct ListView { kind: Kind, rows: Vec<Row>, cursor: usize, loading: bool, error: Option<String> }` with `new(kind)`, `select_down()`, `select_up()`, `selected(&self) -> Option<&Row>`.
    - `struct AppState { context: Context, nav: NavStack, list: ListView, should_quit: bool }` with `new(context: Context) -> Self` (root view = Flow list, list starts `loading=true`).
  - `action.rs`: `enum Action { CursorDown, CursorUp, Back, Quit, ListLoaded { kind: Kind, rows: Vec<Row> }, ListFailed { kind: Kind, error: String } }` + `pub fn update(state: &mut AppState, action: Action) -> Vec<Effect>`.

- [ ] **Step 1: Implement `effect.rs`**

```rust
//! Effects: side-effect requests emitted by `update`, executed by the worker.
use crate::tui::v2::resource::Kind;

#[derive(Debug, Clone)]
pub enum Effect {
    FetchList { kind: Kind },
}
```

- [ ] **Step 2: Implement `state.rs`**

```rust
//! Application state. Pure data — no I/O, no rendering.
use crate::tui::v2::context::Context;
use crate::tui::v2::nav::{NavStack, View};
use crate::tui::v2::resource::{Kind, Row};

#[derive(Debug, Clone)]
pub struct ListView {
    pub kind: Kind,
    pub rows: Vec<Row>,
    pub cursor: usize,
    pub loading: bool,
    pub error: Option<String>,
}

impl ListView {
    pub fn new(kind: Kind) -> Self {
        Self { kind, rows: Vec::new(), cursor: 0, loading: true, error: None }
    }

    pub fn select_down(&mut self) {
        if !self.rows.is_empty() && self.cursor + 1 < self.rows.len() {
            self.cursor += 1;
        }
    }

    pub fn select_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn selected(&self) -> Option<&Row> {
        self.rows.get(self.cursor)
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub context: Context,
    pub nav: NavStack,
    pub list: ListView,
    pub should_quit: bool,
}

impl AppState {
    pub fn new(context: Context) -> Self {
        Self {
            context,
            nav: NavStack::new(View::ResourceList { kind: Kind::Flow }),
            list: ListView::new(Kind::Flow),
            should_quit: false,
        }
    }
}
```

- [ ] **Step 3: Write the failing reducer test**

In `ayx-rs/src/tui/v2/action.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::context::Context;
    use crate::tui::v2::resource::{Kind, Row};
    use crate::tui::v2::state::AppState;

    fn test_state() -> AppState {
        let ctx = Context { profile: "wyatt".into(), workspace: "w".into(), user: "u".into() };
        AppState::new(ctx)
    }

    fn rows(n: usize) -> Vec<Row> {
        (0..n).map(|i| Row { id: format!("fl_{i}"), cells: vec![] }).collect()
    }

    #[test]
    fn list_loaded_populates_and_clears_loading() {
        let mut s = test_state();
        assert!(s.list.loading);
        let effects = update(&mut s, Action::ListLoaded { kind: Kind::Flow, rows: rows(3) });
        assert!(!s.list.loading);
        assert_eq!(s.list.rows.len(), 3);
        assert!(effects.is_empty());
    }

    #[test]
    fn list_failed_sets_error_clears_loading() {
        let mut s = test_state();
        update(&mut s, Action::ListFailed { kind: Kind::Flow, error: "boom".into() });
        assert!(!s.list.loading);
        assert_eq!(s.list.error.as_deref(), Some("boom"));
    }

    #[test]
    fn cursor_moves_within_bounds() {
        let mut s = test_state();
        update(&mut s, Action::ListLoaded { kind: Kind::Flow, rows: rows(2) });
        update(&mut s, Action::CursorDown);
        assert_eq!(s.list.cursor, 1);
        update(&mut s, Action::CursorDown); // clamp at last
        assert_eq!(s.list.cursor, 1);
        update(&mut s, Action::CursorUp);
        assert_eq!(s.list.cursor, 0);
    }

    #[test]
    fn quit_sets_flag() {
        let mut s = test_state();
        update(&mut s, Action::Quit);
        assert!(s.should_quit);
    }

    #[test]
    fn back_at_root_quits() {
        // At the root list view, Back has nothing to pop — Phase 0 treats this
        // as a no-op (quit is via Quit/`q`). Assert it does not panic and stays.
        let mut s = test_state();
        let effects = update(&mut s, Action::Back);
        assert!(!s.should_quit);
        assert!(effects.is_empty());
    }
}
```

- [ ] **Step 4: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::action 2>&1 | tail -20`
Expected: FAIL — `Action`/`update` not found.

- [ ] **Step 5: Implement the reducer**

Top of `action.rs`:

```rust
//! Actions (user intents + async results) and the `update` reducer. The
//! reducer is the only place state mutates; it returns Effects for the
//! worker to run. Pure-ish: no I/O here.
use crate::tui::v2::effect::Effect;
use crate::tui::v2::resource::{Kind, Row};
use crate::tui::v2::state::AppState;

#[derive(Debug, Clone)]
pub enum Action {
    CursorDown,
    CursorUp,
    Back,
    Quit,
    ListLoaded { kind: Kind, rows: Vec<Row> },
    ListFailed { kind: Kind, error: String },
}

pub fn update(state: &mut AppState, action: Action) -> Vec<Effect> {
    match action {
        Action::CursorDown => {
            state.list.select_down();
            Vec::new()
        }
        Action::CursorUp => {
            state.list.select_up();
            Vec::new()
        }
        Action::Back => {
            // Phase 0 has only the root list; nothing to pop. No-op.
            let _ = state.nav.pop();
            Vec::new()
        }
        Action::Quit => {
            state.should_quit = true;
            Vec::new()
        }
        Action::ListLoaded { kind, rows } => {
            if state.list.kind == kind {
                state.list.rows = rows;
                state.list.loading = false;
                state.list.error = None;
                if state.list.cursor >= state.list.rows.len() {
                    state.list.cursor = state.list.rows.len().saturating_sub(1);
                }
            }
            Vec::new()
        }
        Action::ListFailed { kind, error } => {
            if state.list.kind == kind {
                state.list.loading = false;
                state.list.error = Some(error);
            }
            Vec::new()
        }
    }
}

/// The effect to fetch the current list view's data. Called by the entry loop
/// on startup and whenever a fresh load is needed.
pub fn initial_load_effect(state: &AppState) -> Effect {
    Effect::FetchList { kind: state.list.kind }
}
```

- [ ] **Step 6: Run to verify it passes**

Run: `cargo test -p ayx-rs --lib tui::v2::action 2>&1 | tail -20`
Expected: PASS (5 tests).

- [ ] **Step 7: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
git add ayx-rs/src/tui/v2/effect.rs ayx-rs/src/tui/v2/state.rs ayx-rs/src/tui/v2/action.rs
git commit -m "feat(tui-v2): AppState + Action reducer + Effect (TEA core)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Worker — generic effect executor

**Files:**
- Modify: `ayx-rs/src/tui/v2/worker.rs` (replace stub)
- Test: inline `#[cfg(test)]` using `httpmock` is **not** applicable here (the call path resolves auth/config); instead unit-test the pure payload→Action mapping function, and smoke the worker channel.

**Interfaces:**
- Consumes: `Config`, `Effect`, `Action`, `kind_impl`, `one_api_live_request`.
- Produces:
  - `struct Worker { tx: Sender<Job>, rx: Receiver<Outcome>, _handle: JoinHandle<()> }` with `spawn() -> Worker`, `submit(Effect, Config, RequestId)`, `try_recv() -> Result<Outcome, TryRecvError>`, `next_request_id() -> RequestId`.
  - `struct Outcome { id: RequestId, action: Action }`
  - `pub fn list_payload_to_action(kind: Kind, payload: Result<Value, String>) -> Action` (the pure mapper — unit tested).

- [ ] **Step 1: Write the failing test for the pure mapper**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::Kind;
    use serde_json::json;

    #[test]
    fn ok_payload_maps_to_list_loaded_with_rows() {
        let payload = Ok(json!({
            "data": [ { "id": "fl_1", "name": "ETL" }, { "id": "fl_2", "name": "Roll" } ]
        }));
        let action = list_payload_to_action(Kind::Flow, payload);
        match action {
            Action::ListLoaded { kind, rows } => {
                assert_eq!(kind, Kind::Flow);
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].cells[0].text, "ETL");
            }
            other => panic!("expected ListLoaded, got {other:?}"),
        }
    }

    #[test]
    fn err_payload_maps_to_list_failed() {
        let action = list_payload_to_action(Kind::Flow, Err("401 unauthorized".into()));
        match action {
            Action::ListFailed { error, .. } => assert!(error.contains("401")),
            other => panic!("expected ListFailed, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::worker 2>&1 | tail -20`
Expected: FAIL — `list_payload_to_action` not found.

- [ ] **Step 3: Implement the worker**

```rust
//! v2 worker: a single background thread that runs Effects off the UI thread
//! and returns Actions. Mirrors the legacy `tui/worker.rs` discipline
//! (monotonic RequestId, stale-result drop happens in the entry loop).
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};

use ayx_core::profile::Config;
use serde_json::Value;

use crate::tui::v2::action::Action;
use crate::tui::v2::effect::Effect;
use crate::tui::v2::resource::{kind_impl, Kind, Row};

pub type RequestId = u64;

fn next_id() -> RequestId {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

struct Job {
    id: RequestId,
    effect: Effect,
    config: Config,
}

pub struct Outcome {
    pub id: RequestId,
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

    pub fn submit(&self, effect: Effect, config: Config, id: RequestId) {
        let _ = self.tx.send(Job { id, effect, config });
    }

    pub fn try_recv(&self) -> Result<Outcome, TryRecvError> {
        self.rx.try_recv()
    }

    pub fn next_request_id() -> RequestId {
        next_id()
    }
}

fn worker_loop(rx: Receiver<Job>, tx: Sender<Outcome>) {
    while let Ok(job) = rx.recv() {
        let action = match job.effect {
            Effect::FetchList { kind } => {
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
                list_payload_to_action(kind, payload)
            }
        };
        let _ = tx.send(Outcome { id: job.id, action });
    }
}

/// Pure mapping from a raw list payload (or error) to an Action. Unit-tested.
pub fn list_payload_to_action(kind: Kind, payload: Result<Value, String>) -> Action {
    match payload {
        Ok(value) => {
            let imp = kind_impl(kind);
            let rows: Vec<Row> = imp.extract_items(&value).iter().map(|i| imp.row(i)).collect();
            Action::ListLoaded { kind, rows }
        }
        Err(error) => Action::ListFailed { kind, error },
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ayx-rs --lib tui::v2::worker 2>&1 | tail -20`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
git add ayx-rs/src/tui/v2/worker.rs
git commit -m "feat(tui-v2): worker — generic effect executor + pure payload→Action mapper

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Views — header, list+detail, footer

**Files:**
- Modify: `ayx-rs/src/tui/v2/view/mod.rs` (replace stub)
- Create: `ayx-rs/src/tui/v2/view/header.rs`, `ayx-rs/src/tui/v2/view/list.rs`, `ayx-rs/src/tui/v2/view/footer.rs`
- Test: inline `#[cfg(test)]` in `view/mod.rs` using ratatui `TestBackend`.

**Interfaces:**
- Consumes: `AppState`, `theme`, `kind_impl`, `Cell`, `StatusTone`.
- Produces: `pub fn render(frame: &mut ratatui::Frame, state: &AppState)`; internal `header::render`, `list::render`, `footer::render` each taking `(&mut Frame, &AppState, Rect)`.

- [ ] **Step 1: Write the failing TestBackend test**

In `view/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::context::Context;
    use crate::tui::v2::resource::{Kind, Row};
    use crate::tui::v2::state::AppState;
    use ratatui::{backend::TestBackend, Terminal};

    fn state_with_rows() -> AppState {
        let ctx = Context {
            profile: "wyatt".into(),
            workspace: "w_marketing".into(),
            user: "ryan@alteryx.com".into(),
        };
        let mut s = AppState::new(ctx);
        s.list.loading = false;
        s.list.rows = vec![
            Row { id: "fl_1".into(), cells: vec![
                crate::tui::v2::resource::Cell::plain("ETL Pipeline"),
                crate::tui::v2::resource::Cell::plain("2026-06-20"),
                crate::tui::v2::resource::Cell::plain("fl_1"),
            ]},
        ];
        s
    }

    fn rendered_text(state: &AppState) -> String {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, state)).unwrap();
        let buf = terminal.backend().buffer().clone();
        buf.content().iter().map(|c| c.symbol()).collect::<String>()
    }

    #[test]
    fn header_shows_context() {
        let text = rendered_text(&state_with_rows());
        assert!(text.contains("wyatt"));
        assert!(text.contains("w_marketing"));
        assert!(text.contains("ryan@alteryx.com"));
    }

    #[test]
    fn list_shows_flow_row_and_footer_hint() {
        let text = rendered_text(&state_with_rows());
        assert!(text.contains("ETL Pipeline"));
        assert!(text.contains("Palette")); // footer hint present
    }

    #[test]
    fn loading_state_renders_indicator() {
        let mut s = state_with_rows();
        s.list.loading = true;
        s.list.rows.clear();
        let text = {
            let backend = TestBackend::new(100, 20);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal.draw(|f| render(f, &s)).unwrap();
            terminal.backend().buffer().clone().content().iter().map(|c| c.symbol()).collect::<String>()
        };
        assert!(text.to_lowercase().contains("loading"));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::view 2>&1 | tail -20`
Expected: FAIL — `render` not implemented / submodules missing.

- [ ] **Step 3: Implement `view/header.rs`**

```rust
//! Context header + breadcrumb. Always visible — the guard against acting in
//! the wrong workspace.
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::theme;
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

    let crumb = Line::from(vec![
        Span::styled(" ", theme::dim()),
        Span::styled(state.nav.breadcrumb(), theme::dim()),
    ]);
    frame.render_widget(Paragraph::new(crumb), rows[1]);
}
```

- [ ] **Step 4: Implement `view/footer.rs`**

```rust
//! Contextual footer hint bar — plain-language labels, always visible.
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::theme;
use crate::tui::v2::state::AppState;

pub fn render(frame: &mut Frame, _state: &AppState, area: Rect) {
    // Phase 0 footer is fixed to the flow-list context. Later phases make this
    // change per focused view.
    let hint = Line::from(vec![
        key(" ↵ "), label("Open  "),
        key(" / "), label("Filter  "),
        key(" ^K "), label("Palette  "),
        key(" ? "), label("Help  "),
        key(" q "), label("Quit"),
    ]);
    frame.render_widget(Paragraph::new(hint).style(theme::panel()), area);
}

fn key(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), theme::accent_bold())
}
fn label(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), theme::dim())
}
```

- [ ] **Step 5: Implement `view/list.rs`**

```rust
//! Resource list table + reactive detail split panel.
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row as TRow, Table, TableState};
use ratatui::Frame;

use crate::tui::theme;
use crate::tui::v2::resource::{kind_impl, Cell, StatusTone};
use crate::tui::v2::state::AppState;

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let panes = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).split(area);
    render_table(frame, state, panes[0]);
    render_detail(frame, state, panes[1]);
}

fn tone_style(tone: StatusTone) -> Style {
    match tone {
        StatusTone::Neutral => theme::field_value(),
        StatusTone::Ok => theme::ok(),
        StatusTone::Warn => theme::warn(),
        StatusTone::Danger => theme::danger(),
    }
}

fn render_table(frame: &mut Frame, state: &AppState, area: Rect) {
    let imp = kind_impl(state.list.kind);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border(true))
        .title(Span::styled(format!(" {} · {} ", state.list.kind.name(), state.list.rows.len()), theme::accent()));

    if state.list.loading {
        frame.render_widget(Paragraph::new(" ⟳ loading… ").block(block).style(theme::dim()), area);
        return;
    }
    if let Some(err) = &state.list.error {
        frame.render_widget(
            Paragraph::new(format!(" error: {err} ")).block(block).style(theme::danger()),
            area,
        );
        return;
    }
    if state.list.rows.is_empty() {
        frame.render_widget(Paragraph::new(" no items ").block(block).style(theme::muted()), area);
        return;
    }

    let header = TRow::new(
        imp.columns().iter().map(|c| Span::styled(c.title, theme::muted())).collect::<Vec<_>>(),
    );
    let widths: Vec<Constraint> = imp.columns().iter().map(|c| Constraint::Length(c.width)).collect();
    let rows: Vec<TRow> = state.list.rows.iter().map(|r| {
        TRow::new(r.cells.iter().map(render_cell).collect::<Vec<_>>())
    }).collect();

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(theme::selected())
        .highlight_symbol("▸ ");
    let mut ts = TableState::default();
    ts.select(Some(state.list.cursor));
    frame.render_stateful_widget(table, area, &mut ts);
}

fn render_cell(cell: &Cell) -> Span<'static> {
    Span::styled(cell.text.clone(), tone_style(cell.tone))
}

fn render_detail(frame: &mut Frame, state: &AppState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border(false))
        .title(Span::styled(" detail ", theme::muted()));
    let lines: Vec<Line> = match state.list.selected() {
        Some(row) => row.cells.iter().enumerate().map(|(i, c)| {
            let title = kind_impl(state.list.kind).columns().get(i).map(|c| c.title).unwrap_or("");
            Line::from(vec![
                Span::styled(format!("{title}: "), theme::field_label()),
                Span::styled(c.text.clone(), tone_style(c.tone)),
            ])
        }).collect(),
        None => vec![Line::from(Span::styled("no selection", theme::muted()))],
    };
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
```

Note: in ratatui 0.30 the per-row highlight method is `Table::row_highlight_style`. If `cargo build` reports it as `highlight_style`, use that name instead — the build step below catches this immediately.

- [ ] **Step 6: Implement `view/mod.rs` dispatcher**

Top of `view/mod.rs` (above the test module):

```rust
//! Render dispatcher: context header (top) + body (list/detail) + footer.
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::Block;
use ratatui::Frame;

use crate::tui::theme;
use crate::tui::v2::state::AppState;

mod footer;
mod header;
mod list;

pub fn render(frame: &mut Frame, state: &AppState) {
    frame.render_widget(Block::default().style(theme::app()), frame.area());
    let chunks = Layout::vertical([
        Constraint::Length(2), // header + breadcrumb
        Constraint::Min(0),    // body
        Constraint::Length(1), // footer
    ])
    .split(frame.area());

    header::render(frame, state, chunks[0]);
    list::render(frame, state, chunks[1]);
    footer::render(frame, state, chunks[2]);
}
```

- [ ] **Step 7: Run the tests (and fix any ratatui API drift)**

Run: `cargo build -p ayx-rs 2>&1 | tail -30`
Expected: compiles. If `row_highlight_style` / `Table::new` widths API differs in 0.30, fix per the compiler message, then:

Run: `cargo test -p ayx-rs --lib tui::v2::view 2>&1 | tail -20`
Expected: PASS (3 tests).

- [ ] **Step 8: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
git add ayx-rs/src/tui/v2/view
git commit -m "feat(tui-v2): views — context header, list+reactive detail, footer

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 8: Entry loop + env gate (wire it together)

**Files:**
- Modify: `ayx-rs/src/tui/v2/entry.rs` (replace the Task 1 placeholder)
- Modify: `ayx-rs/src/tui/mod.rs` (add the env gate at the top of `run()`)
- Test: inline `#[cfg(test)]` in `entry.rs` — a headless drive test (no real terminal) that pumps an Action through and renders to `TestBackend`.

**Interfaces:**
- Consumes: everything above — `AppState`, `Context`, `update`, `initial_load_effect`, `Worker`, `view::render`, `App`-equivalent config load.
- Produces: `pub fn run() -> anyhow::Result<Envelope>`; `fn map_key(key: crossterm::event::KeyEvent) -> Option<Action>` (pure — unit tested).

- [ ] **Step 1: Write the failing test for `map_key`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn arrows_and_vim_keys_map_to_cursor() {
        assert!(matches!(map_key(k(KeyCode::Down)), Some(Action::CursorDown)));
        assert!(matches!(map_key(k(KeyCode::Char('j'))), Some(Action::CursorDown)));
        assert!(matches!(map_key(k(KeyCode::Up)), Some(Action::CursorUp)));
        assert!(matches!(map_key(k(KeyCode::Char('k'))), Some(Action::CursorUp)));
    }

    #[test]
    fn q_quits_esc_is_back() {
        assert!(matches!(map_key(k(KeyCode::Char('q'))), Some(Action::Quit)));
        assert!(matches!(map_key(k(KeyCode::Esc)), Some(Action::Back)));
    }

    #[test]
    fn unmapped_key_is_none() {
        assert!(map_key(k(KeyCode::Char('z'))).is_none());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --lib tui::v2::entry 2>&1 | tail -20`
Expected: FAIL — `map_key` not found.

- [ ] **Step 3: Implement the entry loop**

Replace `ayx-rs/src/tui/v2/entry.rs`:

```rust
//! TUI v2 entry: config load, terminal setup, the main loop. The loop only
//! drains worker outcomes, draws, and maps keys to Actions — it never blocks
//! on I/O.
use std::io;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use ayx_core::envelope::Envelope;
use ayx_core::profile::{resolve_runtime_profile, Config};

use crate::tui::v2::action::{initial_load_effect, update, Action};
use crate::tui::v2::context::Context;
use crate::tui::v2::state::AppState;
use crate::tui::v2::view;
use crate::tui::v2::worker::{RequestId, Worker};

pub fn run() -> Result<Envelope> {
    // Resolve the active profile + load its config (same source the legacy TUI
    // uses). `resolve_runtime_profile` returns the (name, path) to load.
    let resolved = resolve_runtime_profile(None)?;
    let config = Config::load_from_path_lenient_without_active_overlay(&resolved.path)
        .unwrap_or_default();
    let context = Context::from_config(&config, resolved.name.as_deref());

    let mut state = AppState::new(context);
    let worker = Worker::spawn();

    // Kick the initial flow-list fetch.
    let mut list_request: RequestId = Worker::next_request_id();
    worker.submit(initial_load_effect(&state), config.clone(), list_request);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        previous_hook(info);
    }));

    let loop_result = main_loop(&mut terminal, &mut state, &worker, &config, &mut list_request);

    drop(worker);
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    loop_result?;
    Ok(Envelope::ok("tui v2 session ended"))
}

fn main_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
    worker: &Worker,
    config: &Config,
    list_request: &mut RequestId,
) -> Result<()> {
    loop {
        // Drain any completed worker outcomes (stale-result drop on id).
        while let Ok(outcome) = worker.try_recv() {
            if outcome.id == *list_request {
                let effects = update(state, outcome.action);
                dispatch_effects(effects, worker, config, list_request);
            }
        }

        terminal.draw(|frame| view::render(frame, state))?;

        if event::poll(std::time::Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(action) = map_key(key)
        {
            let effects = update(state, action);
            dispatch_effects(effects, worker, config, list_request);
        }

        if state.should_quit {
            break;
        }
    }
    Ok(())
}

fn dispatch_effects(
    effects: Vec<crate::tui::v2::effect::Effect>,
    worker: &Worker,
    config: &Config,
    list_request: &mut RequestId,
) {
    for effect in effects {
        let id = Worker::next_request_id();
        *list_request = id;
        worker.submit(effect, config.clone(), id);
    }
}

/// Pure key→Action mapping. Unit-tested.
fn map_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => Some(Action::CursorDown),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::CursorUp),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Char('q') => Some(Action::Quit),
        _ => None,
    }
}
```

Note: confirm the `resolve_runtime_profile` signature and its return type's fields (`name`, `path`). From `ayx-core/src/profile.rs:2267`. If it returns a tuple or differently-named fields, adapt the two lines that read `resolved.path` / `resolved.name` — the rest is unaffected. If `Config` is not `Default`, replace `.unwrap_or_default()` with an explicit error surface (render an error state), but the simplest correct Phase-0 behavior is: on config load failure, still start with an empty context so the shell renders.

- [ ] **Step 4: Add the env gate to `tui/mod.rs`**

In `ayx-rs/src/tui/mod.rs`, at the very top of `pub fn run() -> Result<Envelope>` (line ~33), insert before `let mut app = App::new()?;`:

```rust
    // Strangler-fig: the rearchitected spine is opt-in until later phases port
    // the remaining screens. `AYX_TUI_V2=1 ayx tui` launches it.
    if std::env::var("AYX_TUI_V2").is_ok() {
        return v2::run();
    }
```

Ensure `mod v2;` is present among the module declarations (added in Task 1).

- [ ] **Step 5: Run the unit tests + build**

Run: `cargo test -p ayx-rs --lib tui::v2::entry 2>&1 | tail -20`
Expected: PASS (3 tests).

Run: `cargo build -p ayx-rs 2>&1 | tail -20`
Expected: compiles clean.

- [ ] **Step 6: Manual smoke (documented, not automated)**

Run: `AYX_TUI_V2=1 cargo run -p ayx-rs -- tui`
Expected: the v2 shell opens — context header (Profile · Workspace · User), a "flows · N" table that begins on "⟳ loading…" then populates from the active workspace (or shows "error: …" if not authed), a detail panel updating as you press `j`/`k`, and the footer hint bar. `q` quits cleanly with the terminal restored. Then verify the legacy path is untouched: `cargo run -p ayx-rs -- tui` (without the env var) opens the old interface.

- [ ] **Step 7: Full validation + commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
cargo nextest run -p ayx-rs --no-fail-fast
git add ayx-rs/src/tui/v2/entry.rs ayx-rs/src/tui/mod.rs
git commit -m "feat(tui-v2): entry loop + AYX_TUI_V2 env gate — Flows browse end-to-end

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage (Phase 0 row: "New module skeleton; AppState/Action/Effect/update reducer; worker → generic effect executor; context header + slim main loop; add deps. Wire Flows list only through the new spine. Existing tests stay green."):**
- Module skeleton → Task 1 (+ files seeded across tasks). ✓
- AppState/Action/Effect/update reducer → Task 5. ✓
- Worker → generic effect executor → Task 6. ✓
- Context header + slim main loop → Tasks 4, 7 (header), 8 (loop). ✓
- Wire Flows list end-to-end → Tasks 2, 6, 8. ✓
- "Add deps" → intentionally deferred to phase-of-use (palette/input/throbber are Phase 3+); Phase 0 uses ratatui built-ins only. Documented in Tech Stack + Global Constraints. ✓
- Existing tests stay green → only edit to legacy code is `mod v2;` + a 2-line gate; validated by `cargo nextest run -p ayx-rs` in Task 8. ✓
- Reactive detail panel, scrollable-ready detail, async-no-freeze → Tasks 7, 8 (detail updates on cursor move; all fetches via worker). ✓

**2. Placeholder scan:** No "TBD"/"handle errors appropriately"/"similar to Task N". Every code step shows complete code. Two explicit "verify the real signature and adapt" notes (ratatui `row_highlight_style`, `resolve_runtime_profile` return shape) are deliberate API-drift guards with the exact fallback named, not placeholders.

**3. Type consistency:** `Kind`, `Cell`, `StatusTone`, `Row`, `Column`, `ListEndpoint`, `ResourceKind`, `kind_impl` defined in Task 1, used consistently in Tasks 2/5/6/7. `Action` variants (`CursorDown/CursorUp/Back/Quit/ListLoaded/ListFailed`) defined in Task 5, consumed identically in Tasks 6 (`list_payload_to_action` produces `ListLoaded`/`ListFailed`) and 8 (`map_key` produces `CursorDown/CursorUp/Back/Quit`). `Effect::FetchList { kind }` defined Task 5, produced by `initial_load_effect` (Task 5) and consumed by the worker (Task 6) and `dispatch_effects` (Task 8). `Worker::{spawn,submit,try_recv,next_request_id}` + `Outcome { id, action }` defined Task 6, used in Task 8. Consistent.

**Phasing note:** Phases 1–5 (browser-core generalization, all assets, palette/discoverability, multi-workspace switching, actions/polish) each get their own plan once Phase 0 lands and the spine is proven. This plan delivers working, testable software on its own: `AYX_TUI_V2=1 ayx tui` browses flows in the new architecture.
