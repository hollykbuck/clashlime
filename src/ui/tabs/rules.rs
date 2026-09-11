use super::super::widgets::{panel, selection_style};
use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Row, Table, TableState},
};

pub(crate) fn rules(frame: &mut Frame, app: &App, area: Rect) {
    let rows = app.snapshot.rules.rules.iter().map(|rule| {
        Row::new([
            rule.kind.as_str(),
            rule.payload.as_str(),
            rule.proxy.as_str(),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Percentage(62),
            Constraint::Percentage(25),
        ],
    )
    .header(
        Row::new(["Type", "Payload", "Policy"])
            .style(
                Style::default()
                    .fg(app.theme.muted)
                    .add_modifier(Modifier::BOLD),
            )
            .bottom_margin(1),
    )
    .row_highlight_style(selection_style(true, &app.theme))
    .highlight_symbol("▎ ")
    .block(panel(" Rules ", &app.theme));
    let mut state = TableState::default().with_selected(Some(app.rule_index));
    frame.render_stateful_widget(table, area, &mut state);
}
