use std::io;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use serde_json::Value;

use ayx_core::envelope::Envelope;

use self::app::{App, Focus, OneBrowserResource, ProfilesPane, Screen};

mod app;
mod theme;

pub fn run() -> Result<Envelope> {
    let mut app = App::new()?;

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

    let result = run_loop(&mut terminal, &mut app);

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result?;
    Ok(Envelope::ok("tui session ended"))
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        app.tick();
        terminal.draw(|frame| render(frame, app))?;
        if event::poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key);
                }
            }
        }
        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn render(frame: &mut Frame, app: &App) {
    frame.render_widget(Block::default().style(theme::app()), frame.area());

    let vertical = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(frame.area());
    render_header(frame, app, vertical[0]);

    let body = Layout::horizontal([Constraint::Length(20), Constraint::Min(0)]).split(vertical[1]);
    render_sidebar(frame, app, body[0]);
    render_content(frame, app, body[1]);
    render_footer(frame, app, vertical[2]);

    if let Some(toast) = app.toast.as_ref() {
        render_toast(frame, toast.message.as_str(), toast.is_error);
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let mode = if app.is_workspace_target() {
        "workspace"
    } else {
        "profile"
    };
    let badge_style = match mode {
        "workspace" => theme::badge_warn(),
        _ => theme::badge_ok(),
    };

    let line = Line::from(vec![
        Span::styled(" ayx ", theme::accent_bold()),
        Span::styled("› tui ", theme::accent()),
        Span::raw(" "),
        Span::styled(format!(" {} ", mode), badge_style),
        Span::raw(" "),
        Span::styled(app.current_target_label(), theme::accent_bold()),
        Span::raw(" "),
        Span::styled(
            format!("[source: {}]", app.resolution_source),
            theme::muted(),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Sidebar;
    let items = Screen::all()
        .iter()
        .enumerate()
        .map(|(index, screen)| {
            let prefix = if app.screen.index() == index { "▶ " } else { "  " };
            ListItem::new(Line::from(vec![
                if app.screen.index() == index {
                    Span::styled(prefix, theme::accent())
                } else {
                    Span::styled(prefix, theme::muted())
                },
                if app.screen.index() == index && focused {
                    Span::styled(screen.label(), theme::accent_bold())
                } else if app.screen.index() == index {
                    Span::styled(screen.label(), theme::dim())
                } else {
                    Span::styled(screen.label(), theme::muted())
                },
            ]))
        })
        .collect::<Vec<_>>();

    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(theme::border(focused))
        .style(theme::panel());
    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn render_content(frame: &mut Frame, app: &App, area: Rect) {
    match app.screen {
        Screen::Profiles => render_profiles(frame, app, area),
        Screen::Config => render_config(frame, app, area),
        Screen::Credentials => render_credentials(frame, app, area),
        Screen::Connectivity => render_connectivity(frame, app, area),
        Screen::Inspect => render_inspect(frame, app, area),
        Screen::One => render_one_browser(frame, app, area),
        Screen::Help => render_help(frame, area),
    }
}

fn render_profiles(frame: &mut Frame, app: &App, area: Rect) {
    let panes = Layout::horizontal([
        Constraint::Percentage(30),
        Constraint::Percentage(30),
        Constraint::Percentage(40),
    ])
    .split(area);
    let profile_focused = app.focus == Focus::Content && app.profiles_pane == ProfilesPane::Profiles;
    let workspace_focused =
        app.focus == Focus::Content && app.profiles_pane == ProfilesPane::Workspaces;
    let env_focused = app.focus == Focus::Content && app.profiles_pane == ProfilesPane::Environments;

    let profiles = app
        .profiles
        .iter()
        .map(|name| {
            let active = app.active_profile.as_deref() == Some(name.as_str());
            let style = if active {
                theme::accent()
            } else {
                theme::field_value()
            };
            let marker = if active { "●" } else { "○" };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled(name.clone(), style),
            ]))
        })
        .collect::<Vec<_>>();
    let mut profile_state = app.profiles_state.clone();
    frame.render_stateful_widget(
        List::new(profiles)
            .highlight_symbol("▶ ")
            .highlight_style(theme::selected())
            .block(
                Block::default()
                    .title(" Profiles ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(profile_focused))
                    .style(theme::panel()),
            ),
        panes[0],
        &mut profile_state,
    );

    let workspaces = app
        .workspaces
        .iter()
        .map(|workspace| {
            let label = format!("{} [{}]", workspace.name, workspace.active_environment);
            let active = app.active_workspace.as_deref() == Some(workspace.name.as_str());
            let style = if active {
                theme::accent()
            } else {
                theme::field_value()
            };
            let marker = if active { "●" } else { "○" };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled(label, style),
            ]))
        })
        .collect::<Vec<_>>();
    let mut workspace_state = app.workspaces_state.clone();
    frame.render_stateful_widget(
        List::new(workspaces)
            .highlight_symbol("▶ ")
            .highlight_style(theme::selected())
            .block(
                Block::default()
                    .title(" Workspaces ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(workspace_focused))
                    .style(theme::panel()),
            ),
        panes[1],
        &mut workspace_state,
    );

    let mut detail_lines = vec![
        Line::from(vec![
            Span::styled("Current target: ", theme::field_label()),
            Span::styled(app.current_target_label(), theme::accent_bold()),
        ]),
        Line::from(vec![
            Span::styled("Resolved path: ", theme::field_label()),
            Span::raw(app.target_path.display().to_string()),
        ]),
    ];
    if let Some(env) = app.target_environment.as_deref() {
        detail_lines.push(Line::from(vec![
            Span::styled("Environment: ", theme::field_label()),
            Span::styled(env, theme::field_value()),
        ]));
    }
    detail_lines.push(Line::from(""));
    detail_lines.push(Line::from(Span::styled("Selected workspace", theme::accent())));
    if let Some(workspace) = app.selected_workspace() {
        detail_lines.push(Line::from(vec![
            Span::styled("Name: ", theme::field_label()),
            Span::styled(workspace.name.clone(), theme::accent_bold()),
        ]));
        detail_lines.push(Line::from(vec![
            Span::styled("Active env: ", theme::field_label()),
            Span::styled(workspace.active_environment.clone(), theme::field_value()),
        ]));
        for (index, env) in workspace.environments.iter().enumerate() {
            let marker = if workspace.active_environment == *env { "●" } else { "○" };
            let line_style = if app.environments_state.selected() == Some(index) && env_focused {
                theme::selected()
            } else if app.target_environment.as_deref() == Some(env.as_str()) {
                theme::accent()
            } else {
                theme::field_value()
            };
            detail_lines.push(Line::from(Span::styled(
                format!("{marker} {env}"),
                line_style,
            )));
        }
    } else {
        detail_lines.push(Line::from(Span::styled(
            "No workspace selected",
            theme::muted(),
        )));
    }
    detail_lines.push(Line::from(""));
    detail_lines.push(Line::from(Span::styled("Selection", theme::accent())));
    detail_lines.push(Line::from(vec![
        Span::styled("Profile: ", theme::field_label()),
        Span::styled(
            app.selected_profile_name()
                .cloned()
                .unwrap_or_else(|| "none".to_string()),
            theme::field_value(),
        ),
    ]));
    detail_lines.push(Line::from(vec![
        Span::styled("Workspace: ", theme::field_label()),
        Span::styled(
            app.selected_workspace()
                .map(|workspace| workspace.name.clone())
                .unwrap_or_else(|| "none".to_string()),
            theme::field_value(),
        ),
    ]));
    detail_lines.push(Line::from(vec![
        Span::styled("Environment: ", theme::field_label()),
        Span::styled(
            app.target_environment
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            theme::field_value(),
        ),
    ]));
    detail_lines.push(Line::from(""));
    detail_lines.push(Line::from(Span::styled("Actions", theme::accent())));
    detail_lines.push(Line::from("Enter activates the selected profile, workspace, or environment."));
    detail_lines.push(Line::from("e edits the selected config or credentials field."));
    detail_lines.push(Line::from("s saves the current target using the canonical file format."));
    detail_lines.push(Line::from(""));
    detail_lines.push(Line::from(Span::styled("Summary", theme::accent())));
    for line in serde_json::to_string_pretty(&app.current_summary())
        .unwrap_or_default()
        .lines()
        .take(12)
    {
        detail_lines.push(Line::from(line.to_string()));
    }
    let block = Block::default()
        .title(" Detail ")
        .borders(Borders::ALL)
        .border_style(theme::border(env_focused))
        .style(theme::panel());
    frame.render_widget(Paragraph::new(detail_lines).block(block).wrap(Wrap { trim: false }), panes[2]);
}

fn render_credentials(frame: &mut Frame, app: &App, area: Rect) {
    let layout = Layout::vertical([Constraint::Min(10), Constraint::Length(9)]).split(area);

    let mut lines = Vec::new();
    for (index, field) in app.credentials.fields.iter().enumerate() {
        let value = if app.credentials.editing && app.credentials.cursor == index {
            format!("{}▏", app.credentials.edit_buffer)
        } else {
            app.credentials.visible_value(index)
        };
        let value_span = if value.is_empty() {
            Span::styled(field.placeholder, theme::field_placeholder())
        } else {
            Span::styled(value, theme::field_value())
        };
        let style = if app.credentials.cursor == index && app.focus == Focus::Content {
            theme::selected()
        } else {
            theme::panel()
        };
        lines.push(Line::from(vec![
            if app.credentials.cursor == index && app.focus == Focus::Content {
                Span::styled("▶ ", theme::accent())
            } else {
                Span::styled("  ", theme::muted())
            },
            Span::styled(format!("{:>18} ", field.label), theme::field_label()),
            Span::styled("│ ", theme::muted()),
            Span::styled("", style),
            value_span,
        ]));
        lines.push(Line::from(""));
    }

    let title = if app.credentials.editing {
        " One Credentials (editing) "
    } else {
        " One Credentials "
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(theme::border(app.focus == Focus::Content))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        layout[0],
    );

    let validation = serde_json::to_string_pretty(&app.current_validation()).unwrap_or_default();
    let notes = vec![
        Line::from(vec![
            Span::styled("Mode: ", theme::field_label()),
            Span::styled(
                if app.credentials.editing { "edit" } else { "browse" },
                if app.credentials.editing {
                    theme::warn()
                } else {
                    theme::ok()
                },
            ),
        ]),
        Line::from(vec![
            Span::styled("Selected: ", theme::field_label()),
            Span::styled(
                app.credentials.active_field().label,
                theme::accent_bold(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Action: ", theme::field_label()),
            Span::styled(
                if app.credentials.editing {
                    "Enter saves the buffer, Esc cancels"
                } else {
                    "Press e to edit the selected field"
                },
                theme::muted(),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Save mode: ", theme::field_label()),
            Span::styled("canonical YAML + keyring refs", theme::accent()),
        ]),
        Line::from(vec![
            Span::styled("Dirty: ", theme::field_label()),
            Span::styled(
                if app.credentials.dirty { "yes" } else { "no" },
                if app.credentials.dirty {
                    theme::warn()
                } else {
                    theme::ok()
                },
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled("Validation", theme::accent())),
    ];
    let mut text = notes;
    for line in validation.lines() {
        text.push(Line::from(line.to_string()));
    }
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .title(" Profile Status ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(false))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        layout[1],
    );
}

fn render_config(frame: &mut Frame, app: &App, area: Rect) {
    let layout = Layout::vertical([Constraint::Min(10), Constraint::Length(8)]).split(area);

    let mut lines = Vec::new();
    for (index, field) in app.config_form.fields.iter().enumerate() {
        let value = if app.config_form.editing && app.config_form.cursor == index {
            format!("{}▏", app.config_form.edit_buffer)
        } else {
            app.config_form.display_value(index)
        };
        let kind = match field.kind {
            app::ConfigFieldKind::Text => "text",
            app::ConfigFieldKind::Bool => "bool",
        };
        let value_span = if value.is_empty() {
            Span::styled(field.placeholder, theme::field_placeholder())
        } else {
            Span::styled(value, theme::field_value())
        };
        lines.push(Line::from(vec![
            if app.config_form.cursor == index && app.focus == Focus::Content {
                Span::styled("▶ ", theme::accent())
            } else {
                Span::styled("  ", theme::muted())
            },
            Span::styled(format!("{:>18} ", field.label), theme::field_label()),
            Span::styled(format!("({kind}) "), theme::muted()),
            Span::styled("│ ", theme::muted()),
            value_span,
        ]));
        lines.push(Line::from(""));
    }

    let title = if app.config_form.editing {
        " Config (editing) "
    } else {
        " Config "
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(theme::border(app.focus == Focus::Content))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        layout[0],
    );

    let validation = serde_json::to_string_pretty(&app.current_validation()).unwrap_or_default();
    let notes = vec![
        Line::from(vec![
            Span::styled("Mode: ", theme::field_label()),
            Span::styled(
                if app.config_form.editing { "edit" } else { "browse" },
                if app.config_form.editing {
                    theme::warn()
                } else {
                    theme::ok()
                },
            ),
        ]),
        Line::from(vec![
            Span::styled("Selected: ", theme::field_label()),
            Span::styled(app.config_form.active_field().label, theme::accent_bold()),
        ]),
        Line::from(vec![
            Span::styled("Action: ", theme::field_label()),
            Span::styled(
                if app.config_form.editing {
                    "Enter saves the buffer, Esc cancels"
                } else {
                    "Press e to edit the selected field"
                },
                theme::muted(),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Validation: ", theme::field_label()),
            Span::styled("profile post-save", theme::accent()),
        ]),
    ];
    let mut text = notes;
    for line in validation.lines() {
        text.push(Line::from(line.to_string()));
    }
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .title(" Config Status ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(false))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        layout[1],
    );
}

fn render_connectivity(frame: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);
    let top = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[0]);
    let bottom =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[1]);
    let chunks = [top[0], top[1], bottom[0], bottom[1]];

    for (index, panel) in app.connectivity.panels.iter().enumerate() {
        if index >= chunks.len() {
            break;
        }
        let block = Block::default()
            .title(format!(" {} ", panel.title))
            .borders(Borders::ALL)
            .border_style(theme::border(app.focus == Focus::Content))
            .style(theme::panel());
        let body = render_panel_body(panel);
        frame.render_widget(
            Paragraph::new(body).block(block).wrap(Wrap { trim: false }),
            chunks[index],
        );
    }
}

fn render_inspect(frame: &mut Frame, app: &App, area: Rect) {
    let columns = Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)]).split(area);
    let left = Layout::vertical([Constraint::Length(10), Constraint::Min(0)]).split(columns[0]);
    let right = Layout::vertical([Constraint::Length(12), Constraint::Min(0)]).split(columns[1]);

    let mut overview = vec![
        Line::from(vec![
            Span::styled("Target: ", theme::field_label()),
            Span::styled(app.current_target_label(), theme::accent_bold()),
        ]),
        Line::from(vec![
            Span::styled("Kind: ", theme::field_label()),
            Span::styled(
                if app.is_workspace_target() { "workspace" } else { "profile" },
                theme::field_value(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Source: ", theme::field_label()),
            Span::styled(app.resolution_source.clone(), theme::field_value()),
        ]),
        Line::from(vec![
            Span::styled("Path: ", theme::field_label()),
            Span::raw(app.target_path.display().to_string()),
        ]),
    ];
    if let Some(env) = app.target_environment.as_deref() {
        overview.push(Line::from(vec![
            Span::styled("Environment: ", theme::field_label()),
            Span::styled(env, theme::accent()),
        ]));
    }
    frame.render_widget(
        Paragraph::new(overview)
            .block(
                Block::default()
                    .title(" Inspect ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(true))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        left[0],
    );

    let mut env_lines = vec![
        Line::from(Span::styled("Workspace environments", theme::accent())),
    ];
    if let Some(workspace) = app.selected_workspace() {
        env_lines.push(Line::from(vec![
            Span::styled("Name: ", theme::field_label()),
            Span::styled(workspace.name.clone(), theme::field_value()),
        ]));
        env_lines.push(Line::from(vec![
            Span::styled("Active: ", theme::field_label()),
            Span::styled(workspace.active_environment.clone(), theme::field_value()),
        ]));
        env_lines.push(Line::from(""));
        for env in &workspace.config.environments {
            let marker = if workspace.active_environment == *env.0 { "●" } else { "○" };
            let style = if app.target_environment.as_deref() == Some(env.0.as_str()) {
                theme::accent()
            } else {
                theme::field_value()
            };
            env_lines.push(Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled(env.0.clone(), style),
            ]));
        }
    } else {
        env_lines.push(Line::from(Span::styled(
            "No workspace selected",
            theme::muted(),
        )));
    }
    frame.render_widget(
        Paragraph::new(env_lines)
            .block(
                Block::default()
                    .title(" Environment Set ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(false))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        left[1],
    );

    let validation = serde_json::to_string_pretty(&app.current_validation()).unwrap_or_default();
    let summary = serde_json::to_string_pretty(&app.current_summary()).unwrap_or_default();
    let mut status_lines = vec![
        Line::from(vec![
            Span::styled("Selected profile: ", theme::field_label()),
            Span::styled(
                app.selected_profile_name()
                    .cloned()
                    .unwrap_or_else(|| "none".to_string()),
                theme::field_value(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Selected workspace: ", theme::field_label()),
            Span::styled(
                app.selected_workspace()
                    .map(|workspace| workspace.name.clone())
                    .unwrap_or_else(|| "none".to_string()),
                theme::field_value(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Action: ", theme::field_label()),
            Span::styled("Enter returns to Profiles, r reloads indexes", theme::muted()),
        ]),
        Line::from(""),
        Line::from(Span::styled("Browser status", theme::accent())),
    ];
    if let Some(panel) = app.one_browser.panels.first() {
        status_lines.extend(render_panel_summary(panel));
    }
    status_lines.extend([
        Line::from(Span::styled("Validation", theme::accent())),
    ]);
    for line in validation.lines() {
        status_lines.push(Line::from(line.to_string()));
    }
    status_lines.push(Line::from(""));
    status_lines.push(Line::from(Span::styled("Current summary", theme::accent())));
    for line in summary.lines().take(16) {
        status_lines.push(Line::from(line.to_string()));
    }

    frame.render_widget(
        Paragraph::new(status_lines)
            .block(
                Block::default()
                    .title(" Target Status ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(app.focus == Focus::Content))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        right[0],
    );

    let quick_actions = vec![
        Line::from(vec![
            Span::styled("Enter", theme::accent()),
            Span::raw(" refresh or drill into selected item"),
        ]),
        Line::from(vec![
            Span::styled("r", theme::accent()),
            Span::raw(" reload indexes"),
        ]),
        Line::from(vec![
            Span::styled("i", theme::accent()),
            Span::raw(" inspect current selection"),
        ]),
        Line::from(vec![
            Span::styled("b", theme::accent()),
            Span::raw(" go back within browser history"),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(quick_actions)
            .block(
                Block::default()
                    .title(" Quick Actions ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(false))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        right[1],
    );
}

fn render_panel_body(panel: &crate::tui::app::PanelState) -> Vec<Line<'static>> {
    let mut lines = render_panel_summary(panel);
    lines.push(Line::from(""));
    for line in &panel.lines {
        if panel.is_error {
            lines.push(Line::from(Span::styled(line.clone(), theme::danger())));
        } else {
            lines.push(Line::from(line.clone()));
        }
    }
    lines
}

fn render_panel_summary(panel: &crate::tui::app::PanelState) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if panel.is_error {
        lines.push(Line::from(vec![
            Span::styled("Status: ", theme::field_label()),
            Span::styled("failed", theme::danger()),
        ]));
        if let Some(raw) = panel.raw.as_ref() {
            if let Some(kind) = raw.get("error_kind").and_then(Value::as_str) {
                lines.push(Line::from(vec![
                    Span::styled("Kind: ", theme::field_label()),
                    Span::styled(kind.to_string(), theme::warn()),
                ]));
            }
            if let Some(url) = raw.get("request_url").and_then(Value::as_str) {
                lines.push(Line::from(vec![
                    Span::styled("URL: ", theme::field_label()),
                    Span::styled(url.to_string(), theme::field_value()),
                ]));
            }
            if let Some(error) = raw.get("error").and_then(Value::as_str) {
                lines.push(Line::from(vec![
                    Span::styled("Error: ", theme::field_label()),
                    Span::styled(error.to_string(), theme::danger()),
                ]));
            }
            if let Some(chain) = raw.get("error_chain").and_then(Value::as_array) {
                for entry in chain.iter().take(3).filter_map(Value::as_str) {
                    lines.push(Line::from(vec![
                        Span::styled("• ", theme::muted()),
                        Span::styled(entry.to_string(), theme::muted()),
                    ]));
                }
            }
            if let Some(hints) = raw.get("error_hints").and_then(Value::as_array) {
                for hint in hints.iter().take(3).filter_map(Value::as_str) {
                    lines.push(Line::from(vec![
                        Span::styled("Hint: ", theme::field_label()),
                        Span::styled(hint.to_string(), theme::accent()),
                    ]));
                }
            }
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled("Status: ", theme::field_label()),
            Span::styled("ok", theme::ok()),
        ]));
        if let Some(raw) = panel.raw.as_ref() {
            if let Some(url) = raw.get("url").and_then(Value::as_str) {
                lines.push(Line::from(vec![
                    Span::styled("URL: ", theme::field_label()),
                    Span::styled(url.to_string(), theme::field_value()),
                ]));
            }
            if let Some(code) = raw.get("status_code").and_then(Value::as_u64) {
                lines.push(Line::from(vec![
                    Span::styled("HTTP: ", theme::field_label()),
                    Span::styled(code.to_string(), theme::ok()),
                ]));
            }
        }
    }
    lines
}

fn render_one_browser(frame: &mut Frame, app: &App, area: Rect) {
    let layout = Layout::horizontal([Constraint::Length(28), Constraint::Min(0)]).split(area);
    let focused = app.focus == Focus::Content;
    let resources = OneBrowserResource::all()
        .iter()
        .enumerate()
        .map(|(index, resource)| {
            let active = app.one_browser.cursor == index;
            let style = if active {
                theme::accent()
            } else {
                theme::field_value()
            };
            let marker = if active { "▶" } else { " " };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled(resource.label(), style),
            ]))
        })
        .collect::<Vec<_>>();

    let mut state = ListState::default();
    state.select(Some(app.one_browser.cursor));
    frame.render_stateful_widget(
        List::new(resources)
            .highlight_symbol("▶ ")
            .highlight_style(theme::selected())
            .block(
                Block::default()
                    .title(" One Resources ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(focused))
                    .style(theme::panel()),
        ),
        layout[0],
        &mut state,
    );

    let right = Layout::vertical([Constraint::Percentage(62), Constraint::Percentage(38)]).split(layout[1]);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Resource: ", theme::field_label()),
            Span::styled(
                OneBrowserResource::all()[app.one_browser.cursor].label(),
                theme::accent_bold(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Pane: ", theme::field_label()),
            Span::styled(
                match app.one_browser.pane {
                    app::OneBrowserPane::Resources => "resources",
                    app::OneBrowserPane::Items => "items",
                },
                theme::field_value(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Last refresh: ", theme::field_label()),
            Span::styled(
                app.one_browser
                    .last_run
                    .clone()
                    .unwrap_or_else(|| "never".to_string()),
                theme::field_value(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Action: ", theme::field_label()),
            Span::styled("Enter refresh/drill · Tab switch pane · i return to Inspect", theme::muted()),
        ]),
        Line::from(""),
    ];
    if let Some(panel) = app.one_browser.panels.first() {
        lines.push(Line::from(vec![
            Span::styled("Panel: ", theme::field_label()),
            Span::styled(panel.title.clone(), theme::accent()),
        ]));
        lines.push(Line::from(""));
        for line in panel.lines.iter().take(18) {
            lines.push(Line::from(line.clone()));
        }
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" One Browser Detail ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(focused))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        right[0],
    );

    let items = app.active_one_browser_items();
    let item_lines = if let Some(prompt) = app.one_browser.prompt.as_ref() {
        vec![
            Line::from(vec![
                Span::styled("ID prompt for ", theme::field_label()),
                Span::styled(prompt.resource.label(), theme::accent_bold()),
            ]),
            Line::from(""),
            Line::from(Span::styled(prompt.buffer.clone(), theme::field_value())),
        ]
    } else if items.is_empty() {
        vec![Line::from(Span::styled(
            "No drill-down items detected in this response",
            theme::muted(),
        ))]
    } else {
        items
            .iter()
            .enumerate()
            .flat_map(|(index, item)| {
                let active = app.one_browser.item_cursor == index && app.one_browser.pane == app::OneBrowserPane::Items;
                let marker = if active { "▶ " } else { "  " };
                let mut row = vec![Line::from(vec![
                    if active {
                        Span::styled(marker, theme::accent())
                    } else {
                        Span::styled(marker, theme::muted())
                    },
                    Span::styled(&item.label, if active { theme::accent_bold() } else { theme::field_value() }),
                    Span::raw(" "),
                    Span::styled(
                        item.id.as_deref().unwrap_or(""),
                        theme::muted(),
                    ),
                ])];
                if !item.summary.is_empty() {
                    row.push(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(&item.summary, theme::muted()),
                    ]));
                }
                row.push(Line::from(""));
                row
            })
            .collect::<Vec<_>>()
    };

    frame.render_widget(
        Paragraph::new(item_lines)
            .block(
                Block::default()
                    .title(" Drill-Down / Prompt ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(focused))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        right[1],
    );
}

fn render_help(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(vec![
            Span::styled("Navigation", theme::accent_bold()),
            Span::raw("  "),
            Span::styled("Tab", theme::accent()),
            Span::raw(" switch focus / cycle profile panes"),
        ]),
        Line::from(vec![
            Span::styled("Screens", theme::accent_bold()),
            Span::raw("     "),
            Span::styled("1-7", theme::accent()),
            Span::raw(" jump between sidebar views"),
        ]),
        Line::from(vec![
            Span::styled("Profiles", theme::accent_bold()),
            Span::raw("    "),
            Span::styled("Enter", theme::accent()),
            Span::raw(" activate selected profile, workspace, or environment"),
        ]),
        Line::from(vec![
            Span::styled("Inspect", theme::accent_bold()),
            Span::raw("     "),
            Span::styled("i", theme::accent()),
            Span::raw(" open inspector for the current target"),
        ]),
        Line::from(vec![
            Span::styled("One", theme::accent_bold()),
            Span::raw("         "),
            Span::styled("6", theme::accent()),
            Span::raw(" browse live One resources and responses"),
        ]),
        Line::from(vec![
            Span::styled("Config", theme::accent_bold()),
            Span::raw("       "),
            Span::styled("e", theme::accent()),
            Span::raw(" edit config fields, "),
            Span::styled("s", theme::accent()),
            Span::raw(" save"),
        ]),
        Line::from(vec![
            Span::styled("Edit", theme::accent_bold()),
            Span::raw("        "),
            Span::styled("e", theme::accent()),
            Span::raw(" edit field, "),
            Span::styled("s", theme::accent()),
            Span::raw(" save, "),
            Span::styled("c", theme::accent()),
            Span::raw(" clear selected field"),
        ]),
        Line::from(vec![
            Span::styled("Connectivity", theme::accent_bold()),
            Span::raw(" "),
            Span::styled("r", theme::accent()),
            Span::raw(" or "),
            Span::styled("t", theme::accent()),
            Span::raw(" rerun checks"),
        ]),
        Line::from(vec![
            Span::styled("Quit", theme::accent_bold()),
            Span::raw("        "),
            Span::styled("q", theme::accent()),
            Span::raw(" exit the TUI"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Workspace-backed targets persist the selected environment back into the workspace file using the same canonical config shape.",
            theme::muted(),
        )),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Help ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(true))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let help = match app.screen {
        Screen::Profiles => "Tab cycle panes · Enter activate · i inspect · r reload",
        Screen::Config => {
            if app.config_form.editing {
                "Enter save buffer · Esc cancel · Backspace delete"
            } else {
                "e edit · s save · c clear · r reload"
            }
        }
        Screen::Credentials => {
            if app.credentials.editing {
                "Enter save buffer · Esc cancel · Backspace delete"
            } else {
                "e edit · s save · c clear · r reload"
            }
        }
        Screen::Connectivity => "r/t rerun checks",
        Screen::Inspect => "Enter back to Profiles · r reload indexes",
        Screen::One => {
            if app.one_browser.prompt.is_some() {
                "Type id · Enter run · Esc cancel"
            } else {
                match app.one_browser.pane {
                    app::OneBrowserPane::Resources => "Arrows browse resources · Enter refresh/prompt/drill · Tab items · b back",
                    app::OneBrowserPane::Items => "Arrows browse items · Enter drill down · Tab resources · b back",
                }
            }
        }
        Screen::Help => "q quit",
    };
    let line = Line::from(vec![
        Span::styled("status ", theme::field_label()),
        Span::styled(&app.status_message, theme::status_line(false)),
        Span::raw("  "),
        Span::styled("· ", theme::muted()),
        Span::styled(help, theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_toast(frame: &mut Frame, message: &str, is_error: bool) {
    let area = centered_rect(60, 3, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(message)
            .block(
                Block::default()
                    .title(if is_error { " Error " } else { " Notice " })
                    .borders(Borders::ALL)
                    .border_style(if is_error {
                        theme::danger()
                    } else {
                        theme::accent()
                    })
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .split(area);
    let horizontal = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Percentage(width),
        Constraint::Fill(1),
    ])
    .split(vertical[1]);
    horizontal[1]
}
