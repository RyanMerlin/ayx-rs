//! Scrollable detail view: pretty-prints the fetched object as key/value lines.
//! Fixes the legacy 18-line truncation - Paragraph scroll shows any length.
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use serde_json::Value;

use crate::tui::theme;
use crate::tui::v2::state::AppState;

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let Some(d) = state.detail.as_ref() else {
        return;
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border(true))
        .style(theme::panel())
        .title(Span::styled(
            format!(" {} · {} ", d.kind.singular(), d.title),
            theme::accent(),
        ));

    if d.loading {
        frame.render_widget(
            Paragraph::new(" ⟳ loading… ")
                .block(block)
                .style(theme::dim()),
            area,
        );
        return;
    }
    if let Some(err) = &d.error {
        frame.render_widget(
            Paragraph::new(format!(" error: {err} "))
                .block(block)
                .style(theme::danger()),
            area,
        );
        return;
    }

    let lines = d.json.as_ref().map(json_lines).unwrap_or_default();
    frame.render_widget(
        Paragraph::new(lines).block(block).scroll((d.scroll, 0)),
        area,
    );
}

/// Flatten a JSON object's top-level fields to `key: value` lines. Nested
/// objects/arrays are pretty-printed and indented under their key.
fn json_lines(v: &Value) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                match val {
                    Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                        out.push(Line::from(vec![
                            Span::styled(format!("{k}: "), theme::field_label()),
                            Span::styled(scalar(val), theme::field_value()),
                        ]));
                    }
                    _ => {
                        out.push(Line::from(Span::styled(
                            format!("{k}:"),
                            theme::field_label(),
                        )));
                        let pretty = serde_json::to_string_pretty(val).unwrap_or_default();
                        for line in pretty.lines() {
                            out.push(Line::from(Span::styled(
                                format!("  {line}"),
                                theme::field_value(),
                            )));
                        }
                    }
                }
            }
        }
        other => {
            let pretty = serde_json::to_string_pretty(other).unwrap_or_default();
            for line in pretty.lines() {
                out.push(Line::from(Span::styled(
                    line.to_string(),
                    theme::field_value(),
                )));
            }
        }
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled("(empty)", theme::muted())));
    }
    out
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::context::Context;
    use crate::tui::v2::resource::Kind;
    use crate::tui::v2::state::{AppState, DetailView};
    use ratatui::{Terminal, backend::TestBackend};
    use serde_json::json;

    fn text_for(state: &AppState) -> String {
        let backend = TestBackend::new(80, 20);
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

    fn state_with_detail(json: serde_json::Value, loading: bool) -> AppState {
        let ctx = Context {
            profile: "wyatt".into(),
            workspace: "w".into(),
            user: "u".into(),
        };
        let mut s = AppState::new(ctx);
        let mut d = DetailView::new(Kind::Flow, "fl_1".into(), "ETL".into(), 1);
        d.loading = loading;
        d.json = (!loading).then_some(json);
        s.detail = Some(d);
        s
    }

    #[test]
    fn renders_fields() {
        let s = state_with_detail(json!({ "id": "fl_1", "name": "ETL Pipeline" }), false);
        let txt = text_for(&s);
        assert!(txt.contains("ETL Pipeline"));
        assert!(txt.contains("id"));
    }

    #[test]
    fn shows_loading() {
        let s = state_with_detail(json!({}), true);
        assert!(text_for(&s).to_lowercase().contains("loading"));
    }
}
