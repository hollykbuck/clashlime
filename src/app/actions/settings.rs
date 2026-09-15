use crate::{
    app::{
        InputMode, SettingSection,
        backend::Capability,
        input::{CoreTextField, DnsTextField, GeoUrlField},
    },
    backup, core,
};
use serde_json::{Value, json};

impl crate::app::App {
    pub(crate) async fn toggle_setting(&mut self) {
        if self.remote {
            self.toggle_setting_remote().await;
            return;
        }
        use SettingSection::{Core, Dns, Geo, Network, Ports, Tun};
        let mut restart = false;
        let mut dns_hot_patch = false;
        // Payload for mihomo PATCH /configs (configSchema) when the toggled
        // setting is runtime-patchable; hot-patch first, daemon reload on
        // failure — same pattern as DNS toggles below.
        let mut runtime_patch: Option<Value> = None;
        match (self.ui.setting_section, self.ui.setting_index) {
            (Core, 0) => {
                let enable = !core::core_desired_enabled().await;
                if let Err(error) = core::request_core_enabled(enable).await {
                    self.say(format!("Core state change failed: {error}"));
                    return;
                }
            }
            (Core, 1) => {
                let previous = self.config.auto_start;
                self.config.auto_start = !previous;
                if let Err(error) = self.config.save() {
                    self.config.auto_start = previous;
                    self.say(format!("Save failed: {error}"));
                    return;
                }
                if let Err(error) = core::set_supervisor_autostart(self.config.auto_start).await {
                    self.config.auto_start = previous;
                    let rollback = self.config.save().err();
                    self.say(format!("Autostart change failed: {error}"));
                    if let Some(rollback) = rollback {
                        let rollback_text = format!("; rollback failed: {rollback}");
                        self.data.status.push_str(&rollback_text);
                    }
                    return;
                }
                self.say("Autostart setting saved");
                return;
            }
            (Network, 0) => {
                self.config.system_proxy = !self.config.system_proxy;
                restart = true;
            }
            (Network, 1) => {
                self.ui.input = Some(InputMode::EditProxyBypass);
                self.ui.input_buffer = CoreTextField::ProxyBypass.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Network, 2) => {
                self.config.allow_lan = !self.config.allow_lan;
                runtime_patch = Some(json!({ "allow-lan": self.config.allow_lan }));
            }
            (Network, 3) => {
                self.ui.input = Some(InputMode::EditLanAllowed);
                self.ui.input_buffer = CoreTextField::LanAllowed.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Network, 4) => {
                self.ui.input = Some(InputMode::EditLanDisallowed);
                self.ui.input_buffer = CoreTextField::LanDisallowed.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Network, 5) => {
                self.config.ipv6 = !self.config.ipv6;
                runtime_patch = Some(json!({ "ipv6": self.config.ipv6 }));
            }
            (Network, 6) => {
                // NOTE: `sniffing` is in the PATCH schema but a verified
                // no-op on our core (v1.19.30 accepts it yet GET still
                // reports false), so this stays on the daemon reload path.
                self.config.sniffer_enable = !self.config.sniffer_enable;
                restart = true;
            }
            (Network, 7) => {
                // Sniffer override: off keeps the subscription `sniffer`
                // section untouched (rebuild applies it, no hot patch).
                self.config.sniffer.override_profile = !self.config.sniffer.override_profile;
                restart = true;
            }
            (Network, 8) => {
                self.config.sniffer.force_dns_mapping =
                    Some(!self.config.sniffer.force_dns_mapping.unwrap_or(false));
                restart = true;
            }
            (Network, 9) => {
                self.config.sniffer.parse_pure_ip =
                    Some(!self.config.sniffer.parse_pure_ip.unwrap_or(false));
                restart = true;
            }
            (Network, 10) => {
                self.config.sniffer.override_destination =
                    Some(!self.config.sniffer.override_destination.unwrap_or(false));
                restart = true;
            }
            (Network, 11) => {
                self.ui.input = Some(InputMode::EditSniffHttpPorts);
                self.ui.input_buffer = CoreTextField::SniffHttpPorts.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Network, 12) => {
                self.ui.input = Some(InputMode::EditSniffTlsPorts);
                self.ui.input_buffer = CoreTextField::SniffTlsPorts.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Ports, 0) => {
                self.ui.input = Some(InputMode::EditSocksPort);
                self.ui.input_buffer = CoreTextField::SocksPort.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Ports, 1) => {
                self.ui.input = Some(InputMode::EditHttpPort);
                self.ui.input_buffer = CoreTextField::HttpPort.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Ports, 2) => {
                self.ui.input = Some(InputMode::EditRedirPort);
                self.ui.input_buffer = CoreTextField::RedirPort.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Ports, 3) => {
                self.ui.input = Some(InputMode::EditTproxyPort);
                self.ui.input_buffer = CoreTextField::TproxyPort.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Ports, 4) => {
                self.ui.input = Some(InputMode::EditAuth);
                self.ui.input_buffer = CoreTextField::Auth.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Ports, 5) => {
                self.ui.input = Some(InputMode::EditSkipAuth);
                self.ui.input_buffer = CoreTextField::SkipAuth.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Ports, 6) => {
                self.config.tcp_concurrent = Some(!self.config.tcp_concurrent.unwrap_or(false));
                runtime_patch =
                    Some(json!({ "tcp-concurrent": self.config.tcp_concurrent.unwrap_or(false) }));
            }
            (Ports, 7) => {
                self.config.unified_delay = Some(!self.config.unified_delay.unwrap_or(false));
                restart = true;
            }
            (Tun, 0) => {
                self.config.tun.enable = !self.config.tun.enable;
                restart = true;
            }
            (Tun, 1) => {
                // Cycle TUN stack: gVisor -> System -> Mixed.
                self.config.tun.stack = Some(
                    match self.config.tun.stack.as_deref() {
                        Some("gVisor") => "System",
                        Some("System") => "Mixed",
                        _ => "gVisor",
                    }
                    .to_owned(),
                );
                restart = true;
            }
            (Tun, 2) => {
                self.ui.input = Some(InputMode::EditTunDevice);
                self.ui.input_buffer = CoreTextField::TunDevice.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Tun, 3) => {
                self.config.tun.auto_route = Some(!self.config.tun.auto_route.unwrap_or(false));
                restart = true;
            }
            (Tun, 4) => {
                self.config.tun.auto_detect_interface =
                    Some(!self.config.tun.auto_detect_interface.unwrap_or(false));
                restart = true;
            }
            (Tun, 5) => {
                self.ui.input = Some(InputMode::EditTunDnsHijack);
                self.ui.input_buffer = CoreTextField::TunDnsHijack.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Tun, 6) => {
                self.ui.input = Some(InputMode::EditTunMtu);
                self.ui.input_buffer = CoreTextField::TunMtu.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Tun, 7) => {
                self.config.tun.strict_route = Some(!self.config.tun.strict_route.unwrap_or(false));
                restart = true;
            }
            (Tun, 8) => {
                self.config.tun.auto_redirect =
                    Some(!self.config.tun.auto_redirect.unwrap_or(false));
                restart = true;
            }
            (Tun, 9) => {
                self.ui.input = Some(InputMode::EditTunRouteExclude);
                self.ui.input_buffer = CoreTextField::TunRouteExclude.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Core, 2) => {
                self.config.refresh_ms = if self.config.refresh_ms >= 5000 {
                    500
                } else {
                    self.config.refresh_ms + 500
                };
            }
            (Core, 3) => {
                self.ui.input = Some(InputMode::EditMixedPort);
                self.ui.input_buffer = CoreTextField::MixedPort.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Core, 4) => {
                self.ui.input = Some(InputMode::EditController);
                self.ui.input_buffer = CoreTextField::Controller.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Core, 5) => {
                self.ui.input = Some(InputMode::EditSecret);
                self.ui.input_buffer = CoreTextField::Secret.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Core, 6) => {
                // Cycle core log level.
                const LEVELS: [&str; 5] = ["silent", "error", "warning", "info", "debug"];
                let next = LEVELS
                    .iter()
                    .position(|level| *level == self.config.log_level)
                    .map(|i| LEVELS[(i + 1) % LEVELS.len()])
                    .unwrap_or("info");
                self.config.log_level = next.to_owned();
                runtime_patch = Some(json!({ "log-level": self.config.log_level }));
            }
            (Core, 7) => {
                self.ui.input = Some(InputMode::EditDelayTestUrl);
                self.ui.input_buffer = CoreTextField::DelayTestUrl.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Core, 8) => {
                // Cycle process matching: strict -> off -> always -> profile.
                self.config.find_process_mode = match self.config.find_process_mode.as_deref() {
                    Some("strict") => Some("off".into()),
                    Some("off") => Some("always".into()),
                    Some("always") => None,
                    _ => Some("strict".into()),
                };
                restart = true;
            }
            (Dns, 0) => {
                self.config.dns.enable = !self.config.dns.enable;
                dns_hot_patch = true;
            }
            (Dns, 1) => {
                // DNS override: off keeps the subscription `dns` section
                // untouched (rebuild applies it, no hot patch).
                self.config.dns.override_profile = !self.config.dns.override_profile;
                restart = true;
            }
            (Dns, 2) => {
                // Cycle enhanced mode: fake-ip -> redir-host -> normal.
                self.config.dns.enhanced_mode = Some(
                    match self.config.dns.enhanced_mode.as_deref() {
                        Some("fake-ip") => "redir-host",
                        Some("redir-host") => "normal",
                        _ => "fake-ip",
                    }
                    .to_owned(),
                );
                dns_hot_patch = true;
            }
            (Dns, 3) => {
                self.ui.input = Some(InputMode::EditDnsFakeIpRange);
                self.ui.input_buffer = DnsTextField::FakeIpRange.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Dns, 4) => {
                // Cycle fake-ip filter mode: blacklist <-> whitelist.
                self.config.dns.fake_ip_filter_mode = Some(
                    match self.config.dns.fake_ip_filter_mode.as_deref() {
                        Some("blacklist") => "whitelist",
                        _ => "blacklist",
                    }
                    .to_owned(),
                );
                dns_hot_patch = true;
            }
            (Dns, 5) => {
                self.ui.input = Some(InputMode::EditDnsFakeIpFilter);
                self.ui.input_buffer = DnsTextField::FakeIpFilter.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Dns, 6) => {
                self.config.dns.ipv6 = !self.config.dns.ipv6;
                dns_hot_patch = true;
            }
            (Dns, 7) => {
                self.config.dns.respect_rules =
                    Some(!self.config.dns.respect_rules.unwrap_or(false));
                dns_hot_patch = true;
            }
            (Dns, 8) => {
                // Edit DNS listen address
                self.ui.input = Some(InputMode::EditDnsListen);
                self.ui.input_buffer = DnsTextField::Listen.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Dns, 9) => {
                // Edit DNS nameservers (comma separated)
                self.ui.input = Some(InputMode::EditDnsServers);
                self.ui.input_buffer = DnsTextField::Servers.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Dns, 10) => {
                self.ui.input = Some(InputMode::EditDnsDefaultNs);
                self.ui.input_buffer = DnsTextField::DefaultNs.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Dns, 11) => {
                self.ui.input = Some(InputMode::EditDnsDirectNs);
                self.ui.input_buffer = DnsTextField::DirectNs.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Dns, 12) => {
                self.ui.input = Some(InputMode::EditDnsProxyNs);
                self.ui.input_buffer = DnsTextField::ProxyNs.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Dns, 13) => {
                self.ui.input = Some(InputMode::EditDnsFallback);
                self.ui.input_buffer = DnsTextField::Fallback.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Dns, 14) => {
                self.ui.input = Some(InputMode::EditDnsFallbackGeoCode);
                self.ui.input_buffer = DnsTextField::FallbackGeoCode.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Geo, 0) => {
                // Geo data (geoip.metadb / geosite.dat): fetch missing
                // files in the background so slow networks never freeze
                // the UI; Esc cancels.
                self.start_geo_update();
                return;
            }
            (Geo, 1) => {
                // Edit geo download mirror (gh-proxy style prefix)
                self.ui.input = Some(InputMode::EditGeoMirror);
                self.ui.input_buffer = self.config.geo.mirror.clone().unwrap_or_default();
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Geo, 2) => {
                self.ui.input = Some(InputMode::EditGeoIpUrl);
                self.ui.input_buffer = GeoUrlField::GeoIp.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Geo, 3) => {
                self.ui.input = Some(InputMode::EditGeositeUrl);
                self.ui.input_buffer = GeoUrlField::Geosite.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Geo, 4) => {
                self.ui.input = Some(InputMode::EditMmdbUrl);
                self.ui.input_buffer = GeoUrlField::Mmdb.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Geo, 5) => {
                self.ui.input = Some(InputMode::EditAsnUrl);
                self.ui.input_buffer = GeoUrlField::Asn.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            (Geo, 6) => {
                self.config.geo.auto_update = !self.config.geo.auto_update;
                restart = true;
            }
            (Geo, 7) => {
                // Cycle geo update interval through sane presets.
                self.config.geo.update_interval = [6, 12, 24, 48, 168]
                    .into_iter()
                    .find(|preset| *preset > self.config.geo.update_interval)
                    .unwrap_or(6);
                restart = true;
            }
            (Geo, 8) => {
                // Edit proxy for geo downloads (e.g. mihomo mixed port).
                self.ui.input = Some(InputMode::EditGeoProxy);
                self.ui.input_buffer = self.config.geo.proxy.clone().unwrap_or_default();
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
                return;
            }
            _ => {}
        }
        if let Err(error) = self.config.save() {
            self.say(format!("Save failed: {error}"));
            return;
        }
        if dns_hot_patch {
            if !self.config.dns.override_profile {
                // Override off: never touch the running core's DNS; the
                // daemon rebuild restores the profile section instead.
                if let Err(error) = core::request_restart().await {
                    self.say(format!("Saved, restart request failed: {error}"));
                    return;
                }
                self.say("Setting saved, reload requested (override off)");
                return;
            }
            crate::logger::info("app", &format!("dns enable -> {}", self.config.dns.enable));
            match self.api.update_dns(&self.config.dns).await {
                Ok(()) => {
                    let state = if self.config.dns.enable {
                        "enabled"
                    } else {
                        "disabled"
                    };
                    self.say(format!("DNS {state} (hot patched)"));
                    return;
                }
                Err(e) => {
                    crate::logger::warn(
                        "app",
                        &format!("dns hot patch failed, fallback to reload: {e}"),
                    );
                    // fallback to runtime rebuild + reload
                    if let Err(err) = core::request_restart().await {
                        self.say(format!(
                            "DNS saved but reload failed: {err} (hot patch: {e})"
                        ));
                        return;
                    }
                    self.say("DNS saved, reload requested");
                    return;
                }
            }
        }
        if let Some(payload) = runtime_patch {
            match self.api.patch_configs(payload).await {
                Ok(()) => {
                    self.say("Setting saved (hot patched)");
                    return;
                }
                Err(e) => {
                    crate::logger::warn(
                        "app",
                        &format!("setting hot patch failed, fallback to reload: {e}"),
                    );
                    // fallback to runtime rebuild + reload
                    if let Err(err) = core::request_restart().await {
                        self.say(format!(
                            "Setting saved but reload failed: {err} (hot patch: {e})"
                        ));
                        return;
                    }
                    self.say("Setting saved, reload requested");
                    return;
                }
            }
        }
        if restart && let Err(error) = core::request_restart().await {
            self.say(format!("Saved, restart request failed: {error}"));
            return;
        }
        self.say("Setting saved");
    }

    /// Remote mode: no daemon reload, no local runtime rebuild. Local UI
    /// prefs (refresh/controller/secret/delay URL) save locally; runtime
    /// values go straight through `PATCH /configs` on the remote endpoint.
    /// Anything needing a local core restart is rejected.
    async fn toggle_setting_remote(&mut self) {
        use SettingSection::{Core, Dns, Network, Ports};
        // Text inputs that work in remote mode (handled on Enter without
        // any daemon restart — see `handle_core_text_input`).
        match (self.ui.setting_section, self.ui.setting_index) {
            (Core, 2) => {
                self.config.refresh_ms = if self.config.refresh_ms >= 5000 {
                    500
                } else {
                    self.config.refresh_ms + 500
                };
                if let Err(error) = self.config.save() {
                    self.say(format!("Save failed: {error}"));
                    return;
                }
                self.say(format!(
                    "Refresh interval {} ms saved",
                    self.config.refresh_ms
                ));
                return;
            }
            (Core, 4) | (Core, 5) | (Core, 7) => {
                // Controller / secret / delay URL: open the editor, the
                // remote-aware input handler finishes the job.
                self.toggle_setting_remote_input();
                return;
            }
            (Core, 6) => {
                const LEVELS: [&str; 5] = ["silent", "error", "warning", "info", "debug"];
                let next = LEVELS
                    .iter()
                    .position(|level| *level == self.config.log_level)
                    .map(|i| LEVELS[(i + 1) % LEVELS.len()])
                    .unwrap_or("info");
                self.config.log_level = next.to_owned();
                if let Err(error) = self.config.save() {
                    self.say(format!("Save failed: {error}"));
                    return;
                }
                match self
                    .api
                    .patch_configs(json!({ "log-level": self.config.log_level }))
                    .await
                {
                    Ok(()) => self.say(format!("Log level {next} (remote patched)")),
                    Err(error) => self.say(format!("Saved, remote patch failed: {error}")),
                }
                return;
            }
            (Network, 2) => {
                self.config.allow_lan = !self.config.allow_lan;
                if let Err(error) = self.config.save() {
                    self.say(format!("Save failed: {error}"));
                    return;
                }
                match self
                    .api
                    .patch_configs(json!({ "allow-lan": self.config.allow_lan }))
                    .await
                {
                    Ok(()) => self.say("Allow LAN (remote patched)"),
                    Err(error) => self.say(format!("Saved, remote patch failed: {error}")),
                }
                return;
            }
            (Network, 5) => {
                self.config.ipv6 = !self.config.ipv6;
                if let Err(error) = self.config.save() {
                    self.say(format!("Save failed: {error}"));
                    return;
                }
                match self
                    .api
                    .patch_configs(json!({ "ipv6": self.config.ipv6 }))
                    .await
                {
                    Ok(()) => self.say("IPv6 (remote patched)"),
                    Err(error) => self.say(format!("Saved, remote patch failed: {error}")),
                }
                return;
            }
            (Ports, 6) => {
                self.config.tcp_concurrent = Some(!self.config.tcp_concurrent.unwrap_or(false));
                if let Err(error) = self.config.save() {
                    self.say(format!("Save failed: {error}"));
                    return;
                }
                match self
                    .api
                    .patch_configs(
                        json!({ "tcp-concurrent": self.config.tcp_concurrent.unwrap_or(false) }),
                    )
                    .await
                {
                    Ok(()) => self.say("TCP concurrent (remote patched)"),
                    Err(error) => self.say(format!("Saved, remote patch failed: {error}")),
                }
                return;
            }
            (Dns, 0) => {
                self.config.dns.enable = !self.config.dns.enable;
                if let Err(error) = self.config.save() {
                    self.say(format!("Save failed: {error}"));
                    return;
                }
                match self.api.update_dns(&self.config.dns).await {
                    Ok(()) => self.say("DNS (remote patched)"),
                    Err(error) => self.say(format!("Saved, remote patch failed: {error}")),
                }
                return;
            }
            (Dns, 2) => {
                self.config.dns.enhanced_mode = Some(
                    match self.config.dns.enhanced_mode.as_deref() {
                        Some("fake-ip") => "redir-host",
                        Some("redir-host") => "normal",
                        _ => "fake-ip",
                    }
                    .to_owned(),
                );
                if let Err(error) = self.config.save() {
                    self.say(format!("Save failed: {error}"));
                    return;
                }
                match self.api.update_dns(&self.config.dns).await {
                    Ok(()) => self.say("DNS mode (remote patched)"),
                    Err(error) => self.say(format!("Saved, remote patch failed: {error}")),
                }
                return;
            }
            (Dns, 4) => {
                self.config.dns.fake_ip_filter_mode = Some(
                    match self.config.dns.fake_ip_filter_mode.as_deref() {
                        Some("blacklist") => "whitelist",
                        _ => "blacklist",
                    }
                    .to_owned(),
                );
                if let Err(error) = self.config.save() {
                    self.say(format!("Save failed: {error}"));
                    return;
                }
                match self.api.update_dns(&self.config.dns).await {
                    Ok(()) => self.say("Fake IP filter mode (remote patched)"),
                    Err(error) => self.say(format!("Saved, remote patch failed: {error}")),
                }
                return;
            }
            (Dns, 6) => {
                self.config.dns.ipv6 = !self.config.dns.ipv6;
                if let Err(error) = self.config.save() {
                    self.say(format!("Save failed: {error}"));
                    return;
                }
                match self.api.update_dns(&self.config.dns).await {
                    Ok(()) => self.say("DNS IPv6 (remote patched)"),
                    Err(error) => self.say(format!("Saved, remote patch failed: {error}")),
                }
                return;
            }
            (Dns, 7) => {
                self.config.dns.respect_rules =
                    Some(!self.config.dns.respect_rules.unwrap_or(false));
                if let Err(error) = self.config.save() {
                    self.say(format!("Save failed: {error}"));
                    return;
                }
                match self.api.update_dns(&self.config.dns).await {
                    Ok(()) => self.say("Respect rules (remote patched)"),
                    Err(error) => self.say(format!("Saved, remote patch failed: {error}")),
                }
                return;
            }
            (Dns, 3)
            | (Dns, 5)
            | (Dns, 8)
            | (Dns, 9)
            | (Dns, 10)
            | (Dns, 11)
            | (Dns, 12)
            | (Dns, 13)
            | (Dns, 14) => {
                // DNS text editors; the remote-aware input handler patches.
                self.toggle_setting_remote_input();
                return;
            }
            _ => {}
        }
        self.say("Not available in remote mode (local core setting)");
    }

    /// Open the text editor for remote-capable rows. Every other row is
    /// rejected before reaching here.
    fn toggle_setting_remote_input(&mut self) {
        use crate::app::input::{CoreTextField, DnsTextField};
        use SettingSection::{Core, Dns};
        match (self.ui.setting_section, self.ui.setting_index) {
            (Core, 4) => {
                self.ui.input = Some(InputMode::EditController);
                self.ui.input_buffer = CoreTextField::Controller.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
            }
            (Core, 5) => {
                self.ui.input = Some(InputMode::EditSecret);
                self.ui.input_buffer = CoreTextField::Secret.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
            }
            (Core, 7) => {
                self.ui.input = Some(InputMode::EditDelayTestUrl);
                self.ui.input_buffer = CoreTextField::DelayTestUrl.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
            }
            (Dns, 3) => {
                self.ui.input = Some(InputMode::EditDnsFakeIpRange);
                self.ui.input_buffer = DnsTextField::FakeIpRange.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
            }
            (Dns, 5) => {
                self.ui.input = Some(InputMode::EditDnsFakeIpFilter);
                self.ui.input_buffer = DnsTextField::FakeIpFilter.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
            }
            (Dns, 8) => {
                self.ui.input = Some(InputMode::EditDnsListen);
                self.ui.input_buffer = DnsTextField::Listen.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
            }
            (Dns, 9) => {
                self.ui.input = Some(InputMode::EditDnsServers);
                self.ui.input_buffer = DnsTextField::Servers.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
            }
            (Dns, 10) => {
                self.ui.input = Some(InputMode::EditDnsDefaultNs);
                self.ui.input_buffer = DnsTextField::DefaultNs.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
            }
            (Dns, 11) => {
                self.ui.input = Some(InputMode::EditDnsDirectNs);
                self.ui.input_buffer = DnsTextField::DirectNs.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
            }
            (Dns, 12) => {
                self.ui.input = Some(InputMode::EditDnsProxyNs);
                self.ui.input_buffer = DnsTextField::ProxyNs.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
            }
            (Dns, 13) => {
                self.ui.input = Some(InputMode::EditDnsFallback);
                self.ui.input_buffer = DnsTextField::Fallback.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
            }
            (Dns, 14) => {
                self.ui.input = Some(InputMode::EditDnsFallbackGeoCode);
                self.ui.input_buffer = DnsTextField::FallbackGeoCode.initial(self);
                self.ui.input_cursor = self.ui.input_buffer.chars().count();
            }
            _ => self.say("Not available in remote mode (local core setting)"),
        }
    }

    pub(crate) fn create_backup(&mut self) {
        if !self.require(Capability::ManageBackups) {
            return;
        }
        match backup::create() {
            Ok(path) => self.say(format!("Backup created: {}", path.display())),
            Err(error) => self.say(format!("Backup failed: {error}")),
        }
    }

    pub(crate) fn confirm_restore_backup(&mut self) {
        if !self.require(Capability::ManageBackups) {
            return;
        }
        match backup::list() {
            Ok(files) if files.is_empty() => self.say("No local backups"),
            Ok(files) => self.ui.input = Some(InputMode::RestoreBackup(files[0].clone())),
            Err(error) => self.say(format!("Cannot list backups: {error}")),
        }
    }
}
