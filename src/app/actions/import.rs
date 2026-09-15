//! Profile imports run in a background task so the fetch + geo
//! provisioning + `mihomo -t` validation never freeze the TUI event loop.
//! The task owns cloned [`crate::profiles::Profiles`] / [`crate::config::Config`]
//! and hands the updated profiles back on success.

use crate::app::backend::Capability;
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
        if !self.require(Capability::ManageProfiles) {
            return;
        }        if self.import_task.running() {
            self.say("Import already in progress");
            return;
        }
        self.input = None;
        self.clear_input();
        self.say(format!("Importing {value}…"));
        crate::logger::info("app", &format!("background import started: {value}"));
        let mut profiles = self.profiles.clone();
        let config = self.config.clone();
        self.import_task.spawn(|tx| async move {
            let remote = value.starts_with("http://") || value.starts_with("https://");
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
    }

    pub(crate) async fn poll_import_events(&mut self) {
        let drain = self.import_task.drain();
        for event in drain.events {
            self.handle_import_event(event).await;
        }
        if drain.disconnected {
            // Task died without reporting (panic): never leave a
            // sticky Busy message on screen.
            crate::logger::warn("app", "import task ended unexpectedly");
            self.say("Import failed: background task ended unexpectedly");
        }
    }

    async fn handle_import_event(&mut self, event: ImportEvent) {
        match event {
            ImportEvent::Done((profiles, uid)) => {
                self.import_task.stop();
                self.profiles = profiles;
                self.profile_index = self.profiles.items.len().saturating_sub(1);
                self.say(format!("Imported {uid}"));
                self.refresh_full().await;
            }
            ImportEvent::Failed(error) => {
                self.import_task.stop();
                crate::logger::warn("app", &format!("import failed: {error}"));
                self.say(format!("Import failed: {error}"));
            }
        }
    }

    /// Cancel an in-flight import (Esc with no dialog open).
    pub(crate) fn cancel_import(&mut self) {
        self.import_task.stop();
        self.say("Import cancelled");
    }

    pub(crate) fn import_running(&self) -> bool {
        self.import_task.running()
    }
}
