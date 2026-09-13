use super::layout::{
    ShellAreas, list_regions, proxy_columns, settings_areas, shell_areas,
    sidebar_mode_button_areas, tab_regions, topbar_areas,
};
use super::overlays::{
    draw_core_missing, draw_input, draw_log_detail, draw_mode_menu, draw_profile_editor,
};
use super::tabs::{
    connections::connections, dashboard::dashboard, help::help, logs::logs, profiles::profiles,
    proxies::proxies, rules::rules, settings::settings,
};
use super::types::{HitRegion, HitTarget};
use super::widgets::{bytes, input_view, short_title, strip_vs16, truncate_tail};
use crate::app::{App, InputMode, Tab};
use crate::theme::Theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
};

use super::tabs::help::draw_help_overlay;

pub fn draw(frame: &mut Frame, app: &mut App) -> Vec<HitRegion> {
    // No fullscreen fill: terminal background shows through (lazygit-style).
    let shell = shell_areas(frame.area());
    draw_navigation(frame, app, shell.topbar, shell.wide);
    if shell.wide {
        draw_sidebar(frame, app, shell.sidebar);
    }
    draw_page_header(frame, app, shell.header);
    match app.tab {
        Tab::Dashboard => dashboard(frame, app, shell.content),
        Tab::Proxies => proxies(frame, app, shell.content),
        Tab::Profiles => profiles(frame, app, shell.content),
        Tab::Connections => connections(frame, app, shell.content),
        Tab::Rules => rules(frame, app, shell.content),
        Tab::Logs => logs(frame, app, shell.content),
        Tab::Settings => settings(frame, app, shell.content),
        Tab::Help => help(frame, app, shell.content),
    }
    draw_status(frame, app, shell.status, shell.wide);
    if app.help_open {
        draw_help_overlay(frame, &app.theme);
    }
    if app.mode_menu {
        draw_mode_menu(frame, app);
    }
    if app.profile_editor {
        draw_profile_editor(frame, app);
    }
    if app.input.is_some() && !is_search_input(app) {
        draw_input(frame, app);
    }
    if app.core_missing.is_some() {
        draw_core_missing(frame, app);
    }
    if app.log_detail.is_some() {
        draw_log_detail(frame, app);
    }
    hit_regions(app, shell)
}

fn hit_regions(app: &App, shell: ShellAreas) -> Vec<HitRegion> {
    let mut regions = if shell.wide {
        let topbar = topbar_areas(shell.topbar);
        tab_regions(topbar[1])
    } else {
        tab_regions(Rect::new(
            shell.topbar.x,
            shell.topbar.y + 2,
            shell.topbar.width,
            1,
        ))
    };
    if shell.wide
        && let Some(buttons) = sidebar_mode_button_areas(shell.sidebar)
    {
        regions.extend(buttons.into_iter().zip(["rule", "global", "direct"]).map(
            |(area, mode)| HitRegion {
                area,
                target: HitTarget::RoutingMode(mode),
            },
        ));
    }
    if shell.wide {
        // The dashboard status card is gone; its click-to-toggle-core
        // affordance moves to the sidebar status dot.
        regions.push(HitRegion {
            area: Rect::new(
                shell.sidebar.x + 1,
                shell.sidebar.y + 1,
                shell.sidebar.width.saturating_sub(2),
                1,
            ),
            target: HitTarget::CoreToggle,
        });
    }
    match app.tab {
        Tab::Proxies => {
            let columns = proxy_columns(shell.content);
            regions.extend(list_regions(
                columns[0],
                app.proxy_groups().len(),
                app.group_index,
                false,
                HitTarget::ProxyGroup,
            ));
            regions.extend(list_regions(
                columns[1],
                app.selected_group().map_or(0, |(_, group)| group.all.len()),
                app.node_index,
                false,
                HitTarget::ProxyNode,
            ));
        }
        Tab::Profiles => regions.extend(list_regions(
            shell.content,
            app.profiles.items.len(),
            app.profile_index,
            true,
            HitTarget::Profile,
        )),
        Tab::Connections => regions.extend(list_regions(
            shell.content,
            app.snapshot.connections.connections.len(),
            app.connection_index,
            true,
            HitTarget::Connection,
        )),
        Tab::Rules => regions.extend(list_regions(
            super::tabs::rules::rules_table_area(
                shell.content,
                super::tabs::rules::sorted_providers(app).len(),
            ),
            super::tabs::rules::filtered_rules(app).len(),
            app.rule_index,
            true,
            HitTarget::Rule,
        )),
        Tab::Settings => {
            let [_, rows, _] = settings_areas(shell.content);
            let section = app.setting_section.index();
            regions.extend(list_regions(
                rows,
                app.setting_section.row_count(),
                app.setting_index,
                false,
                |row| HitTarget::Setting(section, row),
            ));
        }
        _ => {}
    }
    regions
}

