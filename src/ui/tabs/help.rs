use super::super::widgets::{help_binding, inset_panel, panel, section_line};
use crate::app::App;
use crate::theme::Theme;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    text::Line,
    widgets::{Clear, Paragraph},
};

use super::super::layout::centered;

pub(crate) fn help(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(panel(" Keyboard shortcuts ", &app.theme), area);
    render_help_columns(frame, area.inner(Margin::new(2, 2)), &app.theme);
}

pub(crate) fn draw_help_overlay(frame: &mut Frame, theme: &Theme) {
    let height = frame.area().height.saturating_sub(4).clamp(8, 22);
    let area = centered(76, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        panel(" Keyboard shortcuts  ·  Esc to close ", theme)
            .border_style(Style::default().fg(theme.accent)),
        area,
    );
    render_help_columns(frame, area.inner(Margin::new(2, 2)), theme);
}

fn render_help_columns(frame: &mut Frame, area: Rect, theme: &Theme) {
    let columns = Layout::horizontal([Constraint::Ratio(1, 2); 2]).split(area);
    let navigation = vec![
        section_line("NAVIGATION", theme),
        help_binding("1–8", "Open page", theme),
        help_binding("↑↓ / jk", "Move selection", theme),
        help_binding("Tab", "Switch proxy pane", theme),
        help_binding("←→ / hl", "Switch proxy pane", theme),
        Line::from(""),
        section_line("GLOBAL", theme),
        help_binding("r", "Refresh data", theme),
        help_binding("?", "Toggle shortcuts", theme),
        help_binding("Esc", "Close dialog", theme),
        help_binding("q", "Quit", theme),
        help_binding("Ctrl-C", "Quit", theme),
    ];
    let actions = vec![
        section_line("CONTEXTUAL ACTIONS", theme),
        help_binding("Enter", "Activate selection", theme),
        help_binding("s", "Start / stop core", theme),
        help_binding("m", "Cycle routing mode", theme),
        help_binding("d", "Test node delay", theme),
        help_binding("a", "Import profile", theme),
        help_binding("u", "Update profile", theme),
        help_binding("e", "Update settings", theme),
        help_binding("D", "Delete profile", theme),
        help_binding("x / X", "Close one / all connections", theme),
        help_binding("p", "Update providers", theme),
        help_binding("b / R", "Backup / restore", theme),
        help_binding("u / U", "Check core update", theme),
        help_binding("o", "Open releases", theme),
        Line::from(""),
        section_line("MOUSE", theme),
        help_binding("Click", "Focus item", theme),
        help_binding("Double", "Activate item", theme),
        help_binding("Wheel", "Move selection", theme),
    ];
    frame.render_widget(Paragraph::new(navigation), inset_panel(columns[0]));
    frame.render_widget(Paragraph::new(actions), inset_panel(columns[1]));
}
