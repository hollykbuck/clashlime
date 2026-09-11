use crate::{app::InputMode, backup, core};

impl crate::app::App {
    pub(crate) async fn toggle_setting(&mut self) {
        let mut restart = false;
        let mut dns_hot_patch = false;
        match self.setting_index {
            0 => {
                let enable = !core::core_desired_enabled().await;
                if let Err(error) = core::request_core_enabled(enable).await {
                    self.say(format!("Core state change failed: {error}"));
                    return;
                }
            }
            1 => {
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
            2 => {
                self.config.system_proxy = !self.config.system_proxy;
                restart = true;
            }
            3 => {
                self.config.allow_lan = !self.config.allow_lan;
                restart = true;
            }
            4 => {
                self.config.ipv6 = !self.config.ipv6;
                restart = true;
            }
            5 => {
                self.config.refresh_ms = if self.config.refresh_ms >= 5000 {
                    500
                } else {
                    self.config.refresh_ms + 500
                };
            }
            6 => {
                self.config.dns.enable = !self.config.dns.enable;
                dns_hot_patch = true;
            }
            7 => {
                // Edit DNS listen address
                self.input = Some(InputMode::EditDnsListen);
                self.input_buffer = self.config.dns.listen.clone();
                return;
            }
            8 => {
                // Edit DNS nameservers (comma separated)
                self.input = Some(InputMode::EditDnsServers);
                self.input_buffer = self.config.dns.nameserver.join(", ");
                return;
            }
            9 => {
                // Geo data (geoip.metadb / geosite.dat): fetch missing
                // files in the background so slow networks never freeze
                // the UI; Esc cancels.
                self.start_geo_update();
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
