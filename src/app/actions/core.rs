use crate::app::backend::Capability;
use crate::{config::Config, core, update};
use crossterm::event::{KeyCode, KeyEvent};
use std::path::PathBuf;

use super::super::{CoreDownloadEvent, CoreMissingChoice, InputMode};

impl crate::app::App {
    pub(crate) async fn toggle_core(&mut self) {
        if !self.require(Capability::ManageCore) {
            return;
        }
        let enable = !core::core_desired_enabled().await;
        let result = core::request_core_enabled(enable).await.map(|()| {
            if enable && self.data.profiles.items.is_empty() {
                "Mihomo cannot start: no profile imported. Open Profiles and press a to import."
            } else if enable {
                "Mihomo start requested"
            } else {
                "Mihomo stop requested"
            }
            .to_owned()
        });
        match result {
            Ok(message) => self.say(message),
            Err(error) => self.say(format!("Core operation failed: {error}")),
        }
        self.refresh_full().await;
    }

    pub(crate) async fn handle_core_missing_key(&mut self, key: KeyEvent) {
        if self
            .ui
            .core_missing
            .as_ref()
            .is_some_and(|dialog| dialog.busy)
        {
            // Only Esc is honored mid-download; it cancels the task.
            if key.code == KeyCode::Esc {
                self.cancel_core_download();
            }
            return;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(dialog) = self.ui.core_missing.as_mut() {
                    dialog.choice = CoreMissingChoice::Download;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(dialog) = self.ui.core_missing.as_mut() {
                    dialog.choice = CoreMissingChoice::ProvidePath;
                }
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Char('h')
            | KeyCode::Char('l')
            | KeyCode::Tab
            | KeyCode::BackTab => {
                if let Some(dialog) = self.ui.core_missing.as_mut() {
                    dialog.choice = match dialog.choice {
                        CoreMissingChoice::Download => CoreMissingChoice::ProvidePath,
                        CoreMissingChoice::ProvidePath => CoreMissingChoice::Download,
                    };
                }
            }
            KeyCode::Enter => {
                let choice = self
                    .ui
                    .core_missing
                    .as_ref()
                    .map(|dialog| dialog.choice)
                    .unwrap_or(CoreMissingChoice::Download);
                match choice {
                    CoreMissingChoice::Download => self.download_core().await,
                    CoreMissingChoice::ProvidePath => {
                        self.ui.begin_input(InputMode::CorePath, String::new());
                        self.ui.core_missing = None;
                    }
                }
            }
            KeyCode::Char('x') | KeyCode::Char('X') => {
                let initial = self.config.geo.proxy.clone().unwrap_or_default();
                self.ui.begin_input(InputMode::EditGeoProxy, initial);
            }
            KeyCode::Char('d') | KeyCode::Char('D') => self.download_core().await,
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.ui.begin_input(InputMode::CorePath, String::new());
                self.ui.core_missing = None;
            }
            KeyCode::Esc => {
                // Keep running without a core; supervisor retries in background
                self.ui.core_missing = None;
                self.say("Skipped: no mihomo core. Settings will keep showing the warning.");
            }
            _ => {}
        }
    }

