//! TUI panel configuration (`~/.config/clashlime/ui.toml`).
//!
//! Stores user-specific interface preferences completely decoupled from
//! core/supervisor daemon network settings:
//! - Refresh interval in milliseconds
//! - Default tab on startup
//! - Terminal mouse capture
//! - Default proxy sort order
//! - Default connection filter and sort order
//! - Logs buffer size and default source

use crate::app::{ConnectionFilter, ConnectionSort, LogSource, NodeSort, Tab};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const DEFAULT_REFRESH_MS: u64 = 1000;
const DEFAULT_LOG_MAX_LINES: usize = 1000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DaemonScope {
    #[default]
    User,
    System,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct UiConfig {
    /// Refresh interval for the TUI event loop in milliseconds.
    pub refresh_ms: u64,
    /// Default tab opened on startup.
    pub default_tab: Tab,
    /// Enable mouse capture in terminal.
    pub mouse: bool,
    /// Target supervisor daemon: "user" or "system".
    pub daemon: DaemonScope,
    /// Proxies tab preferences.
    pub proxies: ProxiesUiConfig,
    /// Connections tab preferences.
    pub connections: ConnectionsUiConfig,
    /// Logs tab preferences.
    pub logs: LogsUiConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProxiesUiConfig {
    /// Default sorting mode for proxy nodes: "original", "delay_asc", "delay_desc", "name".
    pub default_sort: NodeSort,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ConnectionsUiConfig {
    /// Default filter: "active", "all", "closed".
    pub default_filter: ConnectionFilter,
    /// Default sort: "start_desc", "upload_desc", "download_desc", "traffic_desc", "host", "chain", "original".
    pub default_sort: ConnectionSort,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct LogsUiConfig {
    /// Maximum log lines retained in memory.
    pub max_lines: usize,
    /// Default log source filter: "all", "core", "daemon", "tui".
    pub default_source: LogSource,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            refresh_ms: DEFAULT_REFRESH_MS,
            default_tab: Tab::Dashboard,
            mouse: true,
            daemon: DaemonScope::User,
            proxies: ProxiesUiConfig::default(),
            connections: ConnectionsUiConfig::default(),
            logs: LogsUiConfig::default(),
        }
    }
}

impl Default for ProxiesUiConfig {
    fn default() -> Self {
        Self {
            default_sort: NodeSort::Original,
        }
    }
}

impl Default for ConnectionsUiConfig {
    fn default() -> Self {
        Self {
            default_filter: ConnectionFilter::Active,
            default_sort: ConnectionSort::StartDesc,
        }
    }
}

impl Default for LogsUiConfig {
    fn default() -> Self {
        Self {
            max_lines: DEFAULT_LOG_MAX_LINES,
            default_source: LogSource::All,
        }
    }
}

impl UiConfig {
    pub fn refresh_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.refresh_ms)
    }

    pub fn default_path() -> PathBuf {
        if let Ok(path) = std::env::var("CLASHLIME_UI_CONFIG") {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("clashlime/ui.toml")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::default_path())
    }

    pub fn load_from(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        match fs::read_to_string(path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(cfg) => cfg,
                Err(error) => {
                    crate::logger::warn(
                        "ui",
                        &format!("failed to parse {}: {error}; using defaults", path.display()),
                    );
                    Self::default()
                }
            },
            Err(error) => {
                crate::logger::warn(
                    "ui",
                    &format!("failed to read {}: {error}; using defaults", path.display()),
                );
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::default_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .context("failed to serialize ui config to toml")?;
        crate::persist::atomic_write(path, content.as_bytes(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_config_default_values() {
        let cfg = UiConfig::default();
        assert_eq!(cfg.refresh_ms, 1000);
        assert_eq!(cfg.default_tab, Tab::Dashboard);
        assert!(cfg.mouse);
        assert_eq!(cfg.daemon, DaemonScope::User);
        assert_eq!(cfg.proxies.default_sort, NodeSort::Original);
        assert_eq!(cfg.connections.default_filter, ConnectionFilter::Active);
        assert_eq!(cfg.connections.default_sort, ConnectionSort::StartDesc);
        assert_eq!(cfg.logs.max_lines, 1000);
        assert_eq!(cfg.logs.default_source, LogSource::All);
    }

    #[test]
    fn ui_config_parses_custom_toml() {
        let toml_str = r#"
refresh_ms = 500
default_tab = "proxies"
mouse = false
daemon = "system"

[proxies]
default_sort = "delay_asc"

[connections]
default_filter = "closed"
default_sort = "traffic_desc"

[logs]
max_lines = 5000
default_source = "core"
"#;
        let cfg: UiConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.refresh_ms, 500);
        assert_eq!(cfg.default_tab, Tab::Proxies);
        assert!(!cfg.mouse);
        assert_eq!(cfg.daemon, DaemonScope::System);
        assert_eq!(cfg.proxies.default_sort, NodeSort::DelayAsc);
        assert_eq!(cfg.connections.default_filter, ConnectionFilter::Closed);
        assert_eq!(cfg.connections.default_sort, ConnectionSort::TrafficDesc);
        assert_eq!(cfg.logs.max_lines, 5000);
        assert_eq!(cfg.logs.default_source, LogSource::Core);
    }

    #[test]
    fn ui_config_save_and_load_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ui.toml");

        let mut cfg = UiConfig::default();
        cfg.refresh_ms = 2500;
        cfg.default_tab = Tab::Connections;
        cfg.daemon = DaemonScope::System;
        cfg.proxies.default_sort = NodeSort::Name;

        cfg.save_to(&path).unwrap();
        assert!(path.exists());

        let reloaded = UiConfig::load_from(&path);
        assert_eq!(cfg, reloaded);
    }
}