fn draw_navigation(frame: &mut Frame, app: &App, area: Rect, wide: bool) {
    if !wide {
        let areas = Layout::vertical([Constraint::Length(2), Constraint::Length(2)]).split(area);
        frame.render_widget(
            Paragraph::new(Line::styled(
                " CLASHLIME ",
                Style::default()
                    .fg(app.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
            areas[0],
        );
        render_tab_strip(frame, app, areas[1]);
        return;
    }

    let [brand, tabs] = topbar_areas(area);
    frame.render_widget(
        Paragraph::new(Line::styled(
            " CLASHLIME ",
            Style::default()
                .fg(app.theme.foreground)
                .add_modifier(Modifier::BOLD),
        )),
        brand,
    );
    render_tab_strip(frame, app, tabs);
}

fn render_tab_strip(frame: &mut Frame, app: &App, area: Rect) {
    let selected = Tab::ALL.iter().position(|tab| *tab == app.tab).unwrap_or(0);
    let titles = Tab::ALL
        .iter()
        .enumerate()
        .map(|(index, tab)| Line::from(format!(" {} {} ", index + 1, short_title(*tab))));
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .divider(" ")
            .style(Style::default().fg(app.theme.muted))
            .highlight_style(
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        area,
    );
}

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    // The old surface fill is gone (transparent theme): a real right
    // border keeps the sidebar separated, lazygit-style.
    frame.render_widget(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(app.theme.border)),
        area,
    );
    if area.height < 18 {
        return;
    }
    let inner = area.inner(Margin::new(1, 1));
    let info = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(3),
    );
    draw_sidebar_info(frame, app, info);
    if let Some(buttons) = sidebar_mode_button_areas(area) {
        draw_mode_buttons(frame, buttons, &app.snapshot.config.mode, &app.theme);
    }
}

