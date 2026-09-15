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

    /// Single tick pipeline for both backends. Per-backend differences live
    /// in [`super::backend::Backend`]: group-order source, supervisor IPC,
    /// file log tails, and the synced/offline labels.
    async fn refresh_inner(&mut self, force_slow: bool) {
        use crate::app::LogSource;
        self.theme.refresh();
        let backend = self.backend();
        if backend.is_local() {
            if let Some(order) = backend.preload_group_order() {
                self.data.proxy_group_order = order;
            }
            self.update_due_profiles();
            self.data.supervisor = core::supervisor_state().await;
        }
        // Log files, merged oldest-first with sources attached:
        // TUI (+ daemon, when local) tails are re-read every tick (small).
        // The mihomo file seeds the backlog only while the `/logs` stream
        // is still pending; once it connects, streamed lines are preserved
        // verbatim instead. Never both: re-adding the file tail on top of
        // kept lines duplicated the whole backlog every tick while an idle
        // core withheld the stream headers.
        self.maintain_log_stream();
        self.maintain_mem_stream();
        self.maintain_traffic_stream();
        let mut combined = backend.file_log_tails();
        if backend.seed_core_backlog(self.tasks.log_backlog_loaded) {
            for line in core::CoreManager::recent_logs(380).unwrap_or_default() {
                combined.push(crate::app::LogEntry {
                    source: LogSource::Core,
                    text: line,
                });
            }
        } else {
            let kept: Vec<crate::app::LogEntry> = self
                .data.logs
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
        self.data.logs = combined;
        match self.api.snapshot_fast().await {
            Ok(snapshot) => {
                // Preserve slow/streamed fields across fast refreshes.
                let (rules, providers, memory) = (
                    std::mem::take(&mut self.data.snapshot.rules),
                    std::mem::take(&mut self.data.snapshot.rule_providers),
                    self.data.snapshot.memory.take(),
                );
                self.data.snapshot = snapshot;
                self.data.snapshot.rules = rules;
                self.data.snapshot.rule_providers = providers;
                self.data.snapshot.memory = memory;
                if backend.is_remote() {
                    self.data.proxy_group_order =
                        super::backend::Backend::remote_group_order(&self.data.snapshot);
                }
                self.data.online = true;
                self.set_default_status(backend.synced_label().into());
                self.refresh_slow_if_due(force_slow).await;
                self.clamp_selections();
            }
            Err(error) => {
                self.data.online = false;
                self.set_default_status(self.offline_status(&error.to_string()));
            }
        }
    }

    async fn refresh_slow_if_due(&mut self, force: bool) {
        if !self.data.online {
            return;
        }
        // Lazy load: rules/providers are only consumed by the Rules tab,
        // so never fetch them while looking elsewhere. Opening the tab
        // fetches on the next tick when nothing was loaded yet.
        if self.ui.tab != crate::app::Tab::Rules {
            return;
        }
        let due = force
            || !self.data.rules_loaded
            || self
                .data.last_slow_refresh
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
                self.data.snapshot.rules = slow.rules;
                self.data.snapshot.rule_providers = slow.rule_providers;
                self.data.rules_loaded = true;
                self.data.last_slow_refresh = Some(Instant::now());
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
        if self.data.profiles.items.is_empty() {
            return "Mihomo is not running: no profile imported. Open Profiles and press a to import."
                .into();
        }
        if !self.data.supervisor.enabled {
            return "Mihomo is stopped: disabled in Settings.".into();
        }
        self.data.supervisor
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
            .data.last_profile_check
            .is_some_and(|last| last.elapsed().as_secs() < 60)
        {
            return;
        }
        self.data.last_profile_check = Some(Instant::now());
        let now = chrono::Utc::now().timestamp();
        let due: Vec<_> = self
            .data.profiles
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
