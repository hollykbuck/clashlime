//! Geo database downloads run in a background task so slow networks
//! never freeze the TUI event loop (cf. the core-download pattern).

use crate::app::backend::Capability;
use crate::app::{GeoMissingChoice, InputMode, UiState};
use crossterm::event::{KeyCode, KeyEvent};

/// Events streamed back from the background geo-download task.
pub enum GeoEvent {
    Stage(String),
    Done(Vec<String>),
    Failed(String),
}

impl crate::app::App {
    /// Start ensuring geo files in the background. Returns immediately so
    /// the UI keeps painting; completion arrives via [`Self::poll_geo_events`].
    pub(crate) fn start_geo_update(&mut self) {
        if !self.require(Capability::ManageGeo) {
            return;
        }
        if self.tasks.geo.running() {
            self.say("Geo update already in progress");
            return;
        }
        let geo = self.config.geo.clone();
        self.tasks.geo.spawn(|tx| async move {
            match crate::geo::ensure_all(&geo).await {
                Ok(fetched) => {
                    let _ = tx.send(GeoEvent::Done(fetched));
                }
                Err(error) => {
                    let _ = tx.send(GeoEvent::Failed(format!("{error:#}")));
                }
            }
        });
        self.say("Updating Geo data…");
        crate::logger::info("geo", "background geo update started");
    }