fn draw_sidebar_info(frame: &mut Frame, app: &App, area: Rect) {
    let (dot, label, color) = core_status(app);
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{dot} "), Style::default().fg(color)),
        Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])];

    let (up, down) = app.speeds;
    lines.push(Line::from(vec![
        Span::styled("↑ ", Style::default().fg(app.theme.success)),
        Span::styled(
            format!("{}/s", bytes(up)),
            Style::default().fg(app.theme.foreground),
        ),
        Span::styled("  ↓ ", Style::default().fg(app.theme.accent)),
        Span::styled(
            format!("{}/s", bytes(down)),
            Style::default().fg(app.theme.foreground),
        ),
    ]));

    // Two lines: the 23-column sidebar leaves ~13 cells after a
    // `PROFILE ` prefix, so the name gets its own full-width line and
    // the kind suffix only tags along when everything fits.
    let (profile_name, profile_kind) = match app.current_profile() {
        Some(profile) => {
            let kind = match profile.kind {
                crate::profiles::ProfileKind::Remote => "remote",
                crate::profiles::ProfileKind::Local => "local",
            };
            (profile.name.clone(), Some(kind))
        }
        None => ("none".into(), None),
    };
    lines.push(Line::from(Span::styled(
        "PROFILE",
        Style::default().fg(app.theme.muted),
    )));
    let name_style = Style::default()
        .fg(app.theme.foreground)
        .add_modifier(Modifier::BOLD);
    let width = area.width as usize;
    let plain: std::borrow::Cow<'_, str> = strip_vs16(&profile_name);
    match profile_kind {
        Some(kind) if format!("{plain} · {kind}").chars().count() <= width => {
            lines.push(Line::from(vec![
                Span::styled(plain.into_owned(), name_style),
                Span::styled(format!(" · {kind}"), Style::default().fg(app.theme.muted)),
            ]));
        }
        _ => {
            lines.push(Line::from(Span::styled(
                truncate_tail(&plain, width),
                name_style,
            )));
        }
    }

    let flag = |on: bool| -> Span<'static> {
        Span::styled(
            if on { "on" } else { "off" },
            Style::default()
                .fg(if on {
                    app.theme.success
                } else {
                    app.theme.muted
                })
                .add_modifier(if on {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        )
    };
    lines.push(Line::from(vec![
        Span::styled("DNS ", Style::default().fg(app.theme.muted)),
        flag(app.config.dns.enable),
        Span::styled(" · SNIFF ", Style::default().fg(app.theme.muted)),
        flag(app.config.sniffer_enable),
    ]));
    lines.push(Line::from(vec![
        Span::styled("SESSIONS ", Style::default().fg(app.theme.muted)),
        Span::styled(
            app.snapshot.connections.connections.len().to_string(),
            Style::default()
                .fg(app.theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(""));

    let mut core_line = Vec::new();
    let version = app.snapshot.version.version.trim();
    core_line.push(Span::styled(
        if version.is_empty() {
            "—".to_string()
        } else {
            version.to_string()
        },
        Style::default().fg(app.theme.foreground),
    ));
    if let Some(memory) = app.snapshot.memory.as_ref() {
        core_line.push(Span::styled(" · ", Style::default().fg(app.theme.muted)));
        core_line.push(Span::styled(
            crate::update::format_size(memory.inuse as usize),
            Style::default().fg(app.theme.foreground),
        ));
    }
    lines.push(Line::from(core_line));

    // NOTE: the human-readable `app.status` is intentionally NOT here: the
    // 21-cell sidebar line truncated every long message. It lives in the
    // full-width bottom status bar instead (see `draw_status`).
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_page_header(frame: &mut Frame, app: &App, area: Rect) {
    if area.height < 2 {
        return;
    }
    let subtitle = match app.tab {
        Tab::Dashboard => "Live overview of your local proxy service",
        Tab::Proxies => "Choose routing groups and test node latency",
        Tab::Profiles => "Manage local and remote configuration profiles",
        Tab::Connections => "Inspect and close active network sessions",
        Tab::Rules => "Review the policies currently loaded by Mihomo",
        Tab::Logs => "Recent runtime output from the system Mihomo core",
        Tab::Settings => "Core behavior, networking and application maintenance",
        Tab::Help => "Keyboard and mouse shortcuts",
    };
    let area = area.inner(Margin::new(1, 0));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                app.tab.title(),
                Style::default()
                    .fg(app.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ·  ", Style::default().fg(app.theme.border)),
            Span::styled(subtitle, Style::default().fg(app.theme.muted)),
        ]))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(app.theme.border)),
        ),
        area,
    );
}

fn draw_mode_buttons(frame: &mut Frame, buttons: [Rect; 3], current: &str, theme: &Theme) {
    // One full-width row per mode; the active one carries ▸ + accent.
    let lines: Vec<Line> = ["rule", "global", "direct"]
        .into_iter()
        .zip(["RULE", "GLOBAL", "DIRECT"])
        .map(|(mode, label)| {
            let active = current.eq_ignore_ascii_case(mode);
            Line::from(vec![
                Span::styled(
                    if active { "▸ " } else { "  " },
                    Style::default().fg(theme.accent),
                ),
                Span::styled(
                    label,
                    if active {
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.muted)
                    },
                ),
            ])
        })
        .collect();
    let area = Rect::new(
        buttons[0].x,
        buttons[0].y,
        buttons[0].width,
        buttons.len() as u16,
    );
    frame.render_widget(Paragraph::new(lines), area);
}

fn core_status(app: &App) -> (&'static str, &'static str, Color) {
    if app.online {
        ("●", "CORE ONLINE", app.theme.success)
    } else if app.profiles.items.is_empty() {
        ("!", "PROFILE REQUIRED", app.theme.warning)
    } else if app.supervisor.running {
        ("◐", "CORE STARTING", app.theme.warning)
    } else {
        ("●", "CORE OFFLINE", app.theme.danger)
    }
}

