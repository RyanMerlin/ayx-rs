# ayx TUI Rearchitecture — Phase 3 (Palette & Discoverability) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the spec's headline control scheme on top of the Phase-1 browser: a `Ctrl+K` fuzzy command palette (unified resource + item results, `nucleo-matcher` ranked), a `?` contextual help overlay, and proper cursor text editing everywhere via `tui-input` (replacing the append-only filter) — all behind the existing `AYX_TUI_V2` gate.

**Architecture:** Same TEA spine. New stateful text buffers (`tui_input::Input`) live in `AppState`; editing happens in the pure reducer by feeding `tui_input::InputRequest` values (the entry loop maps keys → `InputRequest`, so no crossterm event types cross into `action.rs`). The palette is a modal overlay: when open it captures keys, ranks entries with `nucleo-matcher`, and on activate dispatches the same effects as `SwitchKind`/`Open`. Help is a read-only overlay.

**Tech Stack:** Rust (edition 2024), ratatui 0.30, crossterm 0.29, serde_json, anyhow, **+ `nucleo-matcher` 0.3, `tui-input` 0.11 (pure API — `crossterm` backend feature OFF)**.

## Global Constraints

Copied verbatim from the design spec and Phase-0/1 conventions — every task implicitly includes these:

- **Spec:** `.superpowers/specs/2026-06-26-ayx-tui-rearchitecture-design.md` (commit `8eaa9dd`). Builds on Phase 1 (merged, PR #68). The control scheme is spec §"Control scheme (three layers)".
- **Opener key is `Ctrl+K`, not `Ctrl+P`** (zellij binds `Ctrl+P` to pane-mode). `?` opens help. The VSCode `>` commands-only prefix is **optional, never required** — do not require any sigil.
- **Render loop must never block on I/O.** Palette activation emits the same `Effect`s as the existing reducer; no new blocking.
- **No backend/API changes.** No new endpoints; the palette reuses `FetchList`/`FetchDetail`.
- **Legacy TUI untouched.** All work is inside `tui/v2/` (plus the two `Cargo.toml` dependency additions). Do not touch `tui/app.rs`, `tui/mod.rs` (beyond nothing here), `tui/store.rs`, `tui/forms.rs`, `tui/render_helpers.rs`.
- **`tui-input` is used via its pure API only** — `Input`, `InputRequest`, `input.handle(req)`, `input.value()`, `input.visual_cursor()`, `input.reset()`. Do **NOT** use `tui_input::backend::crossterm` / `handle_event` / `to_input_request` (that path pulls tui-input's own crossterm and risks a second crossterm version). Add the dep with `default-features = false`.
- **Reuse the theme.** All colors via `crate::tui::theme`. No new hardcoded colors.
- **Cargo.lock is committed** (binary workspace). After adding deps, commit the updated `Cargo.lock`.
- **Validation gate per task:** `cargo nextest run -p ayx-rs <filter>` (binary crate — `cargo test -p ayx-rs --bin ayx <filter>`, **never `--lib`**), then before commit `cargo fmt --all && cargo clippy -p ayx-rs --all-targets -- -D warnings`.
- **Commits:** conventional; trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

---

## Scope

**In scope:** `Ctrl+K` fuzzy palette with two result categories available today — **Resources** (Browse Flows/Connections/Jobs/People/Workspaces) and **Items** (open a row in the current list); `nucleo-matcher` ranking; `?` help overlay; `tui-input` cursor editing for the in-list `/` filter and the palette query.

**Explicitly deferred (later phases):** Workspace switch entries in the palette + inline OTP (Phase 4 — the palette's entry model is built to extend, but no workspace entries yet). Action entries (run flow / cancel job) (Phase 5). Cross-asset drill (separate Phase-2 plan). Throbbers (Phase 5).

YAGNI: `PaletteAction` gets exactly the two variants needed now (`SwitchKind`, `OpenItem`). Phase 4/5 add `SwitchWorkspace`/`RunAction` variants then.

---

## File Structure

| File | Phase-3 change |
|------|----------------|
| `Cargo.toml` (workspace) | Add `nucleo-matcher` + `tui-input` to `[workspace.dependencies]`. |
| `ayx-rs/Cargo.toml` | `nucleo-matcher.workspace = true`, `tui-input.workspace = true`. |
| `v2/palette.rs` | **New.** `PaletteState`, `PaletteEntry`, `PaletteCategory`, `PaletteAction`, `build_entries`, `rank` (nucleo). |
| `v2/state.rs` | `ListView.filter: String` → `tui_input::Input`; `visible()` reads `.value()`; `AppState` gains `palette: PaletteState` + `help_open: bool`. |
| `v2/action.rs` | Replace `FilterInput(char)`/`FilterBackspace` with `FilterEdit(InputRequest)`; add palette + help actions; extract `do_switch_kind`/`do_open` helpers (DRY with palette activate). |
| `v2/effect.rs` | No change (palette reuses `FetchList`/`FetchDetail`). |
| `v2/entry.rs` | `key_to_input_request` helper; `map_key` precedence: palette → help → filter → normal; `Ctrl+K` open; `?` help. |
| `v2/view/palette.rs` | **New.** Centered modal overlay (Clear + bordered rect): query line + categorized ranked list. |
| `v2/view/help.rs` | **New.** Contextual help overlay. |
| `v2/view/list.rs` | Render the filter `Input` value + cursor (was a plain string). |
| `v2/view/mod.rs` | After body+footer, render palette/help overlays when open; add `mod palette; mod help;`. |
| `v2/view/footer.rs` | `^K Palette` / `? Help` are now real hints; palette/help footers. |

---

### Task 1: Add `nucleo-matcher` + `tui-input` dependencies

**Files:**
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`)
- Modify: `ayx-rs/Cargo.toml` (`[dependencies]`)
- Modify: `Cargo.lock` (committed)

**Interfaces:**
- Produces: crate-available `nucleo_matcher` and `tui_input` (pure API).

- [ ] **Step 1: Add the workspace deps**

In the root `Cargo.toml` `[workspace.dependencies]` (near `ratatui = "0.30"` / `crossterm = "0.29"`), add:

```toml
nucleo-matcher = "0.3"
tui-input = { version = "0.11", default-features = false }
```

Note: `default-features = false` drops tui-input's `crossterm` backend feature — we use only the pure `Input`/`InputRequest` API, so no second crossterm version is pulled.

- [ ] **Step 2: Reference them from `ayx-rs/Cargo.toml`**

In `ayx-rs/Cargo.toml` `[dependencies]` (after `ratatui.workspace = true`), add:

```toml
nucleo-matcher.workspace = true
tui-input.workspace = true
```

- [ ] **Step 3: Build to resolve + lock**

Run: `cargo build -p ayx-rs 2>&1 | tail -20`
Expected: compiles; `Cargo.lock` updated with `nucleo-matcher`, `tui-input`, and their transitive deps. Confirm **only one** `crossterm` version is present:

Run: `cargo tree -p ayx-rs -i crossterm 2>&1 | head -20`
Expected: a single `crossterm v0.29.x` node. If `tui-input` pulled a second crossterm, the `default-features = false` in Step 1 was not applied — fix it.

- [ ] **Step 4: Smoke the pure API compiles**

Add a temporary throwaway check is unnecessary — Task 2 exercises both. Just confirm the build is green.

- [ ] **Step 5: Commit (including Cargo.lock)**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
git add Cargo.toml ayx-rs/Cargo.toml Cargo.lock
git commit -m "build(tui-v2): add nucleo-matcher + tui-input (pure API, no crossterm backend)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Swap the `/` filter to `tui_input::Input`

Replace the Phase-1 append-only `filter: String` with a `tui_input::Input` for real cursor editing, driven by `InputRequest`. Behaviour is otherwise identical (substring match on visible rows).

**Files:**
- Modify: `ayx-rs/src/tui/v2/state.rs`
- Modify: `ayx-rs/src/tui/v2/action.rs`
- Modify: `ayx-rs/src/tui/v2/entry.rs`
- Modify: `ayx-rs/src/tui/v2/view/list.rs`
- Test: inline tests in `state.rs`, `action.rs`

**Interfaces:**
- `state.rs`: `ListView.filter: tui_input::Input` (was `String`); `filtering: bool` unchanged. `visible()` matches against `self.filter.value()`.
- `action.rs`: remove `FilterInput(char)` + `FilterBackspace`; add `FilterEdit(tui_input::InputRequest)`. `FilterClear` resets the input (`Input::default()`); `FilterStart`/`FilterApply` unchanged. Each edit resets `cursor = 0`.
- `entry.rs`: `key_to_input_request(key: KeyEvent) -> Option<tui_input::InputRequest>`; filter routing emits `FilterEdit`.

- [ ] **Step 1: Update `ListView` (failing test first)**

In `state.rs` tests, replace the filter tests to use the new API:

```rust
    #[test]
    fn visible_filters_on_input_value() {
        let mut lv = lv_with(&["Daily ETL", "Sales Rollup", "daily report"]);
        lv.filter = tui_input::Input::default().with_value("daily".to_string());
        let vis = lv.visible();
        assert_eq!(vis.len(), 2);
        assert_eq!(vis[0].cells[0].text, "Daily ETL");
    }
```

(Keep `visible_is_all_when_no_filter` and `selected_indexes_into_visible`, but change any `lv.filter = "x".to_string()` to `lv.filter = tui_input::Input::default().with_value("x".to_string())`.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::state 2>&1 | tail -20`
Expected: FAIL — `filter` is still `String`, `.with_value` not found / type mismatch.

- [ ] **Step 3: Implement the field swap**

In `state.rs`, add the import and change the field + `new` + `visible`:

```rust
use tui_input::Input;
```

In `ListView`:
```rust
    pub filter: Input,
    pub filtering: bool,
```
```rust
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            rows: Vec::new(),
            cursor: 0,
            loading: true,
            error: None,
            token: 0,
            filter: Input::default(),
            filtering: false,
        }
    }
```
In `visible()`, replace the empty-check + needle:
```rust
    pub fn visible(&self) -> Vec<&Row> {
        let term = self.filter.value();
        if term.is_empty() {
            return self.rows.iter().collect();
        }
        let needle = term.to_lowercase();
        self.rows
            .iter()
            .filter(|row| {
                row.cells
                    .first()
                    .map(|cell| cell.text.to_lowercase().contains(&needle))
                    .unwrap_or(false)
            })
            .collect()
    }
```

- [ ] **Step 4: Update the reducer (`action.rs`)**

Add the import:
```rust
use tui_input::InputRequest;
```
In `Action`, remove `FilterInput(char)` and `FilterBackspace`; add:
```rust
    FilterEdit(InputRequest),
```
Replace the two old arms with one, and update `FilterClear` to reset the input:
```rust
        Action::FilterEdit(req) => {
            state.list.filter.handle(req);
            state.list.cursor = 0;
            Vec::new()
        }
```
```rust
        Action::FilterClear => {
            state.list.filter = tui_input::Input::default();
            state.list.filtering = false;
            state.list.cursor = 0;
            Vec::new()
        }
```
Update the filter reducer test to the new action. Replace `filter_flow_narrows_and_resets_cursor`'s body that used `FilterInput('3')` with `FilterEdit`:
```rust
    #[test]
    fn filter_edit_narrows_and_resets_cursor() {
        use tui_input::InputRequest;
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        s.list.rows = (0..5)
            .map(|i| crate::tui::v2::resource::Row {
                id: format!("fl_{i}"),
                cells: vec![crate::tui::v2::resource::Cell::plain(format!("name {i}"))],
            })
            .collect();
        s.list.loading = false;
        update(&mut s, Action::CursorDown);
        update(&mut s, Action::FilterStart);
        assert!(s.list.filtering);
        update(&mut s, Action::FilterEdit(InputRequest::InsertChar('3')));
        assert_eq!(s.list.filter.value(), "3");
        assert_eq!(s.list.cursor, 0);
        assert_eq!(s.list.visible().len(), 1);
        update(&mut s, Action::FilterApply);
        assert!(!s.list.filtering);
        assert_eq!(s.list.filter.value(), "3");
        update(&mut s, Action::FilterClear);
        assert!(s.list.filter.value().is_empty());
        assert!(!s.list.filtering);
    }
```

- [ ] **Step 5: Update `entry.rs` filter routing + add `key_to_input_request`**

Add the helper (above `map_key`):
```rust
use tui_input::InputRequest;

/// Map a key to a tui-input request, using only the pure (backend-agnostic)
/// `InputRequest` API so no crossterm version coupling is introduced. Returns
/// None for keys that are not text-editing input. Ctrl/Alt+char is NOT an insert.
fn key_to_input_request(key: KeyEvent) -> Option<InputRequest> {
    use crossterm::event::KeyModifiers;
    match key.code {
        KeyCode::Char(c)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            Some(InputRequest::InsertChar(c))
        }
        KeyCode::Backspace => Some(InputRequest::DeletePrevChar),
        KeyCode::Delete => Some(InputRequest::DeleteNextChar),
        KeyCode::Left => Some(InputRequest::GoToPrevChar),
        KeyCode::Right => Some(InputRequest::GoToNextChar),
        KeyCode::Home => Some(InputRequest::GoToStart),
        KeyCode::End => Some(InputRequest::GoToEnd),
        _ => None,
    }
}
```
In `map_key`, replace the filter-mode block body:
```rust
    if state.list.filtering && !on_detail {
        return match key.code {
            KeyCode::Enter => Some(Action::FilterApply),
            KeyCode::Esc => Some(Action::FilterClear),
            _ => key_to_input_request(key).map(Action::FilterEdit),
        };
    }
```
Update the entry test `slash_starts_filter_then_typing_feeds_it`:
```rust
    #[test]
    fn slash_starts_filter_then_typing_feeds_it() {
        use tui_input::InputRequest;
        let mut s = list_state();
        assert!(matches!(map_key(&s, k(KeyCode::Char('/'))), Some(Action::FilterStart)));
        s.list.filtering = true;
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('x'))),
            Some(Action::FilterEdit(InputRequest::InsertChar('x')))
        ));
        assert!(matches!(map_key(&s, k(KeyCode::Enter)), Some(Action::FilterApply)));
    }
```

- [ ] **Step 6: Render the filter input in `view/list.rs`**

The title currently embeds `state.list.filter` (a String). Replace with `.value()`:
```rust
    let visible = state.list.visible();
    let term = state.list.filter.value();
    let title = if term.is_empty() {
        format!(" {} · {} ", state.list.kind.name(), state.list.rows.len())
    } else {
        format!(
            " {} · {}/{}  /{}{} ",
            state.list.kind.name(),
            visible.len(),
            state.list.rows.len(),
            term,
            if state.list.filtering { "▏" } else { "" }
        )
    };
```

- [ ] **Step 7: Run + commit**

Run: `cargo nextest run -p ayx-rs tui::v2 2>&1 | tail -6` → PASS.
Run: `cargo build -p ayx-rs 2>&1 | tail -5` → compiles.

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/state.rs ayx-rs/src/tui/v2/action.rs ayx-rs/src/tui/v2/entry.rs ayx-rs/src/tui/v2/view/list.rs
git commit -m "feat(tui-v2): tui-input cursor editing for the / filter

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Palette model + `nucleo-matcher` ranking

**Files:**
- Create: `ayx-rs/src/tui/v2/palette.rs`
- Modify: `ayx-rs/src/tui/v2/mod.rs` (add `pub mod palette;`)
- Test: inline `#[cfg(test)]` in `palette.rs`

**Interfaces:**
- Produces:
  - `enum PaletteCategory { Resource, Item }`
  - `enum PaletteAction { SwitchKind(Kind), OpenItem { kind: Kind, id: String, title: String } }`
  - `struct PaletteEntry { label: String, category: PaletteCategory, action: PaletteAction }`
  - `struct PaletteState { open: bool, input: tui_input::Input, entries: Vec<PaletteEntry>, ranked: Vec<usize>, cursor: usize }` + `PaletteState::default()` (closed).
  - `fn build_entries(state: &AppState) -> Vec<PaletteEntry>` — 5 resource entries + current-list item entries (only for kinds with a detail endpoint).
  - `fn rank(query: &str, entries: &[PaletteEntry]) -> Vec<usize>` — nucleo-ranked indices; empty query → all in order.

- [ ] **Step 1: Write the failing test**

Create `ayx-rs/src/tui/v2/palette.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::context::Context;
    use crate::tui::v2::resource::{Cell, Kind, Row};
    use crate::tui::v2::state::AppState;

    fn state_with_flow_rows() -> AppState {
        let ctx = Context { profile: "w".into(), workspace: "w".into(), user: "u".into() };
        let mut s = AppState::new(ctx); // Flow root
        s.list.loading = false;
        s.list.rows = vec![
            Row { id: "fl_1".into(), cells: vec![Cell::plain("Daily ETL")] },
            Row { id: "fl_2".into(), cells: vec![Cell::plain("Sales Rollup")] },
        ];
        s
    }

    #[test]
    fn build_entries_has_five_resources_plus_items() {
        let s = state_with_flow_rows();
        let entries = build_entries(&s);
        let resources = entries.iter().filter(|e| matches!(e.category, PaletteCategory::Resource)).count();
        let items = entries.iter().filter(|e| matches!(e.category, PaletteCategory::Item)).count();
        assert_eq!(resources, 5);
        assert_eq!(items, 2); // Flow has a detail endpoint → rows become Open items
        assert!(entries.iter().any(|e| e.label == "Open flow: Daily ETL"));
    }

    #[test]
    fn workspace_rows_are_not_openable_items() {
        let ctx = Context { profile: "w".into(), workspace: "w".into(), user: "u".into() };
        let mut s = AppState::new(ctx);
        s.list = crate::tui::v2::state::ListView::new(Kind::Workspace);
        s.list.loading = false;
        s.list.rows = vec![Row { id: "ws_1".into(), cells: vec![Cell::plain("Prod")] }];
        let entries = build_entries(&s);
        // 5 resource entries, but no Item entries (Workspace has no detail endpoint)
        assert_eq!(entries.iter().filter(|e| matches!(e.category, PaletteCategory::Item)).count(), 0);
    }

    #[test]
    fn empty_query_returns_all_in_order() {
        let s = state_with_flow_rows();
        let entries = build_entries(&s);
        let ranked = rank("", &entries);
        assert_eq!(ranked, (0..entries.len()).collect::<Vec<_>>());
    }

    #[test]
    fn query_ranks_matching_entry_first() {
        let s = state_with_flow_rows();
        let entries = build_entries(&s);
        let ranked = rank("daily", &entries);
        assert!(!ranked.is_empty());
        assert_eq!(entries[ranked[0]].label, "Open flow: Daily ETL");
    }

    #[test]
    fn nonmatching_query_returns_empty() {
        let s = state_with_flow_rows();
        let entries = build_entries(&s);
        assert!(rank("zzzqqq", &entries).is_empty());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::palette 2>&1 | tail -20`
Expected: FAIL — module/types not found. (First add `pub mod palette;` to `ayx-rs/src/tui/v2/mod.rs` alongside the other `pub mod` lines, or the test can't compile.)

- [ ] **Step 3: Implement the model + ranking**

Top of `palette.rs` (above the tests):

```rust
//! Command palette model + fuzzy ranking. The palette unifies resource-switch
//! and item-open actions into one ranked list (nucleo-matcher). Workspace/action
//! entries arrive in later phases — `PaletteAction` is extended then.
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use tui_input::Input;

use crate::tui::v2::resource::{Kind, kind_impl};
use crate::tui::v2::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteCategory {
    Resource,
    Item,
}

#[derive(Debug, Clone)]
pub enum PaletteAction {
    SwitchKind(Kind),
    OpenItem { kind: Kind, id: String, title: String },
}

#[derive(Debug, Clone)]
pub struct PaletteEntry {
    pub label: String,
    pub category: PaletteCategory,
    pub action: PaletteAction,
}

#[derive(Debug, Default)]
pub struct PaletteState {
    pub open: bool,
    pub input: Input,
    pub entries: Vec<PaletteEntry>,
    pub ranked: Vec<usize>,
    pub cursor: usize,
}

/// Build the palette entry set from current state: the five resource switches,
/// then the current list's visible rows as Open items (only for kinds that can
/// actually drill into a detail).
pub fn build_entries(state: &AppState) -> Vec<PaletteEntry> {
    let mut entries = Vec::new();
    for &k in Kind::all() {
        entries.push(PaletteEntry {
            label: format!("Browse {}", k.name()),
            category: PaletteCategory::Resource,
            action: PaletteAction::SwitchKind(k),
        });
    }
    let kind = state.list.kind;
    if kind_impl(kind).detail_endpoint().is_some() {
        for row in state.list.visible() {
            if row.id.is_empty() {
                continue;
            }
            let title = row
                .cells
                .first()
                .map(|c| c.text.clone())
                .unwrap_or_else(|| row.id.clone());
            entries.push(PaletteEntry {
                label: format!("Open {}: {}", kind.singular(), title),
                category: PaletteCategory::Item,
                action: PaletteAction::OpenItem { kind, id: row.id.clone(), title },
            });
        }
    }
    entries
}

/// Fuzzy-rank entry indices by label against `query`. Empty query keeps the
/// natural order (resources first, then items). Non-matching entries drop out.
pub fn rank(query: &str, entries: &[PaletteEntry]) -> Vec<usize> {
    if query.is_empty() {
        return (0..entries.len()).collect();
    }
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut buf: Vec<char> = Vec::new();
    let mut scored: Vec<(usize, u32)> = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        let haystack = Utf32Str::new(&entry.label, &mut buf);
        if let Some(score) = pattern.score(haystack, &mut matcher) {
            scored.push((i, score));
        }
    }
    // Highest score first; stable on ties (preserves natural order).
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().map(|(i, _)| i).collect()
}
```

Note (API-drift guard, like Phase 0's ratatui notes): this uses `Pattern::score(haystack: Utf32Str, &mut Matcher) -> Option<u32>` and `Utf32Str::new(&str, &mut Vec<char>)` from nucleo-matcher 0.3. If `cargo build` reports a different `score`/`Utf32Str::new` signature, adjust per the compiler message (e.g. some 0.3.x expose `Pattern::score` returning `Option<u32>` — keep the `(index, score)` collect either way). The build step catches it.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::palette 2>&1 | tail -20`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/palette.rs ayx-rs/src/tui/v2/mod.rs
git commit -m "feat(tui-v2): palette model + nucleo-matcher ranking

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Palette + help actions in the reducer (with DRY switch/open helpers)

Extract the `SwitchKind`/`Open` effect logic into reusable helpers, then add the palette and help actions. Palette activation reuses the helpers so a palette `SwitchKind`/`OpenItem` behaves identically to the keyboard path.

**Files:**
- Modify: `ayx-rs/src/tui/v2/state.rs` (`AppState.palette`, `AppState.help_open`)
- Modify: `ayx-rs/src/tui/v2/action.rs` (helpers + new actions + arms + tests)
- Test: inline tests in `action.rs`

**Interfaces:**
- `state.rs`: `AppState` gains `pub palette: crate::tui::v2::palette::PaletteState` and `pub help_open: bool` (both default closed/false in `new`).
- `action.rs`:
  - `fn do_switch_kind(state: &mut AppState, kind: Kind) -> Vec<Effect>` and `fn do_open(state: &mut AppState, kind: Kind, id: String, title: String) -> Vec<Effect>` (the bodies currently inlined in `SwitchKind`/`Open`).
  - New actions: `PaletteOpen`, `PaletteClose`, `PaletteEdit(tui_input::InputRequest)`, `PaletteUp`, `PaletteDown`, `PaletteActivate`, `HelpToggle`, `HelpClose`.

- [ ] **Step 1: Add the state fields**

In `state.rs`, `use crate::tui::v2::palette::PaletteState;`. In `AppState`:
```rust
    pub palette: PaletteState,
    pub help_open: bool,
```
In `AppState::new`, add `palette: PaletteState::default(), help_open: false,`.

- [ ] **Step 2: Write the failing reducer tests**

In `action.rs` tests:

```rust
    #[test]
    fn palette_open_builds_entries_and_ranks_all() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        update(&mut s, Action::ListLoaded { token: s.list.token, rows: rows(2) });
        update(&mut s, Action::PaletteOpen);
        assert!(s.palette.open);
        assert!(!s.palette.entries.is_empty());
        assert_eq!(s.palette.ranked.len(), s.palette.entries.len());
        assert_eq!(s.palette.cursor, 0);
    }

    #[test]
    fn palette_edit_reranks_and_clamps_cursor() {
        use tui_input::InputRequest;
        let mut s = test_state();
        update(&mut s, Action::PaletteOpen);
        update(&mut s, Action::PaletteEdit(InputRequest::InsertChar('z')));
        update(&mut s, Action::PaletteEdit(InputRequest::InsertChar('z')));
        // "zz" matches nothing → ranked empty, cursor clamped to 0
        assert!(s.palette.ranked.is_empty());
        assert_eq!(s.palette.cursor, 0);
    }

    #[test]
    fn palette_activate_switch_kind_resets_list_and_closes() {
        use crate::tui::v2::resource::Kind;
        let mut s = test_state(); // Flow
        update(&mut s, Action::PaletteOpen);
        // entries[0..5] are the resource switches in Kind::all() order; index 2 = Job
        let job_idx = s.palette.entries.iter().position(|e| matches!(
            e.action, crate::tui::v2::palette::PaletteAction::SwitchKind(Kind::Job))).unwrap();
        s.palette.cursor = s.palette.ranked.iter().position(|&i| i == job_idx).unwrap();
        let effects = update(&mut s, Action::PaletteActivate);
        assert!(!s.palette.open);
        assert_eq!(s.list.kind, Kind::Job);
        assert!(matches!(effects.as_slice(), [Effect::FetchList { kind: Kind::Job, .. }]));
    }

    #[test]
    fn palette_close_and_help_toggle() {
        let mut s = test_state();
        update(&mut s, Action::PaletteOpen);
        update(&mut s, Action::PaletteClose);
        assert!(!s.palette.open);
        update(&mut s, Action::HelpToggle);
        assert!(s.help_open);
        update(&mut s, Action::HelpClose);
        assert!(!s.help_open);
    }
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::action 2>&1 | tail -20`
Expected: FAIL — palette actions not found.

- [ ] **Step 4: Extract helpers + refactor existing arms**

In `action.rs`, add the helpers (after `mint_token`):

```rust
/// Switch the root list to `kind` (reset nav + list, fetch). Shared by the
/// SwitchKind action and palette activation.
pub(crate) fn do_switch_kind(state: &mut AppState, kind: Kind) -> Vec<Effect> {
    if state.list.kind == kind && matches!(state.nav.top(), View::ResourceList { .. }) {
        return Vec::new();
    }
    state.nav = NavStack::new(View::ResourceList { kind });
    state.list = ListView::new(kind);
    state.detail = None;
    let token = mint_token(state);
    state.list.token = token;
    vec![Effect::FetchList { kind, token }]
}

/// Drill into `id` of `kind` (push detail view + fetch). Shared by the Open
/// action and palette activation. No-op if the kind has no detail endpoint or id
/// is empty.
pub(crate) fn do_open(state: &mut AppState, kind: Kind, id: String, title: String) -> Vec<Effect> {
    if kind_impl(kind).detail_endpoint().is_none() || id.is_empty() {
        return Vec::new();
    }
    state.nav.push(View::ResourceDetail { kind, id: id.clone(), title: title.clone() });
    let token = mint_token(state);
    state.detail = Some(DetailView::new(kind, id.clone(), title, token));
    vec![Effect::FetchDetail { kind, id, token }]
}
```

Replace the existing `Action::SwitchKind` arm body with:
```rust
        Action::SwitchKind(kind) => do_switch_kind(state, kind),
```
Replace the existing `Action::Open` arm body with:
```rust
        Action::Open => {
            let kind = state.list.kind;
            let Some(row) = state.list.selected() else { return Vec::new() };
            let id = row.id.clone();
            let title = row
                .cells
                .first()
                .map(|c| c.text.clone())
                .unwrap_or_else(|| id.clone());
            do_open(state, kind, id, title)
        }
```

- [ ] **Step 5: Add the palette + help arms + actions**

Add to `Action`:
```rust
    PaletteOpen,
    PaletteClose,
    PaletteEdit(tui_input::InputRequest),
    PaletteUp,
    PaletteDown,
    PaletteActivate,
    HelpToggle,
    HelpClose,
```
Add the reducer arms:
```rust
        Action::PaletteOpen => {
            state.help_open = false;
            state.palette.open = true;
            state.palette.input = tui_input::Input::default();
            state.palette.entries = crate::tui::v2::palette::build_entries(state);
            state.palette.ranked = crate::tui::v2::palette::rank("", &state.palette.entries);
            state.palette.cursor = 0;
            Vec::new()
        }
        Action::PaletteClose => {
            state.palette.open = false;
            Vec::new()
        }
        Action::PaletteEdit(req) => {
            state.palette.input.handle(req);
            let q = state.palette.input.value().to_string();
            state.palette.ranked = crate::tui::v2::palette::rank(&q, &state.palette.entries);
            if state.palette.cursor >= state.palette.ranked.len() {
                state.palette.cursor = state.palette.ranked.len().saturating_sub(1);
            }
            Vec::new()
        }
        Action::PaletteDown => {
            if !state.palette.ranked.is_empty()
                && state.palette.cursor + 1 < state.palette.ranked.len()
            {
                state.palette.cursor += 1;
            }
            Vec::new()
        }
        Action::PaletteUp => {
            state.palette.cursor = state.palette.cursor.saturating_sub(1);
            Vec::new()
        }
        Action::PaletteActivate => {
            let action = state
                .palette
                .ranked
                .get(state.palette.cursor)
                .and_then(|&i| state.palette.entries.get(i))
                .map(|e| e.action.clone());
            state.palette.open = false;
            match action {
                Some(crate::tui::v2::palette::PaletteAction::SwitchKind(kind)) => {
                    do_switch_kind(state, kind)
                }
                Some(crate::tui::v2::palette::PaletteAction::OpenItem { kind, id, title }) => {
                    do_switch_kind_if_needed_then_open(state, kind, id, title)
                }
                None => Vec::new(),
            }
        }
        Action::HelpToggle => {
            state.help_open = !state.help_open;
            Vec::new()
        }
        Action::HelpClose => {
            state.help_open = false;
            Vec::new()
        }
```

Add the small composing helper used by `OpenItem` (the item may belong to the kind currently listed — which it always does in Phase 3 since items come from the current list — so just `do_open`; defined as a thin wrapper to keep the door open for cross-list items later):
```rust
/// Open an item that belongs to `kind`. In Phase 3 palette items always come
/// from the current list, so this is just `do_open`; a later phase may first
/// switch the list when the item is from another kind.
fn do_switch_kind_if_needed_then_open(
    state: &mut AppState,
    kind: Kind,
    id: String,
    title: String,
) -> Vec<Effect> {
    do_open(state, kind, id, title)
}
```

- [ ] **Step 6: Run to verify it passes**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::action 2>&1 | tail -20`
Expected: PASS (palette/help tests + existing).

- [ ] **Step 7: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/state.rs ayx-rs/src/tui/v2/action.rs
git commit -m "feat(tui-v2): palette + help reducer actions (DRY switch/open helpers)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Palette key routing + `Ctrl+K` / `?` in `entry.rs`

Wire keys: `Ctrl+K` opens the palette anywhere; while open the palette captures keys; `?` toggles help; help captures keys until dismissed. Precedence: palette → help → filter → normal.

**Files:**
- Modify: `ayx-rs/src/tui/v2/entry.rs`
- Test: inline tests in `entry.rs`

**Interfaces:**
- Consumes: the palette/help/filter actions.
- `map_key` precedence order rewritten; `Ctrl+K` detection via `KeyModifiers::CONTROL`.

- [ ] **Step 1: Write the failing tests**

In `entry.rs` tests (helper `k` exists; add a ctrl helper):

```rust
    fn kc(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_k_opens_palette() {
        let s = list_state();
        assert!(matches!(map_key(&s, kc(KeyCode::Char('k'))), Some(Action::PaletteOpen)));
    }

    #[test]
    fn palette_open_captures_keys() {
        use tui_input::InputRequest;
        let mut s = list_state();
        s.palette.open = true;
        assert!(matches!(map_key(&s, k(KeyCode::Esc)), Some(Action::PaletteClose)));
        assert!(matches!(map_key(&s, k(KeyCode::Enter)), Some(Action::PaletteActivate)));
        assert!(matches!(map_key(&s, k(KeyCode::Down)), Some(Action::PaletteDown)));
        assert!(matches!(map_key(&s, k(KeyCode::Up)), Some(Action::PaletteUp)));
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('f'))),
            Some(Action::PaletteEdit(InputRequest::InsertChar('f')))
        ));
    }

    #[test]
    fn question_mark_toggles_help_and_help_captures() {
        let mut s = list_state();
        assert!(matches!(map_key(&s, k(KeyCode::Char('?'))), Some(Action::HelpToggle)));
        s.help_open = true;
        assert!(matches!(map_key(&s, k(KeyCode::Esc)), Some(Action::HelpClose)));
        assert!(matches!(map_key(&s, k(KeyCode::Char('x'))), Some(Action::HelpClose)));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::entry 2>&1 | tail -20`
Expected: FAIL — new routing not implemented.

- [ ] **Step 3: Rewrite `map_key` with precedence**

Replace the whole `map_key` function:

```rust
fn map_key(state: &AppState, key: KeyEvent) -> Option<Action> {
    use crate::tui::v2::nav::View;
    use crate::tui::v2::resource::Kind;
    use crossterm::event::KeyModifiers;

    // 1) Palette is modal — it captures everything while open.
    if state.palette.open {
        return match key.code {
            KeyCode::Esc => Some(Action::PaletteClose),
            KeyCode::Enter => Some(Action::PaletteActivate),
            KeyCode::Down => Some(Action::PaletteDown),
            KeyCode::Up => Some(Action::PaletteUp),
            _ => key_to_input_request(key).map(Action::PaletteEdit),
        };
    }

    // 2) Help overlay — any key dismisses it.
    if state.help_open {
        return Some(Action::HelpClose);
    }

    // 3) Ctrl+K opens the palette from anywhere else.
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('k') | KeyCode::Char('K'))
    {
        return Some(Action::PaletteOpen);
    }

    let on_detail = matches!(state.nav.top(), View::ResourceDetail { .. });

    // 4) Filter input mode (list only).
    if state.list.filtering && !on_detail {
        return match key.code {
            KeyCode::Enter => Some(Action::FilterApply),
            KeyCode::Esc => Some(Action::FilterClear),
            _ => key_to_input_request(key).map(Action::FilterEdit),
        };
    }

    // 5) Normal bindings.
    match key.code {
        KeyCode::Char('?') => Some(Action::HelpToggle),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::CursorDown),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::CursorUp),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Enter => Some(if on_detail { Action::Back } else { Action::Open }),
        KeyCode::Char('/') if !on_detail => Some(Action::FilterStart),
        KeyCode::Char(c @ '1'..='5') => {
            Kind::from_index((c as u8 - b'1') as usize).map(Action::SwitchKind)
        }
        KeyCode::Tab => {
            let n = Kind::all().len();
            Kind::from_index((state.list.kind.index() + 1) % n).map(Action::SwitchKind)
        }
        KeyCode::BackTab => {
            let n = Kind::all().len();
            Kind::from_index((state.list.kind.index() + n - 1) % n).map(Action::SwitchKind)
        }
        _ => None,
    }
}
```

Note: `'k'` is both `CursorUp` (normal) and part of `Ctrl+K`. The Ctrl check (step 3) runs before the normal match and requires the CONTROL modifier, so plain `k` still maps to `CursorUp`. `?` is checked before the vim keys.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::entry 2>&1 | tail -20`
Expected: PASS. Existing entry tests still green (they pass `&list_state()` with palette/help closed).

