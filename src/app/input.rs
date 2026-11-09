use crate::{backup, core, profiles::Profiles};
use crossterm::event::{KeyCode, KeyEvent};
use serde_json::{Value, json};

use super::{CoreMissingChoice, InputMode, UiState};

impl InputMode {
    /// Title and help text for the generic text-input dialog. `None` means
    /// the mode has a dedicated overlay (import source, core path, restore).
    pub(crate) fn editor(&self) -> Option<(&'static str, &'static str)> {
        if let Some(field) = DnsMapEntryField::from_mode(self) {
            return Some((field.label(), field.hint()));
        }
        if let Some(field) = DnsTextField::from_mode(self) {
            return Some((field.label(), field.hint()));
        }
        if let Some(field) = CoreTextField::from_mode(self) {
            return Some((field.label(), field.hint()));
        }
        if let Some(field) = GeoUrlField::from_mode(self) {
            return Some((field.label(), field.hint()));
        }
        if let Some(field) = ProfileTextField::from_mode(self) {
            return Some((field.label(), field.hint()));
        }
        match self {
            Self::ImportProfileName => Some((
                "Profile name",
                "Display name, 1-100 characters; empty keeps the default",
            )),
            Self::ImportProfileInterval => Some((
                "Update interval",
                "Hours 1-8760, or empty to follow the server header",
            )),
            Self::ImportProfileTimeout => Some((
                "Update timeout",
                "Fetch timeout seconds 1-600, or empty for default 30",
            )),
            Self::ImportProfileAuth => Some((
                "Auth token",
                "Sent as Authorization header, or empty to clear",
            )),
            Self::ImportProfileUserAgent => Some((
                "User-Agent",
                "Custom UA for subscription fetch, or empty for default",
            )),
            Self::SearchLogs => Some((
                "Search logs",
                "Substring filter (case-insensitive) or empty to clear",
            )),
            Self::SearchRules => Some((
                "Search rules",
                "Filter by type, payload or policy (case-insensitive)",
            )),
            Self::SearchNodes => Some(("Search nodes", "Filter by node name (case-insensitive)")),
            Self::EditGeoMirror => Some((
                "Geo mirror",
                "Enter mirror prefix (e.g. https://gh-proxy.com) or empty for direct",
            )),
            Self::EditGeoProxy => Some((
                "Geo proxy",
                "Enter proxy URL (e.g. http://127.0.0.1:7897) or empty for direct",
            )),
            _ => None,
        }
    }
}

/// Text-editable DNS fields. Each maps to an `InputMode` and knows how to
/// prefill the input buffer and apply the entered value.
#[derive(Clone, Copy)]
pub(crate) enum DnsTextField {
    Listen,
    Servers,
    FakeIpRange,
    FakeIpFilter,
    DefaultNs,
    DirectNs,
    ProxyNs,
    Fallback,
    FallbackGeoCode,
}

impl DnsTextField {
    pub(crate) fn from_mode(mode: &InputMode) -> Option<Self> {
        match mode {
            InputMode::EditDnsListen => Some(Self::Listen),
            InputMode::EditDnsServers => Some(Self::Servers),
            InputMode::EditDnsFakeIpRange => Some(Self::FakeIpRange),
            InputMode::EditDnsFakeIpFilter => Some(Self::FakeIpFilter),
            InputMode::EditDnsDefaultNs => Some(Self::DefaultNs),
            InputMode::EditDnsDirectNs => Some(Self::DirectNs),
            InputMode::EditDnsProxyNs => Some(Self::ProxyNs),
            InputMode::EditDnsFallback => Some(Self::Fallback),
            InputMode::EditDnsFallbackGeoCode => Some(Self::FallbackGeoCode),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Listen => "DNS listen",
            Self::Servers => "DNS servers",
            Self::FakeIpRange => "Fake IP range",
            Self::FakeIpFilter => "Fake IP filter",
            Self::DefaultNs => "Default nameserver",
            Self::DirectNs => "Direct nameserver",
            Self::ProxyNs => "Proxy nameserver",
            Self::Fallback => "DNS fallback",
            Self::FallbackGeoCode => "Fallback GeoIP code",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Listen => "Enter DNS listen address (e.g. 0.0.0.0:1053)",
            Self::Servers => {
                "Enter comma-separated DNS servers (e.g. 223.5.5.5, 8.8.8.8, tls://9.9.9.9)"
            }
            Self::FakeIpRange => "Enter fake IP range (e.g. 198.18.0.1/16) or empty to clear",
            Self::FakeIpFilter => "Enter comma-separated filter entries or empty to clear",
            Self::DefaultNs => {
                "Enter comma-separated servers for bootstrapping DoT/DoH or empty to clear"
            }
            Self::DirectNs => "Enter comma-separated servers for direct rules or empty to clear",
            Self::ProxyNs => "Enter comma-separated servers for proxy nodes or empty to clear",
            Self::Fallback => "Enter comma-separated fallback servers or empty to clear",
            Self::FallbackGeoCode => {
                "Enter GeoIP country code for fallback filter (e.g. CN) or empty to clear"
            }
        }
    }

    pub(crate) fn initial(self, app: &super::App) -> String {
        let dns = &app.config.dns;
        match self {
            Self::Listen => dns.listen.clone(),
            Self::Servers => dns.nameserver.join(", "),
            Self::FakeIpRange => dns.fake_ip_range.clone().unwrap_or_default(),
            Self::FakeIpFilter => dns.fake_ip_filter.join(", "),
            Self::DefaultNs => dns.default_nameserver.join(", "),
            Self::DirectNs => dns.direct_nameserver.join(", "),
            Self::ProxyNs => dns.proxy_server_nameserver.join(", "),
            Self::Fallback => dns.fallback.join(", "),
            Self::FallbackGeoCode => dns.fallback_filter.geoip_code.clone().unwrap_or_default(),
        }
    }

    /// Apply the entered value to config. Returns a summary for the status
    /// line, or a validation message (buffer is kept for correction).
    fn apply(self, app: &mut super::App, value: &str) -> Result<String, String> {
        let dns = &mut app.config.dns;
        let summary: Result<String, String> = match self {
            Self::Listen => {
                if value.is_empty() {
                    return Err("DNS listen cannot be empty (e.g. 0.0.0.0:1053)".into());
                }
                dns.listen = value.to_owned();
                Ok(value.to_owned())
            }
            Self::Servers => {
                let servers = parse_list(value);
                if servers.is_empty() {
                    return Err(
                        "Enter comma-separated DNS servers (e.g. 223.5.5.5, 8.8.8.8)".into(),
                    );
                }
                dns.nameserver = servers.clone();
                Ok(servers.join(", "))
            }
            Self::FakeIpRange => {
                dns.fake_ip_range = none_if_empty(value);
                Ok(dns.fake_ip_range.clone().unwrap_or_else(|| "—".into()))
            }
            Self::FakeIpFilter => {
                dns.fake_ip_filter = parse_list(value);
                Ok(or_dash(&dns.fake_ip_filter.join(", ")))
            }
            Self::DefaultNs => {
                dns.default_nameserver = parse_list(value);
                Ok(or_dash(&dns.default_nameserver.join(", ")))
            }
            Self::DirectNs => {
                dns.direct_nameserver = parse_list(value);
                Ok(or_dash(&dns.direct_nameserver.join(", ")))
            }
            Self::ProxyNs => {
                dns.proxy_server_nameserver = parse_list(value);
                Ok(or_dash(&dns.proxy_server_nameserver.join(", ")))
            }
            Self::Fallback => {
                dns.fallback = parse_list(value);
                Ok(or_dash(&dns.fallback.join(", ")))
            }
            Self::FallbackGeoCode => {
                dns.fallback_filter.geoip_code = none_if_empty(&value.to_uppercase());
                Ok(dns
                    .fallback_filter
                    .geoip_code
                    .clone()
                    .unwrap_or_else(|| "—".into()))
            }
        };
        let summary = summary?;
        // Editing DNS implies wanting it on, but only after the value
        // validates; a rejected edit must not leave config half-changed.
        dns.enable = true;
        Ok(summary)
    }
}

