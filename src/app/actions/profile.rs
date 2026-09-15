//! Profile activate / update run in background tasks: validation hits
//! `mihomo -t` and updates hit the network, both long enough to freeze the
//! TUI if awaited inline. Tasks own cloned data and hand updated
//! [`crate::profiles::Profiles`] back on success.

use crate::{
    app::{InputMode, input::ProfileTextField},
    core,
    profiles::Profiles,
};

/// Events streamed back from a background profile task.
pub enum ProfileEvent {
    Done { profiles: Profiles, message: String },
    Failed(String),
}

impl crate::app::App {
    fn selected_uid(&self) -> Option<String> {
        self.profiles
            .items
            .get(self.profile_index)
            .map(|item| item.uid.clone())
    }

    /// Validate + activate the selected profile without blocking the UI.
    pub(crate) fn start_select_profile(&mut self) {
        if self.remote {
            self.say("Not available in remote mode (profiles are managed on the remote core)");
            return;
        }        let Some(uid) = self.selected_uid() else {
            return;
        };
        if self.profile_task.running() {
            self.say("Profile operation already in progress");
            return;
        }
        self.say(format!("Validating profile {uid}…"));
        crate::logger::info("app", &format!("background profile activate: {uid}"));
        let profiles = self.profiles.clone();
        let config = self.config.clone();
        self.profile_task.spawn(|tx| async move {
            let mut candidate = profiles.clone();
            candidate.current = Some(uid.clone());
            let result = async {
                core::CoreManager::new()
                    .validate_only(&config, &candidate)
                    .await?;
                candidate.save()?;
                core::request_restart()
                    .await
                    .map_err(|error| anyhow::anyhow!("restart request failed: {error:#}"))?;
                anyhow::Result::<_>::Ok(candidate)
            }
            .await;
            match result {
                Ok(candidate) => {
                    let _ = tx.send(ProfileEvent::Done {
                        profiles: candidate,
                        message: format!("Profile {uid} activated"),
                    });
                }
                Err(error) => {
                    let _ = tx.send(ProfileEvent::Failed(format!("{error:#}")));
                }
            }
        });
    }

    /// Re-download + validate the selected profile without blocking the UI.
    pub(crate) fn start_update_profile(&mut self) {
        if self.remote {
            self.say("Not available in remote mode (profiles are managed on the remote core)");
            return;
        }        let Some(uid) = self.selected_uid() else {
            return;
        };
        if self.profile_task.running() {
            self.say("Profile operation already in progress");
            return;
        }
        self.say(format!("Updating profile {uid}…"));
        crate::logger::info("app", &format!("background profile update: {uid}"));
        let mut profiles = self.profiles.clone();
        let config = self.config.clone();
        self.profile_task.spawn(|tx| async move {
            match profiles.update_validated(&uid, &config).await {
                Ok(()) => {
                    let _ = tx.send(ProfileEvent::Done {
                        profiles,
                        message: format!("Profile {uid} updated"),
                    });
                }
                Err(error) => {
                    let _ = tx.send(ProfileEvent::Failed(format!("{error:#}")));
                }
            }
        });
    }

    pub(crate) async fn poll_profile_events(&mut self) {
        let drain = self.profile_task.drain();
        for event in drain.events {
            self.handle_profile_event(event).await;
        }
        if drain.disconnected {
            crate::logger::warn("app", "profile task ended unexpectedly");
            self.say("Profile operation failed: background task ended unexpectedly");
        }
    }

    async fn handle_profile_event(&mut self, event: ProfileEvent) {
        match event {
            ProfileEvent::Done { profiles, message } => {
                self.profile_task.stop();
                self.profiles = profiles;
                crate::logger::info("app", &message);
                self.say(message);
                self.refresh_full().await;
            }
            ProfileEvent::Failed(error) => {
                self.profile_task.stop();
                crate::logger::warn("app", &format!("profile operation failed: {error}"));
                self.say(format!("Profile operation failed: {error}"));
                self.refresh_full().await;
            }
        }
    }

    /// Update due subscriptions in the background (periodic refresh path).
    /// Partial progress survives: already-updated profiles are kept even if
    /// a later one fails.
    pub(crate) fn start_auto_update(&mut self, due: Vec<String>) {
        if self.profile_task.running() {
            return;
        }
        self.say(format!("Auto-updating {} profile(s)…", due.len()));
        crate::logger::info(
            "app",
            &format!("background auto-update: {}", due.join(", ")),
        );
        let mut profiles = self.profiles.clone();
        let config = self.config.clone();
        self.profile_task.spawn(|tx| async move {
            let current = profiles.current.clone();
            let mut reload = false;
            let mut updated = 0;
            for uid in &due {
                match profiles.update_validated(uid, &config).await {
                    Ok(()) => {
                        updated += 1;
                        reload |= current.as_deref() == Some(uid);
                    }
                    Err(error) => {
                        let _ = tx.send(ProfileEvent::Done {
                            profiles,
                            message: format!(
                                "Auto-update {uid} failed after {updated} updated: {error:#}"
                            ),
                        });
                        return;
                    }
                }
            }
            if reload && let Err(error) = core::request_restart().await {
                let _ = tx.send(ProfileEvent::Done {
                        profiles,
                        message: format!(
                            "Auto-update applied {updated} profile(s), restart request failed: {error:#}"
                        ),
                    });
                return;
            }
            let _ = tx.send(ProfileEvent::Done {
                profiles,
                message: format!("Auto-update applied {updated} profile(s)"),
            });
        });
    }

