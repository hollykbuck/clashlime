use super::super::widgets::panel;
use crate::app::App;
use ratatui::{Frame, layout::Rect, widgets::{List, ListItem}};

pub(crate) fn logs(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<_> = app
        .logs
        .iter()
        .map(|line| ListItem::new(line.as_str()))
        .collect();
    frame.render_widget(
        List::new(items).block(panel(" Mihomo logs ", &app.theme)),
        area,
    );
}
