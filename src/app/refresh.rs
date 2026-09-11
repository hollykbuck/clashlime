use crate::core;
use crate::config::Config;
use std::time::Instant;

impl super::App {
    pub(crate) async fn refresh(&mut self) {
        self.theme.refresh();
        self.proxy_group_order = Config::proxy_group_order();
        self.update_due_profiles().await;
        self.supervisor = core::supervisor_state().await;
        // Merge mihomo + omash logs (omash first, then mihomo), keep 200 latest
        let mihomo_logs = core::CoreManager::recent_logs(150).unwrap_or_default();
        let omash_logs = crate::logger::recent_logs(50);
        let mut combined = Vec::with_capacity(200);
        // Prefix omash logs for distinguish
        for line in omash_logs {
            combined.push(format!("[omash] {line}"));
        }
        combined.extend(mihomo_logs);
        // Keep last 200
        if combined.len() > 200 {
            let drain = combined.len() - 200;
            combined.drain(0..drain);
        }
        self.logs = combined;
        match self.api.snapshot().await {
            Ok(snapshot) => {
                let totals = (
                    snapshot.connections.upload_total,
                    snapshot.connections.download_total,
                );
                let elapsed = self
                    .last_refresh
                    .map_or(1.0, |then| then.elapsed().as_secs_f64())
                    .max(0.1);
                self.speeds = if self.last_refresh.is_some() {
                    (
                        (totals.0.saturating_sub(self.previous_totals.0) as f64 / elapsed) as u64,
                        (totals.1.saturating_sub(self.previous_totals.1) as f64 / elapsed) as u64,
                    )
                } else {
                    (0, 0)
                };
                self.previous_totals = totals;
                self.last_refresh = Some(Instant::now());
                self.snapshot = snapshot;
                self.online = true;
                self.set_default_status("Synced".into());
                self.clamp_selections();
            }
            Err(error) => {
                self.online = false;
                self.set_default_status(self.offline_status(&error.to_string()));
            }
        }
    }

    pub(crate) fn offline_status(&self, api_error: &str) -> String {
        if self.profiles.items.is_empty() {
            return "Mihomo is not running: no profile imported. Open Profiles and press a to import."
                .into();
        }
        if !self.supervisor.enabled {
            return "Mihomo is stopped: disabled in Settings.".into();
        }
        self.supervisor
            .error
            .as_ref()
            .map(|error| format!("Mihomo is not running: {error}"))
            .unwrap_or_else(|| format!("Mihomo API unavailable: {api_error}"))
    }

    pub(crate) async fn update_due_profiles(&mut self) {
        if self
            .last_profile_check
            .is_some_and(|last| last.elapsed().as_secs() < 60)
        {
            return;
        }
        self.last_profile_check = Some(Instant::now());
        let now = chrono::Utc::now().timestamp();
        let due: Vec<_> = self
            .profiles
            .items
            .iter()
            .filter(|profile| {
                profile.url.is_some()
                    && profile.update_interval.is_some_and(|hours| {
                        now.saturating_sub(profile.updated) >= (hours.saturating_mul(3600)) as i64
                    })
            })
            .map(|profile| profile.uid.clone())
            .collect();
        let current = self.profiles.current.clone();
        let mut reload = false;
        for uid in due {
            match self.profiles.update_validated(&uid, &self.config).await {
                Ok(()) => reload |= current.as_deref() == Some(&uid),
                Err(error) => {
                    self.say(format!("Auto-update {uid} failed: {error}"));
                    return;
                }
            }
        }
        if reload && let Err(error) = core::request_restart().await {
            self.say(format!("Auto-update applied, restart request failed: {error}"));
        }
    }
}
