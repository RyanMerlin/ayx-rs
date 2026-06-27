//! TUI v2 entry: config load, terminal setup, and the main event loop.
use std::io;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_core::profile::{Config, resolve_runtime_profile};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tui_input::InputRequest;

use crate::tui::store::default_config_with_profile;
use crate::tui::v2::action::{Action, initial_load_effect, update};
use crate::tui::v2::context::Context;
use crate::tui::v2::effect::Effect;
use crate::tui::v2::nav::View;
use crate::tui::v2::state::AppState;
use crate::tui::v2::view;
use crate::tui::v2::worker::Worker;

pub fn run() -> Result<Envelope> {
    let runtime_resolution = resolve_runtime_profile(None).map_err(anyhow::Error::from)?;
    let target_path = Path::new(&runtime_resolution.resolved_profile_path);
    let config = Config::load_from_path_lenient_without_active_overlay(target_path)
        .unwrap_or_else(|_| default_config_with_profile(&runtime_resolution.selected_profile));
    let context = Context::from_config(&config, Some(&runtime_resolution.selected_profile));

    let mut state = AppState::new(context);
    let worker = Worker::spawn();
    let first = initial_load_effect(&mut state);
    worker.submit(first, config.clone());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        previous_hook(panic_info);
    }));

    let result = main_loop(&mut terminal, &mut state, &worker, &config);

    drop(worker);
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result?;
    Ok(Envelope::ok("tui v2 session ended"))
}

fn main_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
    worker: &Worker,
    config: &Config,
) -> Result<()> {
    loop {
        while let Ok(outcome) = worker.try_recv() {
            let effects = update(state, outcome.action);
            dispatch_effects(effects, worker, config);
        }

        terminal.draw(|frame| view::render(frame, state))?;

        if event::poll(Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(action) = map_key(state, key)
        {
            let effects = update(state, action);
            dispatch_effects(effects, worker, config);
        }

        if state.should_quit {
            break;
        }
    }

    Ok(())
}

fn dispatch_effects(effects: Vec<Effect>, worker: &Worker, config: &Config) {
    for effect in effects {
        worker.submit(effect, config.clone());
    }
}

/// Map a key to a tui-input request, using only the pure (backend-agnostic)
/// `InputRequest` API so no crossterm version coupling is introduced. Returns
/// None for keys that are not text-editing input. Ctrl/Alt+char is NOT an insert.
fn key_to_input_request(key: KeyEvent) -> Option<InputRequest> {
    use crossterm::event::KeyModifiers;

    match key.code {
        KeyCode::Char(c)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            Some(InputRequest::InsertChar(c))
        }
        KeyCode::Backspace => Some(InputRequest::DeletePrevChar),
        KeyCode::Delete => Some(InputRequest::DeleteNextChar),
        KeyCode::Left => Some(InputRequest::GoToPrevChar),
        KeyCode::Right => Some(InputRequest::GoToNextChar),
        KeyCode::Home => Some(InputRequest::GoToStart),
        KeyCode::End => Some(InputRequest::GoToEnd),
        _ => None,
    }
}

