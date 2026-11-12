use super::super::layout::{proxy_columns, proxy_view_areas};
use super::super::widgets::{
    fit_column, focus_panel, panel, selection_style, stamp_text, strip_vs16, truncate_tail,
};
use crate::app::{App, ProxyView};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{HighlightSpacing, List, ListItem, ListState, Paragraph, Tabs},
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
    let (delay, alive) = match app.data.snapshot.proxies.proxies.get(name) {
        Some(proxy) => (
            proxy.history.last().map_or(0, |entry| entry.delay),
            proxy.alive,
        ),
        None => match provider_node(app, name) {
            Some(proxy) => (
                proxy.history.last().map_or(0, |entry| entry.delay),
                Some(proxy.alive),
            ),
            None => return (true, 0),
        },
    };
    // mihomo answers a failed probe with `delay: 0`, so a zero is not
    // "instant" — it is the absence of a measurement. `alive: false`
    // marks a node whose last health check failed even if an older
    // delay is still cached.
    let unknown = delay == 0 || alive == Some(false);
    (unknown, if unknown { 0 } else { delay })
}

/// A node owned by a proxy provider. mihomo does not list these in
/// `/proxies`, so their delay/alive state has to come from the provider
/// snapshot instead.
fn provider_node<'a>(app: &'a App, name: &str) -> Option<&'a crate::api::ProviderProxy> {
    app.data
        .snapshot
        .proxy_providers
        .iter()
        .filter(|provider| !provider.is_compatible())
        .find_map(|provider| provider.proxies.iter().find(|proxy| proxy.name == name))
}
/// Real proxy providers for the provider sub-tab, sorted by name.
///
/// `GET /providers/proxies` also lists every proxy group as a
/// pseudo-provider, but the client only ever asks for names the profile
/// declares under `proxy-providers:`; the `compatible` check is a second
/// guard so a group can never reach the list even if it slips through.
pub(crate) fn proxy_providers(app: &App) -> Vec<(String, &crate::api::ProxyProvider)> {
    let mut providers: Vec<_> = app
        .data
        .snapshot
        .proxy_providers
        .iter()
        .filter(|provider| !provider.is_compatible())
        .map(|provider| (provider.name.clone(), provider))
        .filter(|(name, _)| !name.is_empty())
        .collect();
    providers.sort_by_key(|(name, _)| name.to_lowercase());
    providers
}

/// Labels in the Proxies sub-tab strip. Shared with the mouse hit-region
/// helper so clicks stay aligned with the rendered `Tabs`.
pub(crate) const PROXY_VIEW_TITLES: [&str; 2] = ["Groups", "Providers"];

/// Whether the profile declares providers at all. The provider strip is
/// hidden for the common inline `proxies:` profile.
pub(crate) fn has_proxy_providers(app: &App) -> bool {
    !proxy_providers(app).is_empty() || !app.data.proxy_provider_order.is_empty()
}

pub(crate) fn selected_provider(app: &App) -> Option<(String, &crate::api::ProxyProvider)> {
    let providers = proxy_providers(app);
    let index = app.ui.provider_index.min(providers.len().saturating_sub(1));
    providers.into_iter().nth(index)
}

pub(crate) fn selected_provider_nodes(app: &App) -> usize {
    selected_provider(app).map_or(0, |(_, provider)| provider.proxies.len())
}

pub(crate) fn proxies(frame: &mut Frame, app: &App, area: Rect) {
    let has_providers = has_proxy_providers(app);
    let (tabs_area, body) = proxy_view_areas(area, has_providers);
    if let Some(tabs_area) = tabs_area {
        draw_proxy_view_tabs(frame, app, tabs_area);
    }
    match app.ui.proxy_view {
        ProxyView::Groups => draw_groups(frame, app, body),
        ProxyView::Providers if has_providers => draw_providers(frame, app, body),
        ProxyView::Providers => draw_groups(frame, app, body),
    }
}

fn draw_proxy_view_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let selected = usize::from(app.ui.proxy_view == ProxyView::Providers);
    let titles = PROXY_VIEW_TITLES.iter().map(|title| Line::from(*title));
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .divider("  ")
            .padding("", "")
            .style(Style::default().fg(app.theme.muted))
            .highlight_style(
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        area,
    );
}