fn status_style(app: &App) -> Style {
    let color = match app.status_color_kind() {
        crate::app::StatusKind::Success => app.theme.success,
        crate::app::StatusKind::Warning => app.theme.warning,
        crate::app::StatusKind::Error => app.theme.danger,
        crate::app::StatusKind::Busy => app.theme.accent,
        crate::app::StatusKind::Info => app.theme.foreground,
    };
    Style::default().fg(color)
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect, wide: bool) {
    frame.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(app.theme.border)),
        area,
    );
    if area.height < 3 {
        return;
    }
    let inner = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(1),
    );
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);
    if wide {
        // Full-width status message. Wide mode used to render shortcuts
        // only, leaving the message to the 21-cell sidebar line where it
        // was always cut off.
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                truncate_tail(&app.status, inner.width as usize),
                status_style(app),
            )])),
            rows[0],
        );
    } else {
        let (dot, core_label, color) = core_status(app);
        // Narrow mode hides the sidebar (and its mode buttons), so the
        // routing mode rides along in the status row instead.
        let status = vec![
            Span::styled(format!("{dot} "), Style::default().fg(color)),
            Span::styled(
                core_label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · ", Style::default().fg(app.theme.muted)),
            Span::styled(&app.status, status_style(app)),
            Span::styled(" · ", Style::default().fg(app.theme.muted)),
            Span::styled(
                app.snapshot.config.mode.to_uppercase(),
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        frame.render_widget(Paragraph::new(Line::from(status)), rows[0]);
    }
    frame.render_widget(
        Paragraph::new(if is_search_input(app) {
            search_line(app, inner.width)
        } else {
            shortcut_line(app, inner.width, &app.theme)
        }),
        rows[1],
    );
    if is_search_input(app) {
        place_search_cursor(frame, app, rows[1]);
    }
}

/// Search modes (`/` on Logs/Rules) render inline in the status bar
/// instead of a centered popup, so the filtered content stays visible.
fn is_search_input(app: &App) -> bool {
    matches!(
        app.input,
        Some(InputMode::SearchLogs) | Some(InputMode::SearchRules)
    )
}

fn search_line(app: &App, width: u16) -> Line<'static> {
    let field_width = width.saturating_sub(28) as usize;
    let (visible, _) = input_view(&app.input_buffer, app.input_cursor, field_width.max(1));
    let mut spans = vec![
        Span::styled(
            " /",
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if visible.is_empty() {
                "…".to_string()
            } else {
                visible
            },
            Style::default().fg(app.theme.foreground),
        ),
    ];
    spans.push(Span::styled(
        "  Enter keep · Esc clear",
        Style::default().fg(app.theme.muted),
    ));
    Line::from(spans)
}

fn place_search_cursor(frame: &mut Frame, app: &App, row: Rect) {
    let field_width = row.width.saturating_sub(28) as usize;
    let (_, cursor_offset) = input_view(&app.input_buffer, app.input_cursor, field_width.max(1));
    frame.set_cursor_position((
        row.x + 2 + (cursor_offset as u16).min(row.width.saturating_sub(3)),
        row.y,
    ));
}

fn contextual_hints(app: &App) -> &'static [(&'static str, &'static str)] {
    match app.tab {
        Tab::Dashboard => &[("s", "Core"), ("m", "Mode")],
        Tab::Proxies if app.selected_group_is_manual() => {
            &[("Tab", "Pane"), ("Enter", "Select"), ("d", "Delay")]
        }
        Tab::Proxies => &[("Tab", "Pane"), ("d", "Delay")],
        Tab::Profiles => &[
            ("Enter", "Activate"),
            ("a", "Import"),
            ("u", "Update"),
            ("D", "Delete"),
        ],
        Tab::Connections => &[("x", "Close"), ("X", "Close all")],
        Tab::Rules => &[
            ("j/k", "Scroll"),
            ("/", "Search"),
            ("Enter", "Detail"),
            ("u", "Update providers"),
        ],
        Tab::Logs => &[
            ("j/k", "Scroll"),
            ("←/→", "H-scroll"),
            ("Enter", "Detail"),
            ("G", "Follow"),
            ("f", "Filter"),
            ("v", "Source"),
            ("/", "Search"),
            ("c", "Clear"),
            ("r", "Refresh"),
        ],
        Tab::Settings => &[
            ("←→", "Section"),
            ("Enter", "Change"),
            ("u", "Check"),
            ("o", "Open"),
            ("b", "Backup"),
            ("R", "Restore"),
            ("g", "Geo"),
        ],
        Tab::Help => &[],
    }
}

