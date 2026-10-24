use crate::core;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Slow endpoints (rules dump, /memory) refresh at most this often on the
/// tick path. With hundreds of proxies `/memory` alone can block longer
/// than the whole tick interval, freezing key handling.
const SLOW_REFRESH_INTERVAL_SECS: u64 = 30;

/// Two snapshots closer than this produce meaningless rates: user actions
/// (`r`, tab switches) can trigger a full refresh milliseconds after a
/// tick, and dividing a counter delta by ~0 would spike the display.
const MIN_RATE_INTERVAL: Duration = Duration::from_millis(250);

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
            if let Some(order) = backend.preload_provider_order() {
                // A profile switch can add, drop or rename providers. Drop
                // the stale snapshot so the panel cannot keep painting the
                // previous profile's providers (and re-arm the lazy fetch).
                if self.data.proxy_provider_order != order {
                    self.data.snapshot.proxy_providers.clear();
                    self.data.proxy_providers_loaded = false;
                }
                self.data.proxy_provider_order = order;
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
                .data
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
        self.data.logs = combined;
        match self.api.snapshot_fast().await {
            Ok(snapshot) => {
                // Preserve slow/streamed fields across fast refreshes.
                let (rules, providers, proxy_providers, memory) = (
                    std::mem::take(&mut self.data.snapshot.rules),
                    std::mem::take(&mut self.data.snapshot.rule_providers),
                    std::mem::take(&mut self.data.snapshot.proxy_providers),
                    self.data.snapshot.memory.take(),
                );
                self.sample_connection_rates(&snapshot.connections);
                self.data.snapshot = snapshot;
                self.data.snapshot.rules = rules;
                self.data.snapshot.rule_providers = providers;
                self.data.snapshot.proxy_providers = proxy_providers;
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
                self.data.connection_rates.clear();
                self.data.connection_sample_at = None;
                self.set_default_status(self.offline_status(&error.to_string()));
            }
        }
    }

    /// Turn cumulative `/connections` counters into per-second rates for
    /// the new snapshot, using the previous one and the real elapsed
    /// time. Connections that just appeared start at zero; ids that
    /// vanished are dropped.
    fn sample_connection_rates(&mut self, next: &crate::api::ConnectionResponse) {
        let now = Instant::now();
        let Some(previous_at) = self.data.connection_sample_at else {
            self.data.connection_sample_at = Some(now);
            self.data.connection_rates.clear();
            return;
        };
        let elapsed = now.duration_since(previous_at);
        if elapsed < MIN_RATE_INTERVAL {
            // Keep the last usable rates; the counters are cumulative, so
            // the next accepted sample measures over the longer interval.
            return;
        }
        self.data.connection_rates = connection_rates(
            &self.data.snapshot.connections.connections,
            &next.connections,
            elapsed,
        );
        self.data.connection_sample_at = Some(now);
    }

    async fn refresh_slow_if_due(&mut self, force: bool) {
        if !self.data.online {
            return;
        }
        // Lazy load, per consumer: the Rules tab is the only reader of
        // rules and rule providers, the Proxies tab the only reader of
        // proxy providers. `/rules` alone is a multi-MB dump, so pulling
        // it every 30s while someone watches the proxy list is pure
        // waste; the two slow fetches stay separate.
        let want_rules = self.ui.tab == crate::app::Tab::Rules;
        let want_providers =
            self.ui.tab == crate::app::Tab::Proxies && !self.data.proxy_provider_order.is_empty();
        if !want_rules && !want_providers {
            return;
        }
        let due = force
            || (want_rules && !self.data.rules_loaded)
            || (want_providers && !self.data.proxy_providers_loaded)
            || self
                .data
                .last_slow_refresh
                .is_none_or(|last| last.elapsed().as_secs() >= SLOW_REFRESH_INTERVAL_SECS);
        if !due {
            return;
        }
        if want_rules {
            self.fetch_rules().await;
        }
        if want_providers {
            self.fetch_proxy_providers().await;
        }
    }

    /// Fetch rules and rule providers now. Shared by tab-open (first
    /// paint) and the slow tick (30s refresh while the Rules tab is up).
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

    /// Fetch the declared proxy providers now, one request per name.
    /// A no-op when the profile declares none, so the common inline
    /// profile never issues a provider request at all.
    pub(crate) async fn fetch_proxy_providers(&mut self) {
        let names = self.data.proxy_provider_order.clone();
        if names.is_empty() {
            self.data.proxy_providers_loaded = true;
            return;
        }
        match self.api.proxy_providers(&names).await {
            Ok(providers) => {
                self.data.snapshot.proxy_providers = providers;
                self.data.proxy_providers_loaded = true;
                self.data.last_slow_refresh = Some(Instant::now());
            }
            Err(error) => {
                crate::logger::warn("app", &format!("proxy provider refresh failed: {error:#}"));
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
        self.data
            .supervisor
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
            .data
            .last_profile_check
            .is_some_and(|last| last.elapsed().as_secs() < 60)
        {
            return;
        }
        self.data.last_profile_check = Some(Instant::now());
        let now = chrono::Utc::now().timestamp();
        let due: Vec<_> = self
            .data
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

/// Pure rate calculation kept out of [`super::App`] so it can be tested
/// without a client or a running core.
fn connection_rates(
    previous: &[crate::api::Connection],
    next: &[crate::api::Connection],
    elapsed: Duration,
) -> HashMap<String, (u64, u64)> {
    let previous: HashMap<&str, (u64, u64)> = previous
        .iter()
        .map(|connection| {
            (
                connection.id.as_str(),
                (connection.upload, connection.download),
            )
        })
        .collect();
    let seconds = elapsed.as_secs_f64().max(f64::MIN_POSITIVE);
    next.iter()
        .map(|connection| {
            let (upload, download) =
                previous
                    .get(connection.id.as_str())
                    .map_or((0, 0), |&(upload, download)| {
                        (
                            connection.upload.saturating_sub(upload),
                            connection.download.saturating_sub(download),
                        )
                    });
            (
                connection.id.clone(),
                (
                    (upload as f64 / seconds).round() as u64,
                    (download as f64 / seconds).round() as u64,
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::connection_rates;
    use crate::api::Connection;
    use std::time::Duration;

    fn conn(id: &str, upload: u64, download: u64) -> Connection {
        Connection {
            id: id.into(),
            upload,
            download,
            ..Default::default()
        }
    }

    #[test]
    fn rates_are_deltas_over_elapsed_time() {
        let previous = [conn("a", 100, 1_000), conn("gone", 5, 5)];
        let next = [conn("a", 600, 3_000), conn("new", 10, 20)];
        let rates = connection_rates(&previous, &next, Duration::from_secs(2));
        assert_eq!(rates.get("a"), Some(&(250, 1_000)));
        assert_eq!(rates.get("new"), Some(&(0, 0)));
        assert!(!rates.contains_key("gone"));
    }

    #[test]
    fn rates_do_not_underflow_when_counters_reset() {
        let rates = connection_rates(
            &[conn("a", 900, 900)],
            &[conn("a", 10, 20)],
            Duration::from_secs(1),
        );
        assert_eq!(rates.get("a"), Some(&(0, 0)));
    }
}