- [ ] **Step 5: Commit**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/entry.rs
git commit -m "feat(tui-v2): Ctrl+K palette + ? help key routing (modal precedence)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Palette overlay render

**Files:**
- Create: `ayx-rs/src/tui/v2/view/palette.rs`
- Modify: `ayx-rs/src/tui/v2/view/mod.rs` (`mod palette;` + render when open)
- Test: inline `#[cfg(test)]` in `view/palette.rs`

**Interfaces:**
- Produces: `pub fn render(frame: &mut ratatui::Frame, state: &AppState)` — draws a centered modal (Clear + bordered rect) with the query line and the ranked, categorized entries; selected row highlighted.

- [ ] **Step 1: Write the failing TestBackend test**

Create `view/palette.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::action::{Action, update};
    use crate::tui::v2::context::Context;
    use crate::tui::v2::state::AppState;
    use ratatui::{Terminal, backend::TestBackend};

    fn open_palette_state() -> AppState {
        let ctx = Context { profile: "w".into(), workspace: "w".into(), user: "u".into() };
        let mut s = AppState::new(ctx);
        update(&mut s, Action::PaletteOpen);
        s
    }

    fn text_of(state: &AppState) -> String {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, state)).unwrap();
        terminal.backend().buffer().clone().content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn palette_renders_header_and_resource_entries() {
        let s = open_palette_state();
        let txt = text_of(&s);
        assert!(txt.contains("Command Palette"));
        assert!(txt.contains("Browse flows"));
        assert!(txt.contains("RESOURCES"));
    }

    #[test]
    fn closed_palette_renders_nothing() {
        let ctx = Context { profile: "w".into(), workspace: "w".into(), user: "u".into() };
        let s = AppState::new(ctx); // palette closed
        let txt = text_of(&s);
        assert!(!txt.contains("Command Palette"));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::view::palette 2>&1 | tail -20`