fn shortcut_line(app: &App, width: u16, theme: &Theme) -> Line<'static> {
    let mut spans = Vec::new();
    let mut used = 0usize;
    let reserved = 19usize;
    for &(key, description) in contextual_hints(app) {
        let size = key.chars().count() + description.chars().count() + 5;
        if used + size + reserved > width as usize {
            break;
        }
        push_hint(&mut spans, key, description, theme);
        used += size;
    }
    push_hint(&mut spans, "?", "Help", theme);
    push_hint(&mut spans, "q", "Quit", theme);
    Line::from(spans)
}

fn push_hint(
    spans: &mut Vec<Span<'static>>,
    key: &'static str,
    description: &'static str,
    theme: &Theme,
) {
    let key_color = if matches!(key, "D" | "X" | "R") {
        theme.danger
    } else {
        theme.accent
    };
    spans.push(Span::styled(
        format!(" {key} "),
        Style::default().fg(key_color).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        format!(" {description}  "),
        Style::default().fg(theme.muted),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::MihomoClient;
    use crate::app::{LogSource, SettingSection, StatusKind};
    use crate::config::Config;
    use crate::core::SupervisorState;
    use crate::profiles::Profiles;
    use ratatui::{Terminal, backend::TestBackend};

    fn status_test_app(status: &str) -> App {
        App {
            config: Config::default(),
            api: MihomoClient::new("http://127.0.0.1:9090", String::new()).unwrap(),
            snapshot: Default::default(),
            profiles: Profiles::default(),
            proxy_group_order: Vec::new(),
            theme: Theme::default(),
            supervisor: SupervisorState::default(),
            logs: Vec::new(),
            log_source: LogSource::All,
            log_scroll: 0,
            log_follow: true,
            log_level_filter: None,
            log_query: String::new(),
            log_height: 0,
            log_hscroll: 0,
            log_detail: None,
            tab: Tab::Dashboard,
            group_index: 0,
            node_index: 0,
            connection_index: 0,
            rule_index: 0,
            rule_query: String::new(),
            profile_index: 0,
            setting_index: 0,
            setting_section: SettingSection::Core,
            section_cursor: [0; 6],
            node_focus: false,
            mode_menu: false,
            mode_menu_index: 0,
            profile_editor: false,
            profile_editor_index: 0,
            status: status.into(),
            status_kind: StatusKind::Info,
            status_sticky_until: None,
            online: false,
            last_refresh: None,
            last_slow_refresh: None,
            last_profile_check: None,
            previous_totals: (0, 0),
            speeds: (0, 0),
            input: None,
            input_buffer: String::new(),
            input_cursor: 0,
            help_open: false,
            core_missing: None,
            core_download_rx: None,
            core_download_abort: None,
            core_upgrade: None,
            geo_rx: None,
            geo_task: None,
            import_rx: None,
            import_task: None,
            profile_rx: None,
            profile_task: None,
            update_rx: None,
            update_task: None,
            delay_rx: None,
            delay_task: None,
            log_rx: None,
            log_task: None,
            log_stream_key: String::new(),
            log_stream_live: false,
            log_backlog_loaded: false,
            mihomo_update: Default::default(),
            mouse_regions: Vec::new(),
            last_click: None,
        }
    }

    fn row_text(terminal: &Terminal<TestBackend>, y: u16, width: u16) -> String {
        let buffer = terminal.backend().buffer().clone();
        (0..width)
            .map(|x| buffer.cell((x, y)).unwrap().symbol().to_owned())
            .collect()
    }

    /// The long offline reason must survive intact on the wide status row;
    /// it used to live only in the 21-cell sidebar line (`…`-truncated).
    #[test]
    fn wide_status_bar_shows_full_message() {
        let app = status_test_app(
            "Mihomo is not running: no profile imported. Open Profiles and press a to import.",
        );
        let backend = TestBackend::new(120, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_status(frame, &app, Rect::new(0, 0, 120, 3), true))
            .unwrap();
        let status_row = row_text(&terminal, 1, 120);
        assert!(
            status_row.contains("press a to import"),
            "status cut off: {status_row}"
        );
        let hints_row = row_text(&terminal, 2, 120);
        assert!(hints_row.contains("Quit"), "shortcuts lost: {hints_row}");
    }

    #[test]
    fn narrow_status_bar_keeps_message_and_mode() {
        let app = status_test_app("Synced");
        let backend = TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_status(frame, &app, Rect::new(0, 0, 60, 3), false))
            .unwrap();
        let status_row = row_text(&terminal, 1, 60);
        assert!(
            status_row.contains("Synced"),
            "narrow status lost: {status_row}"
        );
    }
}
