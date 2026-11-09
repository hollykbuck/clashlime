//! Profile activate / update run in background tasks: validation hits
//! `mihomo -t` and updates hit the network, both long enough to freeze the
//! TUI if awaited inline. Tasks own cloned data and hand updated
//! [`crate::profiles::Profiles`] back on success.

use crate::{
    app::{InputMode, backend::Capability, input::ProfileTextField},
    profiles::Profiles,
};

/// Events streamed back from a background profile task.
pub enum ProfileEvent {
    Done { profiles: Profiles, message: String },
    Failed(String),
}

impl crate::app::App {
    fn selected_uid(&self) -> Option<String> {
        self.data
            .profiles
            .items
            .get(self.ui.profile_index)
            .map(|item| item.uid.clone())
    }

    /// Validate + activate the selected profile without blocking the UI.
    pub(crate) fn start_select_profile(&mut self) {
        if !self.require(Capability::ManageProfiles) {
            return;
        }
        let Some(uid) = self.selected_uid() else {
            return;
        };
        if self.tasks.profile.running() {
            self.say("Profile operation already in progress");
            return;
        }
        self.say(format!("Validating profile {uid}…"));
        crate::logger::info("app", &format!("background profile activate: {uid}"));
        self.tasks.profile.spawn(|tx| async move {
            match crate::ipc::activate_profile(&uid).await {
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
        if !self.require(Capability::ManageProfiles) {
            return;
        }
        let Some(uid) = self.selected_uid() else {
            return;
        };
        if self.tasks.profile.running() {
            self.say("Profile operation already in progress");
            return;
        }
        self.say(format!("Updating profile {uid}…"));
        crate::logger::info("app", &format!("background profile update: {uid}"));
        self.tasks.profile.spawn(|tx| async move {
            match crate::ipc::update_profile(&uid).await {
                Ok(profiles) => {
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
        let drain = self.tasks.profile.drain();
        for event in drain.events {
            self.handle_profile_event(event).await;
        }
        // A panic is reported even when a terminal event already arrived;
        // only a panic-free silent end is the vague failure.
        if let Some(panic) = drain.panic {
            crate::logger::warn("app", &format!("profile task panicked: {panic}"));
            self.say(format!("Profile operation failed: {panic}"));
        } else if drain.disconnected && !drain.terminal {
            crate::logger::warn("app", "profile task ended unexpectedly");
            self.say("Profile operation failed: background task ended unexpectedly");
        }
    }

    async fn handle_profile_event(&mut self, event: ProfileEvent) {
        match event {
            ProfileEvent::Done { profiles, message } => {
                self.tasks.profile.stop();
                self.data.profiles = profiles;
                crate::logger::info("app", &message);
                self.say(message);
                self.refresh_full().await;
            }
            ProfileEvent::Failed(error) => {
                self.tasks.profile.stop();
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
        if self.tasks.profile.running() {
            return;
        }
        self.say(format!("Auto-updating {} profile(s)…", due.len()));
        crate::logger::info(
            "app",
            &format!("background auto-update: {}", due.join(", ")),
        );
        let mut profiles = self.data.profiles.clone();
        self.tasks.profile.spawn(|tx| async move {
            let mut updated = 0;
            for uid in &due {
                match crate::ipc::update_profile(uid).await {
                    Ok(latest) => {
                        profiles = latest;
                        updated += 1;
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
            let _ = tx.send(ProfileEvent::Done {
                profiles,
                message: format!("Auto-update applied {updated} profile(s)"),
            });
        });
    }

    /// Cancel an in-flight profile activate / update (Esc).
    pub(crate) fn cancel_profile_task(&mut self) {
        self.tasks.profile.stop();
        self.say("Profile operation cancelled");
    }

    pub(crate) fn profile_task_running(&self) -> bool {
        self.tasks.profile.running()
    }

    pub(crate) async fn delete_profile(&mut self) {
        if !self.require(Capability::ManageProfiles) {
            return;
        }
        let Some(uid) = self
            .data
            .profiles
            .items
            .get(self.ui.profile_index)
            .map(|item| item.uid.clone())
        else {
            return;
        };
        match crate::ipc::delete_profile(&uid).await {
            Ok(profiles) => {
                self.data.profiles = profiles;
                self.say(format!("Profile {uid} deleted"));
            }
            Err(error) => self.say(format!("Delete failed: {error}")),
        }
    }

    /// Rows in the update-settings editor (`e` on Profiles). Row 0 is
    /// the name (works for local profiles too); the rest need a URL.
    pub(crate) const PROFILE_EDITOR_ROWS: usize = 9;

    /// Open the update-settings editor for the selected profile.
    pub(crate) fn open_profile_editor(&mut self) {
        if !self.require(Capability::ManageProfiles) {
            return;
        }
        if self
            .data
            .profiles
            .items
            .get(self.ui.profile_index)
            .is_none()
        {
            return;
        }
        self.ui.profile_editor = true;
        self.ui.profile_editor_index = 0;
    }

    pub(crate) async fn handle_profile_editor_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc | KeyCode::Char('e') => self.ui.profile_editor = false,
            KeyCode::Down | KeyCode::Char('j') => {
                self.ui.profile_editor_index =
                    (self.ui.profile_editor_index + 1) % Self::PROFILE_EDITOR_ROWS;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let rows = Self::PROFILE_EDITOR_ROWS;
                self.ui.profile_editor_index = (self.ui.profile_editor_index + rows - 1) % rows;
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.activate_profile_editor_row().await,
            _ => {}
        }
    }

    async fn activate_profile_editor_row(&mut self) {
        let Some(profile) = self.data.profiles.items.get(self.ui.profile_index) else {
            self.ui.profile_editor = false;
            return;
        };
        let remote = profile.url.is_some();
        // Name is editable for every profile; update rows need a URL.
        if self.ui.profile_editor_index == 0 {
            let input_text = ProfileTextField::Name.initial(self);
            self.ui.begin_input(InputMode::EditProfileName, input_text);
            return;
        }
        if !remote {
            self.say("Only remote profiles have update settings");
            return;
        }
        match self.ui.profile_editor_index {
            2 => {
                self.toggle_profile_flag("Auto update", true, |profile| {
                    &mut profile.auto_update
                })
                .await
            }
            4 => {
                self.toggle_profile_flag("Pin interval", false, |profile| {
                    &mut profile.fixed_interval
                })
                .await
            }
            6 => {
                self.toggle_profile_flag("Fetch via proxy", false, |profile| {
                    &mut profile.use_proxy
                })
                .await
            }
            1 | 3 | 5 | 7 | 8 => {
                let field = match self.ui.profile_editor_index {
                    1 => ProfileTextField::Url,
                    3 => ProfileTextField::Interval,
                    5 => ProfileTextField::Timeout,
                    7 => ProfileTextField::Auth,
                    _ => ProfileTextField::UserAgent,
                };
                let mode = match field {
                    ProfileTextField::Name => InputMode::EditProfileName,
                    ProfileTextField::Url => InputMode::EditProfileUrl,
                    ProfileTextField::Interval => InputMode::EditProfileInterval,
                    ProfileTextField::Timeout => InputMode::EditProfileTimeout,
                    ProfileTextField::Auth => InputMode::EditProfileAuth,
                    ProfileTextField::UserAgent => InputMode::EditProfileUserAgent,
                };
                let input_text = field.initial(self);
                self.ui.begin_input(mode, input_text);
            }
            _ => {}
        }
    }

    /// Flip an `Option<bool>` update flag (`None` stands for `default`).
    /// `to_flag` picks the field so all three toggles share one saver.
    async fn toggle_profile_flag(
        &mut self,
        label: &str,
        default: bool,
        to_flag: impl FnOnce(&mut crate::profiles::Profile) -> &mut Option<bool>,
    ) {
        let previous = self.data.profiles.clone();
        let Some(profile) = self.data.profiles.items.get_mut(self.ui.profile_index) else {
            return;
        };
        let flag = to_flag(profile);
        let enabled = !flag.unwrap_or(default);
        *flag = Some(enabled);
        let candidate = self.data.profiles.clone();
        match crate::ipc::save_profiles(candidate).await {
            Ok(()) => {
                crate::logger::info("app", &format!("profile {label} -> {enabled}"));
                self.say(format!(
                    "Profile {label} {}",
                    if enabled { "on" } else { "off" }
                ));
            }
            Err(error) => {
                self.data.profiles = previous;
                self.say(format!("Save failed: {error}"));
            }
        }
    }
}
