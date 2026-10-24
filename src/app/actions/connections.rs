//! Connections tab interactions: the detail popup opened with Enter.
//! Closing (`x` / `X`) lives in `proxy.rs` next to the API calls.

use crate::api::Connection;
use crate::ui::widgets::{bytes, speed, strip_vs16};

impl crate::app::App {
    /// Handle a key while the `,` sort prefix is armed on Connections.
    /// Returns `None` when the key was not a sort choice, so the caller
    /// can fall through to its normal handling.
    pub(crate) fn handle_connection_sort_prefix_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<()> {
        use crate::app::ConnectionSort;
        use crossterm::event::KeyCode;
        self.ui.connection_sort_prefix = false;
        self.ui.connection_sort = match key.code {
            KeyCode::Char('t') => ConnectionSort::StartDesc,
            KeyCode::Char('u' | 'U') => ConnectionSort::UploadDesc,
            KeyCode::Char('d' | 'D') => ConnectionSort::DownloadDesc,
            KeyCode::Char('T') => ConnectionSort::TrafficDesc,
            KeyCode::Char('h' | 'H') => ConnectionSort::Host,
            KeyCode::Char('c' | 'C') => ConnectionSort::Chain,
            KeyCode::Char('o' | 'O') => ConnectionSort::Original,
            _ => return None,
        };
        // The cursor addresses a row, not a connection: reset it and let
        // the caller re-clamp against the new ordering.
        self.ui.connection_index = 0;
        self.say(format!(
            "Connections sorted by {}",
            self.ui.connection_sort.label()
        ));
        Some(())
    }

    /// Cycle the Connections view: active -> all -> closed. Closed rows
    /// come from the local history, since mihomo forgets them on close.
    pub(crate) fn cycle_connection_filter(&mut self) {
        self.ui.connection_filter = self.ui.connection_filter.next();
        self.ui.connection_index = 0;
        self.say(format!(
            "Connections: {}",
            self.ui.connection_filter.label()
        ));
    }

    /// Open the selected connection in the wrapped detail popup (shared
    /// with the Logs/Rules tabs; Esc closes it).
    pub(crate) fn open_connection_detail(&mut self) {
        let Some(connection) = crate::ui::tabs::connections::sorted_connections(self)
            .get(self.ui.connection_index)
            .copied()
        else {
            return;
        };
        let rate = self.data.connection_rates.get(&connection.id).copied();
        let (id, detail) = (connection.id.clone(), format_connection(connection, rate));
        self.ui.detail_scroll = 0;
        self.ui.detail_connection = Some(id);
        self.ui.log_detail = Some(detail);
    }

    /// Text for the shared detail popup. A connection opened from the
    /// Connections tab is rebuilt from the current snapshot so its rate
    /// rows keep updating; logs/rules keep their captured text.
    pub(crate) fn detail_text(&self) -> Option<String> {
        if let Some(id) = self.ui.detail_connection.as_deref()
            && let Some(connection) = self
                .data
                .snapshot
                .connections
                .connections
                .iter()
                .chain(self.data.closed_connections.iter())
                .find(|connection| connection.id.as_str() == id)
        {
            return Some(format_connection(
                connection,
                self.data.connection_rates.get(id).copied(),
            ));
        }
        self.ui.log_detail.clone()
    }
}

