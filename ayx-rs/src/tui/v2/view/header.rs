//! Context header + resource tabs / breadcrumb. Always visible — the guard
//! against acting in the wrong workspace.
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::theme;
use crate::tui::v2::nav::View;
use crate::tui::v2::resource::Kind;
use crate::tui::v2::state::AppState;

pub fn render(frame: &mut Frame, state: &AppState, area: Rect) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);

    let ctx = &state.context;
    let header = Line::from(vec![
        Span::styled(" Profile: ", theme::muted()),
        Span::styled(ctx.profile.clone(), theme::accent_bold()),
        Span::styled("  ·  Workspace: ", theme::muted()),
        Span::styled(ctx.workspace.clone(), theme::accent_bold()),
        Span::styled("  ·  ", theme::muted()),
        Span::styled(ctx.user.clone(), theme::dim()),
    ]);
    frame.render_widget(Paragraph::new(header).style(theme::panel()), rows[0]);

    match state.nav.top() {
        View::ResourceList { .. } => {
            frame.render_widget(Paragraph::new(tabs_line(state.list.kind)), rows[1]);
        }
        View::ResourceDetail { .. } => {
            let crumb = Line::from(vec![
                Span::styled(" ", theme::dim()),
                Span::styled(state.nav.breadcrumb(), theme::dim()),
            ]);
            frame.render_widget(Paragraph::new(crumb), rows[1]);
        }
    }
}

fn tabs_line(active: Kind) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for &k in Kind::all() {
        let label = format!(" {} {} ", k.index() + 1, k.name());
        let style = if k == active {
            theme::selected()
        } else {
            theme::dim()
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::context::Context;
    use crate::tui::v2::resource::Kind;
    use crate::tui::v2::state::AppState;
    use ratatui::{Terminal, backend::TestBackend};

    fn text_for(state: &AppState) -> String {
        let backend = TestBackend::new(120, 2);
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

    #[test]
    fn tabs_show_all_kinds_on_list_view() {
        let ctx = Context {
            profile: "wyatt".into(),
            workspace: "w".into(),
            user: "u".into(),
        };
        let s = AppState::new(ctx);
        let txt = text_for(&s);
        assert!(txt.contains("flows"));
        assert!(txt.contains("connections"));
        assert!(txt.contains("jobs"));
        assert!(txt.contains("people"));
        assert!(txt.contains("workspaces"));
        let _ = Kind::Flow;
    }
}
