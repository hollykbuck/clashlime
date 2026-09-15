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
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Char('h')
            | KeyCode::Char('l')
            | KeyCode::Tab => {
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
                        self.ui.input = Some(InputMode::CorePath);
                        self.ui.clear_input();
                        self.ui.core_missing = None;
                    }
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => self.download_core().await,
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.ui.input = Some(InputMode::CorePath);
                self.ui.clear_input();
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

        let destination = Config::data_dir().join("bin/mihomo");
        let proxy = crate::geo::effective_proxy(self.config.geo.proxy.as_deref());
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
                        let result = update::download_core(
                            &asset,
                            &destination,
                            Some(&progress_tx),
                            proxy.as_deref(),
                        )
                        .await;
                        drop(progress_tx);
                        let _ = forwarder.await;
                        match result {
                            Ok(path) => report(CoreDownloadEvent::Done {
                                tag: release.tag_name.clone(),
                                path,
                            }),
                            Err(error) => report(CoreDownloadEvent::Failed(error.to_string())),
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
        // Disconnect needs no report here: Done/Failed always precede it,
        // and a cancelled run clears the slot via `stop`.
        for event in self.tasks.core_download.drain().events {
            self.handle_core_download_event(event).await;
        }
    }

    /// Adopt a user-provided core binary into the self-managed slot and drop
    /// any override pointer, so the managed copy is the single source of
    /// truth. The supervisor picks it up on its next start retry.
    pub(crate) fn apply_core_path(&mut self, value: &str) {
        let src = PathBuf::from(value.trim());
        let dest = Config::data_dir().join("bin/mihomo");
        let path = match core::adopt_core_binary(&src, &dest) {
            Ok(path) => path,
            Err(error) => {
                self.say(format!("{error}"));
                return;
            }
        };
        match Config::set_mihomo_override(None) {
            Ok(()) => {
                self.say(format!(
                    "Using self-managed mihomo at {} (from {})",
                    path.display(),
                    src.display()
                ));
                crate::logger::info(
                    "app",
                    &format!("core adopted {} -> {}", src.display(), path.display()),
                );
            }
            Err(error) => self.say(format!("Installed but save failed: {error}")),
        }
    }
}