    pub(crate) async fn handle_geo_missing_key(&mut self, key: KeyEvent) {
        if self
            .ui
            .geo_missing
            .as_ref()
            .is_some_and(|dialog| dialog.busy)
        {
            if key.code == KeyCode::Esc {
                self.cancel_geo_update();
                if let Some(dialog) = self.ui.geo_missing.as_mut() {
                    dialog.busy = false;
                    dialog.message = "download cancelled".into();
                }
            }
            return;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(dialog) = self.ui.geo_missing.as_mut() {
                    dialog.choice = GeoMissingChoice::Download;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(dialog) = self.ui.geo_missing.as_mut() {
                    dialog.choice = GeoMissingChoice::ProvidePath;
                }
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Char('h')
            | KeyCode::Char('l')
            | KeyCode::Tab
            | KeyCode::BackTab => {
                if let Some(dialog) = self.ui.geo_missing.as_mut() {
                    dialog.choice = match dialog.choice {
                        GeoMissingChoice::Download => GeoMissingChoice::ProvidePath,
                        GeoMissingChoice::ProvidePath => GeoMissingChoice::Download,
                    };
                }
            }
            KeyCode::Char('x') | KeyCode::Char('X') => {
                let initial = self.config.geo.proxy.clone().unwrap_or_default();
                self.ui.begin_input(InputMode::EditGeoProxy, initial);
            }
            KeyCode::Enter => {
                let choice = self
                    .ui
                    .geo_missing
                    .as_ref()
                    .map(|dialog| dialog.choice)
                    .unwrap_or(GeoMissingChoice::Download);
                match choice {
                    GeoMissingChoice::Download => self.download_missing_geodata().await,
                    GeoMissingChoice::ProvidePath => {
                        self.ui.begin_input(InputMode::GeoPath, String::new());
                    }
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => self.download_missing_geodata().await,
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.ui.begin_input(InputMode::GeoPath, String::new());
            }
            KeyCode::Esc => {
                self.ui.geo_missing = None;
                self.ui.geo_missing_skipped = true;
                self.say("Skipped: geodata missing. Routing rules may fail until downloaded.");
            }
            _ => {}
        }
    }

    pub(crate) async fn download_missing_geodata(&mut self) {
        if self.tasks.geo.running() {
            return;
        }
        if let Some(dialog) = self.ui.geo_missing.as_mut() {
            dialog.busy = true;
            dialog.progress = None;
            dialog.message = "downloading geodata databases…".into();
        }
        let geo = self.config.geo.clone();
        self.tasks.geo.spawn(|tx| async move {
            let mirror = crate::geo::effective_mirror(geo.mirror.as_deref());
            let proxy = crate::geo::effective_proxy(geo.proxy.as_deref());
            let mut fetched = Vec::new();
            for file in crate::geo::GEO_FILES {
                if file.name == "geoip.dat" {
                    continue;
                }
                let _ = tx.send(GeoEvent::Stage(format!("downloading {}…", file.name)));
                match crate::geo::download(
                    file,
                    mirror.as_deref(),
                    proxy.as_deref(),
                    crate::geo::file_override(file, &geo),
                )
                .await
                {
                    Ok(_) => fetched.push(file.name.to_owned()),
                    Err(error) => {
                        let _ = tx.send(GeoEvent::Failed(format!("{error:#}")));
                        return;
                    }
                }
            }
            let _ = tx.send(GeoEvent::Done(fetched));
        });
    }

    pub(crate) async fn apply_geo_path(&mut self, value: &str) {
        let raw_path = std::path::PathBuf::from(value.trim());
        let path = if raw_path.is_relative() {
            std::env::current_dir().unwrap_or_default().join(&raw_path)
        } else {
            raw_path
        };
        if !path.exists() {
            self.say(format!("Path does not exist: {}", path.display()));
            self.ui.reopen_geo_missing_dialog(GeoMissingChoice::ProvidePath);
            return;
        }

        let is_daemon = crate::config::is_system_mode() || crate::ipc::daemon_alive().await;
        let mut imported = Vec::new();

        let candidates: Vec<(String, std::path::PathBuf)> = if path.is_dir() {
            let mut list = Vec::new();
            for name in ["geoip.metadb", "geosite.dat", "geoip.dat"] {
                let child = path.join(name);
                if child.is_file() {
                    list.push((name.to_string(), child));
                }
            }
            list
        } else if path.is_file() {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let matched = match file_name {
                "geoip.metadb" | "Country.mmdb" => Some("geoip.metadb"),
                "geosite.dat" => Some("geosite.dat"),
                "geoip.dat" => Some("geoip.dat"),
                _ => None,
            };
            if let Some(target) = matched {
                vec![(target.to_string(), path.clone())]
            } else {
                self.say(format!(
                    "Unrecognized geodata filename: {file_name} (expected geoip.metadb or geosite.dat)"
                ));
                self.ui.reopen_geo_missing_dialog(GeoMissingChoice::ProvidePath);
                return;
            }
        } else {
            self.say("Invalid path (not a regular file or directory)");
            return;
        };

        if candidates.is_empty() {
            self.say(format!(
                "No geoip.metadb or geosite.dat found in {}",
                path.display()
            ));
            self.ui.reopen_geo_missing_dialog(GeoMissingChoice::ProvidePath);
            return;
        }

        for (target_name, src_file) in &candidates {
            let res = if is_daemon {
                let staged = std::env::temp_dir().join(format!(
                    "clashlime-geo-adopt-{}-{}.tmp",
                    target_name,
                    std::process::id()
                ));
                match std::fs::copy(src_file, &staged) {
                    Ok(_) => {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt as _;
                            let _ = std::fs::set_permissions(
                                &staged,
                                std::fs::Permissions::from_mode(0o644),
                            );
                        }
                        let adopt_res = crate::ipc::adopt_geo(target_name, &staged).await;
                        let _ = std::fs::remove_file(&staged);
                        adopt_res
                    }
                    Err(e) => Err(anyhow::anyhow!("failed to stage {}: {e}", src_file.display())),
                }
            } else {
                crate::geo::adopt_geo_file(target_name, src_file).map(|_| ())
            };
            match res {
                Ok(()) => imported.push(target_name.clone()),
                Err(e) => {
                    crate::logger::warn("geo", &format!("failed to adopt {target_name}: {e}"));
                }
            }
        }

        if imported.is_empty() {
            self.say("Failed to import geodata files");
        } else {
            self.say(format!("Imported geodata: {}", imported.join(", ")));
            self.refresh_full().await;
            if !UiState::geo_missing() {
                self.ui.geo_missing = None;
            }
        }
    }

    pub(crate) fn poll_geo_events(&mut self) {
        let drain = self.tasks.geo.drain();
        for event in drain.events {
            self.handle_geo_event(event);
        }
        // A panic is reported even when a terminal event already arrived;
        // only a panic-free silent end is the vague failure.
        if let Some(panic) = drain.panic {
            crate::logger::warn("geo", &format!("geo task panicked: {panic}"));
            self.say(format!("Geo update failed: {panic}"));
        } else if drain.disconnected && !drain.terminal {
            crate::logger::warn("geo", "background task ended unexpectedly");
            self.say("Geo update failed: background task ended unexpectedly");
        }
    }

    fn handle_geo_event(&mut self, event: GeoEvent) {
        match event {
            GeoEvent::Stage(stage) => {
                if let Some(dialog) = self.ui.geo_missing.as_mut() {
                    dialog.busy = true;
                    dialog.message = stage;
                }
            }
            GeoEvent::Done(fetched) => {
                self.tasks.geo.stop();
                if let Some(dialog) = self.ui.geo_missing.as_mut() {
                    dialog.busy = false;
                }
                self.ui.geo_missing = None;
                if fetched.is_empty() {
                    self.say(format!("Geo data ready ({})", crate::geo::summary()));
                } else {
                    crate::logger::info("geo", &format!("updated: {}", fetched.join(", ")));
                    self.say(format!("Geo data updated: {}", fetched.join(", ")));
                }
            }
            GeoEvent::Failed(error) => {
                self.tasks.geo.stop();
                crate::logger::warn("geo", &format!("update failed: {error}"));
                if let Some(dialog) = self.ui.geo_missing.as_mut() {
                    dialog.busy = false;
                    dialog.message = format!("failed: {error}");
                }
                self.say(format!("Geo update failed: {error}"));
            }
        }
    }

    /// Cancel an in-flight geo download (Settings Esc).
    pub(crate) fn cancel_geo_update(&mut self) {
        self.tasks.geo.stop();
        self.say("Geo update cancelled");
    }

    pub(crate) fn geo_updating(&self) -> bool {
        self.tasks.geo.running()
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{
        App, DataState, GeoMissingChoice, GeoMissingDialog, InputMode, TaskHub, UiState,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn test_app() -> App {
        let mut app = App {
            config: crate::config::Config::default(),
            api: crate::api::MihomoClient::new("http://127.0.0.1:9090", String::new()).unwrap(),
            remote: false,
            theme: crate::theme::Theme::default(),
            ui: UiState::default(),
            data: DataState::default(),
            tasks: TaskHub::default(),
        };
        app.ui.geo_missing = Some(GeoMissingDialog {
            choice: GeoMissingChoice::Download,
            busy: false,
            message: String::new(),
            progress: None,
        });
        app
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    #[tokio::test]
    async fn geo_missing_dialog_navigates_and_opens_proxy() {
        let mut app = test_app();
        assert_eq!(
            app.ui.geo_missing.as_ref().unwrap().choice,
            GeoMissingChoice::Download
        );

        // Down / j switches to ProvidePath
        app.handle_geo_missing_key(press(KeyCode::Down)).await;
        assert_eq!(
            app.ui.geo_missing.as_ref().unwrap().choice,
            GeoMissingChoice::ProvidePath
        );

        // Up / k switches to Download
        app.handle_geo_missing_key(press(KeyCode::Up)).await;
        assert_eq!(
            app.ui.geo_missing.as_ref().unwrap().choice,
            GeoMissingChoice::Download
        );

        // 'x' opens proxy input
        app.handle_geo_missing_key(press(KeyCode::Char('x'))).await;
        assert!(matches!(
            app.ui.input_mode(),
            Some(InputMode::EditGeoProxy)
        ));
    }
}