Expected: FAIL — `render` missing.

- [ ] **Step 3: Implement the overlay**

Top of `view/palette.rs`:

```rust
//! Ctrl+K command palette overlay — centered modal: query line + ranked,
//! category-grouped entries.
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::tui::theme;
use crate::tui::v2::palette::PaletteCategory;
use crate::tui::v2::state::AppState;

pub fn render(frame: &mut Frame, state: &AppState) {
    if !state.palette.open {
        return;
    }
    let area = centered(frame.area(), 70, 60);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border(true))
        .style(theme::panel())
        .title(Span::styled(" ^K Command Palette ", theme::accent()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);

    // Query line with a cursor caret.
    let query = state.palette.input.value();
    let query_line = Line::from(vec![
        Span::styled("> ", theme::accent_bold()),
        Span::styled(query.to_string(), theme::field_value()),
        Span::styled("▏", theme::dim()),
    ]);
    frame.render_widget(Paragraph::new(query_line), rows[0]);

    // Ranked entries, grouped by category with a header line per group.
    let mut lines: Vec<Line> = Vec::new();
    let mut last_cat: Option<PaletteCategory> = None;
    for (pos, &idx) in state.palette.ranked.iter().enumerate() {
        let entry = &state.palette.entries[idx];
        if last_cat != Some(entry.category) {
            lines.push(Line::from(Span::styled(
                format!(" {} ", category_label(entry.category)),
                theme::muted(),
            )));
            last_cat = Some(entry.category);
        }
        let selected = pos == state.palette.cursor;
        let marker = if selected { "▸ " } else { "  " };
        let style = if selected { theme::selected() } else { theme::field_value() };
        lines.push(Line::from(Span::styled(format!("{marker}{}", entry.label), style)));
    }
    if state.palette.ranked.is_empty() {
        lines.push(Line::from(Span::styled("  no matches", theme::muted())));
    }
    frame.render_widget(Paragraph::new(lines), rows[1]);
}

fn category_label(cat: PaletteCategory) -> &'static str {
    match cat {
        PaletteCategory::Resource => "RESOURCES",
        PaletteCategory::Item => "ITEMS",
    }
}

/// A rect `pct_w`% × `pct_h`% of `area`, centered.
fn centered(area: Rect, pct_w: u16, pct_h: u16) -> Rect {
    let v = Layout::vertical([
        Constraint::Percentage((100 - pct_h) / 2),
        Constraint::Percentage(pct_h),
        Constraint::Percentage((100 - pct_h) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - pct_w) / 2),
        Constraint::Percentage(pct_w),
        Constraint::Percentage((100 - pct_w) / 2),
    ])
    .split(v[1])[1]
}
```

