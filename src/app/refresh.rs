use crate::config::Config;
use crate::core;
use std::time::Instant;

/// Slow endpoints (rules dump, /memory) refresh at most this often on the
/// tick path. With hundreds of proxies `/memory` alone can block longer
/// than the whole tick interval, freezing key handling.
const SLOW_REFRESH_INTERVAL_SECS: u64 = 30;

impl super::App {
    /// Tick refresh: fast endpoints every time, slow ones on cadence.
    pub(crate) async fn refresh(&mut self) {
        self.refresh_inner(false).await;
    }

    /// Refresh with slow endpoints included (after user actions / manual `r`).
    pub(crate) async fn refresh_full(&mut self) {
        self.refresh_inner(true).await;
    }

    async fn refresh_inner(&mut self, force_slow: bool) {
        self.theme.refresh();
        if self.remote {
            self.refresh_remote(force_slow).await;
            return;
        }
        self.proxy_group_order = Config::proxy_group_order();
        self.update_due_profiles();
        self.supervisor = core::supervisor_state().await;
        // Log files, merged oldest-first with sources attached:
        // daemon + tui tails are re-read every tick (small). The mihomo
        // file seeds the backlog only while the `/logs` stream is still
        // pending; once it connects, streamed lines are preserved verbatim
        // instead. Never both: re-adding the file tail on top of kept
        // lines duplicated the whole backlog every tick while an idle
        // core withheld the stream headers.
        use crate::app::{LogEntry, LogSource};
        self.maintain_log_stream();
        self.maintain_mem_stream();
        self.maintain_traffic_stream();
        let daemon_logs = crate::logger::recent_logs_for("clashlime-daemon-", 60);
        let tui_logs = crate::logger::recent_logs(60);
        let mut combined = Vec::with_capacity(500);
        for line in daemon_logs {
            combined.push(LogEntry {
                source: LogSource::Daemon,
                text: format!("{}{line}", LogSource::Daemon.tag()),
            });
        }
        for line in tui_logs {
            combined.push(LogEntry {
                source: LogSource::Tui,
                text: format!("{}{line}", LogSource::Tui.tag()),
            });
        }
        if !self.log_backlog_loaded {
            for line in core::CoreManager::recent_logs(380).unwrap_or_default() {
                combined.push(LogEntry {
                    source: LogSource::Core,
                    text: line,
                });
            }
        } else {
            let kept: Vec<LogEntry> = self
                .logs
                .drain(..)
                .filter(|entry| entry.source == LogSource::Core)
                .collect();
            combined.extend(kept);
        }
        // Keep last 500
        if combined.len() > 500 {
            let drain = combined.len() - 500;
            combined.drain(0..drain);
        }
        self.logs = combined;
        match self.api.snapshot_fast().await {
            Ok(snapshot) => {
                // Preserve slow/streamed fields across fast refreshes.
                let (rules, providers, memory) = (
                    std::mem::take(&mut self.snapshot.rules),
                    std::mem::take(&mut self.snapshot.rule_providers),
                    self.snapshot.memory.take(),
                );
                self.snapshot = snapshot;
                self.snapshot.rules = rules;
                self.snapshot.rule_providers = providers;
                self.snapshot.memory = memory;
                self.online = true;
                self.set_default_status("Synced".into());
                self.refresh_slow_if_due(force_slow).await;
                self.clamp_selections();
            }
            Err(error) => {
                self.online = false;
                self.set_default_status(self.offline_status(&error.to_string()));
            }
        }
    }

    /// Pure remote tick: no daemon IPC, no local files/profiles.
    /// Group order comes from the API snapshot (sorted); logs come only
    /// from the TUI file tail plus the remote `/logs` stream.
    async fn refresh_remote(&mut self, force_slow: bool) {
        use crate::app::{LogEntry, LogSource};
        self.maintain_log_stream();
        self.maintain_mem_stream();
        self.maintain_traffic_stream();
        let tui_logs = crate::logger::recent_logs(60);
        let mut combined = Vec::with_capacity(500);
        for line in tui_logs {
            combined.push(LogEntry {
                source: LogSource::Tui,
                text: format!("{}{line}", LogSource::Tui.tag()),
            });
        }
        let kept: Vec<LogEntry> = self
            .logs
            .drain(..)
            .filter(|entry| entry.source == LogSource::Core)
            .collect();
        combined.extend(kept);
        if combined.len() > 500 {
            let drain = combined.len() - 500;
            combined.drain(0..drain);
        }
        self.logs = combined;
        match self.api.snapshot_fast().await {
            Ok(snapshot) => {
                let (rules, providers, memory) = (
                    std::mem::take(&mut self.snapshot.rules),
                    std::mem::take(&mut self.snapshot.rule_providers),
                    self.snapshot.memory.take(),
                );
                self.snapshot = snapshot;
                self.snapshot.rules = rules;
                self.snapshot.rule_providers = providers;
                self.snapshot.memory = memory;
                self.derive_remote_group_order();
                self.online = true;
                self.set_default_status("Synced (remote)".into());
                self.refresh_slow_if_due(force_slow).await;
                self.clamp_selections();
            }
            Err(error) => {
                self.online = false;
                self.set_default_status(self.offline_status(&error.to_string()));
            }
        }
    }

    /// Remote has no local runtime.yaml; keep a stable sorted order and
    /// preserve the cursor-friendly existing prefix when groups persist.
    fn derive_remote_group_order(&mut self) {
        let mut names: Vec<String> = self.snapshot.proxies.proxies.keys().cloned().collect();
        names.sort();
        // Keep well-known groups first for a stable layout.
        names.sort_by_key(|name| match name.as_str() {
            "GLOBAL" => 0,
            _ => 1,
        });
        self.proxy_group_order = names;
    }

    async fn refresh_slow_if_due(&mut self, force: bool) {
        if !self.online {
            return;
        }
        // Lazy load: rules/providers are only consumed by the Rules tab,
        // so never fetch them while looking elsewhere. Opening the tab
        // fetches on the next tick when nothing was loaded yet.
        if self.tab != crate::app::Tab::Rules {
            return;
        }
        let due = force
            || !self.rules_loaded
            || self
                .last_slow_refresh
                .is_none_or(|last| last.elapsed().as_secs() >= SLOW_REFRESH_INTERVAL_SECS);
        if !due {
            return;
        }
        self.fetch_rules().await;
    }

    /// Fetch rules/providers now. Shared by tab-open (first paint) and the
    /// slow tick (30s refresh while visible).
    pub(crate) async fn fetch_rules(&mut self) {
        match self.api.snapshot_slow().await {
            Ok(slow) => {
                self.snapshot.rules = slow.rules;
                self.snapshot.rule_providers = slow.rule_providers;
                self.rules_loaded = true;
                self.last_slow_refresh = Some(Instant::now());
            }
            Err(error) => {
                crate::logger::warn("app", &format!("slow refresh failed: {error:#}"));
            }
        }
    }

    pub(crate) fn offline_status(&self, api_error: &str) -> String {
        if self.remote {
            return format!("Remote API unavailable: {api_error}");
        }
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

    pub(crate) fn update_due_profiles(&mut self) {
        if self.profile_task_running() {
            return;
        }
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
                    && profile.auto_update_enabled()
                    && profile.update_interval.is_some_and(|hours| {
                        now.saturating_sub(profile.updated) >= (hours.saturating_mul(3600)) as i64
                    })
            })
            .map(|profile| profile.uid.clone())
            .collect();
        if due.is_empty() {
            return;
        }
        self.start_auto_update(due);
    }
}
