//! Contextual footer hint bar — plain-language labels, changes per view.
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::theme;
use crate::tui::v2::nav::View;
use crate::tui::v2::resource::kind_impl;
use crate::tui::v2::state::AppState;

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let hint = if state.list.filtering
        && matches!(
            state.nav.top(),
            View::ResourceList { .. } | View::ScopedList { .. }
        ) {
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
                key(" ^K "),
                label("Palette  "),
                key(" ? "),
                label("Help  "),
                key(" q "),
                label("Quit"),
            ]),
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
            View::ResourceList { .. } => {
                // Only advertise Open when the current kind can actually drill in
                // (Workspaces have no detail endpoint — don't promise a dead key).
                let mut spans = Vec::new();
                if kind_impl(state.list.kind).detail_endpoint().is_some() {
                    spans.push(key(" ↵ "));
                    spans.push(label("Open  "));
                }
                spans.push(key(" / "));
                spans.push(label("Filter  "));
                spans.push(key(" 1-5/⇥ "));
                spans.push(label("Switch  "));
                spans.push(key(" ^K "));
                spans.push(label("Palette  "));
                spans.push(key(" ? "));
                spans.push(label("Help  "));
                spans.push(key(" q "));
                spans.push(label("Quit"));
                Line::from(spans)
            }
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
        terminal
            .backend()
            .buffer()
            .clone()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    fn base() -> AppState {
        let ctx = Context {
            profile: "w".into(),
            workspace: "w".into(),
            user: "u".into(),
        };
        AppState::new(ctx)
    }

    #[test]
    fn list_footer_has_open_and_filter() {
        let txt = text_for(&base());
        assert!(txt.contains("Open"));
        assert!(txt.contains("Filter"));
        assert!(txt.contains("Switch"));
        assert!(txt.contains("Palette"));
        assert!(txt.contains("Help"));
    }

    #[test]
    fn list_footer_omits_open_when_kind_has_no_detail() {
        // Workspaces have no detail endpoint — the footer must not promise Open.
        let mut s = base();
        s.list = crate::tui::v2::state::ListView::new(Kind::Workspace);
        let txt = text_for(&s);
        assert!(
            !txt.contains("Open"),
            "workspace list must not advertise Open"
        );
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
        assert!(txt.contains("Palette"));
    }

    #[test]
    fn scoped_list_footer_has_back() {
        let mut s = base();
        s.list = crate::tui::v2::state::ListView::new(Kind::Job);
        s.nav.push(crate::tui::v2::nav::View::ScopedList {
            child_kind: Kind::Job,
            parent_kind: Kind::Flow,
            parent_id: "fl_1".into(),
            parent_title: "ETL".into(),
        });
        let txt = text_for(&s);
        assert!(txt.contains("Back"));
        assert!(txt.contains("Filter"));
    }
}
