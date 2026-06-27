//! Actions (user intents + async results) and the `update` reducer. The
//! reducer is the only place state mutates; it returns Effects for the
//! worker to run. Pure-ish: no I/O here.
use crate::tui::v2::effect::Effect;
use crate::tui::v2::nav::{NavStack, View};
use crate::tui::v2::palette::{self, PaletteAction};
use crate::tui::v2::resource::kind_impl;
use crate::tui::v2::resource::{Kind, Row};
use crate::tui::v2::state::{AppState, DetailView, ListView};
use serde_json::Value;
use tui_input::InputRequest;

#[derive(Debug, Clone)]
pub enum Action {
    CursorDown,
    CursorUp,
    SwitchKind(Kind),
    Open,
    PaletteOpen,
    PaletteClose,
    PaletteEdit(InputRequest),
    PaletteUp,
    PaletteDown,
    PaletteActivate,
    HelpToggle,
    HelpClose,
    FilterStart,
    FilterEdit(InputRequest),
    FilterApply,
    FilterClear,
    Back,
    Quit,
    ListLoaded { token: u64, rows: Vec<Row> },
    ListFailed { token: u64, error: String },
    DetailLoaded { token: u64, json: Value },
    DetailFailed { token: u64, error: String },
}

/// Mint the next monotonic request token.
pub(crate) fn mint_token(state: &mut AppState) -> u64 {
    state.req_seq += 1;
    state.req_seq
}

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
    vec![Effect::FetchList {
        kind,
        token,
        scope: None,
    }]
}

/// Drill into `id` of `kind` (push detail view + fetch). Shared by the Open
/// action and palette activation. No-op if the kind has no detail endpoint or
/// id is empty.
pub(crate) fn do_open(state: &mut AppState, kind: Kind, id: String, title: String) -> Vec<Effect> {
    if kind_impl(kind).detail_endpoint().is_none() || id.is_empty() {
        return Vec::new();
    }

    state.nav.push(View::ResourceDetail {
        kind,
        id: id.clone(),
        title: title.clone(),
    });
    let token = mint_token(state);
    state.detail = Some(DetailView::new(kind, id.clone(), title, token));
    vec![Effect::FetchDetail { kind, id, token }]
}

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