fn none_if_empty(value: &str) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value.trim().to_owned())
    }
}

fn or_dash(value: &str) -> String {
    if value.is_empty() {
        "—".into()
    } else {
        value.to_owned()
    }
}

/// One half of a `nameserver-policy` / `hosts` entry. An entry is edited as
/// two sub-inputs (domain, then its server list) so the list overlay keeps
/// the keyboard whenever no sub-input is open.
#[derive(Clone, Copy)]
pub(crate) enum DnsMapEntryField {
    PolicyDomain,
    PolicyServers,
    HostsDomain,
    HostsServers,
}

impl DnsMapEntryField {
    fn from_mode(mode: &InputMode) -> Option<Self> {
        match mode {
            InputMode::EditDnsPolicyDomain => Some(Self::PolicyDomain),
            InputMode::EditDnsPolicyServers => Some(Self::PolicyServers),
            InputMode::EditHostsDomain => Some(Self::HostsDomain),
            InputMode::EditHostsServers => Some(Self::HostsServers),
            _ => None,
        }
    }

    fn is_domain(self) -> bool {
        matches!(self, Self::PolicyDomain | Self::HostsDomain)
    }

    fn label(self) -> &'static str {
        match self {
            Self::PolicyDomain => "Policy domain",
            Self::PolicyServers => "Policy servers",
            Self::HostsDomain => "Host",
            Self::HostsServers => "Host addresses",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::PolicyDomain => {
                "Domain, geosite: or rule-set: key (e.g. +.example.com, geosite:cn)"
            }
            Self::PolicyServers => {
                "Comma-separated servers for this domain (e.g. 223.5.5.5, tls://8.8.8.8)"
            }
            Self::HostsDomain => "Hostname to map (e.g. example.com)",
            Self::HostsServers => "Comma-separated addresses (e.g. 1.2.3.4, 5.6.7.8)",
        }
    }
}

/// Text-editable core fields (ports, controller, secret…). Fields that
/// mihomo `PATCH /configs` (`configSchema`) accepts hot-patch instantly
/// with a daemon-reload fallback; the rest apply via config save + daemon
/// reload. Controller/secret also rebuild the API client so the TUI talks
/// to the new endpoint.
#[derive(Clone, Copy)]
pub(crate) enum CoreTextField {
    MixedPort,
    Controller,
    Secret,
    ProxyBypass,
    DelayTestUrl,
    SocksPort,
    HttpPort,
    RedirPort,
    TproxyPort,
    Auth,
    SkipAuth,
    LanAllowed,
    LanDisallowed,
    TunDevice,
    TunDnsHijack,
    TunMtu,
    TunRouteExclude,
    SniffHttpPorts,
    SniffTlsPorts,
}

impl CoreTextField {
    fn from_mode(mode: &InputMode) -> Option<Self> {
        match mode {
            InputMode::EditMixedPort => Some(Self::MixedPort),
            InputMode::EditController => Some(Self::Controller),
            InputMode::EditSecret => Some(Self::Secret),
            InputMode::EditProxyBypass => Some(Self::ProxyBypass),
            InputMode::EditDelayTestUrl => Some(Self::DelayTestUrl),
            InputMode::EditSocksPort => Some(Self::SocksPort),
            InputMode::EditHttpPort => Some(Self::HttpPort),
            InputMode::EditRedirPort => Some(Self::RedirPort),
            InputMode::EditTproxyPort => Some(Self::TproxyPort),
            InputMode::EditAuth => Some(Self::Auth),
            InputMode::EditSkipAuth => Some(Self::SkipAuth),
            InputMode::EditLanAllowed => Some(Self::LanAllowed),
            InputMode::EditLanDisallowed => Some(Self::LanDisallowed),
            InputMode::EditTunDevice => Some(Self::TunDevice),
            InputMode::EditTunDnsHijack => Some(Self::TunDnsHijack),
            InputMode::EditTunMtu => Some(Self::TunMtu),
            InputMode::EditTunRouteExclude => Some(Self::TunRouteExclude),
            InputMode::EditSniffHttpPorts => Some(Self::SniffHttpPorts),
            InputMode::EditSniffTlsPorts => Some(Self::SniffTlsPorts),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::MixedPort => "Mixed port",
            Self::Controller => "Controller",
            Self::Secret => "Controller secret",
            Self::ProxyBypass => "Proxy bypass",
            Self::DelayTestUrl => "Delay test URL",
            Self::SocksPort => "Socks port",
            Self::HttpPort => "HTTP port",
            Self::RedirPort => "Redir port",
            Self::TproxyPort => "Tproxy port",
            Self::Auth => "Authentication",
            Self::SkipAuth => "Skip auth prefixes",
            Self::LanAllowed => "LAN allowed IPs",
            Self::LanDisallowed => "LAN disallowed IPs",
            Self::TunDevice => "TUN device",
            Self::TunDnsHijack => "TUN DNS hijack",
            Self::TunMtu => "TUN MTU",
            Self::TunRouteExclude => "TUN route exclude",
            Self::SniffHttpPorts => "Sniff HTTP ports",
            Self::SniffTlsPorts => "Sniff TLS ports",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::MixedPort => {
                "Enter HTTP/SOCKS mixed port (e.g. 7890) or empty to disable, core restarts"
            }
            Self::Controller => "Enter external controller URL (e.g. http://127.0.0.1:9090)",
            Self::Secret => "Enter controller secret or empty for none",
            Self::ProxyBypass => "Enter comma-separated bypass hosts or empty for default",
            Self::DelayTestUrl => {
                "Enter URL for node latency tests (e.g. https://www.gstatic.com/generate_204)"
            }
            Self::SocksPort => "Enter SOCKS5 port (e.g. 7892) or empty to disable, core restarts",
            Self::HttpPort => "Enter HTTP port (e.g. 7891) or empty to disable, core restarts",
            Self::RedirPort => "Enter redir port (e.g. 7893) or empty to disable, core restarts",
            Self::TproxyPort => "Enter tproxy port (e.g. 7894) or empty to disable, core restarts",
            Self::Auth => "Enter user:pass pairs comma-separated or empty to clear",
            Self::SkipAuth => "Enter IP prefixes comma-separated or empty to clear",
            Self::LanAllowed => "Enter allowed LAN IPs/CIDRs comma-separated or empty to clear",
            Self::LanDisallowed => "Enter blocked LAN IPs/CIDRs comma-separated or empty to clear",
            Self::TunDevice => "Enter TUN device name or empty for auto",
            Self::TunDnsHijack => "Enter hijacked DNS servers comma-separated or empty to clear",
            Self::TunMtu => "Enter MTU 68-9000 or empty for auto",
            Self::TunRouteExclude => {
                "Enter IPs/CIDRs comma-separated (e.g. 192.168.0.0/16) or empty to clear"
            }
            Self::SniffHttpPorts => {
                "Enter ports/ranges comma-separated (e.g. 80, 8080-8880) or empty to clear"
            }
            Self::SniffTlsPorts => {
                "Enter ports/ranges comma-separated (e.g. 443, 8443) or empty to clear"
            }
        }
    }

    pub(crate) fn initial(self, app: &super::App) -> String {
        match self {
            Self::MixedPort => port_text(app.config.mixed_port),
            Self::Controller => app.config.controller.clone(),
            Self::Secret => app.config.secret.clone(),
            Self::ProxyBypass => app.config.proxy_bypass.clone(),
            Self::DelayTestUrl => app.config.delay_test_url.clone(),
            Self::SocksPort => port_text(app.config.socks_port),
            Self::HttpPort => port_text(app.config.http_port),
            Self::RedirPort => port_text(app.config.redir_port),
            Self::TproxyPort => port_text(app.config.tproxy_port),
            Self::Auth => app.config.authentication.join(", "),
            Self::SkipAuth => app.config.skip_auth_prefixes.join(", "),
            Self::LanAllowed => app.config.lan_allowed_ips.join(", "),
            Self::LanDisallowed => app.config.lan_disallowed_ips.join(", "),
            Self::TunDevice => app.config.tun.device.clone().unwrap_or_default(),
            Self::TunDnsHijack => app.config.tun.dns_hijack.join(", "),
            Self::TunMtu => app
                .config
                .tun
                .mtu
                .map(|mtu| mtu.to_string())
                .unwrap_or_default(),
            Self::TunRouteExclude => app.config.tun.route_exclude_address.join(", "),
            Self::SniffHttpPorts => app.config.sniffer.http_ports.join(", "),
            Self::SniffTlsPorts => app.config.sniffer.tls_ports.join(", "),
        }
    }

