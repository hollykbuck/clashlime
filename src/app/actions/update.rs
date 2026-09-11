//! Mihomo core update checks hit the GitHub API and would freeze the TUI
//! if awaited inline, so they run in a background task like everything else
//! network-bound.

use crate::update::GithubRelease;

/// Events streamed back from the background update-check task.
pub enum UpdateCheckEvent {
    Done {
        current: String,
        release: GithubRelease,
        available: bool,
    },
    Failed(String),
}

impl crate::app::App {
    /// Start checking for a mihomo update in the background.
    pub(crate) fn start_mihomo_update_check(&mut self, force: bool) {
        if self.mihomo_update.checking {
            self.say("Update check already in progress");
            return;
        }
        // Prefer binary version, fallback to snapshot version, fallback to "unknown"
        let current = update_current_version(self);
        self.mihomo_update.checking = true;
        self.mihomo_update.message = "checking…".into();
        self.mihomo_update.current = current.clone();
        self.say("Checking mihomo update via GitHub…");
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<UpdateCheckEvent>();
        let handle = tokio::spawn(async move {
            match crate::update::check_update(&current, force).await {
                Ok((release, available)) => {
                    let _ = tx.send(UpdateCheckEvent::Done {
                        current,
                        release,
                        available,
                    });
                }
                Err(error) => {
                    let _ = tx.send(UpdateCheckEvent::Failed(format!("{error:#}")));
                }
            }
        });
        self.update_task = Some(handle);
        self.update_rx = Some(rx);
    }

    pub(crate) fn poll_update_check_events(&mut self) {
        use tokio::sync::mpsc::error::TryRecvError;
        loop {
            let next = self.update_rx.as_mut().map(|rx| rx.try_recv());
            match next {
                Some(Ok(event)) => self.handle_update_check_event(event),
                Some(Err(TryRecvError::Empty)) | None => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    self.update_rx = None;
                    self.update_task = None;
                    self.mihomo_update.checking = false;
                    self.say("Update check failed: background task ended unexpectedly");
                    break;
                }
            }
        }
    }

    fn handle_update_check_event(&mut self, event: UpdateCheckEvent) {
        self.update_rx = None;
        self.update_task = None;
        self.mihomo_update.checking = false;
        match event {
            UpdateCheckEvent::Done {
                current,
                release,
                available,
            } => {
                self.mihomo_update.latest = Some(release.tag_name.clone());
                self.mihomo_update.html_url = Some(release.html_url.clone());
                self.mihomo_update.available = Some(available);
                self.mihomo_update.prerelease = release.prerelease;
                self.mihomo_update.checked_at = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                );
                self.mihomo_update.message = if available {
                    format!("{} → {} available", current, release.tag_name)
                } else {
                    format!("{} up to date", current)
                };
                crate::logger::info(
                    "update",
                    &format!(
                        "check {} -> {} available={}",
                        current, release.tag_name, available
                    ),
                );
                self.say(if available {
                    format!(
                        "Update available: {} → {} ({})",
                        current, release.tag_name, release.html_url
                    )
                } else {
                    format!("Mihomo {} is up to date", current)
                });
            }
            UpdateCheckEvent::Failed(error) => {
                self.mihomo_update.available = None;
                self.mihomo_update.message = format!("failed: {error}");
                crate::logger::warn("update", &format!("check failed: {error}"));
                self.say(format!("Update check failed: {error}"));
            }
        }
    }

    /// Cancel an in-flight update check (Esc).
    pub(crate) fn cancel_update_check(&mut self) {
        if let Some(handle) = self.update_task.take() {
            handle.abort();
        }
        self.update_rx = None;
        self.mihomo_update.checking = false;
        self.say("Update check cancelled");
    }

    pub(crate) fn update_check_running(&self) -> bool {
        self.update_task.is_some()
    }

    pub(crate) fn open_update_url(&mut self) {
        let Some(url) = self.mihomo_update.html_url.clone().or_else(|| {
            // fallback to releases page
            Some("https://github.com/MetaCubeX/mihomo/releases".to_owned())
        }) else {
            self.say("No update URL");
            return;
        };
        let result = std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .or_else(|_| std::process::Command::new("open").arg(&url).spawn())
            .or_else(|_| {
                std::process::Command::new("gio")
                    .args(["open", &url])
                    .spawn()
            });
        match result {
            Ok(_) => self.say(format!("Opening {url}")),
            Err(e) => self.say(format!("Failed to open {url}: {e}")),
        }
    }
}

fn update_current_version(app: &crate::app::App) -> String {
    crate::update::current_version_from_binary()
        .ok()
        .or_else(|| {
            let v = app.snapshot.version.version.clone();
            if v.is_empty() || v == "—" {
                None
            } else {
                Some(v)
            }
        })
        .unwrap_or_else(|| "unknown".into())
}
