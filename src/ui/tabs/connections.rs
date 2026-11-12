use super::super::widgets::{bytes, panel, selection_style, speed, strip_vs16};
use crate::api::Connection;
use crate::app::{App, ConnectionFilter, ConnectionSort};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Cell, Row, Table, TableState},
};
use std::cmp::Reverse;
use std::collections::HashSet;

/// Connections in the current display order. The table, the detail popup
/// and `x` all index this same view.
pub(crate) fn sorted_connections(app: &App) -> Vec<&Connection> {
    let active = &app.data.snapshot.connections.connections;
    let closed = &app.data.closed_connections;
    let mut rows: Vec<&Connection> = match app.ui.connection_filter {
        ConnectionFilter::Active => active.iter().collect(),
        ConnectionFilter::All => closed.iter().chain(active.iter()).collect(),
        ConnectionFilter::Closed => closed.iter().collect(),
    };
    match app.ui.connection_sort {
        ConnectionSort::Original => {}
        ConnectionSort::StartDesc => rows.sort_by_key(|c| Reverse(start_millis(c))),
        ConnectionSort::UploadDesc => rows.sort_by_key(|c| Reverse(c.upload)),
        ConnectionSort::DownloadDesc => rows.sort_by_key(|c| Reverse(c.download)),
        ConnectionSort::TrafficDesc => {
            rows.sort_by_key(|c| Reverse(c.upload.saturating_add(c.download)));
        }
        ConnectionSort::Host => rows.sort_by_key(|c| {
            let host = target(c).to_lowercase();
            (host.is_empty(), host)
        }),
        ConnectionSort::Chain => rows.sort_by_key(|c| {
            let chain = c.chain_path().to_lowercase();
            (chain.is_empty(), chain)
        }),
    }
    rows
}

fn start_millis(connection: &Connection) -> i64 {
    chrono::DateTime::parse_from_rfc3339(&connection.start)
        .map(|time| time.timestamp_millis())
        .unwrap_or(0)
}

/// Destination shown in the table: the sniffed/requested host when there
/// is one, otherwise the IP.
fn target(connection: &Connection) -> &str {
    if connection.metadata.host.is_empty() {
        &connection.metadata.destination_ip
    } else {
        &connection.metadata.host
    }
}

