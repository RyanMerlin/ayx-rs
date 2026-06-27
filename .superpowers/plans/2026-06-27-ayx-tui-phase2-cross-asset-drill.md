# ayx TUI Rearchitecture — Phase 2 (Cross-Asset Drill) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add cross-asset navigation to the Phase-1 browser: from a **flow detail** press `r` to see that flow's **runs** (a scoped job list), and from a **job detail** press `f` to open its **parent flow's detail**. The nav stack grows a `ScopedList` rung so the breadcrumb reads `flows › "ETL Pipeline" › jobs`, and `Back` walks the stack correctly — all behind the existing `AYX_TUI_V2` gate.

**Architecture:** Same TEA spine, **LEAN nav model** (decided 2026-06-27): NO full view-state-stack refactor. Keep the single `state.list` + single `state.detail` slots. `state.list` holds whatever list is currently shown — root OR scoped child. The nav stack gains `View::ScopedList { child_kind, parent_kind, parent_id, parent_title }`. Because a scoped drill clobbers the single list/detail slots, `Back` becomes **rebuild-on-back**: pop the stack, then reconstruct `state.list`/`state.detail` from the new `nav.top()` and re-emit its fetch. Generation tokens (Phase 1) already make refetch-on-back safe — a stale in-flight result is dropped by token mismatch.

Two relations, both triggered from a **detail** view (where the full item JSON is available), keyed and shown in the footer:
- **(a) Flow → runs** (`r` on a flow detail): a scoped `Job` list filtered **client-side** to `job.flow_id == flow.id`. There is **no server filter** on `/v4/jobLibrary` (confirmed), so the filter lives in `list_payload_to_action` via a `scope` carried on the `FetchList` effect.
- **(b) Job → flow** (`f` on a job detail): open the `Flow` detail using the job's `flowId`/`flow_id`, read from the **open job detail's JSON** (`JobGroupSummary` carries `flow_id` — `ayx-one-api/src/types.rs:319-349`). Reuses the existing `do_open` helper.

**Tech Stack:** Rust (edition 2024), ratatui 0.30, crossterm 0.29, serde_json, anyhow. **No new dependencies.** **No backend/API changes** — both relations reuse the existing `FetchList`/`FetchDetail` effects and `/v4/jobLibrary` + `/v4/flows/{id}` endpoints.

## Global Constraints

Copied from the design spec and Phase-0/1/3 conventions — every task implicitly includes these:

- **Spec:** `.superpowers/specs/2026-06-26-ayx-tui-rearchitecture-design.md`. Builds on Phase 1 (PR #68) and Phase 3 (PR #69), both merged to `main`. This plan covers the **cross-asset drill** portion of the spec's Phase 2 row ("ResourceKind impls for Connections/Jobs/People/Workspaces; cross-asset drill (flow→runs, job→flow); nav stack + breadcrumb; status colors"). The asset impls, breadcrumb infra, and status colors already shipped in Phase 1 — **only the cross-asset drill + the `ScopedList` nav rung remain**, which is this plan.
- **Render loop must never block on I/O.** Relations emit the same `Effect`s as the existing reducer; no new blocking.
- **No backend/API changes.** No new endpoints.
- **Legacy TUI untouched.** All work is inside `tui/v2/`. Do not touch `tui/app.rs`, `tui/mod.rs`, `tui/store.rs`, `tui/forms.rs`, `tui/render_helpers.rs`.
- **Reuse the theme.** All colors via `crate::tui::theme`. No new hardcoded colors.
- **The reducer stays pure-ish:** no I/O in `update`/helpers; relations mutate state and return `Effect`s.
- **Cargo.lock is committed** (binary workspace) — but no deps change here, so it should not move.
- **Validation gate per task:** `cargo nextest run -p ayx-rs <filter>` (for a single filtered test on this binary crate use `cargo test -p ayx-rs --bin ayx <filter>`, **never `--lib`**), then before commit `cargo fmt --all && cargo clippy -p ayx-rs --all-targets -- -D warnings`.
- **Commits:** conventional; trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

---

## Scope

**In scope:** `View::ScopedList` nav rung + breadcrumb; a `scope` filter on `FetchList` (flow→runs, client-side `flow_id` match); `ShowRuns` (`r` on flow detail) and `OpenParentFlow` (`f` on job detail) actions + key routing + footer hints; rebuild-on-back so the nav stack walks correctly through scoped rungs.

**Explicitly deferred (later phases / not this plan):** Any other relation (connection→flows, person→jobs, etc.) — only the two locked relations ship. Workspace switching (Phase 4). Mutating actions — run/cancel/enable (Phase 5). Server-side job filtering (no endpoint exists; client-side is the locked decision). Preserving parent-list cursor/filter across a `Back` (rebuild-on-back refetches — see Self-Review follow-ups).

**YAGNI:** `ListScope` filters only `Kind::Flow` parents (the one scoped relation). Other parent kinds pass through unfiltered. Add match arms when a new scoped relation is actually built.

---

## File Structure

| File | Phase-2 change |
|------|----------------|
| `v2/effect.rs` | Add `pub struct ListScope { parent_kind: Kind, parent_id: String }`; add `scope: Option<ListScope>` to `Effect::FetchList`. |
| `v2/worker.rs` | `Effect::FetchList` arm passes `scope` through; `list_payload_to_action` gains a `scope` param and filters items before row-mapping (`item_in_scope` helper). |
| `v2/nav.rs` | Add `View::ScopedList { child_kind, parent_kind, parent_id, parent_title }` + its `crumb()` arm. |
| `v2/view/mod.rs` | `ScopedList` renders the list (`list::render`). |
| `v2/view/header.rs` | `ScopedList` renders the breadcrumb (like detail), not the tabs. |
| `v2/view/footer.rs` | `ScopedList` shows list hints + `⎋ Back`; the filtering guard includes `ScopedList`; detail footer gains relation hints (`r Runs` on flow detail, `f Flow` on job detail). |
| `v2/action.rs` | New actions `ShowRuns`, `OpenParentFlow`; helpers `do_show_runs`, `do_open_parent_flow`, `rebuild_for_top`; `Back` rewritten to pop + `rebuild_for_top`; `FetchList` constructors carry `scope`. |
| `v2/entry.rs` | Detail-view relation keys: `r` on a flow detail → `ShowRuns`; `f` on a job detail → `OpenParentFlow`. |

---

### Task 1: `ListScope` + scope-filtered list fetch

Add the scope plumbing: the `FetchList` effect carries an optional `ListScope`, and `list_payload_to_action` filters raw items by it **before** mapping to rows (so the filter sees `flow_id`, which the display `Row` does not carry). Nothing emits a non-`None` scope yet — Task 3 does — so all existing behavior is unchanged; this task just adds the capability and unit-tests the filter directly.

**Files:**
- Modify: `ayx-rs/src/tui/v2/effect.rs`
- Modify: `ayx-rs/src/tui/v2/worker.rs`
- Modify: `ayx-rs/src/tui/v2/action.rs` (constructor sites only — keep `scope: None`)
- Test: inline tests in `worker.rs`

**Interfaces:**
- `effect.rs`:
  ```rust
  #[derive(Debug, Clone)]
  pub struct ListScope {
      pub parent_kind: Kind,
      pub parent_id: String,
  }
  ```
  `Effect::FetchList { kind: Kind, token: u64, scope: Option<ListScope> }`.
- `worker.rs`: `pub fn list_payload_to_action(kind: Kind, token: u64, scope: Option<&ListScope>, payload: Result<Value, String>) -> Action`.

- [ ] **Step 1: Add `ListScope` + the effect field**

In `effect.rs`:
```rust
//! Effects: side-effect requests emitted by `update`, executed by the worker.
//! Each fetch carries a monotonic `token`; the reducer drops results whose
//! token no longer matches the target view (stale-result protection).
use crate::tui::v2::resource::Kind;

/// A list-fetch scope: restrict results to children of a parent resource.
/// Only `Kind::Flow` parents filter today (flow → runs); other kinds pass
/// through (see `worker::item_in_scope`).
#[derive(Debug, Clone)]
pub struct ListScope {
    pub parent_kind: Kind,
    pub parent_id: String,
}

#[derive(Debug, Clone)]
pub enum Effect {
    FetchList {
        kind: Kind,
        token: u64,
        scope: Option<ListScope>,
    },
    FetchDetail {
        kind: Kind,
        id: String,
        token: u64,
    },
}
```

- [ ] **Step 2: Update `worker.rs` — thread `scope` + add the filter (failing test first)**

Add the scope-filter tests to `worker.rs` tests:
```rust
    #[test]
    fn scope_filters_jobs_by_flow_id() {
        use crate::tui::v2::effect::ListScope;
        let payload = Ok(json!({ "data": [
            { "id": "jg_1", "flowId": "fl_a", "status": "Succeeded" },
            { "id": "jg_2", "flowId": "fl_b", "status": "Failed" },
            { "id": "jg_3", "flow_id": "fl_a", "status": "Running" }
        ]}));
        let scope = ListScope { parent_kind: Kind::Flow, parent_id: "fl_a".into() };
        match list_payload_to_action(Kind::Job, 1, Some(&scope), payload) {
            Action::ListLoaded { rows, .. } => {
                assert_eq!(rows.len(), 2, "only fl_a's jobs survive");
                assert!(rows.iter().all(|r| r.id == "jg_1" || r.id == "jg_3"));
            }
            other => panic!("expected ListLoaded, got {other:?}"),
        }
    }

    #[test]
    fn no_scope_keeps_all_items() {
        let payload = Ok(json!({ "data": [
            { "id": "jg_1", "flowId": "fl_a" }, { "id": "jg_2", "flowId": "fl_b" }
        ]}));
        match list_payload_to_action(Kind::Job, 1, None, payload) {
            Action::ListLoaded { rows, .. } => assert_eq!(rows.len(), 2),
            other => panic!("expected ListLoaded, got {other:?}"),
        }
    }
```
Update the **existing** worker tests that call `list_payload_to_action` (currently 2: `ok_payload_maps_to_list_loaded_with_rows`, `err_payload_maps_to_list_failed`) to pass `None` as the new third arg:
```rust
        match list_payload_to_action(Kind::Flow, 7, None, payload) { ... }
        match list_payload_to_action(Kind::Flow, 7, None, Err("401 unauthorized".into())) { ... }
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::worker 2>&1 | tail -20`
Expected: FAIL — `list_payload_to_action` still takes 3 args / `item_in_scope` missing.

- [ ] **Step 4: Implement the filter + signature**

In `worker.rs`, add the import and the helper, and change the `FetchList` arm + `list_payload_to_action`:
```rust
use crate::tui::v2::effect::{Effect, ListScope};
use crate::tui::v2::resource::{Kind, Row, kind_impl, str_field};
```
(`str_field` is `pub(crate)` in `resource/mod.rs` — re-export-free import works.)

In the `Effect::FetchList` arm, capture `scope` and pass it:
```rust
            Effect::FetchList { kind, token, scope } => {
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
                list_payload_to_action(kind, token, scope.as_ref(), payload)
            }
```
Rewrite `list_payload_to_action` + add `item_in_scope`:
```rust
/// Pure mapping from a raw list payload (or error) to an Action. When a `scope`
/// is present, items are filtered to the parent's children before row-mapping
/// (the display Row does not carry the parent id, so the filter must run on the
/// raw item JSON). Unit-tested.
pub fn list_payload_to_action(
    kind: Kind,
    token: u64,
    scope: Option<&ListScope>,
    payload: Result<Value, String>,
) -> Action {
    match payload {
        Ok(value) => {
            let imp = kind_impl(kind);
            let rows: Vec<Row> = imp
                .extract_items(&value)
                .iter()
                .filter(|item| scope.is_none_or(|s| item_in_scope(item, s)))
                .map(|i| imp.row(i))
                .collect();
            Action::ListLoaded { token, rows }
        }
        Err(error) => Action::ListFailed { token, error },
    }
}

/// Does `item` belong to `scope`'s parent? Only `Kind::Flow` parents filter
/// (flow → runs: keep jobs whose flow id matches). Other parent kinds have no
/// scoped relation yet, so they pass everything through.
fn item_in_scope(item: &Value, scope: &ListScope) -> bool {
    match scope.parent_kind {
        Kind::Flow => {
            str_field(item, &["flowId", "flow_id"]) == Some(scope.parent_id.as_str())
        }
        _ => true,
    }
}
```
Note (lint): `Option::is_none_or` is stable on the repo toolchain (1.96.0 per CI gate). If clippy/compiler rejects it, use `scope.map_or(true, |s| item_in_scope(item, s))`.

- [ ] **Step 5: Update the `FetchList` constructors in `action.rs` (keep `scope: None`)**

Two non-test constructor sites — `do_switch_kind` and `initial_load_effect`:
```rust
    vec![Effect::FetchList { kind, token, scope: None }]
```
```rust
    Effect::FetchList {
        kind: state.list.kind,
        token,
        scope: None,
    }
```
And update the two `action.rs` **test** match patterns that destructure `Effect::FetchList { kind, token }` to `{ kind, token, .. }` (in `switch_kind_resets_list_and_emits_fetch` and `palette_activate_switch_kind_resets_list_and_closes`) so they still compile.

- [ ] **Step 6: Run + commit**

Run: `cargo nextest run -p ayx-rs tui::v2 2>&1 | tail -6` → PASS.
Run: `cargo build -p ayx-rs 2>&1 | tail -5` → compiles.
```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/effect.rs ayx-rs/src/tui/v2/worker.rs ayx-rs/src/tui/v2/action.rs
git commit -m "feat(tui-v2): scope-filtered list fetch (flow→runs plumbing)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: `View::ScopedList` nav rung + render/header/footer arms

Add the scoped-list nav variant and teach the three exhaustive `nav.top()` matches about it: it renders as a **list**, shows a **breadcrumb** (not the resource tabs), and gets a list-style footer **with a Back hint**. No new actions yet — a `ScopedList` can't be reached from the reducer until Task 3, so this task only proves the rendering arms compile and behave.

**Files:**
- Modify: `ayx-rs/src/tui/v2/nav.rs`
- Modify: `ayx-rs/src/tui/v2/view/mod.rs`
- Modify: `ayx-rs/src/tui/v2/view/header.rs`
- Modify: `ayx-rs/src/tui/v2/view/footer.rs`
- Test: inline tests in `nav.rs`, `view/header.rs`, `view/footer.rs`

**Interfaces:**
- `nav.rs`:
  ```rust
  ScopedList {
      child_kind: Kind,
      parent_kind: Kind,
      parent_id: String,
      parent_title: String,
  }
  ```
  `crumb()` for `ScopedList` returns `child_kind.name()` (so the breadcrumb reads `flows › ETL Pipeline › jobs`).

- [ ] **Step 1: Add the variant + crumb (failing test first)**

In `nav.rs` tests, add:
```rust
    #[test]
    fn scoped_list_crumb_is_child_kind_name() {
        let mut nav = NavStack::new(View::ResourceList { kind: Kind::Flow });
        nav.push(View::ResourceDetail {
            kind: Kind::Flow,
            id: "fl_1".into(),
            title: "ETL Pipeline".into(),
        });
        nav.push(View::ScopedList {
            child_kind: Kind::Job,
            parent_kind: Kind::Flow,
            parent_id: "fl_1".into(),
            parent_title: "ETL Pipeline".into(),
        });
        assert_eq!(nav.breadcrumb(), "flows › ETL Pipeline › jobs");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::nav 2>&1 | tail -20`
Expected: FAIL — `ScopedList` variant unknown.

- [ ] **Step 3: Implement the variant + crumb**

In `nav.rs`, add to `enum View`:
```rust
    ScopedList {
        child_kind: Kind,
        parent_kind: Kind,
        parent_id: String,
        parent_title: String,
    },
```
In `crumb()`:
```rust
            View::ScopedList { child_kind, .. } => child_kind.name().to_string(),
```
(`parent_kind`/`parent_id`/`parent_title` are unused in `crumb` — they are read by the reducer's `rebuild_for_top` in Task 3. To avoid a dead-field warning before Task 3 lands, this is acceptable: the fields are `pub`-accessed in tests here and in Task 3. If the workspace lints `dead_code` on the unread fields, Task 3 immediately reads them; within this task they are exercised by the `scoped_list_crumb_is_child_kind_name` test constructing the variant. `#[allow(dead_code)]` is **not** needed because constructing + pattern-matching in tests counts as use.)

- [ ] **Step 4: `view/mod.rs` — render `ScopedList` as a list**

Change the body match:
```rust
    match state.nav.top() {
        View::ResourceList { .. } | View::ScopedList { .. } => {
            list::render(frame, state, chunks[1])
        }
        View::ResourceDetail { .. } => detail::render(frame, state, chunks[1]),
    }
```

- [ ] **Step 5: `view/header.rs` — breadcrumb for `ScopedList`**

Change the match so only the root list shows tabs; scoped lists and details show the breadcrumb:
```rust
    match state.nav.top() {
        View::ResourceList { .. } => {
            frame.render_widget(Paragraph::new(tabs_line(state.list.kind)), rows[1]);
        }
        View::ResourceDetail { .. } | View::ScopedList { .. } => {
            let crumb = Line::from(vec![
                Span::styled(" ", theme::dim()),
                Span::styled(state.nav.breadcrumb(), theme::dim()),
            ]);
            frame.render_widget(Paragraph::new(crumb), rows[1]);
        }
    }
```
Add a header test:
```rust
    #[test]
    fn scoped_list_shows_breadcrumb_not_tabs() {
        let ctx = Context { profile: "w".into(), workspace: "w".into(), user: "u".into() };
        let mut s = AppState::new(ctx);
        s.list = crate::tui::v2::state::ListView::new(Kind::Job);
        s.nav.push(crate::tui::v2::nav::View::ResourceDetail {
            kind: Kind::Flow, id: "fl_1".into(), title: "ETL Pipeline".into(),
        });
        s.nav.push(crate::tui::v2::nav::View::ScopedList {
            child_kind: Kind::Job, parent_kind: Kind::Flow,
            parent_id: "fl_1".into(), parent_title: "ETL Pipeline".into(),
        });
        let txt = text_for(&s);
        assert!(txt.contains("ETL Pipeline"));
        assert!(txt.contains("jobs"));
    }
```

- [ ] **Step 6: `view/footer.rs` — `ScopedList` = list hints + Back; filtering guard includes it**

Update the filtering guard (top of `render`) so a scoped list also shows the filter footer:
```rust
    let hint = if state.list.filtering
        && matches!(
            state.nav.top(),
            View::ResourceList { .. } | View::ScopedList { .. }
        )
    {
```
Add a `ScopedList` arm to the main match. It reuses the list hints but **adds `⎋ Back`** (a scoped list can pop, unlike the root). Factor the shared list spans or duplicate inline — duplicating is fine and explicit:
```rust
            View::ScopedList { .. } => {
                let mut spans = Vec::new();
                if kind_impl(state.list.kind).detail_endpoint().is_some() {
                    spans.push(key(" ↵ "));
                    spans.push(label("Open  "));
                }
                spans.push(key(" / "));
                spans.push(label("Filter  "));
                spans.push(key(" ⎋ "));
                spans.push(label("Back  "));
                spans.push(key(" ^K "));
                spans.push(label("Palette  "));
                spans.push(key(" ? "));
                spans.push(label("Help  "));
                spans.push(key(" q "));
                spans.push(label("Quit"));
                Line::from(spans)
            }
            View::ResourceList { .. } => { /* unchanged */ }
```
Add a footer test:
```rust
    #[test]
    fn scoped_list_footer_has_back() {
        let mut s = base();
        s.list = crate::tui::v2::state::ListView::new(Kind::Job);
        s.nav.push(crate::tui::v2::nav::View::ScopedList {
            child_kind: Kind::Job, parent_kind: Kind::Flow,
            parent_id: "fl_1".into(), parent_title: "ETL".into(),
        });
        let txt = text_for(&s);
        assert!(txt.contains("Back"));
        assert!(txt.contains("Filter"));
    }
```

- [ ] **Step 7: Run + commit**

Run: `cargo nextest run -p ayx-rs tui::v2 2>&1 | tail -6` → PASS.
```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/nav.rs ayx-rs/src/tui/v2/view/mod.rs ayx-rs/src/tui/v2/view/header.rs ayx-rs/src/tui/v2/view/footer.rs
git commit -m "feat(tui-v2): ScopedList nav rung — render as list, breadcrumb, back-hint footer

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Relation reducer actions + rebuild-on-back

Add the two relation actions and rewrite `Back` to the lean rebuild-on-back model. This is the heart of Phase 2.

**Files:**
- Modify: `ayx-rs/src/tui/v2/action.rs`
- Test: inline tests in `action.rs`

**Interfaces:**
- New `Action` variants: `ShowRuns`, `OpenParentFlow`.
- Helpers:
  - `fn do_show_runs(state: &mut AppState) -> Vec<Effect>` — on a flow detail, push `ScopedList{child=Job, parent=Flow,...}`, swap `state.list` to a fresh `Job` list, clear `state.detail`, emit `FetchList{Job, scope: Some(flow)}`.
  - `fn do_open_parent_flow(state: &mut AppState) -> Vec<Effect>` — on a job detail, read `flowId`/`flow_id` (+ `flowName`) from `state.detail.json`, then `do_open(Flow, id, name)`.
  - `fn rebuild_for_top(state: &mut AppState) -> Vec<Effect>` — reconstruct `state.list`/`state.detail` from `nav.top()` and emit its fetch.

- [ ] **Step 1: Write the failing reducer tests**

In `action.rs` tests (helpers `test_state`, `rows`, `initial_load_effect` exist; add `use crate::tui::v2::nav::View;` and `use crate::tui::v2::resource::Kind;` locally as the existing tests do):
```rust
    #[test]
    fn show_runs_pushes_scoped_jobs_and_fetches_with_scope() {
        use crate::tui::v2::effect::ListScope;
        use crate::tui::v2::nav::View;
        use crate::tui::v2::resource::Kind;
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let lt = s.list.token;
        update(&mut s, Action::ListLoaded { token: lt, rows: rows(1) });
        update(&mut s, Action::Open); // flow detail fl_0
        let effects = update(&mut s, Action::ShowRuns);
        assert!(s.detail.is_none());
        assert_eq!(s.list.kind, Kind::Job);
        assert!(matches!(
            s.nav.top(),
            View::ScopedList { child_kind: Kind::Job, parent_kind: Kind::Flow, .. }
        ));
        match effects.as_slice() {
            [Effect::FetchList { kind: Kind::Job, token, scope: Some(ListScope { parent_id, .. }) }] => {
                assert_eq!(parent_id, "fl_0");
                assert_eq!(*token, s.list.token);
            }
            other => panic!("expected scoped FetchList(Job), got {other:?}"),
        }
    }

    #[test]
    fn show_runs_is_noop_off_a_flow_detail() {
        // On a non-flow detail (or no detail), ShowRuns does nothing.
        let mut s = test_state();
        let effects = update(&mut s, Action::ShowRuns);
        assert!(effects.is_empty());
        assert!(s.detail.is_none());
    }

    #[test]
    fn open_parent_flow_opens_flow_detail_from_job_json() {
        use crate::tui::v2::nav::View;
        use crate::tui::v2::resource::Kind;
        use serde_json::json;
        let mut s = test_state();
        update(&mut s, Action::SwitchKind(Kind::Job));
        let lt = s.list.token;
        update(&mut s, Action::ListLoaded { token: lt, rows: rows(1) });
        update(&mut s, Action::Open); // job detail
        let dt = s.detail.as_ref().unwrap().token;
        update(&mut s, Action::DetailLoaded {
            token: dt,
            json: json!({ "id": "jg_0", "flowId": "fl_42", "flowName": "Daily ETL" }),
        });
        let effects = update(&mut s, Action::OpenParentFlow);
        let d = s.detail.as_ref().expect("flow detail opened");
        assert_eq!(d.kind, Kind::Flow);
        assert_eq!(d.id, "fl_42");
        assert!(matches!(s.nav.top(), View::ResourceDetail { kind: Kind::Flow, .. }));
        assert!(matches!(
            effects.as_slice(),
            [Effect::FetchDetail { kind: Kind::Flow, .. }]
        ));
    }

    #[test]
    fn open_parent_flow_noop_when_job_json_lacks_flow_id() {
        use crate::tui::v2::resource::Kind;
        use serde_json::json;
        let mut s = test_state();
        update(&mut s, Action::SwitchKind(Kind::Job));
        let lt = s.list.token;
        update(&mut s, Action::ListLoaded { token: lt, rows: rows(1) });
        update(&mut s, Action::Open);
        let dt = s.detail.as_ref().unwrap().token;
        update(&mut s, Action::DetailLoaded { token: dt, json: json!({ "id": "jg_0" }) });
        let before = s.detail.as_ref().unwrap().kind;
        let effects = update(&mut s, Action::OpenParentFlow);
        assert!(effects.is_empty());
        assert_eq!(s.detail.as_ref().unwrap().kind, before, "still on the job detail");
    }

    #[test]
    fn back_from_scoped_list_rebuilds_parent_detail_and_refetches() {
        use crate::tui::v2::nav::View;
        use crate::tui::v2::resource::Kind;
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let lt = s.list.token;
        update(&mut s, Action::ListLoaded { token: lt, rows: rows(1) });
        update(&mut s, Action::Open);     // flow detail fl_0
        update(&mut s, Action::ShowRuns); // scoped jobs
        let effects = update(&mut s, Action::Back);
        assert!(matches!(s.nav.top(), View::ResourceDetail { kind: Kind::Flow, .. }));
        let d = s.detail.as_ref().expect("parent flow detail rebuilt");
        assert!(d.loading);
        assert_eq!(d.id, "fl_0");
        match effects.as_slice() {
            [Effect::FetchDetail { kind: Kind::Flow, id, token }] => {
                assert_eq!(id, "fl_0");
                assert_eq!(*token, d.token);
            }
            other => panic!("expected FetchDetail(Flow), got {other:?}"),
        }
    }

    #[test]
    fn back_from_detail_rebuilds_root_list_and_refetches() {
        use crate::tui::v2::nav::View;
        use crate::tui::v2::resource::Kind;
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let lt = s.list.token;
        update(&mut s, Action::ListLoaded { token: lt, rows: rows(2) });
        update(&mut s, Action::Open);
        let effects = update(&mut s, Action::Back);
        assert!(s.detail.is_none());
        assert!(matches!(s.nav.top(), View::ResourceList { kind: Kind::Flow }));
        assert!(s.list.loading);
        assert!(matches!(
            effects.as_slice(),
            [Effect::FetchList { kind: Kind::Flow, scope: None, .. }]
        ));
    }
```
The existing `back_clears_detail_and_pops` and `back_at_root_quits` tests stay green: the former checks only `detail.is_none()` + `nav.top()` (both still hold), the latter relies on a root `Back` not popping (so `rebuild_for_top` never runs and effects stay empty).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::action 2>&1 | tail -20`
Expected: FAIL — `ShowRuns`/`OpenParentFlow` unknown.

- [ ] **Step 3: Add imports + actions**

In `action.rs`, extend imports:
```rust
use crate::tui::v2::effect::{Effect, ListScope};
use crate::tui::v2::nav::{NavStack, View};
use crate::tui::v2::resource::{Kind, Row, kind_impl, str_field};
use crate::tui::v2::state::{AppState, DetailView, ListView};
```
Add to `enum Action`:
```rust
    ShowRuns,
    OpenParentFlow,
```

- [ ] **Step 4: Add the helpers**

After `do_open` (and `do_switch_kind_if_needed_then_open`), add:
```rust
/// Flow → runs: from an open flow detail, drill into a scoped Job list filtered
/// to this flow's runs. Replaces the single list slot (lean nav model) and
/// records the relation on the nav stack so Back can rebuild the flow detail.
fn do_show_runs(state: &mut AppState) -> Vec<Effect> {
    let Some(detail) = state.detail.as_ref() else {
        return Vec::new();
    };
    if detail.kind != Kind::Flow {
        return Vec::new();
    }
    let parent_id = detail.id.clone();
    let parent_title = detail.title.clone();

    state.nav.push(View::ScopedList {
        child_kind: Kind::Job,
        parent_kind: Kind::Flow,
        parent_id: parent_id.clone(),
        parent_title,
    });
    state.list = ListView::new(Kind::Job);
    state.detail = None;
    let token = mint_token(state);
    state.list.token = token;
    vec![Effect::FetchList {
        kind: Kind::Job,
        token,
        scope: Some(ListScope {
            parent_kind: Kind::Flow,
            parent_id,
        }),
    }]
}

/// Job → flow: from an open job detail, open the parent flow's detail. The flow
/// id comes from the job detail's JSON (`JobGroupSummary` carries `flow_id`).
/// No-op if the json is absent or has no flow id.
fn do_open_parent_flow(state: &mut AppState) -> Vec<Effect> {
    let Some(detail) = state.detail.as_ref() else {
        return Vec::new();
    };
    if detail.kind != Kind::Job {
        return Vec::new();
    }
    let Some(json) = detail.json.as_ref() else {
        return Vec::new();
    };
    let flow_id = str_field(json, &["flowId", "flow_id"])
        .unwrap_or_default()
        .to_string();
    if flow_id.is_empty() {
        return Vec::new();
    }
    let title = str_field(json, &["flowName", "flow_name"])
        .unwrap_or(&flow_id)
        .to_string();
    do_open(state, Kind::Flow, flow_id, title)
}

/// Rebuild `state.list`/`state.detail` from the current `nav.top()` and emit the
/// fetch that refills it. Called after a `Back` pop: the lean nav model keeps
/// only one list + one detail slot, so a scoped drill clobbers them — walking
/// back up must reconstruct the slot for the revealed view. Generation tokens
/// make the refetch safe (a stale in-flight result is dropped on token
/// mismatch).
fn rebuild_for_top(state: &mut AppState) -> Vec<Effect> {
    match state.nav.top().clone() {
        View::ResourceList { kind } => {
            state.detail = None;
            state.list = ListView::new(kind);
            let token = mint_token(state);
            state.list.token = token;
            vec![Effect::FetchList { kind, token, scope: None }]
        }
        View::ScopedList { child_kind, parent_kind, parent_id, .. } => {
            state.detail = None;
            state.list = ListView::new(child_kind);
            let token = mint_token(state);
            state.list.token = token;
            vec![Effect::FetchList {
                kind: child_kind,
                token,
                scope: Some(ListScope { parent_kind, parent_id }),
            }]
        }
        View::ResourceDetail { kind, id, title } => {
            let token = mint_token(state);
            state.detail = Some(DetailView::new(kind, id.clone(), title, token));
            vec![Effect::FetchDetail { kind, id, token }]
        }
    }
}
```

- [ ] **Step 5: Wire the actions + rewrite `Back`**

Add reducer arms (near `Open`):
```rust
        Action::ShowRuns => do_show_runs(state),
        Action::OpenParentFlow => do_open_parent_flow(state),
```
Replace the `Action::Back` arm:
```rust
        Action::Back => {
            if state.nav.pop() {
                rebuild_for_top(state)
            } else {
                Vec::new()
            }
        }
```

- [ ] **Step 6: Run to verify it passes**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::action 2>&1 | tail -20`
Expected: PASS (new relation/back tests + existing).

- [ ] **Step 7: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/action.rs
git commit -m "feat(tui-v2): flow→runs / job→flow relations + rebuild-on-back

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Relation key routing in `entry.rs`

Wire the detail-view keys: `r` on a flow detail → `ShowRuns`; `f` on a job detail → `OpenParentFlow`. Keys are gated on the **open detail's kind**, so they never collide with list bindings (on a list there is no `state.detail`, and `r`/`f` are unmapped there).

**Files:**
- Modify: `ayx-rs/src/tui/v2/entry.rs`
- Test: inline tests in `entry.rs`

- [ ] **Step 1: Write the failing tests**

In `entry.rs` tests, add imports + cases:
```rust
    use crate::tui::v2::nav::View;
    use crate::tui::v2::resource::Kind;
    use crate::tui::v2::state::DetailView;

    fn flow_detail_state() -> crate::tui::v2::state::AppState {
        let mut s = list_state();
        s.nav.push(View::ResourceDetail { kind: Kind::Flow, id: "fl_1".into(), title: "ETL".into() });
        s.detail = Some(DetailView::new(Kind::Flow, "fl_1".into(), "ETL".into(), 1));
        s
    }
    fn job_detail_state() -> crate::tui::v2::state::AppState {
        let mut s = list_state();
        s.nav.push(View::ResourceDetail { kind: Kind::Job, id: "jg_1".into(), title: "run".into() });
        s.detail = Some(DetailView::new(Kind::Job, "jg_1".into(), "run".into(), 1));
        s
    }

    #[test]
    fn r_on_flow_detail_shows_runs() {
        assert!(matches!(map_key(&flow_detail_state(), k(KeyCode::Char('r'))), Some(Action::ShowRuns)));
    }
    #[test]
    fn f_on_job_detail_opens_parent_flow() {
        assert!(matches!(map_key(&job_detail_state(), k(KeyCode::Char('f'))), Some(Action::OpenParentFlow)));
    }
    #[test]
    fn relation_keys_inert_on_wrong_detail_kind() {
        // f on a flow detail and r on a job detail are not relation keys.
        assert!(map_key(&flow_detail_state(), k(KeyCode::Char('f'))).is_none());
        assert!(map_key(&job_detail_state(), k(KeyCode::Char('r'))).is_none());
    }
    #[test]
    fn relation_keys_inert_on_list() {
        let s = list_state(); // flow list, no detail
        assert!(map_key(&s, k(KeyCode::Char('r'))).is_none());
        assert!(map_key(&s, k(KeyCode::Char('f'))).is_none());
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::entry 2>&1 | tail -20`
Expected: FAIL — `r`/`f` map to `None` today.

- [ ] **Step 3: Add the relation routing**

In `map_key`, insert a block **after** the filter-input block (step 4) and **before** the normal bindings (step 5):
```rust
    // 4.5) Detail-view relation keys (cross-asset drill). Gated on the open
    // detail's kind so they never shadow list bindings (`r`/`f` are unmapped on
    // a list, where `state.detail` is None).
    if let Some(detail) = state.detail.as_ref() {
        match (detail.kind, key.code) {
            (Kind::Flow, KeyCode::Char('r')) => return Some(Action::ShowRuns),
            (Kind::Job, KeyCode::Char('f')) => return Some(Action::OpenParentFlow),
            _ => {}
        }
    }
```
(`Kind` is already imported inside `map_key` via `use crate::tui::v2::resource::Kind;`.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::entry 2>&1 | tail -20`
Expected: PASS. Existing entry tests stay green (they use `list_state()` with no detail, so the new block is skipped; detail scroll via `j`/`k` and `↵/⎋` Back still fall through to normal bindings).

- [ ] **Step 5: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/entry.rs
git commit -m "feat(tui-v2): r=runs (flow detail) / f=flow (job detail) key routing

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Relation hints in the detail footer

Teach the detail footer to advertise the relation key: `r Runs` on a flow detail, `f Flow` on a job detail. (The `ScopedList` footer with its `⎋ Back` already landed in Task 2.)

**Files:**
- Modify: `ayx-rs/src/tui/v2/view/footer.rs`
- Test: inline tests in `view/footer.rs`

- [ ] **Step 1: Write the failing tests**

In `footer.rs` tests, add:
```rust
    #[test]
    fn flow_detail_footer_has_runs_hint() {
        let mut s = base();
        s.nav.push(crate::tui::v2::nav::View::ResourceDetail {
            kind: Kind::Flow, id: "fl_1".into(), title: "ETL".into(),
        });
        s.detail = Some(DetailView::new(Kind::Flow, "fl_1".into(), "ETL".into(), 1));
        let txt = text_for(&s);
        assert!(txt.contains("Runs"));
        assert!(txt.contains("Back"));
    }
    #[test]
    fn job_detail_footer_has_flow_hint() {
        let mut s = base();
        s.nav.push(crate::tui::v2::nav::View::ResourceDetail {
            kind: Kind::Job, id: "jg_1".into(), title: "run".into(),
        });
        s.detail = Some(DetailView::new(Kind::Job, "jg_1".into(), "run".into(), 1));
        let txt = text_for(&s);
        assert!(txt.contains("Flow"));
    }
```
(The existing `detail_footer_has_back_and_scroll` test uses a `Kind::Flow` detail — it will now also contain "Runs"; that test only asserts `Back`/`Scroll`/`Palette`, so it stays green.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::view::footer 2>&1 | tail -20`
Expected: FAIL — no `Runs`/`Flow` hint yet.

- [ ] **Step 3: Implement — prepend a relation hint to the detail footer**

In the `View::ResourceDetail { .. }` arm of `footer.rs`, build the spans with a leading relation hint based on `state.detail`'s kind:
```rust
            View::ResourceDetail { .. } => {
                let mut spans = vec![key(" ↑↓ "), label("Scroll  ")];
                match state.detail.as_ref().map(|d| d.kind) {
                    Some(crate::tui::v2::resource::Kind::Flow) => {
                        spans.push(key(" r "));
                        spans.push(label("Runs  "));
                    }
                    Some(crate::tui::v2::resource::Kind::Job) => {
                        spans.push(key(" f "));
                        spans.push(label("Flow  "));
                    }
                    _ => {}
                }
                spans.push(key(" ↵/⎋ "));
                spans.push(label("Back  "));
                spans.push(key(" ^K "));
                spans.push(label("Palette  "));
                spans.push(key(" ? "));
                spans.push(label("Help  "));
                spans.push(key(" q "));
                spans.push(label("Quit"));
                Line::from(spans)
            }
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::view::footer 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/view/footer.rs
git commit -m "feat(tui-v2): detail footer advertises r Runs / f Flow relations

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Final validation + manual smoke + STATUS

**Files:**
- Modify: `.superpowers/plans/2026-06-27-ayx-tui-phase2-cross-asset-drill.md` (STATUS)

- [ ] **Step 1: Full gate**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace 2>&1 | tail -5
```
Expected: fmt clean, clippy clean, all tests pass (Phase-3 baseline 440 + the new Phase-2 tests).

- [ ] **Step 2: Manual smoke (documented; needs a TTY + authed workspace)**

```bash
AYX_TUI_V2=1 cargo run -p ayx-rs -- tui
```
Verify:
1. Open a **flow** (`↵`) → on the flow detail the footer shows `r Runs` → press `r` → a scoped **jobs** list appears, breadcrumb reads `flows › <flow> › jobs`, and only that flow's runs are listed.
2. `↵` on a run → job detail; `⎋` → back to the scoped jobs list (refetches); `⎋` again → back to the flow detail (refetches); `⎋` again → back to the flow list.
3. Switch to **Jobs** (`3`), open a job → footer shows `f Flow` → press `f` → that job's parent **flow detail** opens (id from the job JSON's `flowId`).
4. `1`–`5`/`Tab` from a scoped list escapes the scope back to a root list.
5. Legacy path (`ayx tui`, no env var) is intact.

Capture: whether the job **detail** endpoint (`/v4/jobGroups/{id}`) actually returns `flowId`/`flow_id` (the `f` relation depends on it). If a real job detail lacks it, log a follow-up to capture `flow_id` at list time instead (the job **list** row already has it via `JobGroupSummary`).

- [ ] **Step 3: Mark complete + commit**

Append a STATUS section (date, commit range, suite count, deferred items), then:
```bash
cd /home/merlin/code/ayx-rs
git add .superpowers/plans/2026-06-27-ayx-tui-phase2-cross-asset-drill.md
git commit -m "docs(tui-v2): mark Phase 2 cross-asset-drill plan complete

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage** (spec Phase 2 row: "ResourceKind impls … cross-asset drill (flow→runs, job→flow); nav stack + breadcrumb; status colors"):
- ResourceKind impls for all 5 assets → **already shipped Phase 1.** ✓ (out of this plan)
- status colors → **already shipped Phase 1** (`tone_style` in `view/list.rs`). ✓ (out of this plan)
- nav stack + breadcrumb → breadcrumb infra exists (Phase 1); this plan adds the `ScopedList` rung so drills appear in it → Task 2. ✓
- cross-asset drill (flow→runs) → Tasks 1 (scope filter), 3 (`ShowRuns`), 4 (key), 5 (footer). ✓
- cross-asset drill (job→flow) → Tasks 3 (`OpenParentFlow`), 4 (key), 5 (footer). ✓

**2. Placeholder scan:** No "TBD"/"similar to Task N". Every code step is complete. The one API-uncertainty (does the job *detail* endpoint return `flowId`?) is handled by a graceful no-op + a documented live-smoke check + a named fallback (capture `flow_id` at list time), not a placeholder.

**3. Type consistency:** `ListScope` defined Task 1 (effect.rs), consumed by `worker::list_payload_to_action` (Task 1) and `action::{do_show_runs, rebuild_for_top}` (Task 3). `Effect::FetchList.scope` added Task 1; every constructor updated same task (`do_switch_kind`, `initial_load_effect`) or later (`do_show_runs`, `rebuild_for_top`). `View::ScopedList` defined Task 2 (nav.rs), matched in `view/mod.rs`/`header.rs`/`footer.rs` (Task 2) and `rebuild_for_top` (Task 3). `Action::{ShowRuns, OpenParentFlow}` defined + handled Task 3, routed Task 4, hinted Task 5. `str_field` (pub(crate), resource/mod.rs) imported by worker.rs (Task 1) + action.rs (Task 3). Consistent.

**4. Lean-model tradeoff (called out):** rebuild-on-back **refetches** the revealed view and resets its cursor/filter. This is the accepted cost of the single-slot model (locked decision 2026-06-27) and is safe via generation tokens. It changes Phase-1 behavior slightly: `Esc` from a *root* detail now reloads the flow list (brief `⟳ loading…`) instead of restoring it instantly. Acceptable for a k9s-style browser (k9s refetches constantly). **Follow-up (non-blocking):** cache the parent list snapshot on the nav rung to restore instantly without a refetch, preserving cursor/filter.

**Phasing note:** Delivers working software — `AYX_TUI_V2=1 ayx tui` gains flow→runs and job→flow drill with a correct breadcrumb and back-stack. Phases 4 (workspace switch) and 5 (mutating actions) are untouched and unblocked.

## STATUS

_(to be filled in at Task 6)_