    /// Validate + apply. Controller/secret rebuild the API client first so
    /// a bad endpoint never leaves the TUI talking to nowhere.
    fn apply(self, app: &mut super::App, value: &str) -> Result<String, String> {
        match self {
            Self::MixedPort => apply_port(&mut app.config.mixed_port, value, "7890"),
            Self::Controller => {
                if value.is_empty() {
                    return Err("Controller cannot be empty (e.g. http://127.0.0.1:9090)".into());
                }
                let client = crate::api::MihomoClient::new(value, app.config.secret.clone())
                    .map_err(|e| format!("Bad controller URL: {e}"))?;
                app.config.controller = value.to_owned();
                app.api = client;
                Ok(value.to_owned())
            }
            Self::Secret => {
                let client =
                    crate::api::MihomoClient::new(&app.config.controller, value.to_owned())
                        .map_err(|e| format!("Cannot apply secret: {e}"))?;
                app.config.secret = value.to_owned();
                app.api = client;
                Ok(if value.is_empty() {
                    "— (cleared)".into()
                } else {
                    "••••••".into()
                })
            }
            Self::ProxyBypass => {
                app.config.proxy_bypass = value.to_owned();
                Ok(or_dash(value))
            }
            Self::DelayTestUrl => {
                if value.is_empty() {
                    return Err(
                        "Delay test URL cannot be empty (e.g. https://www.gstatic.com/generate_204)"
                            .into(),
                    );
                }
                app.config.delay_test_url = value.to_owned();
                Ok(value.to_owned())
            }
            Self::SocksPort => apply_port(&mut app.config.socks_port, value, "7891"),
            Self::HttpPort => apply_port(&mut app.config.http_port, value, "7891"),
            Self::RedirPort => apply_port(&mut app.config.redir_port, value, "7891"),
            Self::TproxyPort => apply_port(&mut app.config.tproxy_port, value, "7891"),
            Self::Auth => {
                app.config.authentication = parse_list(value);
                Ok(or_dash(&app.config.authentication.join(", ")))
            }
            Self::SkipAuth => {
                app.config.skip_auth_prefixes = parse_list(value);
                Ok(or_dash(&app.config.skip_auth_prefixes.join(", ")))
            }
            Self::LanAllowed => {
                let addresses = validate_ip_list(value, "LAN allowed")?;
                app.config.lan_allowed_ips = addresses;
                Ok(or_dash(&app.config.lan_allowed_ips.join(", ")))
            }
            Self::LanDisallowed => {
                let addresses = validate_ip_list(value, "LAN disallowed")?;
                app.config.lan_disallowed_ips = addresses;
                Ok(or_dash(&app.config.lan_disallowed_ips.join(", ")))
            }
            Self::TunDevice => {
                app.config.tun.device = none_if_empty(value);
                Ok(app
                    .config
                    .tun
                    .device
                    .clone()
                    .unwrap_or_else(|| "— (auto)".into()))
            }
            Self::TunDnsHijack => {
                app.config.tun.dns_hijack = parse_list(value);
                Ok(or_dash(&app.config.tun.dns_hijack.join(", ")))
            }
            Self::TunMtu => {
                if value.trim().is_empty() {
                    app.config.tun.mtu = None;
                    return Ok("— (auto)".into());
                }
                let mtu: u16 = value
                    .parse()
                    .map_err(|_| "Enter an MTU 68-9000 or empty for auto".to_owned())?;
                if !(68..=9000).contains(&mtu) {
                    return Err("Enter an MTU 68-9000 or empty for auto".into());
                }
                app.config.tun.mtu = Some(mtu);
                Ok(mtu.to_string())
            }
            Self::TunRouteExclude => {
                let addresses = parse_list(value);
                for address in &addresses {
                    if !is_ip_or_cidr(address) {
                        return Err(format!(
                            "Bad route exclude '{address}' (IP or CIDR, e.g. 192.168.0.0/16)"
                        ));
                    }
                }
                app.config.tun.route_exclude_address = addresses;
                Ok(or_dash(&app.config.tun.route_exclude_address.join(", ")))
            }
            Self::SniffHttpPorts => {
                let ports = parse_list(value);
                for port in &ports {
                    if !is_sniff_port(port) {
                        return Err(format!(
                            "Bad sniff port '{port}' (port or range, e.g. 80, 8080-8880)"
                        ));
                    }
                }
                app.config.sniffer.http_ports = ports;
                Ok(or_dash(&app.config.sniffer.http_ports.join(", ")))
            }
            Self::SniffTlsPorts => {
                let ports = parse_list(value);
                for port in &ports {
                    if !is_sniff_port(port) {
                        return Err(format!(
                            "Bad sniff port '{port}' (port or range, e.g. 443, 8443)"
                        ));
                    }
                }
                app.config.sniffer.tls_ports = ports;
                Ok(or_dash(&app.config.sniffer.tls_ports.join(", ")))
            }
        }
    }

    /// Runtime-patch payload per mihomo `PATCH /configs` (`configSchema`),
    /// built from the just-applied config. `None` for fields outside the
    /// schema (controller/secret, auth, sniffer port lists, TUN…) which
    /// still go through the daemon reload path. Cleared optional ports
    /// patch as 0 (listener disabled); a PATCH failure falls back to the
    /// daemon reload, which converges to the same saved values.
    fn patch_payload(self, app: &super::App) -> Option<Value> {
        match self {
            Self::MixedPort => Some(json!({ "mixed-port": app.config.mixed_port.unwrap_or(0) })),
            Self::HttpPort => Some(json!({ "port": app.config.http_port.unwrap_or(0) })),
            Self::SocksPort => Some(json!({ "socks-port": app.config.socks_port.unwrap_or(0) })),
            Self::RedirPort => Some(json!({ "redir-port": app.config.redir_port.unwrap_or(0) })),
            Self::TproxyPort => Some(json!({ "tproxy-port": app.config.tproxy_port.unwrap_or(0) })),
            Self::SkipAuth => Some(json!({ "skip-auth-prefixes": app.config.skip_auth_prefixes })),
            Self::LanAllowed => Some(json!({ "lan-allowed-ips": app.config.lan_allowed_ips })),
            Self::LanDisallowed => {
                Some(json!({ "lan-disallowed-ips": app.config.lan_disallowed_ips }))
            }
            _ => None,
        }
    }
}

fn port_text(port: Option<u16>) -> String {
    port.map(|port| port.to_string()).unwrap_or_default()
}

fn parse_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

/// `lan-allowed-ips` / `lan-disallowed-ips` entries must be CIDR ranges.
/// mihomo parses them with `netip.ParsePrefix`, so a bare address such as
/// `::1` or `127.0.0.1` aborts the whole config at load time — the core then
/// refuses to start at all. Reject it in the editor instead, where the user
/// can still fix it.
fn validate_ip_list(value: &str, label: &str) -> Result<Vec<String>, String> {
    let addresses = parse_list(value);
    for address in &addresses {
        if !is_ip_cidr(address) {
            return Err(format!(
                "Bad {label} IP '{address}' (CIDR required, e.g. 192.168.0.0/16 or ::1/128)"
            ));
        }
    }
    Ok(addresses)
}

/// Parse `addr/prefix` and reject prefix lengths beyond the address
/// family's width. Shared by the two validators below so the rules cannot
/// drift apart.
fn parse_cidr(value: &str) -> Option<(std::net::IpAddr, u8)> {
    let (addr, prefix) = value.split_once('/')?;
    let ip = addr.parse::<std::net::IpAddr>().ok()?;
    let bits: u8 = prefix.parse().ok()?;
    (bits <= if ip.is_ipv4() { 32 } else { 128 }).then_some((ip, bits))
}