pub(crate) fn connections(frame: &mut Frame, app: &App, area: Rect) {
    let closed_ids: HashSet<&str> = app
        .data
        .closed_connections
        .iter()
        .map(|connection| connection.id.as_str())
        .collect();
    let rows = sorted_connections(app).into_iter().map(|connection| {
        let target = target(connection);
        let rule = if connection.rule_payload.is_empty() {
            connection.rule.clone()
        } else {
            format!("{} {}", connection.rule, connection.rule_payload)
        };
        let rate = app
            .data
            .connection_rates
            .get(&connection.id)
            .copied()
            .unwrap_or((0, 0));
        let closed = closed_ids.contains(connection.id.as_str());
        let destination = format!(
            "{}:{}",
            strip_vs16(target),
            connection.metadata.destination_port
        );
        let mut row = Row::new(vec![
            Cell::from(if closed {
                format!("× {destination}")
            } else {
                destination
            }),
            Cell::from(
                format!(
                    "{} {}",
                    connection.metadata.network, connection.metadata.kind
                )
                .to_uppercase(),
            ),
            Cell::from(strip_vs16(&rule).into_owned()),
            Cell::from(strip_vs16(&connection.chain_path()).into_owned()),
            Cell::from(format!(
                "↑{} ↓{}",
                bytes(connection.upload),
                bytes(connection.download)
            )),
            Cell::from(format!("↑{} ↓{}", speed(rate.0), speed(rate.1))),
        ]);
        if closed {
            row = row.style(
                Style::default()
                    .fg(app.theme.muted)
                    .add_modifier(Modifier::DIM),
            );
        }
        row
    });
    let widths = [
        Constraint::Percentage(24),
        Constraint::Length(8),
        Constraint::Percentage(16),
        Constraint::Percentage(18),
        Constraint::Percentage(15),
        Constraint::Percentage(19),
    ];
    // While armed, advertise the pending choices instead of the active
    // order, mirroring the Proxies node panel.
    let sort_label = if app.ui.connection_sort_prefix {
        " · t/u/d/T/h/c/o ".to_owned()
    } else {
        format!(" · {}", app.ui.connection_sort.label())
    };
    let heading = match app.ui.connection_filter {
        ConnectionFilter::Active => "Active connections",
        ConnectionFilter::All => "Connections",
        ConnectionFilter::Closed => "Closed connections",
    };
    let title = format!("{heading}{sort_label} ");
    let table = Table::new(rows, widths)
        .header(
            Row::new(["Destination", "Network", "Rule", "Chain", "Traffic", "Rate"])
                .style(
                    Style::default()
                        .fg(app.theme.muted)
                        .add_modifier(Modifier::BOLD),
                )
                .bottom_margin(1),
        )
        .row_highlight_style(selection_style(true, &app.theme))
        .highlight_symbol("▎ ")
        .block(panel(&title, &app.theme));
    let mut state = TableState::default().with_selected(Some(app.ui.connection_index));
    frame.render_stateful_widget(table, area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ConnectionResponse, Metadata, Snapshot};
    use crate::app::{DataState, TaskHub, UiState};
    use crate::config::Config;
    use crate::theme::Theme;

    fn connection(
        id: &str,
        host: &str,
        upload: u64,
        download: u64,
        start: &str,
        chains: &[&str],
    ) -> Connection {
        Connection {
            id: id.into(),
            metadata: Metadata {
                host: host.into(),
                destination_ip: "1.1.1.1".into(),
                ..Default::default()
            },
            chains: chains.iter().map(|name| (*name).to_owned()).collect(),
            upload,
            download,
            start: start.into(),
            ..Default::default()
        }
    }

    fn app_with(connections: Vec<Connection>) -> App {
        App {
            config: Config::default(),
            ui_config: crate::ui::UiConfig::default(),
            api: crate::api::MihomoClient::new("http://127.0.0.1:9090", String::new()).unwrap(),
            remote: false,
            theme: Theme::default(),
            ui: UiState::default(),
            data: DataState {
                snapshot: Snapshot {
                    connections: ConnectionResponse { connections },
                    ..Default::default()
                },
                ..Default::default()
            },
            tasks: TaskHub::default(),
        }
    }

    fn ids(app: &App) -> Vec<&str> {
        sorted_connections(app)
            .into_iter()
            .map(|connection| connection.id.as_str())
            .collect()
    }

    fn sample() -> App {
        app_with(vec![
            connection(
                "old",
                "a.example",
                10,
                20,
                "2026-10-24T10:00:00+08:00",
                &["node", "AUTO"],
            ),
            connection(
                "new",
                "b.example",
                30,
                40,
                "2026-10-24T12:00:00+08:00",
                &["node", "PROXY"],
            ),
            connection("never", "c.example", 0, 0, "", &[]),
        ])
    }

    #[test]
    fn newest_is_the_default_and_undated_rows_sink() {
        let app = sample();
        assert_eq!(app.ui.connection_sort, ConnectionSort::StartDesc);
        assert_eq!(ids(&app), vec!["new", "old", "never"]);
    }

    #[test]
    fn traffic_and_host_modes_order_the_view() {
        let mut app = sample();
        app.ui.connection_sort = ConnectionSort::TrafficDesc;
        assert_eq!(ids(&app), vec!["new", "old", "never"]);

        app.ui.connection_sort = ConnectionSort::UploadDesc;
        assert_eq!(ids(&app), vec!["new", "old", "never"]);

        app.ui.connection_sort = ConnectionSort::Host;
        assert_eq!(ids(&app), vec!["old", "new", "never"]);
    }

    #[test]
    fn filter_switches_between_active_all_and_closed() {
        let mut app = sample();
        app.data.closed_connections = vec![connection(
            "gone",
            "z.example",
            1,
            1,
            "2026-10-24T09:00:00+08:00",
            &["node"],
        )];
        assert_eq!(ids(&app), vec!["new", "old", "never"]);

        app.ui.connection_filter = ConnectionFilter::Closed;
        assert_eq!(ids(&app), vec!["gone"]);

        app.ui.connection_filter = ConnectionFilter::All;
        assert_eq!(ids(&app), vec!["new", "old", "gone", "never"]);
    }

    #[test]
    fn original_keeps_the_core_order() {
        let mut app = sample();
        app.ui.connection_sort = ConnectionSort::Original;
        assert_eq!(ids(&app), vec!["old", "new", "never"]);
    }
}
