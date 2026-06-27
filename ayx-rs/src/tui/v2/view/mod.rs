//! Render dispatcher: context header (top) + body (list/detail) + footer.
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::Block;

use crate::tui::theme;
use crate::tui::v2::nav::View;
use crate::tui::v2::state::AppState;

mod detail;
mod footer;
mod header;
mod list;
mod palette;

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
    palette::render(frame, state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::context::Context;
    use crate::tui::v2::resource::Row;
    use crate::tui::v2::state::AppState;
    use ratatui::{Terminal, backend::TestBackend};

    fn state_with_rows() -> AppState {
        let ctx = Context {
            profile: "wyatt".into(),
            workspace: "w_marketing".into(),
            user: "ryan@alteryx.com".into(),
        };
        let mut s = AppState::new(ctx);
        s.list.loading = false;
        s.list.rows = vec![Row {
            id: "fl_1".into(),
            cells: vec![
                crate::tui::v2::resource::Cell::plain("ETL Pipeline"),
                crate::tui::v2::resource::Cell::plain("2026-06-20"),
                crate::tui::v2::resource::Cell::plain("fl_1"),
            ],
        }];
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
        assert!(text.contains("Switch"));
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
            terminal
                .backend()
                .buffer()
                .clone()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
        };
        assert!(text.to_lowercase().contains("loading"));
    }
}