/// CIDR range only. `is_ip_or_cidr` also accepts a bare IP, which mihomo
/// rejects for these two fields, so keep the stricter check separate.
fn is_ip_cidr(value: &str) -> bool {
    parse_cidr(value).is_some()
}

/// Plain IP or CIDR range (`192.168.0.0/16`, `::1/128`), used for
/// `route-exclude-address` validation.
fn is_ip_or_cidr(value: &str) -> bool {
    value.parse::<std::net::IpAddr>().is_ok() || is_ip_cidr(value)
}

/// Bare port (`443`) or port range (`8080-8880`) for `sniff.*.ports`.
fn is_sniff_port(value: &str) -> bool {
    if let Ok(port) = value.parse::<u32>() {
        return (1..=65535).contains(&port);
    }
    let Some((start, end)) = value.split_once('-') else {
        return false;
    };
    match (start.parse::<u32>(), end.parse::<u32>()) {
        (Ok(start), Ok(end)) => (1..=65535).contains(&start) && start <= end && end <= 65535,
        _ => false,
    }
}

/// Dedicated listener ports: empty disables (profile value passes
/// through, hot-patch sends 0), otherwise 1-65535.
fn apply_port(slot: &mut Option<u16>, value: &str, example: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        *slot = None;
        return Ok("— (off)".into());
    }
    let error = || format!("Enter a port 1-65535 (e.g. {example}) or empty to disable");
    let port: u16 = value.parse().map_err(|_| error())?;
    if port == 0 {
        return Err(error());
    }
    *slot = Some(port);
    Ok(port.to_string())
}

/// Per-asset geo download URLs (cf. clash-party `geox-url`). One generic
/// handler covers all four: full `https://` URL wins over the mirror
/// prefix, empty clears back to mirror/direct.
#[derive(Clone, Copy)]
pub(crate) enum GeoUrlField {
    GeoIp,
    Geosite,
    Mmdb,
    Asn,
}

impl GeoUrlField {
    pub(crate) fn from_mode(mode: &InputMode) -> Option<Self> {
        match mode {
            InputMode::EditGeoIpUrl => Some(Self::GeoIp),
            InputMode::EditGeositeUrl => Some(Self::Geosite),
            InputMode::EditMmdbUrl => Some(Self::Mmdb),
            InputMode::EditAsnUrl => Some(Self::Asn),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::GeoIp => "GeoIP URL",
            Self::Geosite => "Geosite URL",
            Self::Mmdb => "MMDB URL",
            Self::Asn => "ASN URL",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::GeoIp => "Full geoip.dat URL or empty for mirror/direct",
            Self::Geosite => "Full geosite.dat URL or empty for mirror/direct",
            Self::Mmdb => "Full geoip.metadb URL or empty for mirror/direct",
            Self::Asn => "Full ASN mmdb URL or empty for mirror/direct",
        }
    }

    pub(crate) fn initial(self, app: &super::App) -> String {
        self.get(app).clone().unwrap_or_default()
    }

    fn get(self, app: &super::App) -> &Option<String> {
        match self {
            Self::GeoIp => &app.config.geo.geoip_url,
            Self::Geosite => &app.config.geo.geosite_url,
            Self::Mmdb => &app.config.geo.mmdb_url,
            Self::Asn => &app.config.geo.asn_url,
        }
    }

    fn set(self, app: &mut super::App, value: Option<String>) {
        match self {
            Self::GeoIp => app.config.geo.geoip_url = value,
            Self::Geosite => app.config.geo.geosite_url = value,
            Self::Mmdb => app.config.geo.mmdb_url = value,
            Self::Asn => app.config.geo.asn_url = value,
        }
    }
}

/// Per-profile update-control text fields (edited from the `e` editor on
/// the Profiles tab). Numbers: empty clears back to default/off.
#[derive(Clone, Copy)]
pub(crate) enum ProfileTextField {
    Name,
    Url,
    Interval,
    Timeout,
    Auth,
    UserAgent,
}

impl ProfileTextField {
    pub(crate) fn from_mode(mode: &InputMode) -> Option<Self> {
        match mode {
            InputMode::EditProfileName => Some(Self::Name),
            InputMode::EditProfileUrl => Some(Self::Url),
            InputMode::EditProfileInterval => Some(Self::Interval),
            InputMode::EditProfileTimeout => Some(Self::Timeout),
            InputMode::EditProfileAuth => Some(Self::Auth),
            InputMode::EditProfileUserAgent => Some(Self::UserAgent),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Name => "Profile name",
            Self::Url => "Subscription URL",
            Self::Interval => "Update interval",
            Self::Timeout => "Update timeout",
            Self::Auth => "Auth token",
            Self::UserAgent => "User-Agent",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Name => "Display name, 1-100 characters",
            Self::Url => "http(s) URL fetched on update; applied on the next update",
            Self::Interval => "Hours between auto-updates 1-8760, or empty for off",
            Self::Timeout => "Fetch timeout in seconds 1-600, or empty for default 30",
            Self::Auth => "Sent as Authorization header, or empty to clear",
            Self::UserAgent => "Custom UA for subscription fetch, or empty for default",
        }
    }

    pub(crate) fn initial(self, app: &super::App) -> String {
        let Some(profile) = app.data.profiles.items.get(app.ui.profile_index) else {
            return String::new();
        };
        match self {
            Self::Name => profile.name.clone(),
            Self::Url => profile.url.clone().unwrap_or_default(),
            Self::Interval => profile
                .update_interval
                .map(|h| h.to_string())
                .unwrap_or_default(),
            Self::Timeout => profile
                .update_timeout
                .map(|s| s.to_string())
                .unwrap_or_default(),
            Self::Auth => profile.auth_token.clone().unwrap_or_default(),
            Self::UserAgent => profile.user_agent.clone().unwrap_or_default(),
        }
    }

    /// Validate + store on the selected profile. Returns a one-line
    /// summary for the status bar, or a message when invalid.
    fn apply(self, app: &mut super::App, raw: &str) -> Result<String, String> {
        let profile = app
            .data
            .profiles
            .items
            .get_mut(app.ui.profile_index)
            .ok_or_else(|| "No profile selected".to_string())?;
        match self {
            Self::Name => {
                if raw.is_empty() {
                    return Err("Enter a non-empty name".to_string());
                }
                if raw.chars().count() > 100 {
                    return Err("Keep the name within 100 characters".to_string());
                }
                profile.name = raw.to_owned();
                Ok(raw.to_owned())
            }
            Self::Url => {
                if raw.is_empty() {
                    return Err("Enter a non-empty http(s) subscription URL".to_string());
                }
                if !raw.starts_with("http://") && !raw.starts_with("https://") {
                    return Err("Subscription URL must start with http:// or https://".to_string());
                }
                profile.url = Some(raw.to_owned());
                Ok("updated".into())
            }
            Self::Interval => {
                if raw.is_empty() {
                    profile.update_interval = None;
                    return Ok("off".into());
                }
                let hours: u64 = raw
                    .parse()
                    .ok()
                    .filter(|hours| (1..=8760).contains(hours))
                    .ok_or_else(|| "Enter hours 1-8760, or empty for off".to_string())?;
                profile.update_interval = Some(hours);
                Ok(format!("{hours} h"))
            }
            Self::Timeout => {
                if raw.is_empty() {
                    profile.update_timeout = None;
                    return Ok("default 30 s".into());
                }
                let secs: u64 = raw
                    .parse()
                    .ok()
                    .filter(|secs| (1..=600).contains(secs))
                    .ok_or_else(|| "Enter seconds 1-600, or empty for default".to_string())?;
                profile.update_timeout = Some(secs);
                Ok(format!("{secs} s"))
            }
            Self::Auth => {
                if raw.is_empty() {
                    profile.auth_token = None;
                    Ok("cleared".into())
                } else {
                    profile.auth_token = Some(raw.to_owned());
                    Ok("set".into())
                }
            }
            Self::UserAgent => {
                if raw.is_empty() {
                    profile.user_agent = None;
                    Ok("default".into())
                } else {
                    profile.user_agent = Some(raw.to_owned());
                    Ok(raw.to_owned())
                }
            }
        }
    }
}

