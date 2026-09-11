//! Profile activate / update run in background tasks: validation hits
//! `mihomo -t` and updates hit the network, both long enough to freeze the
//! TUI if awaited inline. Tasks own cloned data and hand updated
//! [`crate::profiles::Profiles`] back on success.

use crate::{core, profiles::Profiles};

/// Events streamed back from a background profile task.
pub enum ProfileEvent {
    Done {
        profiles: Profiles,
        message: String,
    },
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
        let Some(uid) = self.selected_uid() else {
            return;
        };
        if self.profile_task.is_some() {
            self.say("Profile operation already in progress");
            return;
        }
        self.say(format!("Validating profile {uid}…"));
        crate::logger::info("app", &format!("background profile activate: {uid}"));
        let profiles = self.profiles.clone();
        let config = self.config.clone();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ProfileEvent>();
        let handle = tokio::spawn(async move {
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
        self.profile_task = Some(handle);
        self.profile_rx = Some(rx);
    }

    /// Re-download + validate the selected profile without blocking the UI.
    pub(crate) fn start_update_profile(&mut self) {
        let Some(uid) = self.selected_uid() else {
            return;
        };
        if self.profile_task.is_some() {
            self.say("Profile operation already in progress");
            return;
        }
        self.say(format!("Updating profile {uid}…"));
        crate::logger::info("app", &format!("background profile update: {uid}"));
        let mut profiles = self.profiles.clone();
        let config = self.config.clone();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ProfileEvent>();
        let handle = tokio::spawn(async move {
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
        self.profile_task = Some(handle);
        self.profile_rx = Some(rx);
    }

    pub(crate) async fn poll_profile_events(&mut self) {
        use tokio::sync::mpsc::error::TryRecvError;
        loop {
            let next = self.profile_rx.as_mut().map(|rx| rx.try_recv());
            match next {
                Some(Ok(event)) => self.handle_profile_event(event).await,
                Some(Err(TryRecvError::Empty)) | None => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    self.profile_rx = None;
                    self.profile_task = None;
                    crate::logger::warn("app", "profile task ended unexpectedly");
                    self.say("Profile operation failed: background task ended unexpectedly");
                    break;
                }
            }
        }
    }

    async fn handle_profile_event(&mut self, event: ProfileEvent) {
        match event {
            ProfileEvent::Done { profiles, message } => {
                self.profile_rx = None;
                self.profile_task = None;
                self.profiles = profiles;
                crate::logger::info("app", &message);
                self.say(message);
                self.refresh().await;
            }
            ProfileEvent::Failed(error) => {
                self.profile_rx = None;
                self.profile_task = None;
                crate::logger::warn("app", &format!("profile operation failed: {error}"));
                self.say(format!("Profile operation failed: {error}"));
                self.refresh().await;
            }
        }
    }

    /// Update due subscriptions in the background (periodic refresh path).
    /// Partial progress survives: already-updated profiles are kept even if
    /// a later one fails.
    pub(crate) fn start_auto_update(&mut self, due: Vec<String>) {
        if self.profile_task.is_some() {
            return;
        }
        self.say(format!("Auto-updating {} profile(s)…", due.len()));
        crate::logger::info("app", &format!("background auto-update: {}", due.join(", ")));
        let mut profiles = self.profiles.clone();
        let config = self.config.clone();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ProfileEvent>();
        let handle = tokio::spawn(async move {
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
            if reload {
                if let Err(error) = core::request_restart().await {
                    let _ = tx.send(ProfileEvent::Done {
                        profiles,
                        message: format!(
                            "Auto-update applied {updated} profile(s), restart request failed: {error:#}"
                        ),
                    });
                    return;
                }
            }
            let _ = tx.send(ProfileEvent::Done {
                profiles,
                message: format!("Auto-update applied {updated} profile(s)"),
            });
        });
        self.profile_task = Some(handle);
        self.profile_rx = Some(rx);
    }

    /// Cancel an in-flight profile activate / update (Esc).
    pub(crate) fn cancel_profile_task(&mut self) {
        if let Some(handle) = self.profile_task.take() {
            handle.abort();
        }
        self.profile_rx = None;
        self.say("Profile operation cancelled");
    }

    pub(crate) fn profile_task_running(&self) -> bool {
        self.profile_task.is_some()
    }

    pub(crate) async fn delete_profile(&mut self) {
        let Some(uid) = self
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
}
