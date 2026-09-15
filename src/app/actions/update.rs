//! Mihomo core update checks hit the GitHub API and would freeze the TUI
//! if awaited inline, so they run in a background task like everything else
//! network-bound.

use crate::update::GithubRelease;
use std::path::PathBuf;

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
        let proxy = crate::geo::effective_proxy(self.config.geo.proxy.as_deref());
        let handle = tokio::spawn(async move {
            match crate::update::check_update(&current, force, proxy.as_deref()).await {
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
                self.mihomo_update.release = Some(release.clone());
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

    /// Install the retained release as a core upgrade (`i` in Settings).
    /// Downloads in the background into the self-managed slot, then asks
    /// the daemon for a core *process* restart so the new binary takes
    /// over (a reload would keep the old process). Esc cancels mid-flight.
    pub(crate) fn start_core_upgrade(&mut self) {
        if self.remote {
            self.say("Not available in remote mode (remote core is managed elsewhere)");
            return;
        }        if self.mihomo_update.available != Some(true) {
            self.say("No core update available (press u to check)");
            return;
        }
        let Some(release) = self.mihomo_update.release.clone() else {
            self.say("Release info expired, press u to check again");
            return;
        };
        let Some(slot) = self.upgrade_slot() else {
            return;
        };
        let tag = release.tag_name.clone();
        let proxy = crate::geo::effective_proxy(self.config.geo.proxy.as_deref());
        let (event_tx, rx) =
            tokio::sync::mpsc::unbounded_channel::<super::super::CoreDownloadEvent>();
        let handle = tokio::spawn(async move {
            install_release(release, slot, proxy, event_tx).await;
        });
        self.core_download_abort = Some(handle);
        self.core_download_rx = Some(rx);
        self.core_upgrade = Some(tag.clone());
        self.mihomo_update.download = Some((0, None));
        self.mihomo_update.message = format!("downloading {tag}…");
        self.say(format!("Downloading mihomo {tag}… (Esc cancels)"));
    }

    /// Reinstall the latest release into the self-managed slot (`I` in
    /// Settings), skipping the version comparison: recovery for a broken
    /// core binary. Same channel + process restart as an upgrade.
    pub(crate) fn start_core_reinstall(&mut self) {
        if self.remote {
            self.say("Not available in remote mode (remote core is managed elsewhere)");
            return;
        }        let Some(slot) = self.upgrade_slot() else {
            return;
        };
        let current = self.mihomo_update.current.clone();
        let proxy = crate::geo::effective_proxy(self.config.geo.proxy.as_deref());
        let (event_tx, rx) =
            tokio::sync::mpsc::unbounded_channel::<super::super::CoreDownloadEvent>();
        let handle = tokio::spawn(async move {
            let report = |event| {
                let _ = event_tx.send(event);
            };
            match crate::update::fetch_latest_release(true, proxy.as_deref()).await {
                Ok(release) => {
                    install_release(release, slot, proxy, event_tx).await;
                }
                Err(error) => report(super::super::CoreDownloadEvent::Failed(error.to_string())),
            }
        });
        self.core_download_abort = Some(handle);
        self.core_download_rx = Some(rx);
        self.core_upgrade = Some(current.clone());
        self.mihomo_update.download = Some((0, None));
        self.mihomo_update.message = "resolving latest release…".into();
        self.say("Reinstalling mihomo core… (Esc cancels)");
    }

    /// Guards shared by upgrade/reinstall: idle channel, a core on disk,
    /// and the self-managed slot (an explicit $CLASHLIME_MIHOMO / dialog
    /// override points elsewhere and must be updated by hand).
    fn upgrade_slot(&mut self) -> Option<std::path::PathBuf> {
        if self.core_download_rx.is_some() {
            self.say("Core download already in progress");
            return None;
        }
        if crate::app::App::core_missing() {
            self.say("No core installed yet");
            return None;
        }
        let slot = crate::config::Config::data_dir().join("bin/mihomo");
        if crate::config::Config::mihomo_path() != slot {
            self.say("Core is externally managed, update it manually");
            return None;
        }
        Some(slot)
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

/// Shared install worker for upgrade (`i`, retained release) and
/// reinstall (`I`, freshly fetched): pick this machine's asset, stream it
/// into the self-managed `slot` with progress, report `Done`/`Failed`.
async fn install_release(
    release: GithubRelease,
    slot: PathBuf,
    proxy: Option<String>,
    event_tx: tokio::sync::mpsc::UnboundedSender<super::super::CoreDownloadEvent>,
) {
    use super::super::CoreDownloadEvent::{Done, Failed, Progress, Stage};
    let report = |event| {
        let _ = event_tx.send(event);
    };
    let asset = match crate::update::pick_asset(&release) {
        Ok(asset) => asset,
        Err(error) => {
            report(Failed(error.to_string()));
            return;
        }
    };
    report(Stage(format!(
        "downloading {} ({})…",
        asset.name,
        crate::update::format_size(asset.size as usize)
    )));
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::update::DownloadProgress>();
    let forwarder_tx = event_tx.clone();
    let forwarder = tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            if forwarder_tx.send(Progress(progress)).is_err() {
                break;
            }
        }
    });
    let result =
        crate::update::download_core(&asset, &slot, Some(&progress_tx), proxy.as_deref()).await;
    drop(progress_tx);
    let _ = forwarder.await;
    match result {
        Ok(path) => report(Done {
            tag: release.tag_name.clone(),
            path,
        }),
        Err(error) => report(Failed(error.to_string())),
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
