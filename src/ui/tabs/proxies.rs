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
    let query = app.ui.node_query.to_lowercase();
    let mut rows: Vec<_> = group
        .all
        .iter()
        .enumerate()
        .filter(|(_, name)| query.is_empty() || name.to_lowercase().contains(&query))
        .collect();
    match app.ui.node_sort {
        crate::app::NodeSort::Original => {}
        // Unknown nodes (untested, dead, or the core's `0 ms` for an
        // unreachable node) sink below every measured one in both
        // directions, so the first row is always a node that answered.
        crate::app::NodeSort::DelayAsc => {
            rows.sort_by(|a, b| compare_delay(app, a.1, b.1, false).then_with(|| a.0.cmp(&b.0)));
        }
        crate::app::NodeSort::DelayDesc => {
            rows.sort_by(|a, b| compare_delay(app, a.1, b.1, true).then_with(|| a.0.cmp(&b.0)));
        }
        crate::app::NodeSort::Name => {
            rows.sort_by(|a, b| {
                a.1.to_lowercase()
                    .cmp(&b.1.to_lowercase())
                    .then_with(|| a.0.cmp(&b.0))
            });
        }
    }
    rows
}

/// Order two nodes by measured delay. `descending` reverses only the
/// delay, never the unknown flag: unknown nodes stay at the bottom in
/// both directions (reversing the flag would float them to the top,
/// which is where a dead node must never be).
fn compare_delay(app: &App, a: &str, b: &str, descending: bool) -> std::cmp::Ordering {
    let (unknown_a, delay_a) = sort_key(app, a);
    let (unknown_b, delay_b) = sort_key(app, b);
    unknown_a.cmp(&unknown_b).then_with(|| {
        if descending {
            delay_b.cmp(&delay_a)
        } else {
            delay_a.cmp(&delay_b)
        }
    })
}

/// `(is_unknown, delay)` sort key. `delay` is 0 for unknown nodes so the
/// tuple alone decides the direction; the flag keeps them below every
/// measured node regardless of ascending/descending.
fn sort_key(app: &App, name: &str) -> (bool, u32) {
    let Some(proxy) = app.data.snapshot.proxies.proxies.get(name) else {
        return (true, 0);
    };
    // mihomo answers a failed probe with `delay: 0`, so a zero is not
    // "instant" — it is the absence of a measurement. `alive: false`
    // marks a node whose last health check failed even if an older
    // delay is still cached.
    let delay = proxy.history.last().map_or(0, |entry| entry.delay);
    let unknown = delay == 0 || proxy.alive == Some(false);
    (unknown, if unknown { 0 } else { delay })
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
    let mut group_state = ListState::default().with_selected(Some(app.ui.group_index));
    let group_focused = !app.ui.node_focus;
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
            let proxy = app.data.snapshot.proxies.proxies.get(*name);
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
    let mut node_state = ListState::default().with_selected(Some(app.ui.node_index));
    let node_focused = app.ui.node_focus;
    // While armed, the title advertises the pending choices instead of
    // the active order; once a mode lands, the title shows the mode.
    let sort_label = if app.ui.node_sort_prefix {
        " · d/D/n/o ".to_owned()
    } else {
        format!(" · {}", app.ui.node_sort.label())
    };
    let node_title = if !app.ui.node_query.is_empty() {
        let total = app.selected_group().map_or(0, |(_, group)| group.all.len());
        format!("Nodes · {}/{total}{sort_label} ", view.len())
    } else {
        match app.selected_group() {
            Some((_, group)) if !group.kind.eq_ignore_ascii_case("selector") => {
                format!("Nodes · automatic{sort_label} ")
            }
            _ => format!("Nodes{sort_label} "),
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
    use super::*;
    use crate::api::{MihomoClient, Proxy, ProxyResponse, Snapshot};
    use crate::app::{App, DataState, NodeSort, TaskHub, UiState};
    use crate::config::Config;
    use crate::theme::Theme;
    use std::collections::HashMap;

    #[test]
    fn fits_proxy_group_columns_by_display_width() {
        assert_eq!(fit_column("Group", 7, false), "Group  ");
        assert_eq!(fit_column("Node", 7, true), "   Node");
        assert_eq!(fit_column("节点选择", 6, false), "节点… ");
    }

    /// App with one group whose nodes carry the given delays in ms
    /// (`None` = never tested) and the given `alive` flags.
    fn app_with_delays(names: &[&str], delays: &[Option<u32>], alive: &[Option<bool>]) -> App {
        let proxies = names
            .iter()
            .zip(delays)
            .zip(alive)
            .map(|((name, delay), flag)| {
                (
                    (*name).to_owned(),
                    Proxy {
                        kind: "Selector".into(),
                        now: names[0].into(),
                        all: names.iter().map(|n| (*n).to_owned()).collect(),
                        history: delay
                            .map(|delay| vec![crate::api::DelayHistory { delay }])
                            .unwrap_or_default(),
                        alive: *flag,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let mut app = App {
            config: Config::default(),
            api: MihomoClient::new("http://127.0.0.1:9090", String::new()).unwrap(),
            remote: false,
            theme: Theme::default(),
            ui: UiState {
                tab: crate::app::Tab::Proxies,
                ..Default::default()
            },
            data: DataState {
                snapshot: Snapshot {
                    proxies: ProxyResponse { proxies },
                    ..Default::default()
                },
                ..Default::default()
            },
            tasks: TaskHub::default(),
        };
        app.data.profiles.items.clear();
        app
    }

    fn node_names(app: &App) -> Vec<String> {
        filtered_nodes(app)
            .into_iter()
            .map(|(_, name)| (*name).clone())
            .collect()
    }

    #[test]
    fn delay_sort_puts_fastest_first_and_untested_last() {
        let mut app = app_with_delays(
            &["a", "b", "c"],
            &[Some(300), None, Some(120)],
            &[None, None, None],
        );
        app.ui.node_sort = NodeSort::DelayAsc;
        assert_eq!(node_names(&app), vec!["c", "a", "b"]);
    }

    #[test]
    fn delay_sort_desc_is_the_reverse_but_keeps_untested_last() {
        let mut app = app_with_delays(
            &["a", "b", "c"],
            &[Some(300), None, Some(120)],
            &[None, None, None],
        );
        app.ui.node_sort = NodeSort::DelayDesc;
        assert_eq!(node_names(&app), vec!["a", "c", "b"]);
    }

    /// mihomo reports `delay: 0` for a node it could not reach. Sorting
    /// it as "0 ms is fastest" would pin a dead node to the top.
    #[test]
    fn zero_delay_and_dead_nodes_sink_below_measured_ones() {
        let mut app = app_with_delays(
            &["zero", "dead", "fast", "slow"],
            &[Some(0), Some(50), Some(120), Some(400)],
            &[None, Some(false), None, None],
        );
        app.ui.node_sort = NodeSort::DelayAsc;
        // The two unknown nodes tie, so the original order breaks the tie.
        assert_eq!(node_names(&app), vec!["fast", "slow", "zero", "dead"]);
        app.ui.node_sort = NodeSort::DelayDesc;
        assert_eq!(node_names(&app), vec!["slow", "fast", "zero", "dead"]);
    }

    #[test]
    fn name_sort_is_case_insensitive() {
        let mut app = app_with_delays(
            &["Beta", "alpha", "c"],
            &[Some(10), None, Some(20)],
            &[None, None, None],
        );
        app.ui.node_sort = NodeSort::Name;
        assert_eq!(node_names(&app), vec!["alpha", "Beta", "c"]);
    }
}
