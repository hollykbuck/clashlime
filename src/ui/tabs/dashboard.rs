use super::super::widgets::{bool_text, panel, status_badge, strip_vs16, value_or_dash};
use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

/// Top cards (status/mode/traffic/sessions) were removed: the sidebar
/// already shows status, mode and traffic on every tab, and sessions
/// moved there too. What remains is unique to this page.
pub(crate) fn dashboard(frame: &mut Frame, app: &App, area: Rect) {
    let details_area = area;
    let details = if details_area.width >= 70 {
        Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
            .spacing(2)
            .split(details_area)
    } else {
        Layout::horizontal([Constraint::Percentage(100), Constraint::Length(0)]).split(details_area)
    };
    let info = vec![
        Line::from(vec![
            Span::styled("VERSION       ", Style::default().fg(app.theme.muted)),
            Span::styled(
                value_or_dash(&app.snapshot.version.version),
                Style::default().fg(app.theme.foreground),
            ),
        ]),
        Line::from(vec![
            Span::styled("MIHOMO       ", Style::default().fg(app.theme.muted)),
            Span::raw(if app.profiles.items.is_empty() {
                "Not started · no profile imported".into()
            } else if app.supervisor.running {
                format!(
                    "Running · PID {} · {} restarts · {} reloads",
                    app.supervisor
                        .pid
                        .map_or_else(|| "—".into(), |pid| pid.to_string()),
                    app.supervisor.restarts,
                    app.supervisor.reloads
                )
            } else {
                app.supervisor
                    .error
                    .clone()
                    .unwrap_or_else(|| "stopped".into())
            }),
        ]),
        Line::from(vec![
            Span::styled("CONTROLLER    ", Style::default().fg(app.theme.muted)),
            Span::raw(&app.config.controller),
        ]),
        Line::from(vec![
            Span::styled("MIXED PORT    ", Style::default().fg(app.theme.muted)),
            Span::raw(
                app.snapshot
                    .config
                    .mixed_port
                    .map_or("—".into(), |p| p.to_string()),
            ),
        ]),
        Line::from(vec![
            Span::styled("ALLOW LAN     ", Style::default().fg(app.theme.muted)),
            status_badge(bool_text(app.snapshot.config.allow_lan), &app.theme),
        ]),
        Line::from(vec![
            Span::styled("IPV6          ", Style::default().fg(app.theme.muted)),
            status_badge(bool_text(app.snapshot.config.ipv6), &app.theme),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(info).block(panel(" Runtime ", &app.theme)),
        details[0],
    );

    if details.len() > 1 && details[1].width > 0 {
        let selected = app
            .selected_group()
            .map(|(name, group)| {
                vec![
                    Line::styled("CURRENT ROUTE", Style::default().fg(app.theme.muted)),
                    Line::styled(
                        strip_vs16(name).into_owned(),
                        Style::default()
                            .fg(app.theme.foreground)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Line::from(""),
                    Line::styled("SELECTED NODE", Style::default().fg(app.theme.muted)),
                    Line::styled(
                        strip_vs16(value_or_dash(&group.now)).into_owned(),
                        Style::default()
                            .fg(app.theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Line::from(""),
                    Line::styled(
                        "Open Proxies to switch or test nodes.",
                        Style::default().fg(app.theme.muted),
                    ),
                ]
            })
            .unwrap_or_else(|| {
                if app.profiles.items.is_empty() {
                    vec![
                        Line::styled(
                            "Mihomo has not started.",
                            Style::default()
                                .fg(app.theme.warning)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Line::from(""),
                        Line::styled(
                            "No profile has been imported.",
                            Style::default().fg(app.theme.foreground),
                        ),
                        Line::from(""),
                        Line::styled(
                            "Open Profiles and press a to import a local YAML or subscription URL.",
                            Style::default().fg(app.theme.muted),
                        ),
                    ]
                } else {
                    vec![Line::styled(
                        "No proxy group available",
                        Style::default().fg(app.theme.muted),
                    )]
                }
            });
        frame.render_widget(
            Paragraph::new(selected)
                .wrap(Wrap { trim: true })
                .block(panel(" Active route ", &app.theme)),
            details[1],
        );
    }
}
