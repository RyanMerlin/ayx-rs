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
    Effect::FetchList {
        kind: state.list.kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::context::Context;
    use crate::tui::v2::resource::{Kind, Row};
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
    fn list_loaded_populates_and_clears_loading() {
        let mut s = test_state();
        assert!(s.list.loading);
        let effects = update(
            &mut s,
            Action::ListLoaded {
                kind: Kind::Flow,
                rows: rows(3),
            },
        );
        assert!(!s.list.loading);
        assert_eq!(s.list.rows.len(), 3);
        assert!(effects.is_empty());
    }

    #[test]
    fn list_failed_sets_error_clears_loading() {
        let mut s = test_state();
        update(
            &mut s,
            Action::ListFailed {
                kind: Kind::Flow,
                error: "boom".into(),
            },
        );
        assert!(!s.list.loading);
        assert_eq!(s.list.error.as_deref(), Some("boom"));
    }

    #[test]
    fn cursor_moves_within_bounds() {
        let mut s = test_state();
        update(
            &mut s,
            Action::ListLoaded {
                kind: Kind::Flow,
                rows: rows(2),
            },
        );
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