pub fn update(state: &mut AppState, action: Action) -> Vec<Effect> {
    match action {
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
        Action::SwitchKind(kind) => do_switch_kind(state, kind),
        Action::Open => {
            let kind = state.list.kind;
            let Some(row) = state.list.selected() else {
                return Vec::new();
            };
            let id = row.id.clone();
            let title = row
                .cells
                .first()
                .map(|c| c.text.clone())
                .unwrap_or_else(|| id.clone());
            do_open(state, kind, id, title)
        }
        Action::PaletteOpen => {
            state.help_open = false;
            // Leaving filter-edit mode — otherwise `filtering` could survive a
            // palette-driven drill and silently capture keys back on the list.
            state.list.filtering = false;
            state.palette.open = true;
            state.palette.input = tui_input::Input::default();
            state.palette.entries = palette::build_entries(state);
            state.palette.ranked = palette::rank("", &state.palette.entries);
            state.palette.cursor = 0;
            Vec::new()
        }
        Action::PaletteClose => {
            state.palette.open = false;
            Vec::new()
        }
        Action::PaletteEdit(req) => {
            state.palette.input.handle(req);
            let query = state.palette.input.value().to_string();
            state.palette.ranked = palette::rank(&query, &state.palette.entries);
            if state.palette.cursor >= state.palette.ranked.len() {
                state.palette.cursor = state.palette.ranked.len().saturating_sub(1);
            }
            Vec::new()
        }
        Action::PaletteUp => {
            state.palette.cursor = state.palette.cursor.saturating_sub(1);
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
        Action::PaletteActivate => {
            let action = state
                .palette
                .ranked
                .get(state.palette.cursor)
                .and_then(|&index| state.palette.entries.get(index))
                .map(|entry| entry.action.clone());
            state.palette.open = false;
            match action {
                Some(PaletteAction::SwitchKind(kind)) => do_switch_kind(state, kind),
                Some(PaletteAction::OpenItem { kind, id, title }) => {
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
        Action::FilterStart => {
            state.list.filtering = true;
            Vec::new()
        }
        Action::FilterEdit(req) => {
            state.list.filter.handle(req);
            state.list.cursor = 0;
            Vec::new()
        }
        Action::FilterApply => {
            state.list.filtering = false;
            Vec::new()
        }
        Action::FilterClear => {
            state.list.filter = tui_input::Input::default();
            state.list.filtering = false;
            state.list.cursor = 0;
            Vec::new()
        }
        Action::Back => {
            if state.detail.is_some() {
                state.detail = None;
                let _ = state.nav.pop();
            }
            Vec::new()
        }
        Action::Quit => {
            state.should_quit = true;
            Vec::new()
        }
        Action::ListLoaded { token, rows } => {
            if token == state.list.token {
                state.list.rows = rows;
                state.list.loading = false;
                state.list.error = None;
                let visible_len = state.list.visible().len();
                if state.list.cursor >= visible_len {
                    state.list.cursor = visible_len.saturating_sub(1);
                }
                // If the palette is open over this list (e.g. opened before the
                // initial load returned), refresh its entries so freshly-loaded
                // rows become Open items instead of going stale.
                if state.palette.open {
                    state.palette.entries = palette::build_entries(state);
                    let query = state.palette.input.value().to_string();
                    state.palette.ranked = palette::rank(&query, &state.palette.entries);
                    if state.palette.cursor >= state.palette.ranked.len() {
                        state.palette.cursor = state.palette.ranked.len().saturating_sub(1);
                    }
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
        Action::DetailLoaded { token, json } => {
            if let Some(detail) = state.detail.as_mut()
                && detail.token == token
            {
                detail.json = Some(json);
                detail.loading = false;
                detail.error = None;
            }
            Vec::new()
        }
        Action::DetailFailed { token, error } => {
            if let Some(detail) = state.detail.as_mut()
                && detail.token == token
            {
                detail.loading = false;
                detail.error = Some(error);
            }
            Vec::new()
        }
    }
}

/// The effect to fetch the current list view's data. Called by the entry loop
/// on startup and whenever a fresh load is needed.
pub fn initial_load_effect(state: &mut AppState) -> Effect {
    let token = mint_token(state);
    state.list.token = token;
    state.list.loading = true;
    Effect::FetchList {
        kind: state.list.kind,
        token,
        scope: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::context::Context;
    use crate::tui::v2::resource::Row;
    use crate::tui::v2::state::AppState;

    fn test_state() -> AppState {
        let ctx = Context {
            profile: "wyatt".into(),
            workspace: "w".into(),
            user: "u".into(),
        };
        AppState::new(ctx)
    }

    fn rows(n: usize) -> Vec<Row> {
        (0..n)
            .map(|i| Row {
                id: format!("fl_{i}"),
                cells: vec![],
            })
            .collect()
    }

    #[test]
    fn list_loaded_with_matching_token_applies() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let tok = s.list.token;
        let effects = update(
            &mut s,
            Action::ListLoaded {
                token: tok,
                rows: rows(3),
            },
        );
        assert!(!s.list.loading);
        assert_eq!(s.list.rows.len(), 3);
        assert!(effects.is_empty());
    }

    #[test]
    fn list_loaded_with_stale_token_is_dropped() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        update(
            &mut s,
            Action::ListLoaded {
                token: 999,
                rows: rows(3),
            },
        );
        assert!(s.list.loading, "stale result must not clear loading");
        assert_eq!(s.list.rows.len(), 0);
    }

    #[test]
    fn newer_fetch_supersedes_older_generation() {
        // The actual concurrency contract: a result from a superseded fetch
        // must be dropped while the current generation's result applies.
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let ta = s.list.token;
        let _ = initial_load_effect(&mut s); // second fetch supersedes the first
        let tb = s.list.token;
        assert_ne!(ta, tb);

        // Late result from generation A (token ta) arrives after B was minted.
        update(
            &mut s,
            Action::ListLoaded {
                token: ta,
                rows: rows(5),
            },
        );
        assert!(s.list.loading, "older generation result must be dropped");
        assert_eq!(s.list.rows.len(), 0);

        // Current generation B (token tb) applies.
        update(
            &mut s,
            Action::ListLoaded {
                token: tb,
                rows: rows(2),
            },
        );
        assert!(!s.list.loading);
        assert_eq!(s.list.rows.len(), 2);
    }

    #[test]
    fn list_failed_with_matching_token_sets_error() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let tok = s.list.token;
        update(
            &mut s,
            Action::ListFailed {
                token: tok,
                error: "boom".into(),
            },
        );
        assert!(!s.list.loading);
        assert_eq!(s.list.error.as_deref(), Some("boom"));
    }

    #[test]
    fn cursor_moves_within_bounds() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let tok = s.list.token;
        update(
            &mut s,
            Action::ListLoaded {
                token: tok,
                rows: rows(2),
            },
        );
        update(&mut s, Action::CursorDown);
        assert_eq!(s.list.cursor, 1);
        update(&mut s, Action::CursorDown);
        assert_eq!(s.list.cursor, 1);
        update(&mut s, Action::CursorUp);
        assert_eq!(s.list.cursor, 0);
    }

    #[test]
    fn cursor_scrolls_detail_when_open() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let tok = s.list.token;
        update(
            &mut s,
            Action::ListLoaded {
                token: tok,
                rows: rows(3),
            },
        );
        update(&mut s, Action::Open);
        update(&mut s, Action::CursorDown);
        assert_eq!(s.detail.as_ref().unwrap().scroll, 1);
        update(&mut s, Action::CursorUp);
        assert_eq!(s.detail.as_ref().unwrap().scroll, 0);
        assert_eq!(s.list.cursor, 0);
    }

    #[test]
    fn cursor_clamps_when_rows_shrink() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let t1 = s.list.token;
        update(
            &mut s,
            Action::ListLoaded {
                token: t1,
                rows: rows(3),
            },
        );
        update(&mut s, Action::CursorDown);
        update(&mut s, Action::CursorDown);
        assert_eq!(s.list.cursor, 2);

        let _ = initial_load_effect(&mut s);
        let t2 = s.list.token;
        update(
            &mut s,
            Action::ListLoaded {
                token: t2,
                rows: rows(1),
            },
        );
        assert_eq!(s.list.cursor, 0);
        assert_eq!(s.list.rows.len(), 1);
    }

    #[test]
    fn switch_kind_resets_list_and_emits_fetch() {
        use crate::tui::v2::resource::Kind;

        let mut s = test_state();
        let effects = update(&mut s, Action::SwitchKind(Kind::Job));
        assert_eq!(s.list.kind, Kind::Job);
        assert!(s.list.loading);
        assert!(s.list.rows.is_empty());
        assert!(matches!(
            s.nav.top(),
            crate::tui::v2::nav::View::ResourceList { kind: Kind::Job }
        ));
        match effects.as_slice() {
            [
                Effect::FetchList {
                    kind: Kind::Job,
                    token,
                    ..
                },
            ] => assert_eq!(*token, s.list.token),
            other => panic!("expected one FetchList(Job), got {other:?}"),
        }
    }

    #[test]
    fn switch_to_current_kind_is_noop() {
        use crate::tui::v2::resource::Kind;

        let mut s = test_state();
        let effects = update(&mut s, Action::SwitchKind(Kind::Flow));
        assert!(effects.is_empty());
    }

    #[test]
    fn open_drills_into_selected_row_and_emits_fetch_detail() {
        use crate::tui::v2::resource::Kind;

        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let tok = s.list.token;
        update(
            &mut s,
            Action::ListLoaded {
                token: tok,
                rows: rows(2),
            },
        );

        let effects = update(&mut s, Action::Open);
        let d = s.detail.as_ref().expect("detail view created");
        assert!(d.loading);
        assert_eq!(d.id, "fl_0");
        assert!(matches!(
            s.nav.top(),
            crate::tui::v2::nav::View::ResourceDetail { .. }
        ));

        match effects.as_slice() {
            [
                Effect::FetchDetail {
                    kind: Kind::Flow,
                    id,
                    token,
                },
            ] => {
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
        update(&mut s, Action::SwitchKind(Kind::Workspace));
        let tok = s.list.token;
        update(
            &mut s,
            Action::ListLoaded {
                token: tok,
                rows: rows(1),
            },
        );

        let effects = update(&mut s, Action::Open);
        assert!(s.detail.is_none());
        assert!(effects.is_empty());
    }

    #[test]
    fn back_clears_detail_and_pops() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let tok = s.list.token;
        update(
            &mut s,
            Action::ListLoaded {
                token: tok,
                rows: rows(1),
            },
        );
        update(&mut s, Action::Open);
        assert!(s.detail.is_some());

        update(&mut s, Action::Back);
        assert!(s.detail.is_none());
        assert!(matches!(
            s.nav.top(),
            crate::tui::v2::nav::View::ResourceList { .. }
        ));
    }

    #[test]
    fn detail_loaded_with_matching_token_applies() {
        use serde_json::json;

        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let lt = s.list.token;
        update(
            &mut s,
            Action::ListLoaded {
                token: lt,
                rows: rows(1),
            },
        );
        update(&mut s, Action::Open);
        let dt = s.detail.as_ref().unwrap().token;

        update(
            &mut s,
            Action::DetailLoaded {
                token: dt,
                json: json!({ "id": "fl_0" }),
            },
        );

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
        update(
            &mut s,
            Action::ListLoaded {
                token: lt,
                rows: rows(1),
            },
        );
        update(&mut s, Action::Open);

        update(
            &mut s,
            Action::DetailLoaded {
                token: 9999,
                json: json!({}),
            },
        );

        assert!(
            s.detail.as_ref().unwrap().loading,
            "stale detail must not clear loading"
        );
    }

    #[test]
    fn detail_failed_with_matching_token_sets_error() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let lt = s.list.token;
        update(
            &mut s,
            Action::ListLoaded {
                token: lt,
                rows: rows(1),
            },
        );
        update(&mut s, Action::Open);
        let dt = s.detail.as_ref().unwrap().token;

        update(
            &mut s,
            Action::DetailFailed {
                token: dt,
                error: "boom".into(),
            },
        );

        let detail = s.detail.as_ref().unwrap();
        assert!(!detail.loading);
        assert_eq!(detail.error.as_deref(), Some("boom"));
    }

    #[test]
    fn detail_results_match_detail_slot_not_list_slot() {
        use serde_json::json;

        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let list_token = s.list.token;
        update(
            &mut s,
            Action::ListLoaded {
                token: list_token,
                rows: rows(1),
            },
        );
        update(&mut s, Action::Open);

        let detail_token = s.detail.as_ref().unwrap().token;
        assert_ne!(list_token, detail_token);

        update(
            &mut s,
            Action::DetailLoaded {
                token: list_token,
                json: json!({ "id": "wrong-slot" }),
            },
        );
        assert!(
            s.detail.as_ref().unwrap().loading,
            "list token must not satisfy detail slot"
        );

        update(
            &mut s,
            Action::DetailLoaded {
                token: detail_token,
                json: json!({ "id": "right-slot" }),
            },
        );
        let detail = s.detail.as_ref().unwrap();
        assert!(!detail.loading);
        assert_eq!(detail.json.as_ref().unwrap()["id"], "right-slot");
    }

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
        assert_eq!(s.list.cursor, 0, "cursor resets when filter changes");
        assert_eq!(s.list.visible().len(), 1);
        update(&mut s, Action::FilterApply);
        assert!(!s.list.filtering);
        assert_eq!(s.list.filter.value(), "3", "apply keeps the term");
        update(&mut s, Action::FilterClear);
        assert!(s.list.filter.value().is_empty());
        assert!(!s.list.filtering);
    }

    #[test]
    fn palette_open_builds_entries_and_ranks_all() {
        let mut s = test_state();
        let _ = initial_load_effect(&mut s);
        let token = s.list.token;
        update(
            &mut s,
            Action::ListLoaded {
                token,
                rows: rows(2),
            },
        );
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
        assert!(s.palette.ranked.is_empty());
        assert_eq!(s.palette.cursor, 0);
    }

    #[test]
    fn palette_activate_switch_kind_resets_list_and_closes() {
        use crate::tui::v2::palette::PaletteAction;
        use crate::tui::v2::resource::Kind;

        let mut s = test_state();
        update(&mut s, Action::PaletteOpen);
        let job_idx = s
            .palette
            .entries
            .iter()
            .position(|e| matches!(e.action, PaletteAction::SwitchKind(Kind::Job)))
            .unwrap();
        s.palette.cursor = s.palette.ranked.iter().position(|&i| i == job_idx).unwrap();
        let effects = update(&mut s, Action::PaletteActivate);
        assert!(!s.palette.open);
        assert_eq!(s.list.kind, Kind::Job);
        assert!(matches!(
            effects.as_slice(),
            [Effect::FetchList {
                kind: Kind::Job,
                ..
            }]
        ));
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

    #[test]
    fn palette_open_clears_filtering() {
        // Opening the palette must drop filter-edit mode so it can't survive a
        // palette-driven drill and silently capture keys back on the list.
        let mut s = test_state();
        s.list.filtering = true;
        update(&mut s, Action::PaletteOpen);
        assert!(!s.list.filtering);
    }

    #[test]
    fn list_loaded_refreshes_open_palette() {
        use crate::tui::v2::palette::PaletteCategory;
        let mut s = test_state(); // Flow list, empty
        let _ = initial_load_effect(&mut s);
        let tok = s.list.token;
        update(&mut s, Action::PaletteOpen);
        // Palette opened over an empty list: 5 resources, 0 items.
        assert_eq!(
            s.palette
                .entries
                .iter()
                .filter(|e| matches!(e.category, PaletteCategory::Item))
                .count(),
            0
        );
        update(
            &mut s,
            Action::ListLoaded {
                token: tok,
                rows: rows(2),
            },
        );
        // Newly-loaded rows now appear as Open items in the live palette.
        assert_eq!(
            s.palette
                .entries
                .iter()
                .filter(|e| matches!(e.category, PaletteCategory::Item))
                .count(),
            2
        );
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
