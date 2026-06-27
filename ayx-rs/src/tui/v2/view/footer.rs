//! Contextual footer hint bar — plain-language labels, always visible.
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::theme;
use crate::tui::v2::state::AppState;

pub fn render(frame: &mut Frame, _state: &AppState, area: Rect) {
    let hint = Line::from(vec![
        key(" ↵ "),
        label("Open  "),
        key(" / "),
        label("Filter  "),
        key(" ^K "),
        label("Palette  "),
        key(" ? "),
        label("Help  "),
        key(" q "),
        label("Quit"),
    ]);
    frame.render_widget(Paragraph::new(hint).style(theme::panel()), area);
}

fn key(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), theme::accent_bold())
}

fn label(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), theme::dim())
}
