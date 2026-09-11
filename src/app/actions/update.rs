use crate::update;

impl crate::app::App {
    pub(crate) async fn check_mihomo_update(&mut self, force: bool) {
        if self.mihomo_update.checking {
            self.say("Update check already in progress");
            return;
        }
        self.mihomo_update.checking = true;
        self.mihomo_update.message = "checking…".into();
        self.say("Checking mihomo update via GitHub…");
        // Prefer binary version, fallback to snapshot version, fallback to "unknown"
        let current = update::current_version_from_binary()
            .ok()
            .or_else(|| {
                let v = self.snapshot.version.version.clone();
                if v.is_empty() || v == "—" {
                    None
                } else {
                    Some(v)
                }
            })
            .unwrap_or_else(|| "unknown".into());
        self.mihomo_update.current = current.clone();
        match update::check_update(&current, force).await {
            Ok((release, available)) => {
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
                self.mihomo_update.checking = false;
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
            Err(e) => {
                self.mihomo_update.checking = false;
                self.mihomo_update.available = None;
                self.mihomo_update.message = format!("failed: {e}");
                crate::logger::warn("update", &format!("check failed: {e}"));
                self.say(format!("Update check failed: {e}"));
            }
        }
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
