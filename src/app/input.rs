use crate::{backup, core, profiles::Profiles};
use crossterm::event::{KeyCode, KeyEvent};

use super::{CoreMissingChoice, InputMode};

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

    fn parse_list(value: &str) -> Vec<String> {
        value
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Apply the entered value to config. Returns a summary for the status
    /// line, or a validation message (buffer is kept for correction).
    fn apply(self, app: &mut super::App, value: &str) -> Result<String, String> {
        let dns = &mut app.config.dns;
        // Editing DNS implies wanting it on.
        if !dns.enable {
            dns.enable = true;
        }
        match self {
            Self::Listen => {
                if value.is_empty() {
                    return Err("DNS listen cannot be empty (e.g. 0.0.0.0:1053)".into());
                }
                dns.listen = value.to_owned();
                Ok(value.to_owned())
            }
            Self::Servers => {
                let servers = Self::parse_list(value);
                if servers.is_empty() {
                    return Err("Enter comma-separated DNS servers (e.g. 223.5.5.5, 8.8.8.8)".into());
                }
                dns.nameserver = servers.clone();
                Ok(servers.join(", "))
            }
            Self::FakeIpRange => {
                dns.fake_ip_range = none_if_empty(value);
                Ok(dns.fake_ip_range.clone().unwrap_or_else(|| "—".into()))
            }
            Self::FakeIpFilter => {
                dns.fake_ip_filter = Self::parse_list(value);
                Ok(or_dash(&dns.fake_ip_filter.join(", ")))
            }
            Self::DefaultNs => {
                dns.default_nameserver = Self::parse_list(value);
                Ok(or_dash(&dns.default_nameserver.join(", ")))
            }
            Self::DirectNs => {
                dns.direct_nameserver = Self::parse_list(value);
                Ok(or_dash(&dns.direct_nameserver.join(", ")))
            }
            Self::ProxyNs => {
                dns.proxy_server_nameserver = Self::parse_list(value);
                Ok(or_dash(&dns.proxy_server_nameserver.join(", ")))
            }
            Self::Fallback => {
                dns.fallback = Self::parse_list(value);
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
        }
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

/// Text-editable core fields (ports, controller, secret…). Unlike DNS edits
/// these apply via config save + core restart; controller/secret also
/// rebuild the API client so the TUI talks to the new endpoint.
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

    pub(crate) fn initial(self, app: &super::App) -> String {
        match self {
            Self::MixedPort => app.config.mixed_port.to_string(),
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
            Self::MixedPort => {
                let port: u16 = value
                    .parse()
                    .map_err(|_| "Enter a port 1-65535 (e.g. 7890)".to_owned())?;
                if port == 0 {
                    return Err("Enter a port 1-65535 (e.g. 7890)".into());
                }
                app.config.mixed_port = port;
                Ok(port.to_string())
            }
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
                Ok(if value.is_empty() { "— (cleared)".into() } else { "••••••".into() })
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
            Self::SocksPort => apply_port(&mut app.config.socks_port, value),
            Self::HttpPort => apply_port(&mut app.config.http_port, value),
            Self::RedirPort => apply_port(&mut app.config.redir_port, value),
            Self::TproxyPort => apply_port(&mut app.config.tproxy_port, value),
            Self::Auth => {
                app.config.authentication = parse_list(value);
                Ok(or_dash(&app.config.authentication.join(", ")))
            }
            Self::SkipAuth => {
                app.config.skip_auth_prefixes = parse_list(value);
                Ok(or_dash(&app.config.skip_auth_prefixes.join(", ")))
            }
            Self::LanAllowed => {
                app.config.lan_allowed_ips = parse_list(value);
                Ok(or_dash(&app.config.lan_allowed_ips.join(", ")))
            }
            Self::LanDisallowed => {
                app.config.lan_disallowed_ips = parse_list(value);
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

/// Plain IP or CIDR range (`192.168.0.0/16`, `::1/128`), used for
/// `route-exclude-address` validation.
fn is_ip_or_cidr(value: &str) -> bool {
    use std::net::IpAddr;
    if value.parse::<IpAddr>().is_ok() {
        return true;
    }
    let Some((addr, prefix)) = value.split_once('/') else {
        return false;
    };
    let Ok(ip) = addr.parse::<IpAddr>() else {
        return false;
    };
    let Ok(bits): Result<u8, _> = prefix.parse() else {
        return false;
    };
    bits <= if ip.is_ipv4() { 32 } else { 128 }
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

/// Dedicated listener ports: required (empty keeps the profile value, so
/// there is nothing valid to save).
fn apply_port(slot: &mut Option<u16>, value: &str) -> Result<String, String> {
    let port: u16 = value
        .parse()
        .map_err(|_| "Enter a port 1-65535 (e.g. 7891)".to_owned())?;
    if port == 0 {
        return Err("Enter a port 1-65535 (e.g. 7891)".into());
    }
    *slot = Some(port);
    Ok(port.to_string())
}

impl super::App {
    pub(crate) async fn handle_input(&mut self, key: KeyEvent) {
        if let Some(InputMode::RestoreBackup(path)) = self.input.clone() {
            self.handle_restore_input(key, &path).await;
            return;
        }
        if matches!(self.input, Some(InputMode::CorePath)) {
            self.handle_core_path_input(key);
            return;
        }
        if let Some(field) = self.input.clone().as_ref().and_then(DnsTextField::from_mode)
        {
            self.handle_dns_text_input(key, field).await;
            return;
        }
        if let Some(field) = self.input.clone().as_ref().and_then(CoreTextField::from_mode)
        {
            self.handle_core_text_input(key, field).await;
            return;
        }
        if matches!(self.input, Some(InputMode::EditGeoMirror)) {
            self.handle_geo_mirror_input(key);
            return;
        }
        if matches!(self.input, Some(InputMode::EditGeoProxy)) {
            self.handle_geo_proxy_input(key);
            return;
        }
        self.handle_import_input(key);
    }

    async fn handle_restore_input(&mut self, key: KeyEvent, path: &std::path::Path) {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                self.input = None;
                match backup::restore(path) {
                    Ok(()) => match Profiles::load() {
                        Ok(profiles) => {
                            self.profiles = profiles;
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
                self.input = None;
                self.say("Restore cancelled");
            }
            _ => {}
        }
    }

    fn handle_core_path_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input = None;
                self.input_buffer.clear();
                self.reopen_core_missing_dialog(CoreMissingChoice::ProvidePath);
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => self.input_buffer.push(c),
            KeyCode::Enter => {
                let value = self.input_buffer.trim().to_owned();
                self.input_buffer.clear();
                self.input = None;
                if value.is_empty() {
                    self.say("Enter an absolute path to the mihomo binary");
                } else {
                    self.apply_core_path(&value);
                }
                // Validation may have failed; give the user another chance
                self.reopen_core_missing_dialog(CoreMissingChoice::Download);
            }
            _ => {}
        }
    }

    /// Text-editable DNS fields sharing Esc/Backspace/Char/Enter handling.
    /// Enter applies the value, saves config, and hot-patches the running
    /// core with a restart fallback — same flow for all DNS text fields.
    async fn handle_dns_text_input(&mut self, key: KeyEvent, field: DnsTextField) {
        match key.code {
            KeyCode::Esc => {
                self.input = None;
                self.input_buffer.clear();
                self.say(format!("{} edit cancelled", field.label()));
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => self.input_buffer.push(c),
            KeyCode::Enter => {
                let raw = self.input_buffer.trim().to_owned();
                let summary = match field.apply(self, &raw) {
                    Ok(summary) => summary,
                    Err(message) => {
                        self.say(message);
                        return;
                    }
                };
                self.input_buffer.clear();
                self.input = None;
                if let Err(e) = self.config.save() {
                    self.say(format!("Save failed: {e}"));
                    return;
                }
                crate::logger::info("app", &format!("{} -> {summary}", field.label()));
                match self.api.update_dns(&self.config.dns).await {
                    Ok(()) => self.say(format!("{} {summary} (hot patched)", field.label())),
                    Err(e) => {
                        crate::logger::warn(
                            "app",
                            &format!("{} hot patch failed: {e}", field.label()),
                        );
                        match core::request_restart().await {
                            Ok(()) => {
                                self.say(format!("{} {summary} saved, reload requested", field.label()))
                            }
                            Err(err) => {
                                self.say(format!("Save ok but reload failed: {err} (hot: {e})"))
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Core text fields: validate, save, and restart the core so the new
    /// ports/controller take effect.
    async fn handle_core_text_input(&mut self, key: KeyEvent, field: CoreTextField) {
        match key.code {
            KeyCode::Esc => {
                self.input = None;
                self.input_buffer.clear();
                self.say(format!("{} edit cancelled", field.label()));
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => self.input_buffer.push(c),
            KeyCode::Enter => {
                let raw = self.input_buffer.trim().to_owned();
                let summary = match field.apply(self, &raw) {
                    Ok(summary) => summary,
                    Err(message) => {
                        self.say(message);
                        return;
                    }
                };
                self.input_buffer.clear();
                self.input = None;
                if let Err(e) = self.config.save() {
                    self.say(format!("Save failed: {e}"));
                    return;
                }
                crate::logger::info("app", &format!("{} -> {summary}", field.label()));
                match core::request_restart().await {
                    Ok(()) => self.say(format!("{} {summary} saved, reload requested", field.label())),
                    Err(err) => self.say(format!("Saved, restart request failed: {err}")),
                }
            }
            _ => {}
        }
    }

    /// Edit the geo download mirror (gh-proxy style prefix). Empty clears
    /// back to direct GitHub access. Applies to omash-side downloads
    /// immediately; the running core picks it up on next start.
    fn handle_geo_mirror_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input = None;
                self.input_buffer.clear();
                self.say("Geo mirror edit cancelled");
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => self.input_buffer.push(c),
            KeyCode::Enter => {
                let value = self.input_buffer.trim().to_owned();
                self.input_buffer.clear();
                self.input = None;
                self.config.geo.mirror = if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                };
                if let Err(e) = self.config.save() {
                    self.say(format!("Save failed: {e}"));
                    return;
                }
                crate::logger::info("app", &format!("geo mirror -> {value}"));
                if value.is_empty() {
                    self.say("Geo mirror cleared (direct GitHub)");
                } else {
                    self.say(format!("Geo mirror {value} saved"));
                }
            }
            _ => {}
        }
    }

    /// Edit the proxy used for geo downloads (e.g. mihomo's own
    /// `http://127.0.0.1:7897`). Empty clears back to direct access.
    /// Applies to the next download immediately.
    fn handle_geo_proxy_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input = None;
                self.input_buffer.clear();
                self.say("Geo proxy edit cancelled");
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => self.input_buffer.push(c),
            KeyCode::Enter => {
                let value = self.input_buffer.trim().to_owned();
                self.input_buffer.clear();
                self.input = None;
                self.config.geo.proxy = if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                };
                if let Err(e) = self.config.save() {
                    self.say(format!("Save failed: {e}"));
                    return;
                }
                crate::logger::info("app", &format!("geo proxy -> {value}"));
                if value.is_empty() {
                    self.say("Geo proxy cleared (direct access)");
                } else {
                    self.say(format!("Geo proxy {value} saved"));
                }
            }
            _ => {}
        }
    }

    fn handle_import_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input = None;
                self.input_buffer.clear();
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(character) => self.input_buffer.push(character),
            KeyCode::Enter => {
                let value = self.input_buffer.trim().to_owned();
                if value.is_empty() {
                    self.say("Enter a subscription URL or an absolute YAML file path.");
                    return;
                }
                // Runs in the background (fetch + geo + `mihomo -t` can take
                // a while); progress and the result arrive via the run loop.
                self.start_import(value);
            }
            _ => {}
        }
    }
}
