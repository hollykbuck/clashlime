//! Profile imports run in a background task so the fetch + geo
//! provisioning + `mihomo -t` validation never freeze the TUI event loop.
//! The task owns cloned [`crate::profiles::Profiles`] / [`crate::config::Config`]
//! and hands the updated profiles back on success.

use crate::app::backend::Capability;
use crate::app::state::ImportDraft;
use crate::profiles::{ImportOverrides, Profiles};

/// Events streamed back from the background import task.
pub enum ImportEvent {
    Done((Profiles, String)),
    Failed(String),
}

/// Dialog rows, rendered by the overlay so the layout and the key
/// handler cannot drift apart.
pub(crate) const SOURCE_ROW: usize = 0;
pub(crate) const NAME_ROW: usize = 1;
pub(crate) const ROWS: usize = 9;
/// Trailing action row: fetch, validate and add the profile.
pub(crate) const SUBMIT_ROW: usize = 8;

impl crate::app::App {
    /// Open the import dialog with the URL/path field focused.
    pub(crate) fn open_import_editor(&mut self) {
        if !self.require(Capability::ManageProfiles) {
            return;
        }
        if self.tasks.import.running() {
            self.say("Import already in progress");
            return;
        }
        self.ui.import_editor = true;
        self.ui.import_editor_index = 0;
        self.ui.import_draft = ImportDraft::default();
        self.ui.input = Some(crate::app::InputMode::ImportProfile);
        self.ui.clear_input();
    }

    pub(crate) fn handle_import_editor_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        // A text sub-input owns the keyboard while it is open.
        if self.ui.input.is_some() {
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.ui.import_editor = false;
                self.say("Import cancelled");
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.ui.import_editor_index = (self.ui.import_editor_index + 1) % ROWS;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.ui.import_editor_index = (self.ui.import_editor_index + ROWS - 1) % ROWS;
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.activate_import_editor_row(),
            _ => {}
        }
    }

    fn activate_import_editor_row(&mut self) {
        use crate::app::InputMode;
        let index = self.ui.import_editor_index;
        if index == SUBMIT_ROW {
            self.submit_import_draft();
            return;
        }
        if index == SOURCE_ROW {
            self.ui.input = Some(InputMode::ImportProfile);
            let initial = self.ui.import_draft.source.clone();
            self.ui.set_input(initial);
            return;
        }
        if index == NAME_ROW {
            self.ui.input = Some(InputMode::ImportProfileName);
            let initial = self.ui.import_draft.name.clone();
            self.ui.set_input(initial);
            return;
        }
        // Local YAML files have no subscription fetch, so the update
        // rows are as dead here as they are on a local profile.
        if !self.import_source_is_remote() {
            self.say("Only subscription URLs have update settings");
            return;
        }
        match index {
            2 => self.ui.import_draft.auto_update = !self.ui.import_draft.auto_update,
            5 => self.ui.import_draft.use_proxy = !self.ui.import_draft.use_proxy,
            3 | 4 | 6 | 7 => {
                let (mode, buffer) = match index {
                    3 => (
                        InputMode::ImportProfileInterval,
                        self.ui.import_draft.interval.clone(),
                    ),
                    4 => (
                        InputMode::ImportProfileTimeout,
                        self.ui.import_draft.timeout.clone(),
                    ),
                    6 => (
                        InputMode::ImportProfileAuth,
                        self.ui.import_draft.auth_token.clone(),
                    ),
                    _ => (
                        InputMode::ImportProfileUserAgent,
                        self.ui.import_draft.user_agent.clone(),
                    ),
                };
                self.ui.input = Some(mode);
                self.ui.set_input(buffer);
            }
            _ => {}
        }
    }

    fn import_source_is_remote(&self) -> bool {
        self.ui.import_draft.source_is_remote()
    }

    /// Finish the dialog: hand the draft to the background import.
    pub(crate) fn submit_import_draft(&mut self) {
        let value = self.ui.import_draft.source.trim().to_owned();
        self.ui.import_editor = false;
        self.ui.input = None;
        self.ui.clear_input();
        if value.is_empty() {
            self.say("Enter a subscription URL or an absolute YAML file path.");
            return;
        }
        let name = self.ui.import_draft.name.trim().to_owned();
        let overrides = match self.ui.import_draft.overrides() {
            Ok(overrides) => overrides,
            Err(message) => {
                self.say(message);
                return;
            }
        };
        self.start_import_with(value, name, overrides);
    }

    /// Start importing `value` (URL or local path) in the background.
    /// Returns immediately; completion arrives via [`Self::poll_import_events`].
    fn start_import_with(&mut self, value: String, name: String, overrides: ImportOverrides) {
        if !self.require(Capability::ManageProfiles) {
            return;
        }
        if self.tasks.import.running() {
            self.say("Import already in progress");
            return;
        }
        self.ui.input = None;
        self.ui.clear_input();
        self.say(format!("Importing {value}…"));
        crate::logger::info("app", &format!("background import started: {value}"));
        let mut profiles = self.data.profiles.clone();
        let config = self.config.clone();
        self.tasks.import.spawn(|tx| async move {
            let remote = value.starts_with("http://") || value.starts_with("https://");
            let name = (!name.is_empty()).then_some(name.as_str());
            let result = if remote {
                profiles
                    .import_remote(&value, name, overrides, &config)
                    .await
            } else {
                profiles
                    .import_local(std::path::Path::new(&value), name, &config)
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
        let drain = self.tasks.import.drain();
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
                self.tasks.import.stop();
                self.data.profiles = profiles;
                self.ui.profile_index = self.data.profiles.items.len().saturating_sub(1);
                self.say(format!("Imported {uid}"));
                self.refresh_full().await;
            }
            ImportEvent::Failed(error) => {
                self.tasks.import.stop();
                crate::logger::warn("app", &format!("import failed: {error}"));
                self.say(format!("Import failed: {error}"));
            }
        }
    }

    /// Cancel an in-flight import (Esc with no dialog open).
    pub(crate) fn cancel_import(&mut self) {
        self.tasks.import.stop();
        self.say("Import cancelled");
    }

    pub(crate) fn import_running(&self) -> bool {
        self.tasks.import.running()
    }
}
