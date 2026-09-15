use super::super::widgets::{bytes, panel, selection_style};
use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Row, Table, TableState, Wrap},
};

pub(crate) fn profiles(frame: &mut Frame, app: &App, area: Rect) {
    if app.data.profiles.items.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::styled(
                    "Mihomo is not running because no profile has been imported.",
                    Style::default()
                        .fg(app.theme.warning)
                        .add_modifier(Modifier::BOLD),
                ),
                Line::from(""),
                Line::styled(
                    "Press a to import a local Clash/Mihomo YAML file or a subscription URL.",
                    Style::default().fg(app.theme.foreground),
                ),
                Line::from(""),
                Line::styled(
                    "Mihomo will start automatically after the profile is validated and imported.",
                    Style::default().fg(app.theme.muted),
                ),
            ])
            .wrap(Wrap { trim: true })
            .block(panel("Action required ", &app.theme)),
            area,
        );
        return;
    }
    let rows = app.data.profiles.items.iter().map(|profile| {
        let active = if app.data.profiles.current.as_deref() == Some(&profile.uid) {
            "●"
        } else {
            " "
        };
        let usage = profile
            .subscription
            .map(|sub| {
                let used = sub.upload.saturating_add(sub.download);
                if sub.total == 0 {
                    "—".into()
                } else {
                    format!("{} / {}", bytes(used), bytes(sub.total))
                }
            })
            .unwrap_or_else(|| "—".into());
        Row::new([
            active.into(),
            profile.name.clone(),
            format!("{:?}", profile.kind),
            usage,
            profile.updated.to_string(),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Percentage(30),
            Constraint::Length(10),
            Constraint::Percentage(25),
            Constraint::Length(14),
        ],
    )
    .header(
        Row::new(["", "Name", "Type", "Subscription", "Updated"])
            .style(
                Style::default()
                    .fg(app.theme.muted)
                    .add_modifier(Modifier::BOLD),
            )
            .bottom_margin(1),
    )
    .row_highlight_style(selection_style(true, &app.theme))
    .highlight_symbol("▎ ")
    .block(panel("", &app.theme));
    let mut state = TableState::default().with_selected(Some(app.ui.profile_index));
    frame.render_stateful_widget(table, area, &mut state);
}
