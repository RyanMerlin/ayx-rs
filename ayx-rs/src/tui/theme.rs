use ratatui::style::{Color, Modifier, Style};

pub const ACCENT: Color = Color::Rgb(0, 133, 202);
pub const BORDER_DIM: Color = Color::Rgb(42, 52, 60);
pub const TEXT: Color = Color::Rgb(228, 232, 236);
pub const TEXT_DIM: Color = Color::Rgb(139, 153, 163);
pub const TEXT_MUTED: Color = Color::Rgb(106, 119, 130);
pub const SURFACE: Color = Color::Rgb(12, 18, 24);
pub const PANEL: Color = Color::Rgb(17, 24, 32);
pub const SELECT_BG: Color = Color::Rgb(18, 40, 54);
pub const OK: Color = Color::Rgb(86, 180, 123);
pub const WARN: Color = Color::Rgb(224, 177, 72);
pub const DANGER: Color = Color::Rgb(220, 93, 93);

pub fn app() -> Style {
    Style::default().fg(TEXT).bg(SURFACE)
}

pub fn panel() -> Style {
    Style::default().fg(TEXT).bg(PANEL)
}

pub fn accent() -> Style {
    Style::default().fg(ACCENT)
}

pub fn accent_bold() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().fg(TEXT_DIM)
}

pub fn muted() -> Style {
    Style::default().fg(TEXT_MUTED)
}

pub fn ok() -> Style {
    Style::default().fg(OK)
}

pub fn warn() -> Style {
    Style::default().fg(WARN)
}

pub fn danger() -> Style {
    Style::default().fg(DANGER)
}

pub fn selected() -> Style {
    Style::default()
        .fg(TEXT)
        .bg(SELECT_BG)
        .add_modifier(Modifier::BOLD)
}

pub fn border(focused: bool) -> Style {
    if focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(BORDER_DIM)
    }
}

pub fn badge_ok() -> Style {
    Style::default().fg(OK).bg(Color::Rgb(17, 46, 29))
}

pub fn badge_warn() -> Style {
    Style::default().fg(WARN).bg(Color::Rgb(50, 37, 12))
}

pub fn field_label() -> Style {
    Style::default().fg(TEXT_DIM)
}

pub fn field_value() -> Style {
    Style::default().fg(TEXT)
}

pub fn field_placeholder() -> Style {
    Style::default().fg(TEXT_MUTED)
}

pub fn status_line(error: bool) -> Style {
    if error { danger() } else { ok() }
}
