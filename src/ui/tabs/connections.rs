use super::super::widgets::{bytes, panel, selection_style};
use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Cell, Row, Table, TableState},
};

pub(crate) fn connections(frame: &mut Frame, app: &App, area: Rect) {
    let rows = app
        .snapshot
        .connections
        .connections
        .iter()
        .map(|connection| {
            let target = if connection.metadata.host.is_empty() {
                &connection.metadata.destination_ip
            } else {
                &connection.metadata.host
            };
            Row::new(vec![
                Cell::from(format!(
                    "{}:{}",
                    target, connection.metadata.destination_port
                )),
                Cell::from(
                    format!(
                        "{} {}",
                        connection.metadata.network, connection.metadata.kind
                    )
                    .to_uppercase(),
                ),
                Cell::from(connection.chains.join(" → ")),
                Cell::from(format!(
                    "↑{} ↓{}",
                    bytes(connection.upload),
                    bytes(connection.download)
                )),
            ])
        });
    let widths = [
        Constraint::Percentage(38),
        Constraint::Length(8),
        Constraint::Percentage(32),
        Constraint::Percentage(22),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(["Destination", "Network", "Chain", "Traffic"])
                .style(
                    Style::default()
                        .fg(app.theme.muted)
                        .add_modifier(Modifier::BOLD),
                )
                .bottom_margin(1),
        )
        .row_highlight_style(selection_style(true, &app.theme))
        .highlight_symbol("▎ ")
        .block(panel(" Active connections ", &app.theme));
    let mut state = TableState::default().with_selected(Some(app.connection_index));
    frame.render_stateful_widget(table, area, &mut state);
}