impl super::App {
    pub(crate) async fn handle_input(&mut self, key: KeyEvent) {
        // One clone per key (the enum may own a backup path); the match is
        // exhaustive so a new mode cannot silently fall into the import
        // source editor.
        let Some(mode) = self.ui.input_mode_cloned() else {
            return;
        };
        match mode {
            InputMode::RestoreBackup(path) => self.handle_restore_input(key, &path).await,
            InputMode::CorePath => self.handle_core_path_input(key).await,
            mode @ (InputMode::EditDnsPolicyDomain
            | InputMode::EditDnsPolicyServers
            | InputMode::EditHostsDomain
            | InputMode::EditHostsServers) => {
                let field = DnsMapEntryField::from_mode(&mode)
                    .expect("grouped modes must map to a DNS map field");
                self.handle_dns_map_entry_input(key, field).await;
            }
            mode @ (InputMode::EditDnsListen
            | InputMode::EditDnsServers
            | InputMode::EditDnsFakeIpRange
            | InputMode::EditDnsFakeIpFilter
            | InputMode::EditDnsDefaultNs
            | InputMode::EditDnsDirectNs
            | InputMode::EditDnsProxyNs
            | InputMode::EditDnsFallback
            | InputMode::EditDnsFallbackGeoCode) => {
                let field = DnsTextField::from_mode(&mode)
                    .expect("grouped modes must map to a DNS text field");
                self.handle_dns_text_input(key, field).await;
            }
            mode @ (InputMode::EditMixedPort
            | InputMode::EditController
            | InputMode::EditSecret
            | InputMode::EditProxyBypass
            | InputMode::EditDelayTestUrl
            | InputMode::EditSocksPort
            | InputMode::EditHttpPort
            | InputMode::EditRedirPort
            | InputMode::EditTproxyPort
            | InputMode::EditAuth
            | InputMode::EditSkipAuth
            | InputMode::EditLanAllowed
            | InputMode::EditLanDisallowed
            | InputMode::EditTunDevice
            | InputMode::EditTunDnsHijack
            | InputMode::EditTunMtu
            | InputMode::EditTunRouteExclude
            | InputMode::EditSniffHttpPorts
            | InputMode::EditSniffTlsPorts) => {
                let field = CoreTextField::from_mode(&mode)
                    .expect("grouped modes must map to a core text field");
                self.handle_core_text_input(key, field).await;
            }
            InputMode::SearchLogs => self.handle_search_input(key),
            InputMode::SearchRules => self.handle_rule_search_input(key),
            InputMode::SearchNodes => self.handle_node_search_input(key),
            InputMode::EditGeoMirror => self.handle_geo_mirror_input(key).await,
            InputMode::EditGeoProxy => self.handle_geo_proxy_input(key).await,
            mode @ (InputMode::EditGeoIpUrl
            | InputMode::EditGeositeUrl
            | InputMode::EditMmdbUrl
            | InputMode::EditAsnUrl) => {
                let field = GeoUrlField::from_mode(&mode)
                    .expect("grouped modes must map to a geo URL field");
                self.handle_geo_url_input(key, field).await;
            }
            mode @ (InputMode::EditProfileName
            | InputMode::EditProfileUrl
            | InputMode::EditProfileInterval
            | InputMode::EditProfileTimeout
            | InputMode::EditProfileAuth
            | InputMode::EditProfileUserAgent) => {
                let field = ProfileTextField::from_mode(&mode)
                    .expect("grouped modes must map to a profile text field");
                self.handle_profile_text_input(key, field).await;
            }
            InputMode::ImportProfile => self.handle_import_input(key),
            mode @ (InputMode::ImportProfileName
            | InputMode::ImportProfileInterval
            | InputMode::ImportProfileTimeout
            | InputMode::ImportProfileAuth
            | InputMode::ImportProfileUserAgent) => {
                self.handle_import_draft_input(key, &mode);
            }
        }
    }

    /// Persist config via supervisor IPC, restoring the pre-edit snapshot
    /// when the write fails so the UI never claims a value the disk does not have.
    /// Fails fast if no daemon is running without falling back to a direct file write.
    pub(crate) async fn save_config_or_restore(&mut self, previous: crate::config::Config) -> bool {
        let patch = crate::config::DynamicConfig::from_config(&self.config);
        match crate::ipc::save_config(patch).await {
            Ok(()) => true,
            Err(error) => {
                self.config = previous;
                self.say(format!("Save failed: {error}"));
                false
            }
        }
    }

