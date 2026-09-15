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

/// Local-only capabilities. The remote backend supports none of these:
/// it only drives the remote Mihomo API (proxies, connections, rules,
/// logs, `PATCH /configs`).
///
/// The shared `Manage` prefix is intentional: call sites read as
/// `Capability::ManageProfiles`.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capability {
    /// Start/stop the locally supervised core.
    ManageCore,
    /// Import/activate/update/delete profiles stored on this machine.
    ManageProfiles,
    /// Download geo databases to this machine.
    ManageGeo,
    /// Download/install the core binary into the self-managed slot.
    ManageCoreBinary,
    /// Create/restore local backups.
    ManageBackups,
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

    /// Whether this backend supports a local-only capability. Remote
    /// supports none of them; every guard funnels through here (via
    /// [`super::App::require`]) so a new local-only action cannot silently
    /// forget the remote check.
    pub const fn allows(self, _capability: Capability) -> bool {
        match self {
            Self::Local => true,
            Self::Remote => false,
        }
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
    fn remote_allows_no_local_capability() {
        use super::Capability::*;
        for cap in [
            ManageCore,
            ManageProfiles,
            ManageGeo,
            ManageCoreBinary,
            ManageBackups,
        ] {
            assert!(Backend::Local.allows(cap));
            assert!(!Backend::Remote.allows(cap));
        }
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
