use super::layout::centered;
use super::widgets::{input_view, panel, truncate_tail};
use crate::app::{App, InputMode};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap},
};

/// Modal and input frames with rounded corners.
fn dialog_frame(color: ratatui::style::Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
}

fn input_frame(color: ratatui::style::Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
}

pub(crate) fn draw_input(frame: &mut Frame, app: &App) {
    let Some(mode) = app.ui.input_mode() else {
        return;
    };
    match mode {
        InputMode::ImportProfile => draw_import_input(frame, app),
        InputMode::CorePath => draw_core_path_input(frame, app),
        InputMode::RestoreBackup(path) => {
            let area = centered(76, 7, frame.area());
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(format!(
                    "Overwrite current configuration with {}?",
                    path.display()
                ))
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(app.theme.foreground))
                .block(
                    dialog_frame(app.theme.warning)
                        .padding(Padding::new(2, 2, 1, 1))
                        .title(Span::styled(
                            " Confirm restore · y Yes · n/Esc Cancel ",
                            Style::default()
                                .fg(app.theme.warning)
                                .add_modifier(Modifier::BOLD),
                        )),
                ),
                area,
            );
        }
        _ => {
            if let Some((title, hint)) = mode.editor() {
                draw_text_input(frame, app, title, hint);
            }
        }
    }
}

