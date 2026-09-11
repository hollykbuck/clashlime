use crate::{config::Config, core, update};
use crossterm::event::{KeyCode, KeyEvent};
use std::path::PathBuf;

use super::super::{CoreDownloadEvent, CoreMissingChoice, CoreMissingDialog, InputMode};

impl crate::app::App {
    /// True when no mihomo core can be located; drives the startup dialog.
    pub fn core_missing() -> bool {
        !Config::mihomo_path().is_file()
    }

    pub(crate) fn core_missing_dialog() -> Option<CoreMissingDialog> {
        Self::core_missing().then(|| CoreMissingDialog {
            choice: CoreMissingChoice::Download,
            busy: false,
            message: String::new(),
            progress: None,
        })
    }

    /// Bring back the chooser while the machine still lacks a usable core.
    pub(crate) fn reopen_core_missing_dialog(&mut self, choice: CoreMissingChoice) {
        if Self::core_missing() {
            let previous = self.core_missing.take();
            self.core_missing = Some(previous.unwrap_or(CoreMissingDialog {
                choice,
                busy: false,
                message: String::new(),
                progress: None,
            }));
            if let Some(dialog) = self.core_missing.as_mut() {
                dialog.busy = false;
                dialog.choice = choice;
            }
        }
    }

    pub(crate) async fn toggle_core(&mut self) {
        let enable = !core::core_desired_enabled().await;
        let result = core::request_core_enabled(enable).await.map(|()| {
            if enable && self.profiles.items.is_empty() {
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
        if self.core_missing.as_ref().is_some_and(|dialog| dialog.busy) {
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
                if let Some(dialog) = self.core_missing.as_mut() {
                    dialog.choice = match dialog.choice {
                        CoreMissingChoice::Download => CoreMissingChoice::ProvidePath,
                        CoreMissingChoice::ProvidePath => CoreMissingChoice::Download,
                    };
                }
            }
            KeyCode::Enter => {
                let choice = self
                    .core_missing
                    .as_ref()
                    .map(|dialog| dialog.choice)
                    .unwrap_or(CoreMissingChoice::Download);
                match choice {
                    CoreMissingChoice::Download => self.download_core().await,
                    CoreMissingChoice::ProvidePath => {
                        self.input = Some(InputMode::CorePath);
                        self.input_buffer.clear();
                        self.core_missing = None;
                    }
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => self.download_core().await,
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.input = Some(InputMode::CorePath);
                self.input_buffer.clear();
                self.core_missing = None;
            }
            KeyCode::Esc => {
                // Keep running without a core; supervisor retries in background
                self.core_missing = None;
                self.say("Skipped: no mihomo core. Settings will keep showing the warning.");
            }
            _ => {}
        }
    }

    pub(crate) async fn download_core(&mut self) {
        if self.core_download_rx.is_some() {
            return; // already running
        }
        let Some(dialog) = self.core_missing.as_mut() else {
            return;
        };
        dialog.busy = true;
        dialog.progress = None;
        dialog.message = "resolving latest release…".into();
        let (event_tx, rx) = tokio::sync::mpsc::unbounded_channel::<CoreDownloadEvent>();
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();

        let destination = Config::data_dir().join("bin/mihomo");
        let handle = tokio::spawn(async move {
            let report = |event| {
                let _ = event_tx.send(event);
            };
            report(CoreDownloadEvent::Stage("resolving latest release…".into()));
            match update::fetch_latest_release(true).await {
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
                        let result =
                            update::download_core(&asset, &destination, Some(&progress_tx)).await;
                        drop(progress_tx);
                        let _ = forwarder.await;
                        match result {
                            Ok(path) => report(CoreDownloadEvent::Done(path)),
                            Err(error) => report(CoreDownloadEvent::Failed(error.to_string())),
                        }
                    }
                    Err(error) => report(CoreDownloadEvent::Failed(error.to_string())),
                },
                Err(error) => report(CoreDownloadEvent::Failed(error.to_string())),
            }
        });
        self.core_download_abort = Some(handle);
        self.core_download_rx = Some(rx);
    }

    /// Apply one background-download event to the dialog and status line.
    pub(crate) fn handle_core_download_event(&mut self, event: CoreDownloadEvent) {
        match event {
            CoreDownloadEvent::Stage(stage) => {
                if let Some(dialog) = self.core_missing.as_mut() {
                    dialog.message = stage;
                    dialog.busy = true;
                }
            }
            CoreDownloadEvent::Progress(progress) => {
                if let Some(dialog) = self.core_missing.as_mut() {
                    dialog.progress = Some((progress.downloaded, progress.total));
                }
            }
            CoreDownloadEvent::Done(path) => {
                self.core_download_rx = None;
                self.core_download_abort = None;
                self.core_missing = None;
                self.say(format!("Mihomo installed at {}", path.display()));
                crate::logger::info("update", &format!("core installed at {}", path.display()));
            }
            CoreDownloadEvent::Failed(error) => {
                self.core_download_rx = None;
                self.core_download_abort = None;
                crate::logger::warn("update", &format!("download failed: {error}"));
                if let Some(dialog) = self.core_missing.as_mut() {
                    dialog.busy = false;
                    dialog.message = format!("failed: {error}");
                }
                self.say(format!("Mihomo download failed: {error}"));
            }
        }
    }

    /// Cancel an in-flight core download (dialog Esc).
    pub(crate) fn cancel_core_download(&mut self) {
        if let Some(handle) = self.core_download_abort.take() {
            handle.abort();
        }
        self.core_download_rx = None;
        if let Some(dialog) = self.core_missing.as_mut() {
            dialog.busy = false;
            dialog.progress = None;
            dialog.message = "download cancelled".into();
        }
        self.say("Mihomo download cancelled");
    }

    pub(crate) fn poll_core_download_events(&mut self) {
        while let Some(event) = self
            .core_download_rx
            .as_mut()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.handle_core_download_event(event);
        }
    }

    /// Validate a user-provided core binary and persist it in the dynamic
    /// config so the daemon picks it up on its next tick.
    pub(crate) fn apply_core_path(&mut self, value: &str) {
        let path = PathBuf::from(value.trim());
        if !path.is_file() {
            self.say(format!("No file at {}", path.display()));
            return;
        }
        #[cfg(unix)]
        if let Ok(meta) = std::fs::metadata(&path) {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o111 == 0
                && let Err(error) =
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            {
                self.say(format!(
                    "{} is not executable (chmod failed: {error})",
                    path.display()
                ));
                return;
            }
        }
        // Sanity check: it must print a version string
        if let Err(error) = update::version_from_binary_at(&path) {
            self.say(format!("{} rejected: {error}", path.display()));
            return;
        }
        match Config::set_mihomo_override(Some(&path)) {
            Ok(()) => {
                self.say(format!("Using mihomo at {}", path.display()));
                crate::logger::info("app", &format!("core path override -> {}", path.display()));
            }
            Err(error) => self.say(format!("Binary ok but save failed: {error}")),
        }
    }
}
