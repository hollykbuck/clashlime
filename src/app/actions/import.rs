//! Profile imports run in a background task so the fetch + geo
//! provisioning + `mihomo -t` validation never freeze the TUI event loop.
//! The task owns cloned [`crate::profiles::Profiles`] / [`crate::config::Config`]
//! and hands the updated profiles back on success.

use crate::profiles::Profiles;

/// Events streamed back from the background import task.
pub enum ImportEvent {
    Done((Profiles, String)),
    Failed(String),
}

impl crate::app::App {
    /// Start importing `value` (URL or local path) in the background.
    /// Returns immediately; completion arrives via [`Self::poll_import_events`].
    pub(crate) fn start_import(&mut self, value: String) {
        if self.import_task.is_some() {
            self.say("Import already in progress");
            return;
        }
        self.input = None;
        self.input_buffer.clear();
        self.say(format!("Importing {value}…"));
        crate::logger::info("app", &format!("background import started: {value}"));
        let mut profiles = self.profiles.clone();
        let config = self.config.clone();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ImportEvent>();
        let handle = tokio::spawn(async move {
            let remote =
                value.starts_with("http://") || value.starts_with("https://");
            let result = if remote {
                profiles.import_remote(&value, None, &config).await
            } else {
                profiles
                    .import_local(std::path::Path::new(&value), None, &config)
                    .await
            };
            match result {
                Ok(uid) => {
                    let _ = tx.send(ImportEvent::Done((profiles, uid)));
                }
                Err(error) => {
                    let _ = tx.send(ImportEvent::Failed(format!("{error:#}")));
                }
            }
        });
        self.import_task = Some(handle);
        self.import_rx = Some(rx);
    }

    pub(crate) async fn poll_import_events(&mut self) {
        use tokio::sync::mpsc::error::TryRecvError;
        loop {
            let next = self.import_rx.as_mut().map(|rx| rx.try_recv());
            match next {
                Some(Ok(event)) => self.handle_import_event(event).await,
                Some(Err(TryRecvError::Empty)) | None => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    // Task died without reporting (panic): never leave a
                    // sticky Busy message on screen.
                    self.import_rx = None;
                    self.import_task = None;
                    crate::logger::warn("app", "import task ended unexpectedly");
                    self.say("Import failed: background task ended unexpectedly");
                    break;
                }
            }
        }
    }

    async fn handle_import_event(&mut self, event: ImportEvent) {
        match event {
            ImportEvent::Done((profiles, uid)) => {
                self.import_rx = None;
                self.import_task = None;
                self.profiles = profiles;
                self.profile_index = self.profiles.items.len().saturating_sub(1);
                self.say(format!("Imported {uid}"));
                self.refresh_full().await;
            }
            ImportEvent::Failed(error) => {
                self.import_rx = None;
                self.import_task = None;
                crate::logger::warn("app", &format!("import failed: {error}"));
                self.say(format!("Import failed: {error}"));
            }
        }
    }

    /// Cancel an in-flight import (Esc with no dialog open).
    pub(crate) fn cancel_import(&mut self) {
        if let Some(handle) = self.import_task.take() {
            handle.abort();
        }
        self.import_rx = None;
        self.say("Import cancelled");
    }

    pub(crate) fn import_running(&self) -> bool {
        self.import_task.is_some()
    }
}