- [ ] **Step 4: Render it from `view/mod.rs`**

Add `mod palette;` and `mod help;` (help lands in Task 7; add both now to avoid a second edit — create an empty `help` stub if needed, but Task 7 creates it, so add only `mod palette;` here and add `mod help;` in Task 7). For this task add `mod palette;` and, at the end of `render`, after `footer::render`, draw the overlay:

```rust
    palette::render(frame, state);
```

So the dispatcher tail becomes:
```rust
    header::render(frame, state, chunks[0]);
    match state.nav.top() {
        View::ResourceList { .. } => list::render(frame, state, chunks[1]),
        View::ResourceDetail { .. } => detail::render(frame, state, chunks[1]),
    }
    footer::render(frame, state, chunks[2]);
    palette::render(frame, state);
```

- [ ] **Step 5: Run + commit**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::view 2>&1 | tail -10` → PASS.

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/view/palette.rs ayx-rs/src/tui/v2/view/mod.rs
git commit -m "feat(tui-v2): command palette overlay render

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Help overlay

**Files:**
- Create: `ayx-rs/src/tui/v2/view/help.rs`
- Modify: `ayx-rs/src/tui/v2/view/mod.rs` (`mod help;` + render when open)
- Test: inline `#[cfg(test)]` in `view/help.rs`

