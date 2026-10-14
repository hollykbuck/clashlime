use crate::{
    app::{
        DnsMapEditor, DnsMapTarget, InputMode, SettingSection,
        backend::Capability,
        input::{CoreTextField, DnsTextField, GeoUrlField},
    },
    backup,
    config::StringList,
    core,
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
            (Dns, 15) => {
                self.open_dns_map_editor(DnsMapTarget::NameserverPolicy);
                return;
            }
            (Dns, 16) => {
                self.open_dns_map_editor(DnsMapTarget::Hosts);
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

    /// Open the list editor for a DNS key/value table. Entries are edited
    /// through two stacked text sub-inputs (domain, then servers); the table
    /// is written back and the core reloaded once, on close.
    pub(crate) fn open_dns_map_editor(&mut self, target: DnsMapTarget) {
        let entries: Vec<(String, StringList)> = match target {
            DnsMapTarget::NameserverPolicy => self
                .config
                .dns
                .nameserver_policy
                .iter()
                .map(|(domain, servers)| (domain.clone(), servers.clone()))
                .collect(),
            DnsMapTarget::Hosts => self
                .config
                .dns
                .hosts
                .iter()
                .map(|(domain, servers)| (domain.clone(), servers.clone()))
                .collect(),
        };
        self.ui.dns_map_editor = Some(DnsMapEditor {
            target,
            entries,
            index: 0,
            editing: None,
            pending_domain: None,
            dirty: false,
        });
    }

    /// Keys for the DNS map list overlay. A text sub-input owns the keyboard
    /// while it is open — `handle_key` routes those first, and this guard
    /// keeps the list inert if that ever changes.
    pub(crate) async fn handle_dns_map_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        if self.ui.input.is_some() {
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                let Some((label, dirty)) = self.close_dns_map_editor() else {
                    return;
                };
                if !dirty {
                    self.say(format!("{label} unchanged"));
                    return;
                }
                if self.remote {
                    match self.api.update_dns(&self.config.dns).await {
                        Ok(()) => self.say(format!("{label} saved (remote patched)")),
                        Err(error) => {
                            self.say(format!("{label} saved, remote patch failed: {error}"))
                        }
                    }
                    return;
                }
                // PATCH only sends non-empty keys, so emptying the whole
                // table can never hot-patch — always reload locally.
                match core::request_restart().await {
                    Ok(()) => self.say(format!("{label} saved, reload requested")),
                    Err(error) => {
                        self.say(format!("{label} saved, restart request failed: {error}"))
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_dns_map_index(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_dns_map_index(-1),
            KeyCode::Enter => self.activate_dns_map_row(),
            KeyCode::Char('a') => self.add_dns_map_entry(),
            KeyCode::Char('d') => self.remove_dns_map_entry(),
            _ => {}
        }
    }

    fn move_dns_map_index(&mut self, delta: isize) {
        let Some(editor) = self.ui.dns_map_editor.as_mut() else {
            return;
        };
        let len = editor.entries.len();
        if len == 0 {
            editor.index = 0;
            return;
        }
        let next = (editor.index as isize + delta).rem_euclid(len as isize) as usize;
        editor.index = next;
        // `editing` is armed by Enter, not by moving the cursor.
        editor.editing = None;
        editor.pending_domain = None;
    }

    /// Enter on a row: edit the domain first, then the server list.
    fn activate_dns_map_row(&mut self) {
        let Some(editor) = self.ui.dns_map_editor.as_ref() else {
            return;
        };
        let Some((domain, _)) = editor.entries.get(editor.index) else {
            self.say("No entries · press a to add one");
            return;
        };
        let (domain, servers_mode) = (domain.clone(), editor.target);
        self.ui.input = Some(match servers_mode {
            DnsMapTarget::NameserverPolicy => InputMode::EditDnsPolicyDomain,
            DnsMapTarget::Hosts => InputMode::EditHostsDomain,
        });
        self.ui.input_buffer = domain;
        self.ui.input_cursor = self.ui.input_buffer.chars().count();
        if let Some(editor) = self.ui.dns_map_editor.as_mut() {
            editor.editing = Some(editor.index);
            editor.pending_domain = None;
        }
    }

    fn add_dns_map_entry(&mut self) {
        let Some(editor) = self.ui.dns_map_editor.as_mut() else {
            return;
        };
        let target = editor.target;
        editor.editing = None;
        editor.pending_domain = None;
        self.ui.input = Some(match target {
            DnsMapTarget::NameserverPolicy => InputMode::EditDnsPolicyDomain,
            DnsMapTarget::Hosts => InputMode::EditHostsDomain,
        });
        self.ui.clear_input();
    }

    fn remove_dns_map_entry(&mut self) {
        let Some(editor) = self.ui.dns_map_editor.as_mut() else {
            return;
        };
        if editor.entries.is_empty() {
            self.say("Nothing to delete");
            return;
        }
        let index = editor.index.min(editor.entries.len() - 1);
        let (domain, _) = editor.entries.remove(index);
        editor.dirty = true;
        // Keep the cursor on the next row, or the previous one at the end.
        editor.index = index.min(editor.entries.len().saturating_sub(1));
        editor.editing = None;
        editor.pending_domain = None;
        self.sync_dns_map_to_config();
        if let Err(error) = self.config.save() {
            self.say(format!("Save failed: {error}"));
            return;
        }
        self.say(format!("Deleted {domain}"));
    }

    /// Fold the editor's working copy into `config.dns`. Called after every
    /// mutation so config always mirrors what's on screen — otherwise a
    /// click-to-close (which drops the editor) would discard the edits.
    pub(crate) fn sync_dns_map_to_config(&mut self) {
        let Some(editor) = self.ui.dns_map_editor.as_ref() else {
            return;
        };
        let table: std::collections::BTreeMap<String, StringList> =
            editor.entries.iter().cloned().collect();
        match editor.target {
            DnsMapTarget::NameserverPolicy => self.config.dns.nameserver_policy = table,
            DnsMapTarget::Hosts => self.config.dns.hosts = table,
        }
    }

    /// Close the overlay and report whether the core needs a reload.
    fn close_dns_map_editor(&mut self) -> Option<(String, bool)> {
        let editor = self.ui.dns_map_editor.take()?;
        let label = editor.target.label();
        if !editor.dirty {
            return Some((label.to_owned(), false));
        }
        // Entries were already folded in and saved as they were made; this
        // is the final save so a delete-then-close persists too.
        if let Err(error) = self.config.save() {
            self.say(format!("Save failed: {error}"));
            return Some((label.to_owned(), false));
        }
        crate::logger::info("app", &format!("{label} table updated"));
        Some((label.to_owned(), true))
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
            | (Dns, 14)
            | (Dns, 15)
            | (Dns, 16) => {
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
            (Dns, 15) => self.open_dns_map_editor(DnsMapTarget::NameserverPolicy),
            (Dns, 16) => self.open_dns_map_editor(DnsMapTarget::Hosts),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{DataState, UiState};
    use crate::app::tasks::TaskHub;
    use crate::app::{App, Tab};
    use crate::config::Config;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn test_app() -> App {
        App {
            config: Config::default(),
            api: crate::api::MihomoClient::new("http://127.0.0.1:9090", String::new()).unwrap(),
            remote: false,
            theme: crate::theme::Theme::default(),
            ui: UiState {
                tab: Tab::Settings,
                ..Default::default()
            },
            data: DataState::default(),
            tasks: TaskHub::default(),
        }
    }

    /// These tests commit real entries, and a commit calls `config.save()`.
    /// Point XDG at a temp dir so the user's config.json is never touched
    /// (same isolation `backup::tests` uses).
    struct Env {
        _dir: tempfile::TempDir,
        _guard: std::sync::MutexGuard<'static, ()>,
        orig_cfg: Option<std::ffi::OsString>,
        orig_data: Option<std::ffi::OsString>,
    }

    impl Env {
        fn new() -> Self {
            let guard = crate::config::tests::test_env_lock()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let dir = tempfile::tempdir().unwrap();
            let orig_cfg = std::env::var_os("XDG_CONFIG_HOME");
            let orig_data = std::env::var_os("XDG_DATA_HOME");
            unsafe {
                std::env::set_var("XDG_CONFIG_HOME", dir.path().join("config"));
                std::env::set_var("XDG_DATA_HOME", dir.path().join("data"));
            }
            Self {
                _dir: dir,
                _guard: guard,
                orig_cfg,
                orig_data,
            }
        }
    }

    impl Drop for Env {
        fn drop(&mut self) {
            unsafe {
                match self.orig_cfg.take() {
                    Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                    None => std::env::remove_var("XDG_CONFIG_HOME"),
                }
                match self.orig_data.take() {
                    Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                    None => std::env::remove_var("XDG_DATA_HOME"),
                }
            }
        }
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// Replays the keyboard path a user takes: `a` to add, type the domain,
    /// Enter, type the servers, Enter. Exercises both sub-inputs and the
    /// fold into config, without touching the real config file.
    async fn add_entry(app: &mut App, domain: &str, servers: &str) {
        app.ui.setting_section = SettingSection::Dns;
        app.ui.setting_index = 15;
        app.toggle_setting().await;
        app.handle_dns_map_key(press(KeyCode::Char('a'))).await;
        for c in domain.chars() {
            app.ui.input_insert(c);
        }
        app.handle_input(press(KeyCode::Enter)).await;
        for c in servers.chars() {
            app.ui.input_insert(c);
        }
        app.handle_input(press(KeyCode::Enter)).await;
    }

    #[tokio::test]
    async fn nameserver_policy_entry_reaches_config_and_keeps_string_shape() {
        let _env = Env::new();
        let mut app = test_app();
        add_entry(&mut app, "geosite:cn", "223.5.5.5").await;

        let entry = app.config.dns.nameserver_policy.get("geosite:cn").unwrap();
        assert!(
            matches!(entry, StringList::Single(_)),
            "one server stays a string"
        );
        assert_eq!(entry.values(), vec!["223.5.5.5"]);
        // A single-value entry serializes back as a bare string, not `["…"]`.
        let text = serde_json::to_string(&app.config.dns).unwrap();
        assert!(text.contains("\"geosite:cn\":\"223.5.5.5\""), "{text}");
        // `hosts` is a different table and must stay untouched.
        assert!(app.config.dns.hosts.is_empty());
        // The sub-input closed itself and left the list open on the new row.
        assert!(app.ui.input.is_none());
        let editor = app.ui.dns_map_editor.as_ref().unwrap();
        assert_eq!(editor.index, 0);
        assert!(editor.dirty);
    }

    #[tokio::test]
    async fn multiple_servers_round_trip_as_a_list() {
        let _env = Env::new();
        let mut app = test_app();
        add_entry(&mut app, "+.example.com", "tls://1.1.1.1, 8.8.8.8").await;
        let entry = app
            .config
            .dns
            .nameserver_policy
            .get("+.example.com")
            .unwrap();
        assert!(matches!(entry, StringList::Multiple(_)));
        assert_eq!(entry.values(), vec!["tls://1.1.1.1", "8.8.8.8"]);
    }

    #[tokio::test]
    async fn hosts_row_writes_the_hosts_table() {
        let _env = Env::new();
        let mut app = test_app();
        app.ui.setting_section = SettingSection::Dns;
        app.ui.setting_index = 16;
        app.toggle_setting().await;
        app.handle_dns_map_key(press(KeyCode::Char('a'))).await;
        for c in "router.lan".chars() {
            app.ui.input_insert(c);
        }
        app.handle_input(press(KeyCode::Enter)).await;
        for c in "192.168.1.1".chars() {
            app.ui.input_insert(c);
        }
        app.handle_input(press(KeyCode::Enter)).await;

        assert_eq!(
            app.config
                .dns
                .hosts
                .get("router.lan")
                .map(StringList::values),
            Some(vec!["192.168.1.1".to_owned()])
        );
        assert!(app.config.dns.nameserver_policy.is_empty());
    }

    /// An empty domain or an empty server list must not create an entry:
    /// both consumers skip empty values, so storing one would silently
    /// drop it from the generated config.
    #[tokio::test]
    async fn blank_domain_or_servers_creates_no_entry() {
        let _env = Env::new();
        let mut app = test_app();
        app.ui.setting_section = SettingSection::Dns;
        app.ui.setting_index = 15;
        app.toggle_setting().await;

        app.handle_dns_map_key(press(KeyCode::Char('a'))).await;
        app.handle_input(press(KeyCode::Enter)).await;
        assert!(app.ui.input.is_some(), "empty domain keeps the editor open");
        assert!(app.config.dns.nameserver_policy.is_empty());

        for c in "geosite:cn".chars() {
            app.ui.input_insert(c);
        }
        app.handle_input(press(KeyCode::Enter)).await;
        // Now on the servers sub-input; submit nothing.
        app.handle_input(press(KeyCode::Enter)).await;
        assert!(
            app.ui.input.is_some(),
            "empty servers keeps the editor open"
        );
        assert!(app.config.dns.nameserver_policy.is_empty());
        assert!(app.ui.dns_map_editor.as_ref().unwrap().entries.is_empty());
    }

    #[tokio::test]
    async fn deleting_the_last_entry_empties_the_table() {
        let _env = Env::new();
        let mut app = test_app();
        add_entry(&mut app, "geosite:cn", "223.5.5.5").await;
        app.handle_dns_map_key(press(KeyCode::Char('d'))).await;
        assert!(app.config.dns.nameserver_policy.is_empty());
        assert!(app.ui.dns_map_editor.as_ref().unwrap().dirty);
        // Deleting from an empty table is a no-op, not a panic.
        app.handle_dns_map_key(press(KeyCode::Char('d'))).await;
        assert!(app.config.dns.nameserver_policy.is_empty());
    }

    /// Closing after an edit must report "needs reload"; closing untouched
    /// must not.
    #[tokio::test]
    async fn close_reports_dirty_only_after_a_real_change() {
        let _env = Env::new();
        let mut app = test_app();
        app.ui.setting_section = SettingSection::Dns;
        app.ui.setting_index = 15;
        app.toggle_setting().await;
        assert_eq!(
            app.close_dns_map_editor(),
            Some(("Nameserver policy".into(), false))
        );

        let mut app = test_app();
        add_entry(&mut app, "geosite:cn", "223.5.5.5").await;
        assert_eq!(
            app.close_dns_map_editor(),
            Some(("Nameserver policy".into(), true))
        );
        assert!(app.ui.dns_map_editor.is_none());
    }

    /// While a sub-input is open the list must not react to j/k/a/d.
    #[tokio::test]
    async fn list_keys_are_inert_while_a_sub_input_is_open() {
        let _env = Env::new();
        let mut app = test_app();
        add_entry(&mut app, "geosite:cn", "223.5.5.5").await;
        add_entry(&mut app, "geosite:net", "8.8.8.8").await;
        assert_eq!(app.ui.dns_map_editor.as_ref().unwrap().entries.len(), 2);

        app.handle_dns_map_key(press(KeyCode::Char('a'))).await;
        assert!(app.ui.input.is_some(), "`a` opened the domain sub-input");
        // These would otherwise move the cursor / append / delete.
        app.handle_dns_map_key(press(KeyCode::Char('j'))).await;
        app.handle_dns_map_key(press(KeyCode::Char('d'))).await;
        let editor = app.ui.dns_map_editor.as_ref().unwrap();
        assert_eq!(editor.entries.len(), 2);
        assert_eq!(editor.index, 1);
    }

    #[tokio::test]
    async fn cursor_wraps_over_the_entries() {
        let _env = Env::new();
        let mut app = test_app();
        add_entry(&mut app, "a.com", "1.1.1.1").await;
        add_entry(&mut app, "b.com", "2.2.2.2").await;
        add_entry(&mut app, "c.com", "3.3.3.3").await;
        assert_eq!(app.ui.dns_map_editor.as_ref().unwrap().index, 2);
        app.handle_dns_map_key(press(KeyCode::Char('j'))).await;
        assert_eq!(app.ui.dns_map_editor.as_ref().unwrap().index, 0);
        app.handle_dns_map_key(press(KeyCode::Char('k'))).await;
        assert_eq!(app.ui.dns_map_editor.as_ref().unwrap().index, 2);
    }
}
