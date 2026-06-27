//! Actions (user intents + async results) and the `update` reducer. The
//! reducer is the only place state mutates; it returns Effects for the
//! worker to run. Pure-ish: no I/O here.
use crate::tui::v2::effect::Effect;
use crate::tui::v2::nav::{NavStack, View};
use crate::tui::v2::resource::{Kind, Row};
use crate::tui::v2::state::{AppState, ListView};

#[derive(Debug, Clone)]
pub enum Action {
    CursorDown,
    CursorUp,
    SwitchKind(Kind),
    Back,
    Quit,
    ListLoaded { token: u64, rows: Vec<Row> },
    ListFailed { token: u64, error: String },
}

/// Mint the next monotonic request token.
pub(crate) fn mint_token(state: &mut AppState) -> u64 {
    state.req_seq += 1;
    state.req_seq
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
        Action::SwitchKind(kind) => {
            if state.list.kind == kind && matches!(state.nav.top(), View::ResourceList { .. }) {
                return Vec::new();
            }

            state.nav = NavStack::new(View::ResourceList { kind });
            state.list = ListView::new(kind);
            let token = mint_token(state);
            state.list.token = token;
            vec![Effect::FetchList { kind, token }]
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