    pub(crate) async fn download_core(&mut self) {
        if self.tasks.core_download.running() {
            return; // already running
        }
        let Some(dialog) = self.ui.core_missing.as_mut() else {
            return;
        };
        dialog.busy = true;
        dialog.progress = None;
        dialog.message = "resolving latest release…".into();
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();

        let is_daemon = crate::config::is_system_mode() || crate::ipc::daemon_alive().await;
        let proxy = crate::geo::effective_proxy(self.config.geo.proxy.as_deref());
        let geo_cfg = self.config.geo.clone();
        self.tasks.core_download.spawn(|event_tx| async move {
            let report = |event| {
                let _ = event_tx.send(event);
            };
            report(CoreDownloadEvent::Stage("resolving latest release…".into()));
            match update::fetch_latest_release(true, proxy.as_deref()).await {
                Ok(release) => match update::pick_asset(&release) {
                    Ok(asset) => {
                        report(CoreDownloadEvent::Stage(format!(
                            "downloading {} ({})…",
                            asset.name,
                            update::format_size(asset.size as usize)
                        )));
                        // Forward byte progress to the UI as it arrives.
                        let forwarder_tx = event_tx.clone();
                        let forwarder = tokio::spawn(async move {
                            while let Some(progress) = progress_rx.recv().await {
                                if forwarder_tx
                                    .send(CoreDownloadEvent::Progress(progress))
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        });
                        let download_dest = if is_daemon {
                            std::env::temp_dir().join(format!(
                                "clashlime-mihomo-download-{}.tmp",
                                std::process::id()
                            ))
                        } else {
                            Config::data_dir().join("bin/mihomo")
                        };
                        let result = update::download_core(
                            &asset,
                            &download_dest,
                            Some(&progress_tx),
                            proxy.as_deref(),
                        )
                        .await;
                        drop(progress_tx);
                        let _ = forwarder.await;
                        match result {
                            Ok(path) => {
                                if is_daemon {
                                    let adopt_res = crate::ipc::adopt_core(&path).await;
                                    let _ = std::fs::remove_file(&path);
                                    if let Err(error) = adopt_res {
                                        report(CoreDownloadEvent::Failed(error.to_string()));
                                        return;
                                    }
                                }
                                // Ensure base geodata databases (geoip.metadb, geosite.dat) are present
                                for file in crate::geo::GEO_FILES {
                                    if file.name == "geoip.dat" {
                                        continue;
                                    }
                                    let need_download = if is_daemon {
                                        true
                                    } else {
                                        !crate::geo::present(file.name)
                                    };
                                    if need_download {
                                        report(CoreDownloadEvent::Stage(format!("downloading {}…", file.name)));
                                        let mirror = crate::geo::effective_mirror(geo_cfg.mirror.as_deref());
                                        if let Err(error) = crate::geo::download(
                                            file,
                                            mirror.as_deref(),
                                            proxy.as_deref(),
                                            crate::geo::file_override(file, &geo_cfg),
                                        )
                                        .await
                                        {
                                            crate::logger::warn("geo", &format!("initial download of {} failed: {error}", file.name));
                                        }
                                    }
                                }
                                report(CoreDownloadEvent::Done {
                                    tag: release.tag_name.clone(),
                                    path: if is_daemon { Config::data_dir().join("bin/mihomo") } else { path },
                                });
                            }
                            Err(error) => {
                                if is_daemon {
                                    let _ = std::fs::remove_file(&download_dest);
                                }
                                report(CoreDownloadEvent::Failed(error.to_string()))
                            }
                        }
                    }
                    Err(error) => report(CoreDownloadEvent::Failed(error.to_string())),
                },
                Err(error) => report(CoreDownloadEvent::Failed(error.to_string())),
            }
        });
    }

    /// Apply one background-download event to the dialog and status line.
    /// Upgrade downloads (`core_upgrade`) report into the Settings Mihomo
    /// panel instead; on success the daemon gets a core *process* restart
    /// so the new binary takes over.
    pub(crate) async fn handle_core_download_event(&mut self, event: CoreDownloadEvent) {
        let upgrading = self.data.core_upgrade.is_some();
        match event {
            CoreDownloadEvent::Stage(stage) => {
                if upgrading {
                    self.data.mihomo_update.message = stage;
                } else if let Some(dialog) = self.ui.core_missing.as_mut() {
                    dialog.message = stage;
                    dialog.busy = true;
                }
            }
            CoreDownloadEvent::Progress(progress) => {
                if upgrading {
                    self.data.mihomo_update.download = Some((progress.downloaded, progress.total));
                } else if let Some(dialog) = self.ui.core_missing.as_mut() {
                    dialog.progress = Some((progress.downloaded, progress.total));
                }
            }
            CoreDownloadEvent::Done { tag, path } => {
                self.tasks.core_download.stop();
                if self.data.core_upgrade.take().is_some() {
                    self.data.mihomo_update.download = None;
                    self.data.mihomo_update.available = Some(false);
                    self.data.mihomo_update.message = format!("{tag} installed");
                    crate::logger::info("update", &format!("core upgraded to {tag}"));
                    match core::request_process_restart().await {
                        Ok(()) => self.say(format!("Mihomo {tag} installed, core restarting…")),
                        Err(error) => self.say(format!(
                            "Mihomo {tag} installed but core restart failed: {error}"
                        )),
                    }
                    return;
                }
                self.ui.core_missing = None;
                self.say(format!("Mihomo {tag} installed at {}", path.display()));
                crate::logger::info(
                    "update",
                    &format!("core {tag} installed at {}", path.display()),
                );
            }
            CoreDownloadEvent::Failed(error) => {
                self.tasks.core_download.stop();
                crate::logger::warn("update", &format!("download failed: {error}"));
                if self.data.core_upgrade.take().is_some() {
                    self.data.mihomo_update.download = None;
                    self.data.mihomo_update.message = format!("failed: {error}");
                    self.say(format!("Mihomo upgrade failed: {error}"));
                    return;
                }
                if let Some(dialog) = self.ui.core_missing.as_mut() {
                    dialog.busy = false;
                    dialog.message = format!("failed: {error}");
                }
                self.say(format!("Mihomo download failed: {error}"));
            }
        }
    }

    /// Cancel an in-flight core download (dialog Esc, Settings Esc).
    pub(crate) fn cancel_core_download(&mut self) {
        self.tasks.core_download.stop();
        if self.data.core_upgrade.take().is_some() {
            self.data.mihomo_update.download = None;
            self.data.mihomo_update.message = "download cancelled".into();
        }
        if let Some(dialog) = self.ui.core_missing.as_mut() {
            dialog.busy = false;
            dialog.progress = None;
            dialog.message = "download cancelled".into();
        }
        self.say("Mihomo download cancelled");
    }

    pub(crate) async fn poll_core_download_events(&mut self) {
        let drain = self.tasks.core_download.drain();
        for event in drain.events {
            self.handle_core_download_event(event).await;
        }
        // A panic leaves the download dialog stuck on "busy" with a frozen
        // progress bar, so clear the transient state and show the cause.
        if let Some(panic) = drain.panic {
            crate::logger::warn("update", &format!("core download task panicked: {panic}"));
            if self.data.core_upgrade.take().is_some() {
                self.data.mihomo_update.download = None;
                self.data.mihomo_update.message = format!("failed: {panic}");
            }
            if let Some(dialog) = self.ui.core_missing.as_mut() {
                dialog.busy = false;
                dialog.progress = None;
                dialog.message = format!("failed: {panic}");
            }
            self.say(format!("Mihomo download failed: {panic}"));
        } else if drain.disconnected && !drain.terminal {
            // Ended without Done/Failed and without a captured panic.
            crate::logger::warn("update", "core download task ended without an event");
            if let Some(dialog) = self.ui.core_missing.as_mut() {
                dialog.busy = false;
                dialog.progress = None;
                dialog.message = "failed: background task ended unexpectedly".into();
            }
            self.say("Mihomo download failed: background task ended unexpectedly");
        }
    }

    /// Adopt a user-provided core binary into the self-managed slot and drop
    /// any override pointer, so the managed copy is the single source of
    /// truth. The supervisor picks it up on its next start retry.
    pub(crate) async fn apply_core_path(&mut self, value: &str) {
        let src = PathBuf::from(value.trim());
        let res = if crate::config::is_system_mode() || crate::ipc::daemon_alive().await {
            // Stage to a temporary file in temp_dir so the daemon can read it even if
            // the source is in a restrictive home directory (e.g. 0700).
            let staged = std::env::temp_dir().join(format!(
                "clashlime-core-adopt-{}.tmp",
                std::process::id()
            ));
            match std::fs::copy(&src, &staged) {
                Ok(_) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt as _;
                        let _ = std::fs::set_permissions(
                            &staged,
                            std::fs::Permissions::from_mode(0o755),
                        );
                    }
                    let adopt_res = crate::ipc::adopt_core(&staged).await;
                    let _ = std::fs::remove_file(&staged);
                    adopt_res
                }
                Err(_) => {
                    // Fall back to passing src directly
                    crate::ipc::adopt_core(&src).await
                }
            }
        } else {
            let dest = Config::data_dir().join("bin/mihomo");
            core::adopt_core_binary(&src, &dest)
                .map(|_| ())
                .and_then(|_| Config::set_mihomo_override(None))
        };
        match res {
            Ok(()) => {
                let dest = Config::data_dir().join("bin/mihomo");
                self.say(format!(
                    "Using self-managed mihomo at {} (from {})",
                    dest.display(),
                    src.display()
                ));
                crate::logger::info(
                    "app",
                    &format!("core adopted {} -> {}", src.display(), dest.display()),
                );
                self.ui.core_missing = None;
            }
            Err(error) => self.say(format!("{error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{
        App, CoreMissingChoice, CoreMissingDialog, DataState, InputMode, TaskHub, UiState,
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
        app.ui.core_missing = Some(CoreMissingDialog {
            choice: CoreMissingChoice::Download,
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
    async fn core_missing_dialog_navigates_vertically_and_horizontally() {
        let mut app = test_app();
        assert_eq!(
            app.ui.core_missing.as_ref().unwrap().choice,
            CoreMissingChoice::Download
        );

        // Down / j switches to ProvidePath
        app.handle_core_missing_key(press(KeyCode::Down)).await;
        assert_eq!(
            app.ui.core_missing.as_ref().unwrap().choice,
            CoreMissingChoice::ProvidePath
        );

        // Up / k switches to Download
        app.handle_core_missing_key(press(KeyCode::Up)).await;
        assert_eq!(
            app.ui.core_missing.as_ref().unwrap().choice,
            CoreMissingChoice::Download
        );

        app.handle_core_missing_key(press(KeyCode::Char('j'))).await;
        assert_eq!(
            app.ui.core_missing.as_ref().unwrap().choice,
            CoreMissingChoice::ProvidePath
        );

        app.handle_core_missing_key(press(KeyCode::Char('k'))).await;
        assert_eq!(
            app.ui.core_missing.as_ref().unwrap().choice,
            CoreMissingChoice::Download
        );

        // Left / Right / Tab still toggle
        app.handle_core_missing_key(press(KeyCode::Tab)).await;
        assert_eq!(
            app.ui.core_missing.as_ref().unwrap().choice,
            CoreMissingChoice::ProvidePath
        );
        app.handle_core_missing_key(press(KeyCode::Right)).await;
        assert_eq!(
            app.ui.core_missing.as_ref().unwrap().choice,
            CoreMissingChoice::Download
        );

        // 'x' opens proxy input
        app.handle_core_missing_key(press(KeyCode::Char('x'))).await;
        assert!(matches!(
            app.ui.input_mode(),
            Some(InputMode::EditGeoProxy)
        ));
    }
}
