//! Command palette model + fuzzy ranking. The palette unifies resource-switch
//! and item-open actions into one ranked list (nucleo-matcher). Workspace/action
//! entries arrive in later phases - `PaletteAction` is extended then.
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
    OpenItem {
        kind: Kind,
        id: String,
        title: String,
    },
}

#[derive(Debug, Clone)]
pub struct PaletteEntry {
    pub label: String,
    pub category: PaletteCategory,
    pub action: PaletteAction,
}

#[derive(Debug, Clone, Default)]
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

    for &kind in Kind::all() {
        entries.push(PaletteEntry {
            label: format!("Browse {}", kind.name()),
            category: PaletteCategory::Resource,
            action: PaletteAction::SwitchKind(kind),
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
                .map(|cell| cell.text.clone())
                .unwrap_or_else(|| row.id.clone());
            entries.push(PaletteEntry {
                label: format!("Open {}: {}", kind.singular(), title),
                category: PaletteCategory::Item,
                action: PaletteAction::OpenItem {
                    kind,
                    id: row.id.clone(),
                    title,
                },
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

    for (index, entry) in entries.iter().enumerate() {
        let haystack = Utf32Str::new(&entry.label, &mut buf);
        if let Some(score) = pattern.score(haystack, &mut matcher) {
            scored.push((index, score));
        }
    }

    // Highest score first; stable on ties so natural order remains the fallback.
    scored.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    scored.into_iter().map(|(index, _)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::context::Context;
    use crate::tui::v2::resource::{Cell, Kind, Row};
    use crate::tui::v2::state::AppState;

    fn state_with_flow_rows() -> AppState {
        let ctx = Context {
            profile: "w".into(),
            workspace: "w".into(),
            user: "u".into(),
        };
        let mut s = AppState::new(ctx); // Flow root
        s.list.loading = false;
        s.list.rows = vec![
            Row {
                id: "fl_1".into(),
                cells: vec![Cell::plain("Daily ETL")],
            },
            Row {
                id: "fl_2".into(),
                cells: vec![Cell::plain("Sales Rollup")],
            },
        ];
        s
    }

    #[test]
    fn build_entries_has_five_resources_plus_items() {
        let s = state_with_flow_rows();
        let entries = build_entries(&s);
        let resources = entries
            .iter()
            .filter(|e| matches!(e.category, PaletteCategory::Resource))
            .count();
        let items = entries
            .iter()
            .filter(|e| matches!(e.category, PaletteCategory::Item))
            .count();
        assert_eq!(resources, 5);
        assert_eq!(items, 2); // Flow has a detail endpoint -> rows become Open items
        assert!(entries.iter().any(|e| e.label == "Open flow: Daily ETL"));
    }

    #[test]
    fn workspace_rows_are_not_openable_items() {
        let ctx = Context {
            profile: "w".into(),
            workspace: "w".into(),
            user: "u".into(),
        };
        let mut s = AppState::new(ctx);
        s.list = crate::tui::v2::state::ListView::new(Kind::Workspace);
        s.list.loading = false;
        s.list.rows = vec![Row {
            id: "ws_1".into(),
            cells: vec![Cell::plain("Prod")],
        }];
        let entries = build_entries(&s);
        // 5 resource entries, but no Item entries (Workspace has no detail endpoint)
        assert_eq!(
            entries
                .iter()
                .filter(|e| matches!(e.category, PaletteCategory::Item))
                .count(),
            0
        );
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
