use crate::{
    app::{
        InputMode, SettingSection,
        input::{CoreTextField, DnsTextField},
    },
    backup, core,
};

impl crate::app::App {
    pub(crate) async fn toggle_setting(&mut self) {
        use SettingSection::{Core, Dns, Geo, Network, Ports, Tun};
        let mut restart = false;
        let mut dns_hot_patch = false;
        match (self.setting_section, self.setting_index) {
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
                        self.status.push_str(&rollback_text);
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
                self.input = Some(InputMode::EditProxyBypass);
                self.input_buffer = CoreTextField::ProxyBypass.initial(self);
                return;
            }
            (Network, 2) => {
                self.config.allow_lan = !self.config.allow_lan;
                restart = true;
            }
            (Network, 3) => {
                self.input = Some(InputMode::EditLanAllowed);
                self.input_buffer = CoreTextField::LanAllowed.initial(self);
                return;
            }
            (Network, 4) => {
                self.input = Some(InputMode::EditLanDisallowed);
                self.input_buffer = CoreTextField::LanDisallowed.initial(self);
                return;
            }
            (Network, 5) => {
                self.config.ipv6 = !self.config.ipv6;
                restart = true;
            }
            (Network, 6) => {
                self.config.sniffer_enable = !self.config.sniffer_enable;
                restart = true;
            }
            (Ports, 0) => {
                self.input = Some(InputMode::EditSocksPort);
                self.input_buffer = CoreTextField::SocksPort.initial(self);
                return;
            }
            (Ports, 1) => {
                self.input = Some(InputMode::EditHttpPort);
                self.input_buffer = CoreTextField::HttpPort.initial(self);
                return;
            }
            (Ports, 2) => {
                self.input = Some(InputMode::EditRedirPort);
                self.input_buffer = CoreTextField::RedirPort.initial(self);
                return;
            }
            (Ports, 3) => {
                self.input = Some(InputMode::EditTproxyPort);
                self.input_buffer = CoreTextField::TproxyPort.initial(self);
                return;
            }
            (Ports, 4) => {
                self.input = Some(InputMode::EditAuth);
                self.input_buffer = CoreTextField::Auth.initial(self);
                return;
            }
            (Ports, 5) => {
                self.input = Some(InputMode::EditSkipAuth);
                self.input_buffer = CoreTextField::SkipAuth.initial(self);
                return;
            }
            (Ports, 6) => {
                self.config.tcp_concurrent = Some(!self.config.tcp_concurrent.unwrap_or(false));
                restart = true;
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
                self.input = Some(InputMode::EditTunDevice);
                self.input_buffer = CoreTextField::TunDevice.initial(self);
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
                self.input = Some(InputMode::EditTunDnsHijack);
                self.input_buffer = CoreTextField::TunDnsHijack.initial(self);
                return;
            }
            (Tun, 6) => {
                self.input = Some(InputMode::EditTunMtu);
                self.input_buffer = CoreTextField::TunMtu.initial(self);
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
                self.input = Some(InputMode::EditTunRouteExclude);
                self.input_buffer = CoreTextField::TunRouteExclude.initial(self);
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
                self.input = Some(InputMode::EditMixedPort);
                self.input_buffer = CoreTextField::MixedPort.initial(self);
                return;
            }
            (Core, 4) => {
                self.input = Some(InputMode::EditController);
                self.input_buffer = CoreTextField::Controller.initial(self);
                return;
            }
            (Core, 5) => {
                self.input = Some(InputMode::EditSecret);
                self.input_buffer = CoreTextField::Secret.initial(self);
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
                restart = true;
            }
            (Core, 7) => {
                self.input = Some(InputMode::EditDelayTestUrl);
                self.input_buffer = CoreTextField::DelayTestUrl.initial(self);
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
            (Dns, 2) => {
                self.input = Some(InputMode::EditDnsFakeIpRange);
                self.input_buffer = DnsTextField::FakeIpRange.initial(self);
                return;
            }
            (Dns, 3) => {
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
            (Dns, 4) => {
                self.input = Some(InputMode::EditDnsFakeIpFilter);
                self.input_buffer = DnsTextField::FakeIpFilter.initial(self);
                return;
            }
            (Dns, 5) => {
                self.config.dns.ipv6 = !self.config.dns.ipv6;
                dns_hot_patch = true;
            }
            (Dns, 6) => {
                self.config.dns.respect_rules =
                    Some(!self.config.dns.respect_rules.unwrap_or(false));
                dns_hot_patch = true;
            }
            (Dns, 7) => {
                // Edit DNS listen address
                self.input = Some(InputMode::EditDnsListen);
                self.input_buffer = DnsTextField::Listen.initial(self);
                return;
            }
            (Dns, 8) => {
                // Edit DNS nameservers (comma separated)
                self.input = Some(InputMode::EditDnsServers);
                self.input_buffer = DnsTextField::Servers.initial(self);
                return;
            }
            (Dns, 9) => {
                self.input = Some(InputMode::EditDnsDefaultNs);
                self.input_buffer = DnsTextField::DefaultNs.initial(self);
                return;
            }
            (Dns, 10) => {
                self.input = Some(InputMode::EditDnsDirectNs);
                self.input_buffer = DnsTextField::DirectNs.initial(self);
                return;
            }
            (Dns, 11) => {
                self.input = Some(InputMode::EditDnsProxyNs);
                self.input_buffer = DnsTextField::ProxyNs.initial(self);
                return;
            }
            (Dns, 12) => {
                self.input = Some(InputMode::EditDnsFallback);
                self.input_buffer = DnsTextField::Fallback.initial(self);
                return;
            }
            (Dns, 13) => {
                self.input = Some(InputMode::EditDnsFallbackGeoCode);
                self.input_buffer = DnsTextField::FallbackGeoCode.initial(self);
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
                self.input = Some(InputMode::EditGeoMirror);
                self.input_buffer = self.config.geo.mirror.clone().unwrap_or_default();
                return;
            }
            (Geo, 2) => {
                self.config.geo.auto_update = !self.config.geo.auto_update;
                restart = true;
            }
            (Geo, 3) => {
                // Cycle geo update interval through sane presets.
                self.config.geo.update_interval = [6, 12, 24, 48, 168]
                    .into_iter()
                    .find(|preset| *preset > self.config.geo.update_interval)
                    .unwrap_or(6);
                restart = true;
            }
            (Geo, 4) => {
                // Edit proxy for geo downloads (e.g. mihomo mixed port).
                self.input = Some(InputMode::EditGeoProxy);
                self.input_buffer = self.config.geo.proxy.clone().unwrap_or_default();
                return;
            }
            _ => {}
        }
        if let Err(error) = self.config.save() {
            self.say(format!("Save failed: {error}"));
            return;
        }
        if dns_hot_patch {
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
                        self.say(format!("DNS saved but reload failed: {err} (hot patch: {e})"));
                        return;
                    }
                    self.say("DNS saved, reload requested");
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

    pub(crate) fn create_backup(&mut self) {
        match backup::create() {
            Ok(path) => self.say(format!("Backup created: {}", path.display())),
            Err(error) => self.say(format!("Backup failed: {error}")),
        }
    }

    pub(crate) fn confirm_restore_backup(&mut self) {
        match backup::list() {
            Ok(files) if files.is_empty() => self.say("No local backups"),
            Ok(files) => self.input = Some(InputMode::RestoreBackup(files[0].clone())),
            Err(error) => self.say(format!("Cannot list backups: {error}")),
        }
    }
}