/// Label every non-empty field of one connection so the popup reads as a
/// structured record rather than a wall of text. Optional fields
/// (process, rules, chain, …) drop out when the core did not report them.
fn format_connection(connection: &Connection, rate: Option<(u64, u64)>) -> String {
    let metadata = &connection.metadata;
    let mut rows: Vec<(String, String)> = Vec::new();

    // Show the requested host separately from the resolved destination
    // when mihomo reports both, so a domain rule is not hidden behind
    // its IP (the dashboard keeps the two columns apart as well).
    if !metadata.host.is_empty() && !metadata.destination_ip.is_empty() {
        rows.push((
            "Host".into(),
            join_host_port(&metadata.host, &metadata.destination_port),
        ));
    }
    let destination = if metadata.destination_ip.is_empty() {
        &metadata.host
    } else {
        &metadata.destination_ip
    };
    if !destination.is_empty() || !metadata.destination_port.is_empty() {
        rows.push((
            "Destination".into(),
            join_host_port(destination, &metadata.destination_port),
        ));
    }
    if !metadata.source_ip.is_empty() || !metadata.source_port.is_empty() {
        rows.push((
            "Source".into(),
            join_host_port(&metadata.source_ip, &metadata.source_port),
        ));
    }
    if let Some(kind) = destination_type(connection) {
        rows.push(("Destination type".into(), kind.into()));
    }
    if !metadata.remote_destination.is_empty() {
        rows.push(("Remote address".into(), metadata.remote_destination.clone()));
    }
    if !metadata.source_geo_ip.is_empty() {
        rows.push(("Source GeoIP".into(), metadata.source_geo_ip.join(", ")));
    }
    if !metadata.destination_geo_ip.is_empty() {
        rows.push((
            "Destination GeoIP".into(),
            metadata.destination_geo_ip.join(", "),
        ));
    }
    if !metadata.source_ip_asn.is_empty() {
        rows.push(("Source ASN".into(), metadata.source_ip_asn.clone()));
    }
    if !metadata.destination_ip_asn.is_empty() {
        rows.push((
            "Destination ASN".into(),
            metadata.destination_ip_asn.clone(),
        ));
    }
    // The sniffed host is worth surfacing separately: it is what the
    // rules match on when the client sent no SNI/authority.
    if !metadata.sniff_host.is_empty() && metadata.sniff_host != metadata.host {
        rows.push(("Sniffed host".into(), metadata.sniff_host.clone()));
    }
    let network = format!("{} {}", metadata.network, metadata.kind)
        .trim()
        .to_uppercase();
    if !network.is_empty() {
        rows.push(("Network".into(), network));
    }
    let mut process = metadata.process.clone();
    if metadata.uid != 0 {
        process = if process.is_empty() {
            format!("uid {}", metadata.uid)
        } else {
            format!("{process} (uid {})", metadata.uid)
        };
    }
    if !process.is_empty() {
        rows.push(("Process".into(), process));
    }
    if !metadata.process_path.is_empty() {
        rows.push(("Process path".into(), metadata.process_path.clone()));
    }
    if !metadata.dns_mode.is_empty() {
        rows.push(("DNS mode".into(), metadata.dns_mode.clone()));
    }
    if !metadata.inbound_name.is_empty() || !metadata.inbound_user.is_empty() {
        let inbound = match (
            metadata.inbound_name.is_empty(),
            metadata.inbound_user.is_empty(),
        ) {
            (false, false) => format!("{} ({})", metadata.inbound_name, metadata.inbound_user),
            (false, true) => metadata.inbound_name.clone(),
            (true, false) => metadata.inbound_user.clone(),
            (true, true) => String::new(),
        };
        rows.push(("Inbound".into(), inbound));
    }
    if !metadata.inbound_ip.is_empty() || !metadata.inbound_port.is_empty() {
        rows.push((
            "Inbound address".into(),
            join_host_port(&metadata.inbound_ip, &metadata.inbound_port),
        ));
    }
    let rule = if connection.rule_payload.is_empty() {
        connection.rule.clone()
    } else {
        format!("{} {}", connection.rule, connection.rule_payload)
    };
    if !rule.trim().is_empty() {
        rows.push(("Rule".into(), rule));
    }
    if !metadata.special_proxy.is_empty() {
        rows.push(("Special proxy".into(), metadata.special_proxy.clone()));
    }
    if !metadata.special_rules.is_empty() {
        rows.push(("Special rules".into(), metadata.special_rules.clone()));
    }
    if !metadata.rematch_name.is_empty() {
        rows.push(("Rematch".into(), metadata.rematch_name.clone()));
    }
    if metadata.dscp != 0 {
        rows.push(("DSCP".into(), metadata.dscp.to_string()));
    }
    if !connection.chains.is_empty() {
        rows.push(("Chain".into(), connection.chain_path()));
        if let Some(outbound) = connection.chains.first() {
            rows.push(("Outbound".into(), outbound.clone()));
        }
    }
    rows.push((
        "Traffic".into(),
        format!(
            "↑{} ↓{}",
            bytes(connection.upload),
            bytes(connection.download)
        ),
    ));
    if let Some((upload, download)) = rate {
        rows.push(("Download speed".into(), speed(download)));
        rows.push(("Upload speed".into(), speed(upload)));
    }
    if !connection.start.is_empty() {
        rows.push((
            "Started".into(),
            crate::api::MihomoClient::short_time(&connection.start),
        ));
    }
    if !connection.id.is_empty() {
        rows.push(("ID".into(), connection.id.clone()));
    }

    let width = rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    rows.iter()
        .map(|(label, value)| format!("{label:<width$}  {}", strip_vs16(value)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn join_host_port(host: &str, port: &str) -> String {
    if port.is_empty() {
        host.to_string()
    } else {
        format!("{host}:{port}")
    }
}

/// `IPv4`, `IPv6` or `FQDN` for the destination, mirroring the
/// dashboard's Destination-type column.
fn destination_type(connection: &Connection) -> Option<&'static str> {
    let destination = if connection.metadata.destination_ip.is_empty() {
        &connection.metadata.host
    } else {
        &connection.metadata.destination_ip
    };
    if destination.is_empty() {
        return None;
    }
    if destination.parse::<std::net::Ipv4Addr>().is_ok() {
        Some("IPv4")
    } else if destination.parse::<std::net::Ipv6Addr>().is_ok() {
        Some("IPv6")
    } else {
        Some("FQDN")
    }
}

#[cfg(test)]
mod tests {
    use super::format_connection;
    use crate::api::{Connection, Metadata};

    #[test]
    fn detail_lists_only_reported_fields() {
        let connection = Connection {
            id: "abc123".into(),
            metadata: Metadata {
                host: "example.com".into(),
                destination_ip: "1.1.1.1".into(),
                destination_port: "443".into(),
                source_ip: "127.0.0.1".into(),
                source_port: "51000".into(),
                network: "tcp".into(),
                kind: "HTTP".into(),
                ..Default::default()
            },
            chains: vec!["node".into(), "AUTO".into(), "PROXY".into()],
            upload: 2048,
            download: 1024,
            start: "2026-09-12T16:10:01+08:00".into(),
            rule: "DomainSuffix".into(),
            rule_payload: "example.com".into(),
        };
        let detail = format_connection(&connection, None);
        assert!(detail.contains("Destination"), "{detail}");
        assert!(detail.contains("example.com:443"), "{detail}");
        assert!(detail.contains("Source"), "{detail}");
        assert!(detail.contains("127.0.0.1:51000"), "{detail}");
        assert!(detail.contains("TCP HTTP"), "{detail}");
        assert!(detail.contains("PROXY → AUTO → node"), "{detail}");
        assert!(detail.contains("DomainSuffix example.com"), "{detail}");
        assert!(detail.contains("16:10:01"), "{detail}");
        assert!(detail.contains("abc123"), "{detail}");
        // Fields the core did not send must not appear as empty rows.
        assert!(!detail.contains("Process"), "{detail}");
        assert!(!detail.contains("Inbound"), "{detail}");
    }

    #[test]
    fn detail_includes_inbound_remote_geo_and_special_fields() {
        let connection = Connection {
            metadata: Metadata {
                host: "example.com".into(),
                destination_ip: "1.2.3.4".into(),
                destination_port: "443".into(),
                source_ip: "127.0.0.1".into(),
                source_port: "51000".into(),
                inbound_ip: "127.0.0.1".into(),
                inbound_port: "7890".into(),
                inbound_name: "mixed".into(),
                inbound_user: "alice".into(),
                remote_destination: "5.6.7.8".into(),
                source_geo_ip: vec!["CN".into()],
                destination_geo_ip: vec!["US".into()],
                source_ip_asn: "AS4134".into(),
                destination_ip_asn: "AS15169".into(),
                special_proxy: "node".into(),
                special_rules: "rule".into(),
                rematch_name: "other".into(),
                dscp: 46,
                ..Default::default()
            },
            chains: vec!["node".into(), "PROXY".into()],
            ..Default::default()
        };
        let detail = format_connection(&connection, None);
        for value in [
            "Destination type",
            "IPv4",
            "Remote address",
            "5.6.7.8",
            "Source GeoIP",
            "Destination GeoIP",
            "AS4134",
            "AS15169",
            "mixed (alice)",
            "127.0.0.1:7890",
            "Special proxy",
            "Special rules",
            "Rematch",
            "DSCP",
            "46",
        ] {
            assert!(detail.contains(value), "missing {value}: {detail}");
        }
    }

    #[test]
    fn detail_shows_live_rates_when_sampled() {
        let detail = format_connection(&Connection::default(), Some((1_536, 4_096)));
        assert!(detail.contains("Download speed"), "{detail}");
        assert!(detail.contains("4.0 K/s"), "{detail}");
        assert!(detail.contains("Upload speed"), "{detail}");
        assert!(detail.contains("1.5 K/s"), "{detail}");
    }

    #[test]
    fn open_connection_detail_rerenders_with_live_rates() {
        use crate::api::{ConnectionResponse, Snapshot};
        use crate::app::{App, DataState, TaskHub, UiState};
        use crate::config::Config;
        use crate::theme::Theme;

        let connection = Connection {
            id: "live".into(),
            ..Default::default()
        };
        let mut app = App {
            config: Config::default(),
            api: crate::api::MihomoClient::new("http://127.0.0.1:9090", String::new()).unwrap(),
            remote: false,
            theme: Theme::default(),
            ui: UiState {
                detail_connection: Some("live".into()),
                ..Default::default()
            },
            data: DataState {
                snapshot: Snapshot {
                    connections: ConnectionResponse {
                        connections: vec![connection],
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            tasks: TaskHub::default(),
        };
        app.data
            .connection_rates
            .insert("live".into(), (1_024, 2_048));
        let first = app.detail_text().unwrap();
        assert!(first.contains("2.0 K/s"), "{first}");
        assert!(first.contains("1.0 K/s"), "{first}");

        app.data
            .connection_rates
            .insert("live".into(), (4_096, 8_192));
        let second = app.detail_text().unwrap();
        assert!(second.contains("8.0 K/s"), "{second}");
        assert!(second.contains("4.0 K/s"), "{second}");

        // Closed rows leave the active snapshot; the popup must fall back
        // to the local history and keep the last known rate.
        let closed = app.data.snapshot.connections.connections.remove(0);
        app.data.closed_connections.push(closed);
        let third = app.detail_text().unwrap();
        assert!(third.contains("8.0 K/s"), "{third}");
    }

    #[test]
    fn detail_falls_back_to_destination_ip() {
        let connection = Connection {
            metadata: Metadata {
                destination_ip: "9.9.9.9".into(),
                destination_port: "53".into(),
                uid: 1000,
                ..Default::default()
            },
            ..Default::default()
        };
        let detail = format_connection(&connection, None);
        assert!(detail.contains("9.9.9.9:53"), "{detail}");
        assert!(detail.contains("uid 1000"), "{detail}");
    }
}
