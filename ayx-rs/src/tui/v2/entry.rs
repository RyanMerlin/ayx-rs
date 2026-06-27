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

fn map_key(state: &AppState, key: KeyEvent) -> Option<Action> {
    use crate::tui::v2::resource::Kind;

    let on_detail = matches!(state.nav.top(), View::ResourceDetail { .. });

    if state.list.filtering && !on_detail {
        return match key.code {
            KeyCode::Char(c) => Some(Action::FilterInput(c)),
            KeyCode::Backspace => Some(Action::FilterBackspace),
            KeyCode::Enter => Some(Action::FilterApply),
            KeyCode::Esc => Some(Action::FilterClear),
            _ => None,
        };
    }

    match key.code {
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
            let next = (state.list.kind.index() + 1) % n;
            Kind::from_index(next).map(Action::SwitchKind)
        }
        KeyCode::BackTab => {
            let n = Kind::all().len();
            let prev = (state.list.kind.index() + n - 1) % n;
            Kind::from_index(prev).map(Action::SwitchKind)
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
        let mut s = list_state();
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('/'))),
            Some(Action::FilterStart)
        ));
        s.list.filtering = true;
        assert!(matches!(
            map_key(&s, k(KeyCode::Char('x'))),
            Some(Action::FilterInput('x'))
        ));
        assert!(matches!(
            map_key(&s, k(KeyCode::Enter)),
            Some(Action::FilterApply)
        ));
    }

    #[test]
    fn unmapped_key_is_none() {
        let s = list_state();
        assert!(map_key(&s, k(KeyCode::Char('z'))).is_none());
    }
}