fn map_key(state: &AppState, key: KeyEvent) -> Option<Action> {
    use crate::tui::v2::nav::View;
    use crate::tui::v2::resource::Kind;
    use crossterm::event::KeyModifiers;

    // 1) Palette is modal - it captures everything while open.
    if state.palette.open {
        return match key.code {
            KeyCode::Esc => Some(Action::PaletteClose),
            KeyCode::Enter => Some(Action::PaletteActivate),
            KeyCode::Down => Some(Action::PaletteDown),
            KeyCode::Up => Some(Action::PaletteUp),
            _ => key_to_input_request(key).map(Action::PaletteEdit),
        };
    }

    // 2) Help overlay - any key dismisses it.
    if state.help_open {
        return Some(Action::HelpClose);
    }

    // 3) Ctrl+K opens the palette from anywhere else.
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('k') | KeyCode::Char('K'))
    {
        return Some(Action::PaletteOpen);
    }

    let on_detail = matches!(state.nav.top(), View::ResourceDetail { .. });
    let detail_kind = state
        .detail
        .as_ref()
        .filter(|_| on_detail)
        .map(|detail| detail.kind);

    // 4) Filter input mode (list only).
    if state.list.filtering && !on_detail {
        return match key.code {
            KeyCode::Enter => Some(Action::FilterApply),
            KeyCode::Esc => Some(Action::FilterClear),
            _ => key_to_input_request(key).map(Action::FilterEdit),
        };
    }

    // 5) Normal bindings.
    match key.code {
        KeyCode::Char('r') if matches!(detail_kind, Some(Kind::Flow)) => Some(Action::ShowRuns),
        KeyCode::Char('f') if matches!(detail_kind, Some(Kind::Job)) => {
            Some(Action::OpenParentFlow)
        }
        KeyCode::Char('?') => Some(Action::HelpToggle),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::CursorDown),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::CursorUp),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Enter => Some(if on_detail {
            Action::Back
        } else {
            Action::Open
        }),
        KeyCode::Char('/') if !on_detail => Some(Action::FilterStart),
        KeyCode::Char(c @ '1'..='5') => {
            Kind::from_index((c as u8 - b'1') as usize).map(Action::SwitchKind)
        }
        KeyCode::Tab => {
            let n = Kind::all().len();
            Kind::from_index((state.list.kind.index() + 1) % n).map(Action::SwitchKind)
        }
        KeyCode::BackTab => {
            let n = Kind::all().len();
            Kind::from_index((state.list.kind.index() + n - 1) % n).map(Action::SwitchKind)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::action::Action;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn list_state() -> crate::tui::v2::state::AppState {
        let ctx = crate::tui::v2::context::Context {
            profile: "w".into(),
            workspace: "w".into(),
            user: "u".into(),
        };
        crate::tui::v2::state::AppState::new(ctx)
    }

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn kc(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn detail_state(kind: crate::tui::v2::resource::Kind) -> AppState {
        let ctx = crate::tui::v2::context::Context {
            profile: "w".into(),
            workspace: "w".into(),
            user: "u".into(),
        };
        let mut state = crate::tui::v2::state::AppState::new(ctx);
        state.nav.push(View::ResourceDetail {
            kind,
            id: "id-1".into(),
            title: "detail".into(),
        });
        state.detail = Some(crate::tui::v2::state::DetailView {
            kind,
            id: "id-1".into(),
            title: "detail".into(),
            loading: false,
            json: None,
            error: None,
            scroll: 0,
            token: 1,
        });
        state
    }

    #[test]
    fn arrows_and_vim_keys_map_to_cursor() {
        let s = list_state();
        assert!(matches!(
            map_key(&s, k(KeyCode::Down)),
            Some(Action::CursorDown)
        ));
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('j'))),
            Some(Action::CursorDown)
        ));
        assert!(matches!(
            map_key(&s, k(KeyCode::Up)),
            Some(Action::CursorUp)
        ));
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('k'))),
            Some(Action::CursorUp)
        ));
    }

    #[test]
    fn ctrl_k_opens_palette() {
        let s = list_state();
        assert!(matches!(
            map_key(&s, kc(KeyCode::Char('k'))),
            Some(Action::PaletteOpen)
        ));
    }

    #[test]
    fn q_quits_esc_is_back() {
        let s = list_state();
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('q'))),
            Some(Action::Quit)
        ));
        assert!(matches!(map_key(&s, k(KeyCode::Esc)), Some(Action::Back)));
    }

    #[test]
    fn number_keys_switch_kind() {
        use crate::tui::v2::resource::Kind;

        let s = list_state();
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('1'))),
            Some(Action::SwitchKind(Kind::Flow))
        ));
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('3'))),
            Some(Action::SwitchKind(Kind::Job))
        ));
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('5'))),
            Some(Action::SwitchKind(Kind::Workspace))
        ));
        assert!(map_key(&s, k(KeyCode::Char('6'))).is_none());
    }

    #[test]
    fn enter_opens_on_list() {
        let s = list_state();
        assert!(matches!(map_key(&s, k(KeyCode::Enter)), Some(Action::Open)));
    }

    #[test]
    fn flow_detail_r_opens_runs() {
        use crate::tui::v2::resource::Kind;

        let s = detail_state(Kind::Flow);
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('r'))),
            Some(Action::ShowRuns)
        ));
    }

    #[test]
    fn job_detail_f_opens_parent_flow() {
        use crate::tui::v2::resource::Kind;

        let s = detail_state(Kind::Job);
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('f'))),
            Some(Action::OpenParentFlow)
        ));
    }

    #[test]
    fn relation_keys_do_not_cross_wire() {
        use crate::tui::v2::resource::Kind;

        let flow = detail_state(Kind::Flow);
        let job = detail_state(Kind::Job);
        assert!(map_key(&flow, k(KeyCode::Char('f'))).is_none());
        assert!(map_key(&job, k(KeyCode::Char('r'))).is_none());
    }

    #[test]
    fn tab_cycles_kind() {
        use crate::tui::v2::resource::Kind;

        let s = list_state();
        assert!(matches!(
            map_key(&s, k(KeyCode::Tab)),
            Some(Action::SwitchKind(Kind::Connection))
        ));
        assert!(matches!(
            map_key(&s, k(KeyCode::BackTab)),
            Some(Action::SwitchKind(Kind::Workspace))
        ));
    }

    #[test]
    fn slash_starts_filter_then_typing_feeds_it() {
        use tui_input::InputRequest;

        let mut s = list_state();
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('/'))),
            Some(Action::FilterStart)
        ));
        s.list.filtering = true;
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('x'))),
            Some(Action::FilterEdit(InputRequest::InsertChar('x')))
        ));
        assert!(matches!(
            map_key(&s, k(KeyCode::Enter)),
            Some(Action::FilterApply)
        ));
    }

    #[test]
    fn palette_open_captures_keys() {
        use tui_input::InputRequest;

        let mut s = list_state();
        s.palette.open = true;
        assert!(matches!(
            map_key(&s, k(KeyCode::Esc)),
            Some(Action::PaletteClose)
        ));
        assert!(matches!(
            map_key(&s, k(KeyCode::Enter)),
            Some(Action::PaletteActivate)
        ));
        assert!(matches!(
            map_key(&s, k(KeyCode::Down)),
            Some(Action::PaletteDown)
        ));
        assert!(matches!(
            map_key(&s, k(KeyCode::Up)),
            Some(Action::PaletteUp)
        ));
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('f'))),
            Some(Action::PaletteEdit(InputRequest::InsertChar('f')))
        ));
    }

    #[test]
    fn question_mark_toggles_help_and_help_captures() {
        let mut s = list_state();
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('?'))),
            Some(Action::HelpToggle)
        ));
        s.help_open = true;
        assert!(matches!(
            map_key(&s, k(KeyCode::Esc)),
            Some(Action::HelpClose)
        ));
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('x'))),
            Some(Action::HelpClose)
        ));
    }

    #[test]
    fn unmapped_key_is_none() {
        let s = list_state();
        assert!(map_key(&s, k(KeyCode::Char('z'))).is_none());
    }
}