    /// Cancel an in-flight profile activate / update (Esc).
    pub(crate) fn cancel_profile_task(&mut self) {
        self.profile_task.stop();
        self.say("Profile operation cancelled");
    }

    pub(crate) fn profile_task_running(&self) -> bool {
        self.profile_task.running()
    }

    pub(crate) async fn delete_profile(&mut self) {
        if self.remote {
            self.say("Not available in remote mode (profiles are managed on the remote core)");
            return;
        }        let Some(uid) = self
            .profiles
            .items
            .get(self.profile_index)
            .map(|item| item.uid.clone())
        else {
            return;
        };
        match self.profiles.delete(&uid) {
            Ok(()) => self.say(format!("Profile {uid} deleted")),
            Err(error) => self.say(format!("Delete failed: {error}")),
        }
    }

    /// Rows in the update-settings editor (`e` on Profiles). Row 0 is
    /// the name (works for local profiles too); the rest need a URL.
    pub(crate) const PROFILE_EDITOR_ROWS: usize = 8;

    /// Open the update-settings editor for the selected profile.
    pub(crate) fn open_profile_editor(&mut self) {
        if self.remote {
            self.say("Not available in remote mode (profiles are managed on the remote core)");
            return;
        }        if self.profiles.items.get(self.profile_index).is_none() {
            return;
        }
        self.profile_editor = true;
        self.profile_editor_index = 0;
    }

    pub(crate) fn handle_profile_editor_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc | KeyCode::Char('e') => self.profile_editor = false,
            KeyCode::Down | KeyCode::Char('j') => {
                self.profile_editor_index =
                    (self.profile_editor_index + 1) % Self::PROFILE_EDITOR_ROWS;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let rows = Self::PROFILE_EDITOR_ROWS;
                self.profile_editor_index = (self.profile_editor_index + rows - 1) % rows;
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.activate_profile_editor_row(),
            _ => {}
        }
    }

    fn activate_profile_editor_row(&mut self) {
        let Some(profile) = self.profiles.items.get(self.profile_index) else {
            self.profile_editor = false;
            return;
        };
        let remote = profile.url.is_some();
        // Name is editable for every profile; update rows need a URL.
        if self.profile_editor_index == 0 {
            self.input = Some(InputMode::EditProfileName);
            self.input_buffer = ProfileTextField::Name.initial(self);
            self.input_cursor = self.input_buffer.chars().count();
            return;
        }
        if !remote {
            self.say("Only remote profiles have update settings");
            return;
        }
        match self.profile_editor_index {
            1 => self.toggle_profile_flag("Auto update", true, |profile| &mut profile.auto_update),
            3 => self
                .toggle_profile_flag("Pin interval", false, |profile| &mut profile.fixed_interval),
            5 => {
                self.toggle_profile_flag("Fetch via proxy", false, |profile| &mut profile.use_proxy)
            }
            2 | 4 | 6 | 7 => {
                let field = match self.profile_editor_index {
                    2 => ProfileTextField::Interval,
                    4 => ProfileTextField::Timeout,
                    6 => ProfileTextField::Auth,
                    _ => ProfileTextField::UserAgent,
                };
                let mode = match field {
                    ProfileTextField::Name => InputMode::EditProfileName,
                    ProfileTextField::Interval => InputMode::EditProfileInterval,
                    ProfileTextField::Timeout => InputMode::EditProfileTimeout,
                    ProfileTextField::Auth => InputMode::EditProfileAuth,
                    ProfileTextField::UserAgent => InputMode::EditProfileUserAgent,
                };
                self.input = Some(mode);
                self.input_buffer = field.initial(self);
                self.input_cursor = self.input_buffer.chars().count();
            }
            _ => {}
        }
    }

    /// Flip an `Option<bool>` update flag (`None` stands for `default`).
    /// `to_flag` picks the field so all three toggles share one saver.
    fn toggle_profile_flag(
        &mut self,
        label: &str,
        default: bool,
        to_flag: impl FnOnce(&mut crate::profiles::Profile) -> &mut Option<bool>,
    ) {
        let Some(profile) = self.profiles.items.get_mut(self.profile_index) else {
            return;
        };
        let flag = to_flag(profile);
        let enabled = !flag.unwrap_or(default);
        *flag = Some(enabled);
        match self.profiles.save() {
            Ok(()) => {
                crate::logger::info("app", &format!("profile {label} -> {enabled}"));
                self.say(format!(
                    "Profile {label} {}",
                    if enabled { "on" } else { "off" }
                ));
            }
            Err(error) => self.say(format!("Save failed: {error}")),
        }
    }
}
