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
use crate::tui::v2::state::AppState;
use crate::tui::v2::view;
use crate::tui::v2::worker::{RequestId, Worker};

pub fn run() -> Result<Envelope> {
    let runtime_resolution = resolve_runtime_profile(None).map_err(anyhow::Error::from)?;
    let target_path = Path::new(&runtime_resolution.resolved_profile_path);
    let config = Config::load_from_path_lenient_without_active_overlay(target_path)
        .unwrap_or_else(|_| default_config_with_profile(&runtime_resolution.selected_profile));
    let context = Context::from_config(&config, Some(&runtime_resolution.selected_profile));

    let mut state = AppState::new(context);
    let worker = Worker::spawn();
    let mut list_request = 0;
    dispatch_effects(
        vec![initial_load_effect(&state)],
        &worker,
        &config,
        &mut list_request,
    );

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

    let result = main_loop(
        &mut terminal,
        &mut state,
        &worker,
        &config,
        &mut list_request,
    );

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
    list_request: &mut RequestId,
) -> Result<()> {
    loop {
        while let Ok(outcome) = worker.try_recv() {
            if outcome.id == *list_request {
                let effects = update(state, outcome.action);
                dispatch_effects(effects, worker, config, list_request);
            }
        }

        terminal.draw(|frame| view::render(frame, state))?;

        if event::poll(Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(action) = map_key(key)
        {
            let effects = update(state, action);
            dispatch_effects(effects, worker, config, list_request);
        }

        if state.should_quit {
            break;
        }
    }

    Ok(())
}

fn dispatch_effects(
    effects: Vec<Effect>,
    worker: &Worker,
    config: &Config,
    list_request: &mut RequestId,
) {
    // Phase 0 emits at most one effect per update. Under that invariant, tracking
    // only the last request id is correct. Revisit list_request tracking once
    // update() can emit fetch effects in later phases.
    debug_assert!(
        effects.len() <= 1,
        "Phase 0 expects at most one effect per update; revisit list_request tracking"
    );
    for effect in effects {
        let request_id = Worker::next_request_id();
        *list_request = request_id;
        worker.submit(effect, config.clone(), request_id);
    }
}

fn map_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => Some(Action::CursorDown),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::CursorUp),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Char('q') => Some(Action::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::action::Action;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn arrows_and_vim_keys_map_to_cursor() {
        assert!(matches!(
            map_key(k(KeyCode::Down)),
            Some(Action::CursorDown)
        ));
        assert!(matches!(
            map_key(k(KeyCode::Char('j'))),
            Some(Action::CursorDown)
        ));
        assert!(matches!(map_key(k(KeyCode::Up)), Some(Action::CursorUp)));
        assert!(matches!(
            map_key(k(KeyCode::Char('k'))),
            Some(Action::CursorUp)
        ));
    }

    #[test]
    fn q_quits_esc_is_back() {
        assert!(matches!(map_key(k(KeyCode::Char('q'))), Some(Action::Quit)));
        assert!(matches!(map_key(k(KeyCode::Esc)), Some(Action::Back)));
    }

    #[test]
    fn unmapped_key_is_none() {
        assert!(map_key(k(KeyCode::Char('z'))).is_none());
    }
}
