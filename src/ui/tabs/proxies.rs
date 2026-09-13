use super::super::layout::proxy_columns;
use super::super::widgets::{fit_column, focus_panel, selection_style, strip_vs16};
use crate::app::App;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{HighlightSpacing, List, ListItem, ListState},
};

/// Nodes of the selected group surviving the substring filter, with their
/// original indices so the cursor, mouse regions, delay tests and selection
/// all address the same rows.
pub(crate) fn filtered_nodes(app: &App) -> Vec<(usize, &String)> {
    let Some((_, group)) = app.selected_group() else {
        return Vec::new();
    };
    let query = app.node_query.to_lowercase();
    group
        .all
        .iter()
        .enumerate()
        .filter(|(_, name)| query.is_empty() || name.to_lowercase().contains(&query))
        .map(|(index, name)| (index, name))
        .collect()
}

pub(crate) fn proxies(frame: &mut Frame, app: &App, area: Rect) {
    let columns = proxy_columns(area);
    let groups = app.proxy_groups();
    let row_width = columns[0].width.saturating_sub(6) as usize;
    let proxy_width = 10.min(row_width.saturating_sub(2) / 2);
    let name_width = row_width.saturating_sub(proxy_width + 1);
    let group_items: Vec<_> = groups
        .iter()
        .map(|(name, proxy)| {
            ListItem::new(Line::from(vec![
                Span::raw(fit_column(&strip_vs16(name), name_width, false)),
                Span::raw(" "),
                Span::styled(
                    fit_column(&strip_vs16(&proxy.now), proxy_width, true),
                    Style::default().fg(app.theme.muted),
                ),
            ]))
        })
        .collect();
    let mut group_state = ListState::default().with_selected(Some(app.group_index));
    let group_focused = !app.node_focus;
    frame.render_stateful_widget(
        List::new(group_items)
            .highlight_symbol("▎ ")
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(selection_style(group_focused, &app.theme))
            .block(focus_panel("Proxy groups ", group_focused, &app.theme)),
        columns[0],
        &mut group_state,
    );

    let node_row_width = columns[1].width.saturating_sub(6) as usize;
    let delay_width = 8.min(node_row_width.saturating_sub(6) / 3);
    let node_name_width = node_row_width.saturating_sub(delay_width + 5);
    let view = filtered_nodes(app);
    let group_now = app
        .selected_group()
        .map(|(_, group)| group.now.clone())
        .unwrap_or_default();
    let nodes: Vec<_> = view
        .iter()
        .map(|(_, name)| {
            let proxy = app.snapshot.proxies.proxies.get(*name);
            let delay = proxy
                .and_then(|p| p.history.last())
                .map_or_else(|| "—".into(), |h| format!("{} ms", h.delay));
            let (alive, alive_color) = match proxy.and_then(|p| p.alive) {
                Some(true) => ("●", app.theme.success),
                Some(false) => ("×", app.theme.danger),
                None => (" ", app.theme.muted),
            };
            let active = if group_now == **name { "✓" } else { " " };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{active} "), Style::default().fg(app.theme.accent)),
                Span::raw(fit_column(&strip_vs16(name), node_name_width, false)),
                Span::raw(" "),
                Span::styled(alive, Style::default().fg(alive_color)),
                Span::raw(" "),
                Span::styled(
                    fit_column(&delay, delay_width, true),
                    Style::default().fg(app.theme.muted),
                ),
            ]))
        })
        .collect();
    let mut node_state = ListState::default().with_selected(Some(app.node_index));
    let node_focused = app.node_focus;
    let node_title = if !app.node_query.is_empty() {
        let total = app
            .selected_group()
            .map_or(0, |(_, group)| group.all.len());
        format!("Nodes · {}/{} ", view.len(), total)
    } else {
        match app.selected_group() {
            Some((_, group)) if !group.kind.eq_ignore_ascii_case("selector") => {
                "Nodes · automatic ".to_owned()
            }
            _ => "Nodes ".to_owned(),
        }
    };
    frame.render_stateful_widget(
        List::new(nodes)
            .highlight_symbol("▎ ")
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(selection_style(node_focused, &app.theme))
            .block(focus_panel(&node_title, node_focused, &app.theme)),
        columns[1],
        &mut node_state,
    );
}

#[cfg(test)]
mod tests {
    use super::super::super::widgets::fit_column;

    #[test]
    fn fits_proxy_group_columns_by_display_width() {
        assert_eq!(fit_column("Group", 7, false), "Group  ");
        assert_eq!(fit_column("Node", 7, true), "   Node");
        assert_eq!(fit_column("节点选择", 6, false), "节点… ");
    }
}