**Interfaces:**
- Produces: `pub fn render(frame: &mut ratatui::Frame, state: &AppState)` — a centered overlay listing key bindings; drawn only when `state.help_open`.

- [ ] **Step 1: Write the failing test**

Create `view/help.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::context::Context;
    use crate::tui::v2::state::AppState;
    use ratatui::{Terminal, backend::TestBackend};

    fn text_of(state: &AppState) -> String {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, state)).unwrap();
        terminal.backend().buffer().clone().content().iter().map(|c| c.symbol()).collect()
    }

    fn base() -> AppState {
        let ctx = Context { profile: "w".into(), workspace: "w".into(), user: "u".into() };
        AppState::new(ctx)
    }

    #[test]
    fn help_lists_key_bindings_when_open() {
        let mut s = base();
        s.help_open = true;
        let txt = text_of(&s);
        assert!(txt.contains("Help"));
        assert!(txt.contains("Palette"));
        assert!(txt.contains("Filter"));
        assert!(txt.contains("Switch"));
    }

    #[test]
    fn help_renders_nothing_when_closed() {
        let txt = text_of(&base());
        assert!(!txt.contains("Keys"));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::view::help 2>&1 | tail -20`
Expected: FAIL — `render` missing.

- [ ] **Step 3: Implement**

