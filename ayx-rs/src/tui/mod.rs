// See tui/app.rs for the rationale. Stage 3 of the audit roadmap will split
// this module and drop these allows.
#![allow(clippy::clone_on_copy, clippy::bind_instead_of_map)]

use std::io;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use serde_json::Value;

use ayx_core::envelope::Envelope;
use ayx_core::profile::derive_alteryx_one_token_endpoint;

use self::app::{App, ConfigSection, CrudPrompt, Focus, OneBrowserResource, ProfilesPane, Screen};
use self::store::ProfileScope;

mod app;
mod forms;
mod one_browser;
mod render_helpers;
mod store;
mod theme;
mod worker;

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

    // Drop the app before restoring the terminal so background workers can
    // release their senders and exit cleanly.
    drop(app);

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
        if event::poll(std::time::Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.handle_key(key);
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
    if app.screen == Screen::Inspect {
        let base = app
            .inspect_return
            .map(|(s, _)| s)
            .unwrap_or(Screen::Profiles);
        render_screen_content(frame, app, base, body[1]);
        let popup = centered_rect(88, 78, body[1]);
        frame.render_widget(Clear, popup);
        render_inspect_popup(frame, app, popup);
    } else {
        render_screen_content(frame, app, app.screen, body[1]);
    }
    render_footer(frame, app, vertical[2]);

    if let Some(toast) = app.toast.as_ref() {
        render_toast(frame, toast.message.as_str(), toast.is_error);
    }
    if let Some(prompt) = app.crud_prompt.as_ref() {
        render_crud_prompt(frame, prompt);
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
            let prefix = if app.screen.index() == index {
                "▶ "
            } else {
                "  "
            };
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

fn render_screen_content(frame: &mut Frame, app: &App, screen: Screen, area: Rect) {
    match screen {
        Screen::Profiles => render_profiles(frame, app, area),
        Screen::Config => render_config(frame, app, area),
        Screen::Credentials => render_credentials(frame, app, area),
        Screen::Connectivity => render_connectivity(frame, app, area),
        Screen::Inspect => render_inspect(frame, app, area),
        Screen::One => render_one_browser(frame, app, area),
        Screen::Help => render_help(frame, area),
    }
}

fn render_inspect_popup(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(
        Block::default()
            .title(" Inspect ")
            .borders(Borders::ALL)
            .border_style(theme::accent_bold())
            .style(theme::panel()),
        area,
    );
    let inner = area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    render_inspect(frame, app, inner);
}

fn render_profiles(frame: &mut Frame, app: &App, area: Rect) {
    let panes = Layout::horizontal([
        Constraint::Percentage(30),
        Constraint::Percentage(30),
        Constraint::Percentage(40),
    ])
    .split(area);
    let profile_focused =
        app.focus == Focus::Content && app.profiles_pane == ProfilesPane::Profiles;
    let workspace_focused =
        app.focus == Focus::Content && app.profiles_pane == ProfilesPane::Workspaces;
    let env_focused =
        app.focus == Focus::Content && app.profiles_pane == ProfilesPane::Environments;

    let profiles = app
        .visible_profiles()
        .iter()
        .map(|record| {
            let active = app.active_profile.as_deref() == Some(record.name.as_str());
            let style = if active {
                theme::accent()
            } else {
                theme::field_value()
            };
            let marker = if active { "●" } else { "○" };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled(record.name.clone(), style),
                Span::styled(
                    format!(
                        " [{}]",
                        match record.scope {
                            ProfileScope::One => "one",
                            ProfileScope::Server => "server",
                            ProfileScope::Combined => "combined",
                        }
                    ),
                    theme::muted(),
                ),
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
                    .title(format!(" {} ", app.profile_view.label()))
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
                    .title(" Workspaces · Server envs ")
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
    detail_lines.push(Line::from(Span::styled("Ownership", theme::accent())));
    detail_lines.push(Line::from(vec![
        Span::styled("Profile view: ", theme::field_label()),
        Span::styled(app.profile_view.label(), theme::status_line(false)),
    ]));
    detail_lines.push(Line::from(vec![
        Span::styled("Target kind: ", theme::field_label()),
        Span::styled(
            match app.selected_profile_scope() {
                Some(crate::tui::store::ProfileScope::One) => "One",
                Some(crate::tui::store::ProfileScope::Server) => "Server",
                Some(crate::tui::store::ProfileScope::Combined) => "Combined",
                None => "Unknown",
            },
            theme::status_line(false),
        ),
    ]));
    detail_lines.push(Line::from(""));
    detail_lines.push(Line::from(Span::styled(
        "Selected workspace",
        theme::accent(),
    )));
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
            let marker = if workspace.active_environment == *env {
                "●"
            } else {
                "○"
            };
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
    detail_lines.push(Line::from(
        "Enter activates the selected profile, workspace, or environment.",
    ));
    detail_lines.push(Line::from(
        "n creates a profile, d duplicates, R renames, x deletes.",
    ));
    detail_lines.push(Line::from(
        "e edits the selected config or credentials field.",
    ));
    detail_lines.push(Line::from(
        "s saves the current target using the canonical file format.",
    ));
    detail_lines.push(Line::from(""));
    detail_lines.push(Line::from(Span::styled("Summary", theme::accent())));
    for line in yaml_lines(&app.current_summary()).into_iter().take(12) {
        detail_lines.push(line);
    }
    let block = Block::default()
        .title(" Detail ")
        .borders(Borders::ALL)
        .border_style(theme::border(env_focused))
        .style(theme::panel());
    frame.render_widget(
        Paragraph::new(detail_lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        panes[2],
    );
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
        format!(" Alteryx One ({}) ", app.credentials_storage_target_label())
    } else {
        format!(" Alteryx One [{}] ", app.credentials_storage_target_label())
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

    let validation = yaml_lines(&app.current_validation());
    let notes = vec![
        Line::from(vec![
            Span::styled("Mode: ", theme::field_label()),
            Span::styled(
                if app.credentials.editing {
                    "edit"
                } else {
                    "browse"
                },
                if app.credentials.editing {
                    theme::warn()
                } else {
                    theme::ok()
                },
            ),
        ]),
        Line::from(vec![
            Span::styled("Selected: ", theme::field_label()),
            Span::styled(app.credentials.active_field().label, theme::accent_bold()),
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
        Line::from(vec![
            Span::styled("Stored in: ", theme::field_label()),
            Span::styled(app.credentials_storage_target_label(), theme::accent()),
        ]),
        Line::from(vec![
            Span::styled("Derived token endpoint: ", theme::field_label()),
            Span::styled(
                app.current_config
                    .alteryx_one
                    .as_ref()
                    .and_then(|one| one.effective_token_endpoint_url())
                    .unwrap_or_else(|| {
                        let base_url = app.credentials.fields[1].value.trim();
                        if base_url.is_empty() {
                            "enter a base URL to derive /as/token".to_string()
                        } else {
                            derive_alteryx_one_token_endpoint(base_url)
                        }
                    }),
                theme::accent(),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Save mode: ", theme::field_label()),
            Span::styled("canonical YAML + derived token endpoint", theme::accent()),
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
    for line in validation {
        text.push(line);
    }
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .title(" Alteryx One Status ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(false))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        layout[1],
    );
}

fn render_config(frame: &mut Frame, app: &App, area: Rect) {
    let columns =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)]).split(area);
    let left = Layout::vertical([Constraint::Length(4), Constraint::Min(0)]).split(columns[0]);
    let body = Layout::vertical([Constraint::Min(0), Constraint::Length(12)]).split(left[1]);
    let right_rows = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(columns[1]);
    let right_top = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(right_rows[0]);
    let right_bottom = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(right_rows[1]);

    let mut tab_lines = vec![Line::from(vec![
        Span::styled("Sections: ", theme::field_label()),
        Span::styled(
            ConfigSection::all()
                .iter()
                .map(|section| {
                    if *section == app.config_form.active_section() {
                        format!("[{}]", section.label())
                    } else {
                        section.label().to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("  "),
            theme::accent_bold(),
        ),
    ])];
    tab_lines.push(Line::from(vec![
        Span::styled("Tab/Shift-Tab ", theme::accent()),
        Span::styled("cycle sections", theme::muted()),
    ]));
    frame.render_widget(
        Paragraph::new(tab_lines)
            .block(
                Block::default()
                    .title(" Alteryx Server Editor ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(app.focus == Focus::Content))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        left[0],
    );

    let fields = app.config_form.active_fields();
    let available_lines = body[0].height.saturating_sub(3) as usize;
    let visible_fields = (available_lines / 2).max(1);
    let (start, end) = visible_field_window(fields.len(), app.config_form.cursor, visible_fields);

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Section: ", theme::field_label()),
        Span::styled(
            app.config_form.active_section().label(),
            theme::accent_bold(),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Action: ", theme::field_label()),
        Span::styled("e edit · c clear · s save · r reload", theme::muted()),
    ]));
    lines.push(Line::from(""));
    if start > 0 {
        lines.push(Line::from(vec![
            Span::styled("...", theme::muted()),
            Span::styled(" more above ", theme::muted()),
        ]));
    }
    for (index, field) in fields.iter().enumerate().skip(start).take(end - start) {
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
    if end < fields.len() {
        lines.push(Line::from(vec![
            Span::styled("...", theme::muted()),
            Span::styled(" more below ", theme::muted()),
        ]));
    }

    let title = if app.config_form.editing {
        format!(
            " Alteryx Server [{}] ({}) ",
            app.config_form.active_section().label(),
            app.config_storage_target_label()
        )
    } else {
        format!(
            " Alteryx Server [{}] [{}] ",
            app.config_form.active_section().label(),
            app.config_storage_target_label()
        )
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
        body[0],
    );

    let validation = yaml_lines(&app.current_validation());
    let notes = vec![
        Line::from(vec![
            Span::styled("Mode: ", theme::field_label()),
            Span::styled(
                if app.config_form.editing {
                    "edit"
                } else {
                    "browse"
                },
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
        Line::from(vec![
            Span::styled("Stored in: ", theme::field_label()),
            Span::styled(app.config_storage_target_label(), theme::accent()),
        ]),
        Line::from(vec![
            Span::styled("Scope: ", theme::field_label()),
            Span::styled(
                "overview and the four server subsections are editable",
                theme::muted(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Sections: ", theme::field_label()),
            Span::styled(
                "Overview, Server API, Mongo, SQL Server, Observability",
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
    for line in validation {
        text.push(line);
    }
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .title(" Alteryx Server Status ")
                    .borders(Borders::ALL)
                    .border_style(theme::border(false))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        body[1],
    );

    render_config_section(
        frame,
        "Server API",
        right_top[0],
        render_server_api_summary(app),
    );
    render_config_section(frame, "Mongo", right_top[1], render_mongo_summary(app));
    render_config_section(
        frame,
        "SQL Server",
        right_bottom[0],
        render_sqlserver_summary(app),
    );
    render_config_section(
        frame,
        "Observability",
        right_bottom[1],
        render_observability_summary(app),
    );
}

fn render_connectivity(frame: &mut Frame, app: &App, area: Rect) {
    let rows =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);
    let top =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[0]);
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
    let columns =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)]).split(area);
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
                if app.is_workspace_target() {
                    "workspace"
                } else {
                    "profile"
                },
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

    let mut env_lines = vec![Line::from(Span::styled(
        "Workspace environments",
        theme::accent(),
    ))];
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
            let marker = if workspace.active_environment == *env.0 {
                "●"
            } else {
                "○"
            };
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

    let validation = yaml_lines(&app.current_validation());
    let summary = yaml_lines(&app.current_summary());
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
            Span::styled(
                "Enter returns to Profiles, r reloads indexes",
                theme::muted(),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled("Browser status", theme::accent())),
    ];
    if let Some(panel) = app.one_browser.panels.first() {
        status_lines.extend(render_panel_summary(panel));
    }
    status_lines.extend([Line::from(Span::styled("Validation", theme::accent()))]);
    for line in validation {
        status_lines.push(line);
    }
    status_lines.push(Line::from(""));
    status_lines.push(Line::from(Span::styled("Current summary", theme::accent())));
    for line in summary.into_iter().take(16) {
        status_lines.push(line);
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
            lines.push(yaml_text_line(line));
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

fn render_config_section(frame: &mut Frame, title: &str, area: Rect, lines: Vec<Line<'static>>) {
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(format!(" {} ", title))
                    .borders(Borders::ALL)
                    .border_style(theme::border(false))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn visible_field_window(len: usize, cursor: usize, capacity: usize) -> (usize, usize) {
    if len <= capacity {
        return (0, len);
    }
    let capacity = capacity.max(1);
    let half = capacity / 2;
    let mut start = cursor.saturating_sub(half);
    if start + capacity > len {
        start = len - capacity;
    }
    (start, start + capacity)
}

fn render_server_api_summary(app: &App) -> Vec<Line<'static>> {
    let config = &app.current_config;
    let base_url = config
        .server_api
        .as_ref()
        .map(|api| api.base_url.clone())
        .or_else(|| config.api.as_ref().map(|api| api.base_url.clone()))
        .or_else(|| {
            config
                .server
                .as_ref()
                .map(|server| server.webapi_url.clone())
        });
    let client_id = config
        .server_api
        .as_ref()
        .map(|api| api.client_id.clone())
        .or_else(|| {
            config
                .api
                .as_ref()
                .and_then(|api| api.auth.client_id.clone())
        })
        .or_else(|| {
            config
                .server
                .as_ref()
                .map(|server| server.curator_api_key.clone())
        });
    let client_secret_present = config
        .server_api
        .as_ref()
        .map(|api| !api.client_secret.trim().is_empty())
        .or_else(|| {
            config.api.as_ref().map(|api| {
                !api.auth
                    .client_secret
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            })
        })
        .or_else(|| {
            config
                .server
                .as_ref()
                .map(|server| !server.curator_api_secret.trim().is_empty())
        })
        .unwrap_or(false);

    vec![
        section_kv_line(
            "Base URL",
            base_url.unwrap_or_else(|| "missing".to_string()),
            if config.server_api.is_some() || config.api.is_some() || config.server.is_some() {
                theme::field_value()
            } else {
                theme::warn()
            },
        ),
        section_kv_line(
            "Client ID",
            client_id.unwrap_or_else(|| "missing".to_string()),
            if config.server_api.is_some() || config.api.is_some() || config.server.is_some() {
                theme::field_value()
            } else {
                theme::warn()
            },
        ),
        section_kv_line(
            "Client Secret",
            if client_secret_present {
                "stored".to_string()
            } else {
                "missing".to_string()
            },
            if client_secret_present {
                theme::ok()
            } else {
                theme::warn()
            },
        ),
        section_kv_line(
            "Auth Mode",
            config
                .api
                .as_ref()
                .map(|api| format!("{:?}", api.auth.mode))
                .unwrap_or_else(|| "server_api".to_string()),
            theme::accent(),
        ),
    ]
}

fn render_mongo_summary(app: &App) -> Vec<Line<'static>> {
    let config = &app.current_config;
    let mongo = &config.mongo;
    let mut lines = vec![
        section_kv_line("Mode", format!("{:?}", mongo.mode), theme::accent()),
        section_kv_line(
            "Gallery DB",
            mongo.databases.gallery_name.clone(),
            theme::field_value(),
        ),
        section_kv_line(
            "Service DB",
            mongo.databases.service_name.clone(),
            theme::field_value(),
        ),
    ];
    match mongo.mode {
        ayx_core::profile::MongoMode::Embedded => {
            let embedded = mongo.embedded.as_ref();
            lines.push(section_kv_line(
                "Runtime",
                embedded
                    .and_then(|v| v.runtime_settings_path.clone())
                    .unwrap_or_else(|| "missing".to_string()),
                if embedded
                    .and_then(|v| v.runtime_settings_path.as_ref())
                    .is_some()
                {
                    theme::field_value()
                } else {
                    theme::warn()
                },
            ));
        }
        ayx_core::profile::MongoMode::Managed => {
            let managed = mongo.managed.as_ref();
            let host_or_url = managed
                .and_then(|v| v.url.clone().or_else(|| v.host.clone()))
                .unwrap_or_else(|| "missing".to_string());
            let port = managed
                .and_then(|v| Some(v.port.to_string()))
                .unwrap_or_else(|| "missing".to_string());
            lines.push(section_kv_line(
                "Host/URL",
                host_or_url,
                if managed.is_some() {
                    theme::field_value()
                } else {
                    theme::warn()
                },
            ));
            lines.push(section_kv_line(
                "Port",
                port,
                if managed.is_some() {
                    theme::field_value()
                } else {
                    theme::warn()
                },
            ));
        }
    }
    lines
}

fn render_sqlserver_summary(app: &App) -> Vec<Line<'static>> {
    let config = &app.current_config;
    if let Some(sqlserver) = config.sqlserver.as_ref() {
        vec![
            section_kv_line(
                "Controller",
                connection_summary(sqlserver.controller.as_ref()),
                theme::field_value(),
            ),
            section_kv_line(
                "Server UI",
                connection_summary(sqlserver.server_ui.as_ref()),
                theme::field_value(),
            ),
        ]
    } else {
        vec![section_kv_line(
            "Status",
            "not configured".to_string(),
            theme::warn(),
        )]
    }
}

fn render_observability_summary(app: &App) -> Vec<Line<'static>> {
    let config = &app.current_config;
    if let Some(observability) = config.observability.as_ref() {
        let api_logging = observability.api_logging.as_ref();
        vec![
            section_kv_line(
                "API Logging",
                api_logging
                    .map(|value| value.enabled.to_string())
                    .unwrap_or_else(|| "missing".to_string()),
                api_logging
                    .map(|value| {
                        if value.enabled {
                            theme::ok()
                        } else {
                            theme::warn()
                        }
                    })
                    .unwrap_or_else(theme::warn),
            ),
            section_kv_line(
                "Path",
                api_logging
                    .and_then(|value| value.path.clone())
                    .unwrap_or_else(|| "missing".to_string()),
                theme::field_value(),
            ),
            section_kv_line(
                "Redact",
                api_logging
                    .and_then(|value| value.redact_bodies)
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "missing".to_string()),
                theme::field_value(),
            ),
        ]
    } else {
        vec![section_kv_line(
            "Status",
            "not configured".to_string(),
            theme::warn(),
        )]
    }
}

fn connection_summary(conn: Option<&ayx_core::profile::SqlServerConnectionProfile>) -> String {
    let Some(conn) = conn else {
        return "missing".to_string();
    };
    let host = conn.host.as_deref().unwrap_or("host");
    let port = conn
        .port
        .map(|value| value.to_string())
        .unwrap_or_else(|| "port".to_string());
    let database = conn.database.as_deref().unwrap_or("database");
    format!("{host}:{port} / {database}")
}

fn section_kv_line(
    label: &str,
    value: String,
    value_style: ratatui::style::Style,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:>12} ", label), theme::field_label()),
        Span::styled(value, value_style),
    ])
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

    let right =
        Layout::vertical([Constraint::Percentage(62), Constraint::Percentage(38)]).split(layout[1]);

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
            Span::styled(
                "Enter refresh/drill · Tab switch pane · b back · Esc back",
                theme::muted(),
            ),
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
                let active = app.one_browser.item_cursor == index
                    && app.one_browser.pane == app::OneBrowserPane::Items;
                let marker = if active { "▶ " } else { "  " };
                let mut row = vec![Line::from(vec![
                    if active {
                        Span::styled(marker, theme::accent())
                    } else {
                        Span::styled(marker, theme::muted())
                    },
                    Span::styled(
                        &item.label,
                        if active {
                            theme::accent_bold()
                        } else {
                            theme::field_value()
                        },
                    ),
                    Span::raw(" "),
                    Span::styled(item.id.as_deref().unwrap_or(""), theme::muted()),
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
            Span::raw(", "),
            Span::styled("n/d/R/x", theme::accent()),
            Span::raw(" create, duplicate, rename, delete profile"),
        ]),
        Line::from(vec![
            Span::styled("Profile view", theme::accent_bold()),
            Span::raw(" "),
            Span::styled("o/s/a", theme::accent()),
            Span::raw(" filter One, Server, or All profiles"),
        ]),
        Line::from(vec![
            Span::styled("Inspect", theme::accent_bold()),
            Span::raw("     "),
            Span::styled("i", theme::accent()),
            Span::raw(" open/close inspector popup for the current target"),
        ]),
        Line::from(vec![
            Span::styled("Alteryx One", theme::accent_bold()),
            Span::raw("         "),
            Span::styled("2", theme::accent()),
            Span::raw(" browse live One resources and responses"),
        ]),
        Line::from(vec![
            Span::styled("Alteryx Server", theme::accent_bold()),
            Span::raw("       "),
            Span::styled("3", theme::accent()),
            Span::raw(" inspect or edit an explicit profile/workspace file, "),
            Span::styled("s", theme::accent()),
            Span::raw(" save"),
        ]),
        Line::from(vec![
            Span::styled("Edit", theme::accent_bold()),
            Span::raw("        "),
            Span::styled("e", theme::accent()),
            Span::raw(" edit selected field, "),
            Span::styled("s", theme::accent()),
            Span::raw(" save to the selected target, "),
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
            "Profile selection is now view-scoped. One and Server profiles can be managed independently, while the current runtime target still composes from the selected file(s).",
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
    let help = if let Some(prompt) = app.crud_prompt.as_ref() {
        match prompt {
            CrudPrompt::Text { .. } => "Type name · Enter confirm · Esc cancel · Backspace delete",
            CrudPrompt::Confirm { .. } => "Enter/Y confirm · N/Esc cancel",
        }
    } else {
        match app.screen {
            Screen::Profiles => {
                "Arrows move · Enter activate · n new · d duplicate · R rename · x delete · Tab cycle panes · i inspect"
            }
            Screen::Config => {
                if app.config_form.editing {
                    "Enter save buffer · Esc cancel · Backspace delete"
                } else {
                    "Arrows move · Tab/Shift-Tab cycle sections · e edit · s save · c clear · r reload"
                }
            }
            Screen::Credentials => {
                if app.credentials.editing {
                    "Enter save buffer · Esc cancel · Backspace delete"
                } else {
                    "Arrows move · e edit · s save to active profile · c clear · r reload · Esc back"
                }
            }
            Screen::Connectivity => "r/t rerun checks · Esc back",
            Screen::Inspect => "Esc close popup · Enter close · r reload indexes",
            Screen::One => {
                if app.one_browser.prompt.is_some() {
                    "Type id · Enter run · Esc cancel"
                } else {
                    match app.one_browser.pane {
                        app::OneBrowserPane::Resources => {
                            "Arrows browse resources · Enter refresh/prompt/drill · Tab items · b back · Esc back"
                        }
                        app::OneBrowserPane::Items => {
                            "Arrows browse items · Enter drill down · Tab resources · b back · Esc back"
                        }
                    }
                }
            }
            Screen::Help => "q quit · Esc back",
        }
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

fn render_crud_prompt(frame: &mut Frame, prompt: &CrudPrompt) {
    let area = centered_rect(72, 14, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .title(format!(" {} ", prompt.title()))
            .borders(Borders::ALL)
            .border_style(theme::accent_bold())
            .style(theme::panel()),
        area,
    );
    let inner = area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    let mut lines = vec![
        Line::from(Span::styled(prompt.message(), theme::accent())),
        Line::from(""),
    ];
    if let Some(buffer) = prompt.buffer() {
        lines.push(Line::from(Span::styled(buffer, theme::field_value())));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Enter confirm · Esc cancel · Backspace delete",
            theme::muted(),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Enter/Y confirm · N/Esc cancel",
            theme::muted(),
        )));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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

fn yaml_lines(value: &serde_json::Value) -> Vec<Line<'static>> {
    serde_yaml::to_string(value)
        .unwrap_or_else(|_| value.to_string())
        .lines()
        .map(yaml_text_line)
        .collect()
}

fn yaml_text_line(line: &str) -> Line<'static> {
    let trimmed = line.trim_start();
    let indent = line.len().saturating_sub(trimmed.len());
    if trimmed.is_empty() {
        return Line::from(String::new());
    }

    let mut spans = Vec::new();
    if indent > 0 {
        spans.push(Span::raw(" ".repeat(indent)));
    }

    if let Some(rest) = trimmed.strip_prefix("- ") {
        spans.push(Span::styled("- ", theme::accent()));
        spans.extend(yaml_value_spans(rest));
        return Line::from(spans);
    }

    if let Some((key, value)) = trimmed.split_once(": ") {
        spans.push(Span::styled(format!("{key}:"), theme::accent_bold()));
        spans.push(Span::raw(" "));
        spans.extend(yaml_value_spans(value));
        return Line::from(spans);
    }

    if trimmed.ends_with(':') {
        spans.push(Span::styled(trimmed.to_string(), theme::accent_bold()));
        return Line::from(spans);
    }

    spans.push(Span::styled(trimmed.to_string(), theme::field_value()));
    Line::from(spans)
}

fn yaml_value_spans(value: &str) -> Vec<Span<'static>> {
    let style = match value.trim() {
        "true" => theme::ok(),
        "false" => theme::warn(),
        "null" | "~" => theme::muted(),
        other if other.parse::<i64>().is_ok() || other.parse::<f64>().is_ok() => theme::accent(),
        other if other.starts_with('"') || other.starts_with('\'') => theme::field_value(),
        _ => theme::field_value(),
    };
    vec![Span::styled(value.to_string(), style)]
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
