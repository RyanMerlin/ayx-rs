//! Ctrl+K command palette overlay - centered modal: query line + ranked,
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
    let mut lines: Vec<Line<'static>> = Vec::new();
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
        let style = if selected {
            theme::selected()
        } else {
            theme::field_value()
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{}", entry.label),
            style,
        )));
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

/// A rect `pct_w`% x `pct_h`% of `area`, centered.
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
    use crate::tui::v2::action::{Action, update};
    use crate::tui::v2::context::Context;
    use crate::tui::v2::state::AppState;
    use ratatui::{Terminal, backend::TestBackend};

    fn open_palette_state() -> AppState {
        let ctx = Context {
            profile: "w".into(),
            workspace: "w".into(),
            user: "u".into(),
        };
        let mut s = AppState::new(ctx);
        update(&mut s, Action::PaletteOpen);
        s
    }

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
        let ctx = Context {
            profile: "w".into(),
            workspace: "w".into(),
            user: "u".into(),
        };
        let s = AppState::new(ctx); // palette closed
        let txt = text_of(&s);
        assert!(!txt.contains("Command Palette"));
    }
}
