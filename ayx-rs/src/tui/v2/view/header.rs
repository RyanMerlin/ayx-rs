//! Context header + breadcrumb. Always visible — the guard against acting in
//! the wrong workspace.
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::theme;
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

    let crumb = Line::from(vec![
        Span::styled(" ", theme::dim()),
        Span::styled(state.nav.breadcrumb(), theme::dim()),
    ]);
    frame.render_widget(Paragraph::new(crumb), rows[1]);
}