Top of `view/help.rs`:

```rust
//! `?` help overlay — a centered, read-only key-binding reference. Any key
//! dismisses it (handled in `map_key`).
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::tui::theme;
use crate::tui::v2::state::AppState;

const KEYS: &[(&str, &str)] = &[
    ("↑ ↓ / j k", "Move cursor / scroll detail"),
    ("↵", "Open selected · Back from detail"),
    ("⎋", "Back · close overlay"),
    ("/", "Filter the current list"),
    ("1–5 · ⇥", "Switch resource (Flows…Workspaces)"),
    ("^K", "Command palette"),
    ("?", "This help"),
    ("q", "Quit"),
];

pub fn render(frame: &mut Frame, state: &AppState) {
    if !state.help_open {
        return;
    }
    let area = centered(frame.area(), 60, 60);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border(true))
        .style(theme::panel())
        .title(Span::styled(" Help — Keys ", theme::accent()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = KEYS
        .iter()
        .map(|(k, desc)| {
            Line::from(vec![
                Span::styled(format!(" {k:<12}"), theme::accent_bold()),
                Span::styled((*desc).to_string(), theme::field_value()),
            ])
        })
        .chain(std::iter::once(Line::from(Span::styled(
            " (any key to close)",
            theme::muted(),
        ))))
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn centered(area: Rect, pct_w: u16, pct_h: u16) -> Rect {
    let v = Layout::vertical([
        Constraint::Percentage((100 - pct_h) / 2),
        Constraint::Percentage(pct_h),
        Constraint::Percentage((100 - pct_h) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - pct_w) / 2),
        Constraint::Percentage(pct_w),
        Constraint::Percentage((100 - pct_w) / 2),
    ])
    .split(v[1])[1]
}
```

