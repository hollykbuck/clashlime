use crate::core;
use crate::config::Config;
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
        self.proxy_group_order = Config::proxy_group_order();
        self.update_due_profiles();
        self.supervisor = core::supervisor_state().await;
        // Log files, merged oldest-first with sources attached:
        // daemon + tui tails are re-read every tick (small), the mihomo
        // file only seeds the startup backlog — live lines arrive via the
        // `/logs` stream task. Streamed core lines are preserved verbatim.
        use crate::app::{LogEntry, LogSource};
        self.maintain_log_stream();
        let daemon_logs = crate::logger::recent_logs_for("omash-daemon-", 60);
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
        }
        let kept: Vec<LogEntry> = self
            .logs
            .drain(..)
            .filter(|entry| entry.source == LogSource::Core)
            .collect();
        combined.extend(kept);
        // Keep last 500
        if combined.len() > 500 {
            let drain = combined.len() - 500;
            combined.drain(0..drain);
        }
        self.logs = combined;
        match self.api.snapshot_fast().await {
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
                // Preserve slow fields across fast refreshes.
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

    async fn refresh_slow_if_due(&mut self, force: bool) {
        if !self.online {
            return;
        }
        let due = force
            || self.last_slow_refresh.is_none_or(|last| {
                last.elapsed().as_secs() >= SLOW_REFRESH_INTERVAL_SECS
            });
        if !due {
            return;
        }
        match self.api.snapshot_slow().await {
            Ok(slow) => {
                self.snapshot.rules = slow.rules;
                self.snapshot.rule_providers = slow.rule_providers;
                // The /memory stream opens with an `inuse: 0` sentinel; keep
                // the last real sample instead of flashing 0 B.
                let keep_previous = slow.memory.as_ref().is_some_and(|sample| sample.inuse == 0)
                    && self
                        .snapshot
                        .memory
                        .as_ref()
                        .is_some_and(|sample| sample.inuse > 0);
                if !keep_previous {
                    self.snapshot.memory = slow.memory;
                }
                self.last_slow_refresh = Some(Instant::now());
            }
            Err(error) => {
                crate::logger::warn("app", &format!("slow refresh failed: {error:#}"));
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
