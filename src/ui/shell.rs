use super::layout::{
    ShellAreas, dashboard_card_areas, list_regions, proxy_columns, settings_areas,
    shell_areas, sidebar_mode_button_areas, tab_regions,
};
use super::overlays::{draw_core_missing, draw_input};
use super::tabs::{
    connections::connections, dashboard::dashboard, help::help, logs::logs, profiles::profiles,
    proxies::proxies, rules::rules, settings::settings,
};
use super::types::{HitRegion, HitTarget};
use super::widgets::{bytes, short_title, truncate_tail};
use crate::app::{App, Tab};
use crate::theme::Theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
};

use super::tabs::help::draw_help_overlay;

pub fn draw(frame: &mut Frame, app: &App) -> Vec<HitRegion> {
    frame.render_widget(
        Block::default().style(Style::default().bg(app.theme.background)),
        frame.area(),
    );
    let shell = shell_areas(frame.area());
    draw_navigation(frame, app, shell.navigation, shell.wide);
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
    if app.input.is_some() {
        draw_input(frame, app);
    }
    if app.core_missing.is_some() {
        draw_core_missing(frame, app);
    }
    hit_regions(app, shell)
}

fn hit_regions(app: &App, shell: ShellAreas) -> Vec<HitRegion> {
    let mut regions = tab_regions(shell.navigation, shell.wide);
    if shell.wide
        && let Some(buttons) = sidebar_mode_button_areas(shell.navigation)
    {
        regions.extend(buttons.into_iter().zip(["rule", "global", "direct"]).map(
            |(area, mode)| HitRegion {
                area,
                target: HitTarget::RoutingMode(mode),
            },
        ));
    }
    match app.tab {
        Tab::Dashboard => {
            let cards = dashboard_card_areas(shell.content);
            regions.push(HitRegion {
                area: cards[0],
                target: HitTarget::CoreToggle,
            });
        }
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
            shell.content,
            app.snapshot.rules.rules.len(),
            app.rule_index,
            true,
            HitTarget::Rule,
        )),
        Tab::Settings => {
            let [settings, _] = settings_areas(shell.content);
            regions.extend(list_regions(
                settings,
                crate::app::SETTINGS_COUNT,
                app.setting_index,
                false,
                HitTarget::Setting,
            ));
        }
        _ => {}
    }
    regions
}

fn draw_navigation(frame: &mut Frame, app: &App, area: Rect, wide: bool) {
    let selected = Tab::ALL.iter().position(|tab| *tab == app.tab).unwrap_or(0);
    if !wide {
        let areas = Layout::vertical([Constraint::Length(2), Constraint::Length(2)]).split(area);
        frame.render_widget(
            Paragraph::new(Line::styled(
                " O M A S H ",
                Style::default()
                    .fg(app.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
            areas[0],
        );
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
                        .bg(app.theme.surface_active)
                        .add_modifier(Modifier::BOLD),
                ),
            areas[1],
        );
        return;
    }

    frame.render_widget(
        Block::default().style(Style::default().bg(app.theme.surface)),
        area,
    );
    let inner = area.inner(Margin::new(1, 1));
    let brand = Rect::new(inner.x, inner.y, inner.width, 3);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![Span::styled(
                "█▀█ █▄ ▄█ █▀█ █▀▀ █ █",
                Style::default()
                    .fg(app.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![Span::styled(
                "█▄█ █ ▀ █ █▀█ ▄▄█ █▀█",
                Style::default()
                    .fg(app.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            )]),
        ])
        .alignment(Alignment::Center),
        brand,
    );

    for (index, tab) in Tab::ALL.iter().copied().enumerate() {
        let active = tab == app.tab;
        let row = Rect::new(
            area.x + 1,
            area.y + 5 + index as u16,
            area.width.saturating_sub(2),
            1,
        );
        let style = if active {
            Style::default()
                .fg(app.theme.accent)
                .bg(app.theme.surface_active)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(app.theme.foreground)
        };
        let marker = if active { "›" } else { " " };
        frame.render_widget(
            Paragraph::new(format!("{marker} {}  {}", index + 1, tab.title())).style(style),
            row,
        );
    }

    if area.height >= 25 {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "─".repeat(area.width.saturating_sub(2) as usize),
                Style::default().fg(app.theme.border),
            )),
            Rect::new(area.x + 1, area.y + 14, area.width.saturating_sub(2), 1),
        );
        let panel = Rect::new(
            area.x + 1,
            area.y + 15,
            area.width.saturating_sub(2),
            area.bottom().saturating_sub(3).saturating_sub(area.y + 15),
        );
        draw_sidebar_info(frame, app, panel);
    }
    if let Some(buttons) = sidebar_mode_button_areas(area) {
        draw_mode_buttons(frame, buttons, &app.snapshot.config.mode, &app.theme);
    }
}