The test asserts "Filter"/"Switch"/"Palette" — present in the KEYS descriptions/labels.

- [ ] **Step 4: Render from `view/mod.rs`**

Add `mod help;` and, after `palette::render`, add `help::render(frame, state);`. (Palette and help are mutually exclusive — `PaletteOpen` clears `help_open` and `HelpToggle`/normal keys can't fire while the palette is open — but rendering both guards on their own flags so order is harmless.)

- [ ] **Step 5: Run + commit**

Run: `cargo test -p ayx-rs --bin ayx tui::v2::view 2>&1 | tail -10` → PASS.

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all
cargo clippy -p ayx-rs --all-targets -- -D warnings
git add ayx-rs/src/tui/v2/view/help.rs ayx-rs/src/tui/v2/view/mod.rs
git commit -m "feat(tui-v2): ? help overlay

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 8: Final validation + manual smoke + status

**Files:**
- Modify: `.superpowers/plans/2026-06-27-ayx-tui-phase3-palette.md` (STATUS)

- [ ] **Step 1: Full gate**

```bash
cd /home/merlin/code/ayx-rs
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace 2>&1 | tail -5
cargo tree -p ayx-rs -i crossterm 2>&1 | head   # confirm single crossterm version
```
Expected: fmt clean, clippy clean, all tests pass, exactly one `crossterm v0.29.x`.

- [ ] **Step 2: Manual smoke (documented; needs a TTY + authed workspace)**

```bash
AYX_TUI_V2=1 cargo run -p ayx-rs -- tui
```
Verify: `Ctrl+K` opens the palette; typing fuzzy-filters (e.g. `jo` → "Browse jobs"); `↑/↓` move, `↵` activates (switches resource or opens an item), `⎋` closes; in a list, type `/` then edit with arrows/backspace mid-string (tui-input cursor works); `?` shows help, any key closes; `q` quits. Confirm the legacy path (`ayx tui`, no env var) is intact.

- [ ] **Step 3: Mark complete + commit**

Append a STATUS section (date, commit range, suite count, the deferred items: workspace + action palette entries → Phase 4/5), then:

```bash
cd /home/merlin/code/ayx-rs
git add .superpowers/plans/2026-06-27-ayx-tui-phase3-palette.md
git commit -m "docs(tui-v2): mark Phase 3 palette plan complete

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage** (spec §"Control scheme" + Phase 3 row "Ctrl+K fuzzy palette (unified results); ? help overlay; tui-input replaces append-only editing everywhere"):
- `Ctrl+K` fuzzy palette → Tasks 3 (model+rank), 4 (actions), 5 (routing), 6 (render). ✓
- Unified results (resources + items), categorized → Task 3 `build_entries` + Task 6 category headers. ✓
- `nucleo-matcher` ranking → Task 3. ✓
- `?` help overlay → Task 7 + routing in Task 5. ✓
- `tui-input` replaces append-only editing everywhere → Task 2 (filter) + Task 4/6 (palette query uses `Input`). ✓
- Opener `Ctrl+K` not `Ctrl+P`; `>` optional never required → Task 5 (Ctrl+K), and no sigil is required (Task 3 ranks plain text). ✓
- Deferred (workspace/action entries, throbbers) documented in Scope. ✓

**2. Placeholder scan:** No "TBD"/"handle errors"/"similar to Task N". Every code step is complete. The two explicit API-drift guards (nucleo `Pattern::score`/`Utf32Str::new` shape; single-crossterm check) name the exact fallback, not placeholders.

**3. Type consistency:** `PaletteState`/`PaletteEntry`/`PaletteCategory`/`PaletteAction`/`build_entries`/`rank` defined Task 3, consumed in Tasks 4 (reducer), 6 (render). `do_switch_kind`/`do_open` defined Task 4, reused by `SwitchKind`/`Open`/`PaletteActivate`. `FilterEdit(InputRequest)` defined Task 2, routed in Task 2/5. `key_to_input_request` defined Task 2, reused in Task 5. `AppState.palette`/`help_open` defined Task 4, read in Tasks 5/6/7. `tui_input::Input` filter field (Task 2) read by `visible()` and `view/list.rs`. Consistent.

**Phasing note:** Delivers working software — `AYX_TUI_V2=1 ayx tui` gains a working Ctrl+K palette (resource + item entries), `?` help, and real cursor editing. Phase 4 (workspace switch) and Phase 5 (actions) extend `PaletteAction`/`build_entries` with new entry kinds; the palette engine is built to absorb them as data.