/// Import dialog (`a` on Profiles). Mirrors the update-settings editor
/// row for row, so a subscription lands fully configured instead of
/// being imported bare and configured in a second pass.
pub(crate) fn draw_import_input(frame: &mut Frame, app: &App) {
    use crate::app::actions::import::{SOURCE_ROW, SUBMIT_ROW};
    const LABEL_WIDTH: usize = 17; // "{label:<17}" below
    const INDENT: u16 = 2; // "▸ " marker
    const WIDTH: u16 = 76;
    const FIELD_ROWS: usize = 8;
    let draft = &app.ui.import_draft;
    let editing_source = matches!(
        app.ui.input_mode(),
        Some(crate::app::InputMode::ImportProfile)
    ) && app.ui.import_editor_index == SOURCE_ROW;
    // While the Source row is being typed into the draft is not
    // committed yet, but the update rows must react immediately: an
    // `http(s)://` prefix is what makes them meaningful.
    let live_source;
    let source = if editing_source {
        live_source = app.ui.input_value().trim();
        live_source
    } else {
        draft.source.trim()
    };
    let remote = source.starts_with("http://") || source.starts_with("https://");
    let on_off = |enabled: bool| {
        if enabled {
            Span::styled("on", Style::default().fg(app.theme.success))
        } else {
            Span::styled("off", Style::default().fg(app.theme.muted))
        }
    };
    // A local YAML path has no subscription fetch, so every update row
    // shows a dash exactly like they do for a local profile.
    let dash = Span::styled("\u{2014}", Style::default().fg(app.theme.muted));
    let text = |value: &str, fallback: &str| {
        let shown = if value.trim().is_empty() {
            fallback.to_string()
        } else {
            value.trim().to_string()
        };
        Span::styled(shown, Style::default().fg(app.theme.foreground))
    };

    let area = centered(WIDTH, (FIELD_ROWS + 4) as u16, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        panel(" Import profile ", &app.theme)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(app.theme.accent)),
        area,
    );
    let inner = area.inner(Margin::new(2, 1));
    // Room left for the value column once the marker and label are drawn.
    let value_width = (inner.width as usize)
        .saturating_sub(LABEL_WIDTH + INDENT as usize)
        .max(8);

    let source_cell = if editing_source {
        let (visible, offset) =
            input_view(app.ui.input_value(), app.ui.input_cursor(), value_width);
        frame.set_cursor_position((
            inner.x + INDENT + LABEL_WIDTH as u16 + offset.min(value_width) as u16,
            inner.y,
        ));
        if visible.is_empty() {
            Span::styled(
                truncate_tail("https://\u{2026} or /path/config.yaml", value_width),
                Style::default().fg(app.theme.muted),
            )
        } else {
            Span::styled(visible, Style::default().fg(app.theme.foreground))
        }
    } else if source.is_empty() {
        Span::styled(
            truncate_tail("https://\u{2026} or /path/config.yaml", value_width),
            Style::default().fg(app.theme.muted),
        )
    } else {
        Span::styled(
            truncate_tail(source, value_width),
            Style::default().fg(app.theme.foreground),
        )
    };

    let rows: Vec<(String, Span, String)> = vec![
        (
            "Source".into(),
            source_cell,
            "subscription URL or YAML path".into(),
        ),
        (
            "Name".into(),
            text(&draft.name, "default"),
            "display name".into(),
        ),
        (
            "Auto update".into(),
            if remote {
                on_off(draft.auto_update)
            } else {
                dash.clone()
            },
            "automatic refresh when due".into(),
        ),
        (
            "Update interval".into(),
            if remote {
                // Show the unit so the row reads like the editor's
                // "24 h"; the draft itself stays a bare number.
                match draft.interval.trim().parse::<u64>() {
                    Ok(hours) => Span::styled(
                        format!("{hours} h"),
                        Style::default().fg(app.theme.foreground),
                    ),
                    Err(_) => text(&draft.interval, "24 h default"),
                }
            } else {
                dash.clone()
            },
            "hours, empty defaults to 24 h".into(),
        ),
        (
            "Update timeout".into(),
            if remote {
                match draft.timeout.trim().parse::<u64>() {
                    Ok(secs) => Span::styled(
                        format!("{secs} s"),
                        Style::default().fg(app.theme.foreground),
                    ),
                    Err(_) => text(&draft.timeout, "30 s default"),
                }
            } else {
                dash.clone()
            },
            "fetch timeout".into(),
        ),
        (
            "Fetch via proxy".into(),
            if remote {
                on_off(draft.use_proxy)
            } else {
                dash.clone()
            },
            "through the core mixed port".into(),
        ),
        (
            "Auth token".into(),
            if remote {
                text(&draft.auth_token, "unset")
            } else {
                dash.clone()
            },
            "Authorization header".into(),
        ),
        (
            "User-Agent".into(),
            if remote {
                Span::styled(
                    truncate_tail(draft.user_agent.trim(), value_width),
                    Style::default().fg(app.theme.foreground),
                )
            } else {
                dash.clone()
            },
            "subscription fetch UA".into(),
        ),
    ];

    let mut lines = Vec::new();
    for (index, (label, value, hint)) in rows.iter().enumerate() {
        let selected = index == app.ui.import_editor_index;
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "\u{25b8} " } else { "  " },
                Style::default().fg(app.theme.accent),
            ),
            Span::styled(
                format!("{label:<LABEL_WIDTH$}"),
                Style::default()
                    .fg(if selected {
                        app.theme.accent
                    } else {
                        app.theme.foreground
                    })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            value.clone(),
            Span::styled(format!("  {hint}"), Style::default().fg(app.theme.muted)),
        ]));
    }
    let submit_selected = app.ui.import_editor_index == SUBMIT_ROW;
    lines.push(Line::from(vec![
        Span::styled(
            if submit_selected { "\u{25b8} " } else { "  " },
            Style::default().fg(app.theme.accent),
        ),
        Span::styled(
            format!("{:<LABEL_WIDTH$}", "Import"),
            Style::default()
                .fg(if submit_selected {
                    app.theme.accent
                } else {
                    app.theme.foreground
                })
                .add_modifier(if submit_selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ),
        Span::styled(
            "fetch, validate and add",
            Style::default().fg(app.theme.success),
        ),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter/Space toggle or edit \u{00b7} j/k move \u{00b7} Esc close",
        Style::default().fg(app.theme.muted),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_text_input(frame: &mut Frame, app: &App, title: &str, hint: &str) {
    let width = 70.min(frame.area().width.saturating_sub(4));
    let area = centered(width, 3, frame.area());
    frame.render_widget(Clear, area);
    let field_width = area.width.saturating_sub(2) as usize;
    let (visible, cursor_offset) =
        input_view(app.ui.input_value(), app.ui.input_cursor(), field_width);
    let field_content = if app.ui.input_value().is_empty() {
        if !hint.is_empty() {
            Line::styled(
                truncate_tail(hint, field_width),
                Style::default().fg(app.theme.muted),
            )
        } else {
            Line::styled("…", Style::default().fg(app.theme.muted))
        }
    } else {
        Line::styled(visible.clone(), Style::default().fg(app.theme.foreground))
    };
    frame.render_widget(
        Paragraph::new(field_content).block(
            input_frame(app.theme.accent).title(Span::styled(
                format!(" {title} "),
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
        ),
        area,
    );
    let cursor_offset = cursor_offset as u16;
    frame.set_cursor_position((
        area.x + 1 + cursor_offset.min(area.width.saturating_sub(2)),
        area.y + 1,
    ));
}

/// Full text of one log line, rule or connection, wrapped. Opened with
/// Enter on the Logs/Rules/Connections tabs; Esc closes it. Connection
/// details are rebuilt from the live snapshot each frame so rates tick.
pub(crate) fn draw_log_detail(frame: &mut Frame, app: &App) {
    let Some(line) = app.detail_text() else {
        return;
    };
    let title = match app.ui.tab {
        crate::app::Tab::Rules => " Rule ",
        crate::app::Tab::Connections => " Connection ",
        _ => " Log line ",
    };
    let height = (frame.area().height * 60 / 100).clamp(8, 30);
    let area = centered(84, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        dialog_frame(app.theme.accent).title(Span::styled(
            title,
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
        area,
    );
    let inner = area.inner(Margin::new(2, 1));
    let rows = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).split(inner);
    frame.render_widget(
        Paragraph::new(super::widgets::strip_vs16(&line).into_owned())
            .wrap(Wrap { trim: false })
            .scroll((app.ui.detail_scroll, 0))
            .style(Style::default().fg(app.theme.foreground)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " j/k ",
                Style::default()
                    .fg(app.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Scroll · ", Style::default().fg(app.theme.foreground)),
            Span::styled(
                " Esc ",
                Style::default()
                    .fg(app.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Close", Style::default().fg(app.theme.foreground)),
        ])),
        rows[1],
    );
}

pub(crate) fn draw_mode_menu(frame: &mut Frame, app: &App) {
    use crate::app::App as AppType;
    let area = centered(58, 10, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        panel("Routing mode ", &app.theme)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(app.theme.accent)),
        area,
    );
    let inner = area.inner(Margin::new(2, 1));
    let mut lines = Vec::new();
    for (index, (mode, description)) in AppType::MODES.iter().enumerate() {
        let selected = index == app.ui.mode_menu_index;
        let current = app.data.snapshot.config.mode.eq_ignore_ascii_case(mode);
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▸ " } else { "  " },
                Style::default().fg(app.theme.accent),
            ),
            Span::styled(
                format!("{mode:<7}"),
                Style::default()
                    .fg(if selected {
                        app.theme.accent
                    } else {
                        app.theme.foreground
                    })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            Span::styled(
                (*description).to_string(),
                Style::default().fg(app.theme.muted),
            ),
            Span::styled(
                if current { " ●" } else { "" },
                Style::default().fg(app.theme.success),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "r/g/d select · j/k move · Enter confirm · Esc close",
        Style::default().fg(app.theme.muted),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Per-profile update settings (`e` on Profiles). Toggles apply on
/// Enter/Space, text rows drop into a text sub-input; everything saves
/// to profiles.yaml immediately, so closing never loses anything.
pub(crate) fn draw_profile_editor(frame: &mut Frame, app: &App) {
    use crate::profiles::DEFAULT_UPDATE_TIMEOUT_SECS;
    let Some(profile) = app.data.profiles.items.get(app.ui.profile_index) else {
        return;
    };
    let remote = profile.url.is_some();
    let on_off = |enabled: bool| {
        if enabled {
            Span::styled("on", Style::default().fg(app.theme.success))
        } else {
            Span::styled("off", Style::default().fg(app.theme.muted))
        }
    };
    // Local profiles have no subscription fetch: update rows show a dash.
    let dash = Span::styled("—", Style::default().fg(app.theme.muted));
    let rows: Vec<(String, Span, String)> = vec![
        (
            "Name".into(),
            Span::styled(
                profile.name.clone(),
                Style::default().fg(app.theme.foreground),
            ),
            "display name".into(),
        ),
        (
            "Subscription URL".into(),
            if remote {
                Span::styled(
                    truncate_tail(profile.url.as_deref().unwrap_or_default(), 36),
                    Style::default().fg(app.theme.foreground),
                )
            } else {
                dash.clone()
            },
            "update fetches this URL".into(),
        ),
        (
            "Auto update".into(),
            if remote {
                on_off(profile.auto_update_enabled())
            } else {
                dash.clone()
            },
            "automatic refresh when due".into(),
        ),
        (
            "Update interval".into(),
            if remote {
                Span::styled(
                    profile
                        .update_interval
                        .map(|hours| format!("{hours} h"))
                        .unwrap_or_else(|| {
                            format!(
                                "{} h default",
                                crate::profiles::DEFAULT_UPDATE_INTERVAL_HOURS
                            )
                        }),
                    Style::default().fg(app.theme.foreground),
                )
            } else {
                dash.clone()
            },
            "hours between auto-updates".into(),
        ),
        (
            "Pin interval".into(),
            if remote {
                on_off(profile.interval_pinned())
            } else {
                dash.clone()
            },
            "server header cannot change it".into(),
        ),
        (
            "Update timeout".into(),
            if remote {
                Span::styled(
                    profile
                        .update_timeout
                        .map(|secs| format!("{secs} s"))
                        .unwrap_or_else(|| format!("{DEFAULT_UPDATE_TIMEOUT_SECS} s default")),
                    Style::default().fg(app.theme.foreground),
                )
            } else {
                dash.clone()
            },
            "fetch timeout".into(),
        ),
        (
            "Fetch via proxy".into(),
            if remote {
                on_off(profile.use_proxy.unwrap_or(false))
            } else {
                dash.clone()
            },
            "through the core mixed port".into(),
        ),
        (
            "Auth token".into(),
            if remote {
                Span::styled(
                    if profile.auth_token.as_deref().is_some_and(|t| !t.is_empty()) {
                        "set •••"
                    } else {
                        "unset"
                    },
                    Style::default().fg(app.theme.foreground),
                )
            } else {
                dash.clone()
            },
            "Authorization header".into(),
        ),
        (
            "User-Agent".into(),
            if remote {
                Span::styled(
                    profile
                        .user_agent
                        .clone()
                        .unwrap_or_else(|| "default".into()),
                    Style::default().fg(app.theme.foreground),
                )
            } else {
                dash.clone()
            },
            "subscription fetch UA".into(),
        ),
    ];
    let area = centered(72, (rows.len() + 5) as u16, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        panel(&format!("Update settings · {} ", profile.name), &app.theme)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(app.theme.accent)),
        area,
    );
    let inner = area.inner(Margin::new(2, 1));
    let mut lines = Vec::new();
    for (index, (label, value, hint)) in rows.iter().enumerate() {
        let selected = index == app.ui.profile_editor_index;
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▸ " } else { "  " },
                Style::default().fg(app.theme.accent),
            ),
            Span::styled(
                format!("{label:<18}"),
                Style::default()
                    .fg(if selected {
                        app.theme.accent
                    } else {
                        app.theme.foreground
                    })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            value.clone(),
            Span::styled(format!("  {hint}"), Style::default().fg(app.theme.muted)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter/Space toggle or edit · j/k move · Esc close",
        Style::default().fg(app.theme.muted),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

/// List editor for a DNS key/value table (`nameserver-policy`, `hosts`).
/// Rows are `domain → servers`; the text sub-inputs for one row stack on top
/// of this (see `draw_input`), so the list stays visible underneath.
pub(crate) fn draw_dns_map_editor(frame: &mut Frame, app: &App) {
    let Some(editor) = app.ui.dns_map_editor.as_ref() else {
        return;
    };
    let label = editor.target.label();
    // Cap the body so a long table scrolls instead of growing past the
    // screen; the hint row below is always visible.
    let visible = editor.entries.len().clamp(3, 12);
    let area = centered(76, visible as u16 + 5, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        panel(
            &format!(" {label} · {} entries ", editor.entries.len()),
            &app.theme,
        )
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.theme.accent)),
        area,
    );
    let inner = area.inner(Margin::new(2, 1));
    let mut lines = Vec::new();
    if editor.entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "no entries · press a to add one",
            Style::default().fg(app.theme.muted),
        )));
    } else {
        // Keep the cursor row inside the visible window.
        let first = editor
            .index
            .saturating_sub(visible.saturating_sub(1))
            .min(editor.entries.len().saturating_sub(visible));
        for (offset, (domain, servers)) in
            editor.entries.iter().skip(first).take(visible).enumerate()
        {
            let index = first + offset;
            let selected = index == editor.index;
            let marker = if selected { "▎" } else { " " };
            lines.push(Line::from(vec![
                Span::styled(format!("{marker} "), Style::default().fg(app.theme.accent)),
                Span::styled(
                    format!("{:<26}", truncate_tail(domain, 26)),
                    Style::default()
                        .fg(if selected {
                            app.theme.accent
                        } else {
                            app.theme.foreground
                        })
                        .add_modifier(if selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(
                    servers.values().join(", "),
                    Style::default().fg(app.theme.foreground),
                ),
            ]));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter edit · a add · d delete · j/k move · Esc close",
        Style::default().fg(app.theme.muted),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

pub(crate) fn draw_core_missing(frame: &mut Frame, app: &App) {
    let Some(dialog) = app.ui.core_missing.as_ref() else {
        return;
    };
    let area = centered(72, 11, frame.area());
    frame.render_widget(Clear, area);
    let block = dialog_frame(app.theme.warning).title(Span::styled(
        " Mihomo core not found ",
        Style::default()
            .fg(app.theme.warning)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(block, area);

    let inner = area.inner(Margin::new(2, 1));
    let rows = Layout::vertical([
        Constraint::Length(2), // explanation
        Constraint::Length(1), // tried path
        Constraint::Length(1), // spacing
        Constraint::Length(1), // download option
        Constraint::Length(1), // provide-path option
        Constraint::Fill(1),   // status message
        Constraint::Length(1), // key hints
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(
            "clashlime could not find a mihomo binary. Download the latest \
             release from GitHub, or point clashlime at an existing binary.",
        )
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(app.theme.foreground)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "Tried: {}",
            crate::config::Config::mihomo_path().display()
        ))
        .style(Style::default().fg(app.theme.muted)),
        rows[1],
    );

    let option = |selected: bool, app: &App| {
        if selected && !dialog.busy {
            (
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
                "▸",
            )
        } else {
            (Style::default().fg(app.theme.muted), " ")
        }
    };

    let (download_style, marker) = option(
        dialog.choice == crate::app::CoreMissingChoice::Download,
        app,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {marker} "), download_style),
            Span::styled("[D] Download latest release  ", download_style),
            Span::styled(
                "installs to ~/.local/share/clashlime/bin/mihomo",
                Style::default().fg(app.theme.muted),
            ),
        ])),
        rows[3],
    );
    let (path_style, marker) = option(
        dialog.choice == crate::app::CoreMissingChoice::ProvidePath,
        app,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {marker} "), path_style),
            Span::styled("[P] Use an existing mihomo binary  ", path_style),
            Span::styled(
                "copies to ~/.local/share/clashlime/bin/mihomo",
                Style::default().fg(app.theme.muted),
            ),
        ])),
        rows[4],
    );

    if dialog.busy || !dialog.message.is_empty() {
        let style = if dialog.busy {
            Style::default().fg(app.theme.accent)
        } else {
            Style::default().fg(app.theme.danger)
        };
        frame.render_widget(
            Paragraph::new(dialog.message.clone())
                .style(style)
                .wrap(Wrap { trim: false }),
            rows[5],
        );
    }
    if let Some((downloaded, total)) = dialog.progress
        && dialog.busy
    {
        let width = rows[5].width.saturating_sub(2) as usize;
        if let Some(total) = total.filter(|total| *total > 0) {
            let ratio = (downloaded.min(total)) as f64 / total as f64;
            let filled = ((width as f64) * ratio).round() as usize;
            let label = format!(
                " {:>7} / {:<7} {:>3.0}%",
                crate::update::format_size(downloaded as usize),
                crate::update::format_size(total as usize),
                ratio * 100.0
            );
            let bar_width = width.saturating_sub(label.chars().count());
            let bar: String =
                "━".repeat(filled.min(bar_width)) + &"─".repeat(bar_width.saturating_sub(filled));
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(bar, Style::default().fg(app.theme.accent)),
                    Span::styled(label, Style::default().fg(app.theme.muted)),
                ])),
                Rect::new(rows[5].x, rows[5].y + 1, rows[5].width, 1),
            );
        } else {
            frame.render_widget(
                Paragraph::new(format!(
                    " {} received",
                    crate::update::format_size(downloaded as usize)
                ))
                .style(Style::default().fg(app.theme.muted)),
                Rect::new(rows[5].x, rows[5].y + 1, rows[5].width, 1),
            );
        }
    }

    let hint_row = |busy| {
        if busy {
            Line::from(vec![
                Span::styled(
                    " Esc ",
                    Style::default()
                        .fg(app.theme.danger)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " Cancel download",
                    Style::default().fg(app.theme.foreground),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled(" ↑/↓ ", Style::default().fg(app.theme.foreground)),
                Span::styled(" Switch   ", Style::default().fg(app.theme.foreground)),
                Span::styled(" Enter ", Style::default().fg(app.theme.accent)),
                Span::styled(" Confirm   ", Style::default().fg(app.theme.foreground)),
                Span::styled(" Esc ", Style::default().fg(app.theme.foreground)),
                Span::styled(" Skip", Style::default().fg(app.theme.foreground)),
            ])
        }
    };
    frame.render_widget(Paragraph::new(hint_row(dialog.busy)), rows[6]);
}

fn draw_core_path_input(frame: &mut Frame, app: &App) {
    let area = centered(82, 9, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        dialog_frame(app.theme.accent).title(Span::styled(
            " Use existing mihomo binary ",
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
        area,
    );
    let inner = area.inner(Margin::new(2, 1));
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new("Enter the absolute path of an existing mihomo binary.")
            .style(Style::default().fg(app.theme.foreground)),
        rows[0],
    );
    let field_width = rows[1].width.saturating_sub(2) as usize;
    let (visible, cursor_offset) =
        input_view(app.ui.input_value(), app.ui.input_cursor(), field_width);
    let field_content = if app.ui.input_value().is_empty() {
        Line::styled("/usr/bin/mihomo", Style::default().fg(app.theme.muted))
    } else {
        Line::styled(visible.clone(), Style::default().fg(app.theme.foreground))
    };
    frame.render_widget(
        Paragraph::new(field_content).block(input_frame(app.theme.accent)),
        rows[1],
    );
    let cursor_offset = cursor_offset as u16;
    frame.set_cursor_position((
        rows[1].x + 1 + cursor_offset.min(rows[1].width.saturating_sub(2)),
        rows[1].y + 1,
    ));
    frame.render_widget(
        Paragraph::new("The binary is verified with `mihomo -v` before use.")
            .style(Style::default().fg(app.theme.muted)),
        rows[2],
    );
}

#[cfg(test)]
mod tests {
    use super::{dialog_frame, draw_text_input, input_frame};
    use crate::app::{App, InputMode};
    use ratatui::{Terminal, backend::TestBackend};

    /// Rounded corners are a box-drawing choice, not a style one: they
    /// must survive into the rendered buffer or terminals with a
    /// rounded-capable font show hard corners anyway.
    #[test]
    fn dialog_frame_renders_rounded_corners() {
        let backend = TestBackend::new(6, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(dialog_frame(ratatui::style::Color::Red), frame.area());
            })
            .unwrap();
        let rendered = terminal.backend().to_string();
        // TestBackend pads every row to the full width; the widget
        // fills it. `to_string` quotes and pads each row, so compare
        // against the unquoted content.
        let lines: Vec<String> = rendered
            .lines()
            .map(|line| line.trim_matches('"').trim_end().to_string())
            .collect();
        assert!(lines[0].starts_with('╭'), "top-left: {:?}", lines[0]);
        assert!(lines[0].ends_with('╮'), "top-right: {:?}", lines[0]);
        assert!(lines[2].starts_with('╰'), "bottom-left: {:?}", lines[2]);
        assert!(lines[2].ends_with('╯'), "bottom-right: {:?}", lines[2]);
    }

    /// The nested text inputs use rounded borders to match the modal frame.
    #[test]
    fn input_frame_renders_rounded_corners() {
        let backend = TestBackend::new(6, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(input_frame(ratatui::style::Color::Cyan), frame.area());
            })
            .unwrap();
        let rendered = terminal.backend().to_string();
        let lines: Vec<&str> = rendered
            .lines()
            .map(|l| l.trim_matches('"').trim_end())
            .collect();
        assert!(lines[0].starts_with('╭'), "top-left: {:?}", lines[0]);
        assert!(lines[0].ends_with('╮'), "top-right: {:?}", lines[0]);
        assert!(lines[2].starts_with('╰'), "bottom-left: {:?}", lines[2]);
        assert!(lines[2].ends_with('╯'), "bottom-right: {:?}", lines[2]);
    }

    /// Text input renders as a compact 3-row single rounded input box without
    /// nested borders or separate Enter/Esc hint rows (Lazygit-style).
    #[test]
    fn text_input_renders_compact_single_box() {
        let mut app = App {
            config: crate::config::Config::default(),
            api: crate::api::MihomoClient::new("http://127.0.0.1:9090", String::new()).unwrap(),
            remote: false,
            theme: crate::theme::Theme::default(),
            ui: crate::app::UiState::default(),
            data: crate::app::DataState::default(),
            tasks: crate::app::TaskHub::default(),
        };
        app.ui
            .begin_input(InputMode::EditController, "http://127.0.0.1:9090".into());
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw_text_input(
                    frame,
                    &app,
                    "Controller",
                    "Enter external controller URL (e.g. http://127.0.0.1:9090)",
                );
            })
            .unwrap();
        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("Controller"));
        assert!(rendered.contains("http://127.0.0.1:9090"));
        // No footer key hints
        assert!(!rendered.contains("Enter"));
        assert!(!rendered.contains("Save"));
        assert!(!rendered.contains("Esc"));
        assert!(!rendered.contains("Cancel"));
    }
}
