//! Connections tab interactions: the detail popup opened with Enter.
//! Closing (`x` / `X`) lives in `proxy.rs` next to the API calls.

use crate::api::Connection;
use crate::ui::widgets::{bytes, strip_vs16};

impl crate::app::App {
    /// Open the selected connection in the wrapped detail popup (shared
    /// with the Logs/Rules tabs; Esc closes it).
    pub(crate) fn open_connection_detail(&mut self) {
        let Some(connection) = self
            .data
            .snapshot
            .connections
            .connections
            .get(self.ui.connection_index)
        else {
            return;
        };
        self.ui.log_detail = Some(format_connection(connection));
    }
}

/// Label every non-empty field of one connection so the popup reads as a
/// structured record rather than a wall of text. Optional fields
/// (process, rules, chain, …) drop out when the core did not report them.
fn format_connection(connection: &Connection) -> String {
    let metadata = &connection.metadata;
    let mut rows: Vec<(String, String)> = Vec::new();

    let host = if metadata.host.is_empty() {
        &metadata.destination_ip
    } else {
        &metadata.host
    };
    if !host.is_empty() || !metadata.destination_port.is_empty() {
        rows.push((
            "Destination".into(),
            join_host_port(host, &metadata.destination_port),
        ));
    }
    if !metadata.source_ip.is_empty() || !metadata.source_port.is_empty() {
        rows.push((
            "Source".into(),
            join_host_port(&metadata.source_ip, &metadata.source_port),
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
    if !metadata.inbound_name.is_empty() {
        rows.push(("Inbound".into(), metadata.inbound_name.clone()));
    }
    let rule = if connection.rule_payload.is_empty() {
        connection.rule.clone()
    } else {
        format!("{} {}", connection.rule, connection.rule_payload)
    };
    if !rule.trim().is_empty() {
        rows.push(("Rule".into(), rule));
    }
    if !connection.chains.is_empty() {
        rows.push(("Chain".into(), connection.chain_path()));
    }
    rows.push((
        "Traffic".into(),
        format!(
            "↑{} ↓{}",
            bytes(connection.upload),
            bytes(connection.download)
        ),
    ));
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
        let detail = format_connection(&connection);
        assert!(detail.contains("Destination  example.com:443"), "{detail}");
        assert!(detail.contains("Source       127.0.0.1:51000"), "{detail}");
        assert!(detail.contains("Network      TCP HTTP"), "{detail}");
        assert!(detail.contains("PROXY → AUTO → node"), "{detail}");
        assert!(detail.contains("DomainSuffix example.com"), "{detail}");
        assert!(detail.contains("16:10:01"), "{detail}");
        assert!(detail.contains("abc123"), "{detail}");
        // Fields the core did not send must not appear as empty rows.
        assert!(!detail.contains("Process"), "{detail}");
        assert!(!detail.contains("Inbound"), "{detail}");
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
        let detail = format_connection(&connection);
        assert!(detail.contains("Destination  9.9.9.9:53"), "{detail}");
        assert!(detail.contains("uid 1000"), "{detail}");
    }
}
