use crate::{backup, core, profiles::Profiles};
use crossterm::event::{KeyCode, KeyEvent};

use super::{CoreMissingChoice, InputMode};

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
        if matches!(self.input, Some(InputMode::EditDnsListen)) {
            self.handle_dns_listen_input(key).await;
            return;
        }
        if matches!(self.input, Some(InputMode::EditDnsServers)) {
            self.handle_dns_servers_input(key).await;
            return;
        }
        self.handle_import_input(key).await;
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

    async fn handle_dns_listen_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input = None;
                self.input_buffer.clear();
                self.say("DNS listen edit cancelled");
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => self.input_buffer.push(c),
            KeyCode::Enter => {
                let value = self.input_buffer.trim().to_owned();
                if value.is_empty() {
                    self.say("DNS listen cannot be empty (e.g. 0.0.0.0:1053)");
                    return;
                }
                self.input_buffer.clear();
                self.input = None;
                self.config.dns.listen = value.clone();
                // Auto-enable DNS when listen edited
                if !self.config.dns.enable {
                    self.config.dns.enable = true;
                }
                if let Err(e) = self.config.save() {
                    self.say(format!("Save failed: {e}"));
                    return;
                }
                crate::logger::info("app", &format!("dns listen -> {value}"));
                match self.api.update_dns(&self.config.dns).await {
                    Ok(()) => self.say(format!("DNS listen {value} (hot patched)")),
                    Err(e) => {
                        crate::logger::warn(
                            "app",
                            &format!("dns listen hot patch failed: {e}"),
                        );
                        match core::request_restart().await {
                            Ok(()) => {
                                self.say(format!("DNS listen {value} saved, reload requested"))
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

    async fn handle_dns_servers_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input = None;
                self.input_buffer.clear();
                self.say("DNS servers edit cancelled");
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => self.input_buffer.push(c),
            KeyCode::Enter => {
                let value = self.input_buffer.trim().to_owned();
                if value.is_empty() {
                    self.say("Enter comma-separated DNS servers (e.g. 223.5.5.5, 8.8.8.8)");
                    return;
                }
                let servers: Vec<String> = value
                    .split(',')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
                if servers.is_empty() {
                    self.say("No valid servers parsed");
                    return;
                }
                self.input_buffer.clear();
                self.input = None;
                self.config.dns.nameserver = servers.clone();
                if !self.config.dns.enable {
                    self.config.dns.enable = true;
                }
                if let Err(e) = self.config.save() {
                    self.say(format!("Save failed: {e}"));
                    return;
                }
                crate::logger::info("app", &format!("dns servers -> {}", servers.join(", ")));
                match self.api.update_dns(&self.config.dns).await {
                    Ok(()) => {
                        self.say(format!("DNS servers {} (hot patched)", servers.join(", ")));
                    }
                    Err(e) => {
                        crate::logger::warn(
                            "app",
                            &format!("dns servers hot patch failed: {e}"),
                        );
                        match core::request_restart().await {
                            Ok(()) => {
                                self.say("DNS servers saved, reload requested".to_owned())
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

    async fn handle_import_input(&mut self, key: KeyEvent) {
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
                self.input_buffer.clear();
                self.input = None;
                self.say(format!("Importing {value}…"));
                crate::logger::info("app", &format!("importing {value}"));
                let result = if value.starts_with("http://") || value.starts_with("https://") {
                    self.profiles
                        .import_remote(&value, None, &self.config)
                        .await
                } else {
                    self.profiles
                        .import_local(std::path::Path::new(&value), None, &self.config)
                        .await
                };
                match result {
                    Ok(uid) => {
                        self.say(format!("Imported {uid}"));
                        self.profile_index = self.profiles.items.len().saturating_sub(1);
                    }
                    Err(error) => {
                        crate::logger::warn("app", &format!("import failed: {error:#}"));
                        self.say(format!("Import failed: {error:#}"));
                    }
                }
            }
            _ => {}
        }
    }
}
