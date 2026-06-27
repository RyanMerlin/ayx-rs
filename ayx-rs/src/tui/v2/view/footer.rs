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
