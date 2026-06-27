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
    ("r", "On a flow: show its runs"),
    ("f", "On a run: open its flow"),
    ("^K", "Command Palette"),
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
        .title(Span::styled(
            format!(" Help — Keys · ayx v{} ", env!("CARGO_PKG_VERSION")),
            theme::accent(),
        ));
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
    fn help_shows_compiled_version() {
        // The version is the diagnostic for "am I on the right build" — it must
        // be the crate version baked at compile time, not a hardcoded string.
        let mut s = base();
        s.help_open = true;
        let txt = text_of(&s);
        assert!(txt.contains("ayx v"));
        assert!(txt.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn help_renders_nothing_when_closed() {
        let txt = text_of(&base());
        assert!(!txt.contains("Keys"));
    }
}
