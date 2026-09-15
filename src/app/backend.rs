//! Backend strategy: local daemon-managed core vs. pure remote API.
//!
//! Step 1 of the `App` de-god-object refactor. The refresh tick runs one
//! pipeline (`refresh_inner`); only the hooks below differ per backend, so
//! a fix in the shared skeleton can no longer miss one of the two paths.

use crate::api::Snapshot;

/// Which control plane the TUI talks to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Local,
    Remote,
}

impl Backend {
    pub const fn of(remote: bool) -> Self {
        if remote { Self::Remote } else { Self::Local }
    }

    pub const fn is_local(self) -> bool {
        matches!(self, Self::Local)
    }

    pub const fn is_remote(self) -> bool {
        matches!(self, Self::Remote)
    }

    /// Status line after a successful tick.
    pub const fn synced_label(self) -> &'static str {
        match self {
            Self::Local => "Synced",
            Self::Remote => "Synced (remote)",
        }
    }

    /// Pre-snapshot group order. Local comes from the generated
    /// `runtime.yaml`; remote has no local file and derives its order from
    /// the snapshot after fetch (see `remote_group_order`).
    pub fn preload_group_order(self) -> Option<Vec<String>> {
        match self {
            Self::Local => Some(crate::config::Config::proxy_group_order()),
            Self::Remote => None,
        }
    }

    /// File-backed log tails merged under the streamed `/logs` lines.
    /// Local merges daemon + TUI tails; remote only has the TUI tail (the
    /// daemon/core files describe a machine we do not manage).
    pub fn file_log_tails(self) -> Vec<super::LogEntry> {
        use super::{LogEntry, LogSource};
        let mut tails = Vec::new();
        if self.is_local() {
            for line in crate::logger::recent_logs_for("clashlime-daemon-", 60) {
                tails.push(LogEntry {
                    source: LogSource::Daemon,
                    text: format!("{}{line}", LogSource::Daemon.tag()),
                });
            }
        }
        for line in crate::logger::recent_logs(60) {
            tails.push(LogEntry {
                source: LogSource::Tui,
                text: format!("{}{line}", LogSource::Tui.tag()),
            });
        }
        tails
    }

    /// Whether the local core log file seeds the backlog. Only local, and
    /// only while the `/logs` stream has not connected yet (once it does,
    /// streamed lines are preserved verbatim instead of re-adding the file
    /// tail every tick).
    pub const fn seed_core_backlog(self, log_backlog_loaded: bool) -> bool {
        matches!(self, Self::Local) && !log_backlog_loaded
    }

    /// Remote has no local `runtime.yaml`; derive a stable order from the
    /// snapshot (`GLOBAL` first, rest sorted).
    pub fn remote_group_order(snapshot: &Snapshot) -> Vec<String> {
        let mut names: Vec<String> = snapshot.proxies.proxies.keys().cloned().collect();
        names.sort();
        names.sort_by_key(|name| match name.as_str() {
            "GLOBAL" => 0,
            _ => 1,
        });
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn synced_labels_differ_per_backend() {
        assert_eq!(Backend::Local.synced_label(), "Synced");
        assert_eq!(Backend::Remote.synced_label(), "Synced (remote)");
    }

    #[test]
    fn remote_group_order_pins_global_first() {
        let mut snapshot = Snapshot::default();
        snapshot.proxies.proxies = ["zeta", "GLOBAL", "alpha"]
            .into_iter()
            .map(|name| {
                (
                    name.to_owned(),
                    crate::api::Proxy {
                        kind: "Selector".to_owned(),
                        now: "n".to_owned(),
                        all: vec!["n".to_owned()],
                        ..Default::default()
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let order = Backend::remote_group_order(&snapshot);
        assert_eq!(order, vec!["GLOBAL", "alpha", "zeta"]);
    }

    #[test]
    fn core_backlog_only_seeds_locally_before_stream() {
        assert!(Backend::Local.seed_core_backlog(false));
        assert!(!Backend::Local.seed_core_backlog(true));
        assert!(!Backend::Remote.seed_core_backlog(false));
    }
}