fn draw_sidebar_info(frame: &mut Frame, app: &App, area: Rect) {
    let (dot, label, color) = core_status(app);
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{dot} "), Style::default().fg(color)),
        Span::styled(label, Style::default().fg(color).add_modifier(Modifier::BOLD)),
    ])];

    let (up, down) = app.speeds;
    lines.push(Line::from(vec![
        Span::styled("↑ ", Style::default().fg(app.theme.success)),
        Span::styled(format!("{}/s", bytes(up)), Style::default().fg(app.theme.foreground)),
        Span::styled("  ↓ ", Style::default().fg(app.theme.accent)),
        Span::styled(format!("{}/s", bytes(down)), Style::default().fg(app.theme.foreground)),
    ]));

    let profile_line = match app.current_profile() {
        Some(profile) => {
            let kind = match profile.kind {
                crate::profiles::ProfileKind::Remote => "remote",
                crate::profiles::ProfileKind::Local => "local",
            };
            format!("{} ({kind})", profile.name)
        }
        None => "none".into(),
    };
    lines.push(Line::from(vec![
        Span::styled("PROFILE ", Style::default().fg(app.theme.muted)),
        Span::styled(truncate_tail(&profile_line, area.width.saturating_sub(8) as usize), Style::default().fg(app.theme.foreground)),
    ]));

    let flag = |on: bool| -> Span<'static> {
        Span::styled(
            if on { "on" } else { "off" },
            Style::default()
                .fg(if on { app.theme.success } else { app.theme.muted })
                .add_modifier(if on { Modifier::BOLD } else { Modifier::empty() }),
        )
    };
    lines.push(Line::from(vec![
        Span::styled("DNS ", Style::default().fg(app.theme.muted)),
        flag(app.config.dns.enable),
        Span::styled(" · SNIFF ", Style::default().fg(app.theme.muted)),
        flag(app.config.sniffer_enable),
    ]));

    let mut core_line = Vec::new();
    let version = app.snapshot.version.version.trim();
    core_line.push(Span::styled(
        if version.is_empty() { "—".to_string() } else { version.to_string() },
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

    lines.push(Line::from(Span::styled(
        truncate_tail(&app.status, area.width as usize),
        status_style(app),
    )));

    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_page_header(frame: &mut Frame, app: &App, area: Rect) {
    if area.height < 3 {
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
        Paragraph::new(vec![
            Line::from(""),
            Line::styled(
                app.tab.title(),
                Style::default()
                    .fg(app.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::styled(subtitle, Style::default().fg(app.theme.muted)),
        ])
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(app.theme.border)),
        ),
        area,
    );
}

fn draw_mode_buttons(frame: &mut Frame, buttons: [Rect; 3], current: &str, theme: &Theme) {
    for ((button, mode), label) in buttons
        .into_iter()
        .zip(["rule", "global", "direct"])
        .zip(["RULE", "GLOBAL", "DIRECT"])
    {
        let active = current.eq_ignore_ascii_case(mode);
        frame.render_widget(
            Paragraph::new(label)
                .alignment(Alignment::Center)
                .style(if active {
                    Style::default()
                        .fg(theme.background)
                        .bg(theme.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.muted).bg(theme.surface_active)
                }),
            button,
        );
    }
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
    if area.height < 2 {
        return;
    }
    let inner = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(1),
    );
    if wide {
        frame.render_widget(
            Paragraph::new(shortcut_line(app, inner.width, &app.theme)),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
    } else {
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);
        let (dot, core_label, color) = core_status(app);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{dot} "), Style::default().fg(color)),
                Span::styled(
                    core_label,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" · ", Style::default().fg(app.theme.muted)),
                Span::styled(&app.status, status_style(app)),
            ])),
            rows[0],
        );
        frame.render_widget(
            Paragraph::new(shortcut_line(app, inner.width, &app.theme)),
            rows[1],
        );
    }
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
        Tab::Rules => &[],
        Tab::Logs => &[("r", "Refresh")],
        Tab::Settings => &[
            ("Enter", "Change"),
            ("u", "Check"),
            ("o", "Open"),
            ("b", "Backup"),
            ("R", "Restore"),
            ("g", "Geo data"),
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
        Style::default()
            .fg(key_color)
            .bg(theme.surface_active)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        format!(" {description}  "),
        Style::default().fg(theme.muted),
    ));
}