    async fn handle_restore_input(&mut self, key: KeyEvent, path: &std::path::Path) {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                self.ui.close_input();
                match backup::restore(path) {
                    Ok(()) => match Profiles::load() {
                        Ok(profiles) => {
                            self.data.profiles = profiles;
                            self.say(format!("Restored {}", path.display()));
                        }
                        Err(error) => {
                            self.say(format!("Restored, but reload failed: {error}"));
                        }
                    },
                    Err(error) => self.say(format!("Restore failed: {error}")),
                }
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                self.ui.close_input();
                self.say("Restore cancelled");
            }
            _ => {}
        }
    }

    async fn handle_core_path_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.ui.close_input();
                self.ui
                    .reopen_core_missing_dialog(CoreMissingChoice::ProvidePath);
            }
            KeyCode::Backspace => {
                self.ui.input_backspace();
            }
            KeyCode::Enter => {
                let value = self.ui.input_value().trim().to_owned();
                self.ui.close_input();
                if value.is_empty() {
                    self.say("Enter an absolute path to the mihomo binary");
                    self.ui
                        .reopen_core_missing_dialog(CoreMissingChoice::ProvidePath);
                } else {
                    self.apply_core_path(&value).await;
                    if UiState::core_missing() {
                        self.ui
                            .reopen_core_missing_dialog(CoreMissingChoice::Download);
                    }
                }
            }
            _ => {
                if self.ui.input_nav(&key) {
                    return;
                }
                if let Some(c) = UiState::input_typing(&key) {
                    self.ui.input_insert(c);
                }
            }
        }
    }

    /// Text-editable DNS fields sharing Esc/Backspace/Char/Enter handling.
    /// Enter applies the value, saves config, and hot-patches the running
    /// core with a restart fallback — same flow for all DNS text fields.
    async fn handle_dns_text_input(&mut self, key: KeyEvent, field: DnsTextField) {
        match key.code {
            KeyCode::Esc => {
                self.ui.close_input();
                self.say(format!("{} edit cancelled", field.label()));
            }
            KeyCode::Backspace => {
                self.ui.input_backspace();
            }
            KeyCode::Enter => {
                let raw = self.ui.input_value().trim().to_owned();
                let previous = self.config.clone();
                let summary = match field.apply(self, &raw) {
                    Ok(summary) => summary,
                    Err(message) => {
                        self.say(message);
                        return;
                    }
                };
                self.ui.close_input();
                if !self.save_config_or_restore(previous).await {
                    return;
                }
                crate::logger::info("app", &format!("{} -> {summary}", field.label()));
                if self.remote {
                    // Remote: direct PATCH only, never touch the local daemon.
                    match self.api.update_dns(&self.config.dns).await {
                        Ok(()) => self.say(format!("{} {summary} (remote patched)", field.label())),
                        Err(e) => self.say(format!("Saved, remote patch failed: {e}")),
                    }
                    return;
                }
                if !self.config.dns.override_profile {
                    // Override off: never touch the running core's DNS; the
                    // daemon rebuild restores the profile section instead.
                    match core::request_restart().await {
                        Ok(()) => self.say(format!(
                            "{} {summary} saved, reload requested (override off)",
                            field.label()
                        )),
                        Err(err) => self.say(format!("Saved, restart request failed: {err}")),
                    }
                    return;
                }
                match self.api.update_dns(&self.config.dns).await {
                    Ok(()) => self.say(format!("{} {summary} (hot patched)", field.label())),
                    Err(e) => {
                        crate::logger::warn(
                            "app",
                            &format!("{} hot patch failed: {e}", field.label()),
                        );
                        match core::request_restart().await {
                            Ok(()) => self.say(format!(
                                "{} {summary} saved, reload requested",
                                field.label()
                            )),
                            Err(err) => {
                                self.say(format!("Save ok but reload failed: {err} (hot: {e})"))
                            }
                        }
                    }
                }
            }
            _ => {
                if self.ui.input_nav(&key) {
                    return;
                }
                if let Some(c) = UiState::input_typing(&key) {
                    self.ui.input_insert(c);
                }
            }
        }
    }

    /// Sub-input for one `nameserver-policy` / `hosts` entry. The domain is
    /// captured first, then the servers; only the second Enter commits the
    /// entry to disk. No reload here — the list overlay batches that on close
    /// so editing three entries doesn't restart the core three times.
    async fn handle_dns_map_entry_input(&mut self, key: KeyEvent, field: DnsMapEntryField) {
        match key.code {
            KeyCode::Esc => {
                self.ui.close_input();
                if let Some(editor) = self.ui.dns_map_editor.as_mut() {
                    editor.pending_domain = None;
                    editor.editing = None;
                }
                self.say("Entry edit cancelled");
            }
            KeyCode::Backspace => {
                self.ui.input_backspace();
            }
            KeyCode::Enter => {
                let raw = self.ui.input_value().trim().to_owned();
                let Some(target) = self.ui.dns_map_editor.as_ref().map(|e| e.target) else {
                    self.ui.close_input();
                    return;
                };
                if field.is_domain() {
                    if raw.is_empty() {
                        self.say(match target {
                            super::DnsMapTarget::NameserverPolicy => {
                                "Domain cannot be empty (e.g. geosite:cn or +.example.com)"
                            }
                            super::DnsMapTarget::Hosts => "Host cannot be empty (e.g. example.com)",
                        });
                        return;
                    }
                    // Prefill the servers sub-input from the row being edited.
                    let existing = self
                        .ui
                        .dns_map_editor
                        .as_ref()
                        .and_then(|editor| {
                            let index = editor.editing?;
                            editor.entries.get(index)
                        })
                        .map(|(_, servers)| servers.values().join(", "))
                        .unwrap_or_default();
                    if let Some(editor) = self.ui.dns_map_editor.as_mut() {
                        editor.pending_domain = Some(raw);
                    }
                    let mode = match target {
                        super::DnsMapTarget::NameserverPolicy => InputMode::EditDnsPolicyServers,
                        super::DnsMapTarget::Hosts => InputMode::EditHostsServers,
                    };
                    self.ui.begin_input(mode, existing);
                    return;
                }
                // Servers sub-input: this is the commit point.
                let servers = parse_list(&raw);
                if servers.is_empty() {
                    self.say("Enter at least one server (e.g. 223.5.5.5, tls://8.8.8.8)");
                    return;
                }
                let Some(list) = crate::config::StringList::from_values(servers) else {
                    self.ui.close_input();
                    return;
                };
                // `sync_dns_map_to_config` folds `entries` into an ordered
                // map, which keeps one row per domain (last wins) — so
                // committing a domain another row already owns would leave
                // the overlay showing a rule the saved config cannot hold.
                // Skip the row being edited: re-saving a row's own domain is
                // not a clash.
                let conflict = self.ui.dns_map_editor.as_ref().and_then(|editor| {
                    let domain = editor.pending_domain.as_deref()?;
                    editor
                        .entries
                        .iter()
                        .enumerate()
                        .find(|(index, (existing, _))| {
                            existing == domain && Some(*index) != editor.editing
                        })
                        .map(|(_, (existing, _))| existing.clone())
                });
                if let Some(domain) = conflict {
                    // Bail out exactly like a cancelled edit: nothing is
                    // written, `dirty` stays put, and the row is still there
                    // to edit with Enter.
                    self.ui.close_input();
                    if let Some(editor) = self.ui.dns_map_editor.as_mut() {
                        editor.pending_domain = None;
                        editor.editing = None;
                    }
                    self.say(format!("{domain} already exists — press Enter to edit it"));
                    return;
                }
                let committed = self.ui.dns_map_editor.as_mut().and_then(|editor| {
                    let domain = editor.pending_domain.take()?;
                    // Editing an existing row replaces it in place; a new
                    // entry (`editing == None`) appends and takes that row.
                    let index = match editor.editing.take() {
                        Some(index) if index < editor.entries.len() => {
                            editor.entries[index] = (domain, list);
                            index
                        }
                        _ => {
                            editor.entries.push((domain, list));
                            editor.entries.len() - 1
                        }
                    };
                    editor.index = index;
                    editor.dirty = true;
                    Some(index)
                });
                self.ui.close_input();
                let Some(index) = committed else { return };
                // Fold into config before saving — the editor's `entries`
                // are the working copy, config is what gets persisted.
                let previous = self.config.clone();
                self.sync_dns_map_to_config();
                if !self.save_config_or_restore(previous).await {
                    return;
                }
                let summary = self
                    .ui
                    .dns_map_editor
                    .as_ref()
                    .and_then(|editor| editor.entries.get(index))
                    .map(|(domain, servers)| format!("{domain} → {}", servers.values().join(", ")))
                    .unwrap_or_default();
                self.say(format!("{} · {summary}", target.label()));
            }
            _ => {
                if self.ui.input_nav(&key) {
                    return;
                }
                if let Some(c) = UiState::input_typing(&key) {
                    self.ui.input_insert(c);
                }
            }
        }
    }

    /// Core text fields: validate, save, then hot-patch via PATCH /configs
    /// when the field is runtime-patchable (ports, LAN/auth lists…), so the
    /// new value takes effect instantly. PATCH failure — or a field outside
    /// the schema — falls back to a daemon reload, which converges to the
    /// same saved values (PUT /configs hot reload, process restart only
    /// when that fails).
    async fn handle_core_text_input(&mut self, key: KeyEvent, field: CoreTextField) {
        match key.code {
            KeyCode::Esc => {
                self.ui.close_input();
                self.say(format!("{} edit cancelled", field.label()));
            }
            KeyCode::Backspace => {
                self.ui.input_backspace();
            }
            KeyCode::Enter => {
                let raw = self.ui.input_value().trim().to_owned();
                let previous_config = self.config.clone();
                let previous_api = self.api.clone();
                let summary = match field.apply(self, &raw) {
                    Ok(summary) => summary,
                    Err(message) => {
                        self.say(message);
                        return;
                    }
                };
                self.ui.close_input();
                if !self.save_config_or_restore(previous_config).await {
                    self.api = previous_api;
                    return;
                }
                crate::logger::info("app", &format!("{} -> {summary}", field.label()));
                if self.remote {
                    // Remote: controller/secret/delay URL apply locally;
                    // runtime fields PATCH the remote endpoint directly.
                    if let Some(payload) = field.patch_payload(self) {
                        match self.api.patch_configs(payload).await {
                            Ok(()) => {
                                self.say(format!("{} {summary} (remote patched)", field.label()));
                                return;
                            }
                            Err(e) => {
                                self.say(format!("Saved, remote patch failed: {e}"));
                                return;
                            }
                        }
                    }
                    self.say(format!("{} {summary} saved", field.label()));
                    return;
                }
                if let Some(payload) = field.patch_payload(self) {
                    match self.api.patch_configs(payload).await {
                        Ok(()) => {
                            self.say(format!("{} {summary} (hot patched)", field.label()));
                            return;
                        }
                        Err(e) => {
                            crate::logger::warn(
                                "app",
                                &format!("{} hot patch failed: {e}", field.label()),
                            );
                        }
                    }
                }
                match core::request_restart().await {
                    Ok(()) => self.say(format!(
                        "{} {summary} saved, reload requested",
                        field.label()
                    )),
                    Err(err) => self.say(format!("Saved, restart request failed: {err}")),
                }
            }
            _ => {
                if self.ui.input_nav(&key) {
                    return;
                }
                if let Some(c) = UiState::input_typing(&key) {
                    self.ui.input_insert(c);
                }
            }
        }
    }

    /// Edit the geo download mirror (gh-proxy style prefix). Empty clears
    /// back to direct GitHub access. Applies to clashlime-side downloads
    /// immediately; the running core picks it up on next start.
    async fn handle_geo_mirror_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.ui.close_input();
                self.say("Geo mirror edit cancelled");
            }
            KeyCode::Backspace => {
                self.ui.input_backspace();
            }
            KeyCode::Enter => {
                let value = self.ui.input_value().trim().to_owned();
                self.ui.close_input();
                let previous = self.config.clone();
                self.config.geo.mirror = if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                };
                if !self.save_config_or_restore(previous).await {
                    return;
                }
                crate::logger::info("app", &format!("geo mirror -> {value}"));
                if value.is_empty() {
                    self.say("Geo mirror cleared (direct GitHub)");
                } else {
                    self.say(format!("Geo mirror {value} saved"));
                }
            }
            _ => {
                if self.ui.input_nav(&key) {
                    return;
                }
                if let Some(c) = UiState::input_typing(&key) {
                    self.ui.input_insert(c);
                }
            }
        }
    }

    /// Edit the proxy used for geo downloads (e.g. mihomo's own
    /// `http://127.0.0.1:7897`). Empty clears back to direct access.
    /// Applies to the next download immediately.
    async fn handle_geo_proxy_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.ui.close_input();
                self.say("Geo proxy edit cancelled");
            }
            KeyCode::Backspace => {
                self.ui.input_backspace();
            }
            KeyCode::Enter => {
                let value = self.ui.input_value().trim().to_owned();
                self.ui.close_input();
                let previous = self.config.clone();
                self.config.geo.proxy = if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                };
                if !self.save_config_or_restore(previous).await {
                    return;
                }
                crate::logger::info("app", &format!("geo proxy -> {value}"));
                if value.is_empty() {
                    self.say("Geo proxy cleared (direct access)");
                } else {
                    self.say(format!("Geo proxy {value} saved"));
                }
            }
            _ => {
                if self.ui.input_nav(&key) {
                    return;
                }
                if let Some(c) = UiState::input_typing(&key) {
                    self.ui.input_insert(c);
                }
            }
        }
    }

    async fn handle_geo_url_input(&mut self, key: KeyEvent, field: GeoUrlField) {
        match key.code {
            KeyCode::Esc => {
                self.ui.close_input();
                self.say(format!("{} edit cancelled", field.label()));
            }
            KeyCode::Backspace => {
                self.ui.input_backspace();
            }
            KeyCode::Enter => {
                let value = self.ui.input_value().trim().to_owned();
                if !value.is_empty()
                    && !value.starts_with("https://")
                    && !value.starts_with("http://")
                {
                    self.say(format!(
                        "{} must be a full https:// URL or empty",
                        field.label()
                    ));
                    return;
                }
                self.ui.close_input();
                let previous = self.config.clone();
                field.set(
                    self,
                    if value.is_empty() {
                        None
                    } else {
                        Some(value.clone())
                    },
                );
                if !self.save_config_or_restore(previous).await {
                    return;
                }
                crate::logger::info("app", &format!("{} -> {value}", field.label()));
                if value.is_empty() {
                    self.say(format!("{} cleared (mirror/direct)", field.label()));
                } else {
                    self.say(format!("{} saved", field.label()));
                }
            }
            _ => {
                if self.ui.input_nav(&key) {
                    return;
                }
                if let Some(c) = UiState::input_typing(&key) {
                    self.ui.input_insert(c);
                }
            }
        }
    }

    /// Per-profile update-control text fields. Enter validates, stores on
    /// the selected profile and saves profiles.yaml — no core restart:
    /// fetch settings take effect on the next update.
    async fn handle_profile_text_input(&mut self, key: KeyEvent, field: ProfileTextField) {
        match key.code {
            KeyCode::Esc => {
                self.ui.close_input();
                self.say(format!("{} edit cancelled", field.label()));
            }
            KeyCode::Backspace => {
                self.ui.input_backspace();
            }
            KeyCode::Enter => {
                let raw = self.ui.input_value().trim().to_owned();
                let previous = self.data.profiles.clone();
                let summary = match field.apply(self, &raw) {
                    Ok(summary) => summary,
                    Err(message) => {
                        self.say(message);
                        return;
                    }
                };
                self.ui.close_input();
                let candidate = self.data.profiles.clone();
                if let Err(error) = crate::ipc::save_profiles(candidate).await {
                    self.data.profiles = previous;
                    self.say(format!("Save failed: {error}"));
                    return;
                }
                crate::logger::info("app", &format!("profile {} -> {summary}", field.label()));
                if matches!(field, ProfileTextField::Name) {
                    self.say(format!("Profile renamed to {summary}"));
                } else {
                    self.say(format!("Profile {} {summary} saved", field.label()));
                }
            }
            _ => {
                if self.ui.input_nav(&key) {
                    return;
                }
                if let Some(c) = UiState::input_typing(&key) {
                    self.ui.input_insert(c);
                }
            }
        }
    }

    /// Search the Logs tab. Enter applies the substring filter and
    /// resumes following the tail; Esc clears the search entirely.
    fn handle_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.ui.close_input();
                self.ui.log_query.clear();
                self.follow_logs();
                self.say("Log search cleared");
            }
            KeyCode::Backspace => {
                self.ui.input_backspace();
                self.apply_live_log_search();
            }
            KeyCode::Enter => {
                self.ui.log_query = self.ui.input_value().trim().to_owned();
                self.ui.close_input();
                self.ui.log_hscroll = 0;
                self.follow_logs();
                if self.ui.log_query.is_empty() {
                    self.say("Log search cleared");
                } else {
                    self.say(format!("Log search: '{}'", self.ui.log_query));
                }
            }
            _ => {
                if self.ui.input_nav(&key) {
                    self.apply_live_log_search();
                    return;
                }
                if let Some(c) = UiState::input_typing(&key) {
                    self.ui.input_insert(c);
                    self.apply_live_log_search();
                }
            }
        }
    }

    /// Live-apply while typing: the Logs title count updates per keystroke.
    /// Cursor navigation must not reset horizontal scroll when the filter
    /// itself did not change.
    fn apply_live_log_search(&mut self) {
        let query = self.ui.input_value().trim().to_owned();
        if query == self.ui.log_query {
            return;
        }
        self.ui.log_query = query;
        self.ui.log_hscroll = 0;
    }

    /// Search the Rules tab. Enter applies the substring filter over
    /// type/payload/policy and resets the cursor; Esc clears it.
    fn handle_rule_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.ui.close_input();
                self.ui.rule_query.clear();
                self.ui.rule_index = 0;
                self.say("Rule search cleared");
            }
            KeyCode::Backspace => {
                self.ui.input_backspace();
                self.apply_live_rule_search();
            }
            KeyCode::Enter => {
                self.ui.rule_query = self.ui.input_value().trim().to_owned();
                self.ui.close_input();
                self.ui.rule_index = 0;
                if self.ui.rule_query.is_empty() {
                    self.say("Rule search cleared");
                } else {
                    self.say(format!("Rule search: '{}'", self.ui.rule_query));
                }
            }
            _ => {
                if self.ui.input_nav(&key) {
                    self.apply_live_rule_search();
                    return;
                }
                if let Some(c) = UiState::input_typing(&key) {
                    self.ui.input_insert(c);
                    self.apply_live_rule_search();
                }
            }
        }
    }

    /// Live-apply while typing: the Rules title count updates per keystroke.
    /// Cursor-only keys keep the current selection.
    fn apply_live_rule_search(&mut self) {
        let query = self.ui.input_value().trim().to_owned();
        if query == self.ui.rule_query {
            return;
        }
        self.ui.rule_query = query;
        self.ui.rule_index = 0;
    }

    /// Search the Proxies node list. Enter keeps the live-applied substring
    /// filter over node names and resets the cursor; Esc clears it.
    fn handle_node_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.ui.close_input();
                self.ui.node_query.clear();
                self.ui.node_index = 0;
                self.say("Node search cleared");
            }
            KeyCode::Backspace => {
                self.ui.input_backspace();
                self.apply_live_node_search();
            }
            KeyCode::Enter => {
                self.ui.node_query = self.ui.input_value().trim().to_owned();
                self.ui.close_input();
                self.ui.node_index = 0;
                if self.ui.node_query.is_empty() {
                    self.say("Node search cleared");
                } else {
                    self.say(format!("Node search: '{}'", self.ui.node_query));
                }
            }
            _ => {
                if self.ui.input_nav(&key) {
                    self.apply_live_node_search();
                    return;
                }
                if let Some(c) = UiState::input_typing(&key) {
                    self.ui.input_insert(c);
                    self.apply_live_node_search();
                }
            }
        }
    }

    /// Live-apply while typing: the Nodes title count updates per keystroke.
    /// Cursor-only keys keep the current selection.
    fn apply_live_node_search(&mut self) {
        let query = self.ui.input_value().trim().to_owned();
        if query == self.ui.node_query {
            return;
        }
        self.ui.node_query = query;
        self.ui.node_index = 0;
    }

    fn handle_import_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.ui.close_input();
                // Esc backs out to the dialog when it is open; the
                // dialog itself is dismissed by a second Esc.
                if self.ui.import_editor {
                    self.say("Import cancelled");
                }
            }
            KeyCode::Backspace => {
                self.ui.input_backspace();
            }
            KeyCode::Enter => {
                // Commit to the draft and return to the dialog; the
                // import itself only runs from the dialog's Import row.
                self.ui.import_draft.source = self.ui.input_value().trim().to_owned();
                self.ui.close_input();
            }
            _ => {
                if self.ui.input_nav(&key) {
                    return;
                }
                if let Some(c) = UiState::input_typing(&key) {
                    self.ui.input_insert(c);
                }
            }
        }
    }

    /// Text sub-inputs of the import dialog. Enter writes the value back
    /// into the draft and returns to the dialog; nothing is saved to
    /// disk until the import itself is submitted.
    fn handle_import_draft_input(&mut self, key: KeyEvent, mode: &InputMode) {
        match key.code {
            KeyCode::Esc => {
                self.ui.close_input();
            }
            KeyCode::Backspace => {
                self.ui.input_backspace();
            }
            KeyCode::Enter => {
                let raw = self.ui.input_value().trim().to_owned();
                match mode {
                    InputMode::ImportProfileName => {
                        if raw.chars().count() > 100 {
                            self.say("Keep the name within 100 characters");
                            return;
                        }
                        self.ui.import_draft.name = raw;
                    }
                    InputMode::ImportProfileInterval => {
                        self.ui.import_draft.interval = raw;
                    }
                    InputMode::ImportProfileTimeout => {
                        self.ui.import_draft.timeout = raw;
                    }
                    InputMode::ImportProfileAuth => {
                        self.ui.import_draft.auth_token = raw;
                    }
                    InputMode::ImportProfileUserAgent => {
                        self.ui.import_draft.user_agent = raw;
                    }
                    _ => {}
                }
                self.ui.close_input();
            }
            _ => {
                if self.ui.input_nav(&key) {
                    return;
                }
                if let Some(c) = UiState::input_typing(&key) {
                    self.ui.input_insert(c);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DnsTextField, apply_port, is_ip_cidr, is_ip_or_cidr, validate_ip_list};
    use crate::app::{App, DataState, InputMode, TaskHub, UiState};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn test_app() -> App {
        App {
            config: crate::config::Config::default(),
            api: crate::api::MihomoClient::new("http://127.0.0.1:9090", String::new()).unwrap(),
            remote: false,
            theme: crate::theme::Theme::default(),
            ui: UiState::default(),
            data: DataState::default(),
            tasks: TaskHub::default(),
        }
    }

    #[test]
    fn invalid_dns_value_does_not_enable_dns() {
        let mut app = test_app();
        app.config.dns.enable = false;
        assert!(DnsTextField::Listen.apply(&mut app, "").is_err());
        assert!(!app.config.dns.enable);
        assert!(DnsTextField::Servers.apply(&mut app, " , ").is_err());
        assert!(!app.config.dns.enable);
        assert_eq!(
            DnsTextField::Listen.apply(&mut app, "0.0.0.0:1053"),
            Ok("0.0.0.0:1053".into())
        );
        assert!(app.config.dns.enable);
    }

    #[test]
    fn generic_editor_modes_have_title_and_hint() {
        for mode in [
            InputMode::ImportProfileName,
            InputMode::SearchLogs,
            InputMode::EditDnsListen,
            InputMode::EditDnsPolicyDomain,
            InputMode::EditHostsServers,
            InputMode::EditMixedPort,
            InputMode::EditGeoMirror,
            InputMode::EditGeoProxy,
            InputMode::EditGeoIpUrl,
            InputMode::EditProfileName,
        ] {
            let (title, hint) = mode
                .editor()
                .unwrap_or_else(|| panic!("no editor: {mode:?}"));
            assert!(!title.is_empty(), "{mode:?}");
            assert!(!hint.is_empty(), "{mode:?}");
        }
        for mode in [
            InputMode::ImportProfile,
            InputMode::CorePath,
            InputMode::RestoreBackup("backup.tar".into()),
        ] {
            assert!(mode.editor().is_none(), "{mode:?} has a dedicated overlay");
        }
    }

    #[tokio::test]
    async fn search_cursor_navigation_does_not_reset_live_results() {
        let mut app = test_app();
        app.ui.log_query = "error".into();
        app.ui.log_hscroll = 7;
        app.ui.begin_input(InputMode::SearchLogs, "error".into());

        app.handle_input(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .await;
        assert_eq!(app.ui.log_query, "error");
        assert_eq!(app.ui.log_hscroll, 7);

        app.handle_input(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE))
            .await;
        app.handle_input(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
            .await;
        assert_eq!(app.ui.log_query, "rror");
        assert_eq!(app.ui.log_hscroll, 0);
    }

    #[test]
    fn empty_port_input_disables() {
        let mut slot = Some(7891);
        assert_eq!(apply_port(&mut slot, "", "7891"), Ok("— (off)".into()));
        assert_eq!(slot, None);
        assert_eq!(apply_port(&mut slot, "   ", "7891"), Ok("— (off)".into()));
        assert_eq!(slot, None);
    }

    #[test]
    fn port_input_validates_range() {
        let mut slot = None;
        assert_eq!(apply_port(&mut slot, "7892", "7891"), Ok("7892".into()));
        assert_eq!(slot, Some(7892));
        assert!(apply_port(&mut slot, "0", "7891").is_err());
        assert!(apply_port(&mut slot, "99999", "7891").is_err());
        assert!(apply_port(&mut slot, "abc", "7891").is_err());
        // Rejected values keep the previous setting.
        assert_eq!(slot, Some(7892));
    }

    #[test]
    fn lan_ip_lists_require_cidr() {
        // mihomo parses these with netip.ParsePrefix: a bare address aborts
        // the whole config, so the editor must reject it up front.
        assert!(validate_ip_list("::1", "LAN allowed").is_err());
        assert!(validate_ip_list("127.0.0.1", "LAN allowed").is_err());
        assert!(validate_ip_list("10.0.0.0/8, ::1", "LAN allowed").is_err());
        assert_eq!(
            validate_ip_list("10.0.0.0/8, ::1/128", "LAN allowed"),
            Ok(vec!["10.0.0.0/8".to_owned(), "::1/128".to_owned()])
        );
        // Empty clears the list (mihomo then uses its 0.0.0.0/0 + ::/0 default).
        assert_eq!(validate_ip_list("  ", "LAN allowed"), Ok(vec![]));
        // The error names the offending entry and the expected shape.
        let error = validate_ip_list("::1", "LAN allowed").unwrap_err();
        assert!(error.contains("::1"), "{error}");
        assert!(error.contains("CIDR"), "{error}");
    }

    #[test]
    fn cidr_check_is_stricter_than_ip_or_cidr() {
        assert!(!is_ip_cidr("127.0.0.1"));
        assert!(is_ip_cidr("127.0.0.1/32"));
        assert!(is_ip_cidr("::1/128"));
        // Out-of-range prefix lengths stay rejected.
        assert!(!is_ip_cidr("127.0.0.1/33"));
        assert!(!is_ip_cidr("::1/129"));
        // The looser helper (route-exclude-address) still accepts bare IPs.
        assert!(is_ip_or_cidr("127.0.0.1"));
    }
}
