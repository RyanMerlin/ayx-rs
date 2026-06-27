//! Resource list table + reactive detail split panel.
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row as TRow, Table, TableState};

use crate::tui::theme;
use crate::tui::v2::resource::{Cell, StatusTone, kind_impl};
use crate::tui::v2::state::AppState;

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let panes =
        Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).split(area);
    render_table(frame, state, panes[0]);
    render_detail(frame, state, panes[1]);
}

fn tone_style(tone: StatusTone) -> Style {
    match tone {
        StatusTone::Neutral => theme::field_value(),
        StatusTone::Ok => theme::ok(),
        StatusTone::Warn => theme::warn(),
        StatusTone::Danger => theme::danger(),
    }
}

fn render_table(frame: &mut Frame, state: &AppState, area: Rect) {
    let imp = kind_impl(state.list.kind);
    let visible = state.list.visible();
    let title = if state.list.filter.is_empty() {
        format!(" {} · {} ", state.list.kind.name(), state.list.rows.len())
    } else {
        format!(
            " {} · {}/{}  /{}{} ",
            state.list.kind.name(),
            visible.len(),
            state.list.rows.len(),
            state.list.filter,
            if state.list.filtering { "▏" } else { "" }
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border(true))
        .style(theme::panel())
        .title(Span::styled(title, theme::accent()));

    if state.list.loading {
        frame.render_widget(
            Paragraph::new(" ⟳ loading… ")
                .block(block)
                .style(theme::dim()),
            area,
        );
        return;
    }
    if let Some(err) = &state.list.error {
        frame.render_widget(
            Paragraph::new(format!(" error: {err} "))
                .block(block)
                .style(theme::danger()),
            area,
        );
        return;
    }
    if visible.is_empty() {
        frame.render_widget(
            Paragraph::new(" no matches ")
                .block(block)
                .style(theme::muted()),
            area,
        );
        return;
    }

    let header = TRow::new(
        imp.columns()
            .iter()
            .map(|c| Span::styled(c.title, theme::muted()))
            .collect::<Vec<_>>(),
    );
    let widths: Vec<Constraint> = imp
        .columns()
        .iter()
        .map(|c| Constraint::Length(c.width))
        .collect();
    let rows: Vec<TRow> = visible
        .iter()
        .map(|r| TRow::new(r.cells.iter().map(render_cell).collect::<Vec<_>>()))
        .collect();

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(theme::selected())
        .highlight_symbol("▸ ");
    let mut ts = TableState::default();
    ts.select(Some(state.list.cursor));
    frame.render_stateful_widget(table, area, &mut ts);
}

fn render_cell(cell: &Cell) -> Span<'static> {
    Span::styled(cell.text.clone(), tone_style(cell.tone))
}

fn render_detail(frame: &mut Frame, state: &AppState, area: Rect) {
    let imp = kind_impl(state.list.kind);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border(false))
        .style(theme::panel())
        .title(Span::styled(" detail ", theme::muted()));
    let lines: Vec<Line<'static>> = match state.list.selected() {
        Some(row) => row
            .cells
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let title = imp.columns().get(i).map(|c| c.title).unwrap_or("");
                Line::from(vec![
                    Span::styled(format!("{title}: "), theme::field_label()),
                    Span::styled(c.text.clone(), tone_style(c.tone)),
                ])
            })
            .collect(),
        None => vec![Line::from(Span::styled("no selection", theme::muted()))],
    };
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