fn draw_groups(frame: &mut Frame, app: &App, area: Rect) {
    let columns = proxy_columns(area);
    let groups = app.proxy_groups();
    let row_width = columns[0].width.saturating_sub(6) as usize;
    let proxy_width = 10.min(row_width.saturating_sub(2) / 2);
    let name_width = row_width.saturating_sub(proxy_width + 1);
    let group_items: Vec<_> = groups
        .iter()
        .map(|(name, proxy)| {
            let label = if proxy.fixed.is_empty() {
                name.to_string()
            } else {
                format!("*{name}")
            };
            ListItem::new(Line::from(vec![
                Span::raw(fit_column(&strip_vs16(&label), name_width, false)),
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
    let node_name_width = node_row_width.saturating_sub(delay_width + 6);
    let view = filtered_nodes(app);
    let (group_now, group_fixed) = app
        .selected_group()
        .map(|(_, group)| (group.now.clone(), group.fixed.clone()))
        .unwrap_or_default();
    let nodes: Vec<_> = view
        .iter()
        .map(|(_, name)| {
            let proxy = app.data.snapshot.proxies.proxies.get(*name);
            let provider_proxy = provider_node(app, name);
            let delay = proxy
                .and_then(|p| p.history.last())
                .or_else(|| provider_proxy.and_then(|p| p.history.last()))
                .map_or_else(|| "—".into(), |h| format!("{} ms", h.delay));
            let alive_flag = proxy
                .and_then(|p| p.alive)
                .or_else(|| provider_proxy.map(|p| p.alive));
            let (alive, alive_color) = match alive_flag {
                Some(true) => ("●", app.theme.success),
                Some(false) => ("×", app.theme.danger),
                None => (" ", app.theme.muted),
            };
            let active = if group_now == **name { "✓" } else { " " };
            let fixed = if group_fixed == **name { "*" } else { " " };
            ListItem::new(Line::from(vec![
                Span::styled(active, Style::default().fg(app.theme.accent)),
                Span::styled(fixed, Style::default().fg(app.theme.warning)),
                Span::raw(" "),
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

fn draw_providers(frame: &mut Frame, app: &App, area: Rect) {
    let providers = proxy_providers(app);
    let columns = proxy_columns(area);
    let provider_focused = !app.ui.node_focus;
    let row_width = columns[0].width.saturating_sub(6) as usize;
    let count_width = 9.min(row_width.saturating_sub(2) / 2);
    let name_width = row_width.saturating_sub(count_width + 1);
    let items: Vec<_> = providers
        .iter()
        .map(|(name, provider)| {
            let count = format!("{}/{}", provider.alive_count(), provider.proxies.len());
            ListItem::new(Line::from(vec![
                Span::raw(fit_column(&strip_vs16(name), name_width, false)),
                Span::raw(" "),
                Span::styled(
                    fit_column(&count, count_width, true),
                    Style::default().fg(app.theme.muted),
                ),
            ]))
        })
        .collect();
    let selected = app.ui.provider_index.min(providers.len().saturating_sub(1));
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("▎ ")
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(selection_style(provider_focused, &app.theme))
            .block(focus_panel(
                &format!("Providers · {} ", providers.len()),
                provider_focused,
                &app.theme,
            )),
        columns[0],
        &mut state,
    );

    let node_focused = app.ui.node_focus;
    let Some((name, provider)) = providers
        .get(selected)
        .map(|(name, provider)| (name.clone(), *provider))
    else {
        frame.render_widget(panel("Provider nodes ", &app.theme), columns[1]);
        return;
    };
    let right = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(columns[1]);
    let vehicle = if provider.vehicle_type.is_empty() {
        "?"
    } else {
        provider.vehicle_type.as_str()
    };
    let test_url = if provider.test_url.is_empty() {
        "—"
    } else {
        provider.test_url.as_str()
    };
    let meta = vec![
        Line::from(vec![
            Span::styled("test ", Style::default().fg(app.theme.muted)),
            Span::raw(truncate_tail(
                test_url,
                right[0].width.saturating_sub(6) as usize,
            )),
        ]),
        Line::from(vec![
            Span::styled("updated ", Style::default().fg(app.theme.muted)),
            Span::raw(stamp_text(&provider.updated_at)),
            Span::raw(format!(
                " · {vehicle} · {}/{} alive",
                provider.alive_count(),
                provider.proxies.len()
            )),
        ]),
    ];
    frame.render_widget(Paragraph::new(meta), right[0]);

    let node_row_width = right[1].width.saturating_sub(6) as usize;
    let delay_width = 8.min(node_row_width.saturating_sub(6) / 3);
    let name_width = node_row_width.saturating_sub(delay_width + 3);
    let nodes: Vec<_> = provider
        .proxies
        .iter()
        .map(|proxy| {
            let delay = proxy
                .history
                .last()
                .map_or_else(|| "—".into(), |entry| format!("{} ms", entry.delay));
            let (alive, color) = if proxy.alive {
                ("●", app.theme.success)
            } else {
                ("×", app.theme.danger)
            };
            ListItem::new(Line::from(vec![
                Span::styled(alive, Style::default().fg(color)),
                Span::raw(" "),
                Span::raw(fit_column(&strip_vs16(&proxy.name), name_width, false)),
                Span::raw(" "),
                Span::styled(
                    fit_column(&delay, delay_width, true),
                    Style::default().fg(app.theme.muted),
                ),
            ]))
        })
        .collect();
    let selected_node = app
        .ui
        .provider_node_index
        .min(provider.proxies.len().saturating_sub(1));
    let mut node_state = ListState::default().with_selected(Some(selected_node));
    frame.render_stateful_widget(
        List::new(nodes)
            .highlight_symbol("▎ ")
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(selection_style(node_focused, &app.theme))
            .block(focus_panel(
                &format!("Nodes · {name} "),
                node_focused,
                &app.theme,
            )),
        right[1],
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

    fn app_with_providers(providers: Vec<crate::api::ProxyProvider>) -> App {
        App {
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
                    proxy_providers: providers,
                    ..Default::default()
                },
                ..Default::default()
            },
            tasks: TaskHub::default(),
        }
    }

    fn provider(
        name: &str,
        vehicle: &str,
        alive: usize,
        total: usize,
    ) -> crate::api::ProxyProvider {
        crate::api::ProxyProvider {
            name: name.into(),
            vehicle_type: vehicle.into(),
            proxies: (0..total)
                .map(|index| crate::api::ProviderProxy {
                    name: format!("{name}-{index}"),
                    alive: index < alive,
                    ..Default::default()
                })
                .collect(),
            test_url: "http://example.test".into(),
            updated_at: "2026-09-12T10:02:11Z".into(),
        }
    }

    /// mihomo exposes every proxy group as a `compatible` pseudo-provider.
    /// Showing those would just re-render the group list below, so the
    /// panel must drop them.
    #[test]
    fn provider_panel_skips_proxy_groups_reexposed_as_providers() {
        let app = app_with_providers(vec![
            provider("sub", "HTTP", 1, 2),
            provider("Netflix", "Compatible", 2, 2),
        ]);
        let shown: Vec<_> = proxy_providers(&app)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(shown, vec!["sub".to_string()]);
    }

    fn row_text(terminal: &ratatui::Terminal<ratatui::backend::TestBackend>, y: u16) -> String {
        let buffer = terminal.backend().buffer().clone();
        (0..90)
            .map(|x| buffer.cell((x, y)).unwrap().symbol().to_owned())
            .collect()
    }

    #[test]
    fn provider_sub_tab_replaces_the_old_header_panel() {
        let mut app = app_with_providers(vec![
            provider("sub", "HTTP", 1, 3),
            provider("Netflix", "Compatible", 2, 2),
        ]);
        app.data.profiles.items.clear();
        app.data.snapshot.proxies.proxies.insert(
            "AI".into(),
            Proxy {
                kind: "Selector".into(),
                now: "n".into(),
                all: vec!["n".into()],
                ..Default::default()
            },
        );
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 20)).unwrap();
        terminal
            .draw(|frame| proxies(frame, &app, Rect::new(0, 0, 90, 20)))
            .unwrap();

        let all: String = (0..20).map(|y| row_text(&terminal, y)).collect();
        assert!(all.contains("Groups"), "{all}");
        assert!(all.contains("Providers"), "{all}");
        assert!(all.contains("Proxy groups"), "{all}");
        assert!(all.contains("AI"), "{all}");
        assert!(!all.contains("Proxy providers"), "{all}");
        assert!(!all.contains("Netflix"), "{all}");
    }

    #[test]
    fn provider_view_shows_provider_metadata_and_nodes() {
        let mut app = app_with_providers(vec![
            provider("sub", "HTTP", 1, 3),
            provider("Netflix", "Compatible", 2, 2),
        ]);
        app.data.profiles.items.clear();
        app.ui.proxy_view = ProxyView::Providers;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 20)).unwrap();
        terminal
            .draw(|frame| proxies(frame, &app, Rect::new(0, 0, 90, 20)))
            .unwrap();

        let all: String = (0..20).map(|y| row_text(&terminal, y)).collect();
        assert!(all.contains("Providers · 1"), "{all}");
        assert!(all.contains("sub"), "{all}");
        assert!(all.contains("test http://example.test"), "{all}");
        assert!(all.contains("HTTP · 1/3 alive"), "{all}");
        assert!(all.contains("Nodes · sub"), "{all}");
        assert!(all.contains("sub-0"), "{all}");
        assert!(!all.contains("Netflix"), "{all}");
    }

    #[test]
    fn no_provider_sub_tab_when_the_profile_declares_none() {
        let mut app = app_with_providers(vec![]);
        app.data.profiles.items.clear();
        app.ui.proxy_view = ProxyView::Providers;
        app.data.snapshot.proxies.proxies.insert(
            "AI".into(),
            Proxy {
                kind: "Selector".into(),
                now: "n".into(),
                all: vec!["n".into()],
                ..Default::default()
            },
        );
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 20)).unwrap();
        terminal
            .draw(|frame| proxies(frame, &app, Rect::new(0, 0, 90, 20)))
            .unwrap();
        let all: String = (0..20).map(|y| row_text(&terminal, y)).collect();
        assert!(!all.contains("Providers"), "{all}");
        assert!(row_text(&terminal, 0).contains("Proxy groups"), "{all}");
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
                        fixed: String::new(),
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

    #[test]
    fn fixed_url_test_node_and_group_show_a_pin_marker() {
        let mut app = app_with_delays(
            &["a", "b"],
            &[Some(10), Some(20)],
            &[Some(true), Some(true)],
        );
        let group = app.data.snapshot.proxies.proxies.get_mut("a").unwrap();
        group.kind = "URLTest".into();
        group.fixed = "b".into();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 20)).unwrap();
        terminal
            .draw(|frame| proxies(frame, &app, Rect::new(0, 0, 90, 20)))
            .unwrap();
        let all: String = (0..20).map(|y| row_text(&terminal, y)).collect();
        assert!(all.contains("*a"), "group pin missing: {all}");
        assert!(all.contains("* b"), "node pin missing: {all}");
    }

    /// Provider nodes are absent from `/proxies`, so their delay and
    /// alive flag must be read from the provider snapshot instead.
    #[test]
    fn provider_nodes_show_and_sort_by_their_own_delay() {
        let mut app = app_with_providers(vec![crate::api::ProxyProvider {
            name: "sub".into(),
            vehicle_type: "HTTP".into(),
            proxies: vec![
                crate::api::ProviderProxy {
                    name: "slow".into(),
                    kind: "Shadowsocks".into(),
                    alive: true,
                    history: vec![crate::api::DelayHistory { delay: 400 }],
                },
                crate::api::ProviderProxy {
                    name: "fast".into(),
                    kind: "Shadowsocks".into(),
                    alive: true,
                    history: vec![crate::api::DelayHistory { delay: 120 }],
                },
            ],
            ..Default::default()
        }]);
        app.data.profiles.items.clear();
        app.data.snapshot.proxies.proxies.insert(
            "g".into(),
            Proxy {
                kind: "Selector".into(),
                now: "slow".into(),
                all: vec!["slow".into(), "fast".into()],
                ..Default::default()
            },
        );
        app.ui.node_sort = NodeSort::DelayAsc;
        assert_eq!(node_names(&app), vec!["fast", "slow"]);

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 20)).unwrap();
        terminal
            .draw(|frame| proxies(frame, &app, Rect::new(0, 0, 90, 20)))
            .unwrap();
        let all: String = (0..20).map(|y| row_text(&terminal, y)).collect();
        assert!(all.contains("120 ms"), "{all}");
        assert!(all.contains("400 ms"), "{all}");
    }
}
