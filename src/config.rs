use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, path::PathBuf, time::Duration};

fn which_mihomo() -> Result<PathBuf, ()> {
    let path_var = std::env::var_os("PATH").ok_or(())?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join("mihomo");
        if candidate.is_file() {
            return Ok(candidate);
        }
        // Windows compatibility: mihomo.exe
        let candidate_exe = dir.join("mihomo.exe");
        if candidate_exe.is_file() {
            return Ok(candidate_exe);
        }
    }
    Err(())
}

#[derive(Debug, Default, Deserialize)]
struct RuntimeConfig {
    #[serde(rename = "proxy-groups", default)]
    proxy_groups: Vec<RuntimeGroup>,
}

#[derive(Debug, Deserialize)]
struct RuntimeGroup {
    name: String,
}

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    /// Run the internal core supervisor
    #[arg(long, hide = true)]
    pub daemon: bool,
    /// Refresh interval in milliseconds
    #[arg(long, env = "OMASH_REFRESH_MS")]
    pub refresh_ms: Option<u64>,
    /// Alternative configuration file
    #[arg(long)]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Internal status-bar integration
    #[command(hide = true)]
    Bar(BarArgs),
    /// Check mihomo core update via GitHub Releases
    Update(UpdateArgs),
}

#[derive(Debug, Args)]
pub struct BarArgs {
    #[command(subcommand)]
    pub command: BarCommand,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    #[command(subcommand)]
    pub command: UpdateCommand,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ProxyMode {
    Rule,
    Global,
    Direct,
}

impl ProxyMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Global => "global",
            Self::Direct => "direct",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum BarCommand {
    /// Print status-bar state as JSON
    State,
    /// Change the Mihomo routing mode
    Mode { mode: ProxyMode },
    /// Select a proxy in a selector group
    Proxy { group: String, proxy: String },
    /// Test every proxy in a selector group
    Delay { group: String },
}

#[derive(Debug, Subcommand)]
pub enum UpdateCommand {
    /// Check mihomo core update via GitHub Releases
    Check {
        /// Force bypass cache
        #[arg(long)]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct DnsConfig {
    pub enable: bool,
    pub listen: String,
    pub ipv6: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nameserver: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback: Vec<String>,
    #[serde(
        rename = "enhanced-mode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub enhanced_mode: Option<String>,
    #[serde(
        rename = "fake-ip-range",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fake_ip_range: Option<String>,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            enable: false,
            listen: "0.0.0.0:1053".into(),
            ipv6: false,
            nameserver: vec!["223.5.5.5".into(), "119.29.29.29".into()],
            fallback: vec!["tls://8.8.4.4".into()],
            enhanced_mode: Some("fake-ip".into()),
            fake_ip_range: Some("198.18.0.1/16".into()),
        }
    }
}

/// Dynamic patch persisted as JSON in XDG_DATA_HOME.
/// Static TOML in XDG_CONFIG_HOME provides initial defaults; dynamic JSON overrides.
/// Only `Some` fields override static.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct DynamicConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub controller: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_test_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_start: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mixed_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_lan: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_proxy: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_bypass: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns: Option<DnsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sniffer_enable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mihomo_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub controller: String,
    pub secret: String,
    pub refresh_ms: u64,
    pub delay_test_url: String,
    pub auto_start: bool,
    pub mixed_port: u16,
    pub allow_lan: bool,
    pub ipv6: bool,
    pub system_proxy: bool,
    pub proxy_bypass: String,
    #[serde(default)]
    pub dns: DnsConfig,
    /// Sniffer override injected into the generated runtime config.
    #[serde(default)]
    pub sniffer_enable: bool,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// Explicit mihomo binary location chosen at runtime (dynamic JSON only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mihomo_path: Option<String>,
}

fn default_log_level() -> String {
    "info".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            controller: "http://127.0.0.1:9090".into(),
            secret: String::new(),
            refresh_ms: 1500,
            delay_test_url: "https://www.gstatic.com/generate_204".into(),
            auto_start: true,
            mixed_port: 7897,
            allow_lan: false,
            ipv6: true,
            system_proxy: true,
            proxy_bypass: "localhost,127.0.0.1,::1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12".into(),
            dns: DnsConfig::default(),
            sniffer_enable: false,
            log_level: default_log_level(),
            mihomo_path: None,
        }
    }
}

impl DynamicConfig {
    fn is_empty(&self) -> bool {
        self.controller.is_none()
            && self.refresh_ms.is_none()
            && self.delay_test_url.is_none()
            && self.auto_start.is_none()
            && self.mixed_port.is_none()
            && self.allow_lan.is_none()
            && self.ipv6.is_none()
            && self.system_proxy.is_none()
            && self.proxy_bypass.is_none()
            && self.dns.is_none()
            && self.sniffer_enable.is_none()
            && self.log_level.is_none()
            && self.mihomo_path.is_none()
    }
}

impl Config {
    pub fn load(cli: &Cli) -> Result<Self> {
        // 静态：XDG_CONFIG_HOME/omash/config.toml（或 --config 指定），提供初始默认值
        let static_path = cli.config.clone().unwrap_or_else(Self::default_path);
        let (mut static_cfg, legacy_fields) = if static_path.exists() {
            let text = fs::read_to_string(&static_path)
                .with_context(|| format!("failed to read {}", static_path.display()))?;
            let legacy_fields = toml::from_str::<toml::Value>(&text)
                .ok()
                .and_then(|document| document.as_table().cloned())
                .is_some_and(|table| {
                    table.contains_key("manage_core")
                        || table.contains_key("mihomo_path")
                        || table.contains_key("tun")
                });
            (
                toml::from_str(&text)
                    .with_context(|| format!("invalid config in {}", static_path.display()))?,
                legacy_fields,
            )
        } else {
            (Self::default(), false)
        };
        // `mihomo_path` is runtime-managed (dialog + dynamic JSON); never let a
        // stale static value persist or shadow the dynamic override.
        static_cfg.mihomo_path = None;
        // 静态缺 secret 时生成并持久化到静态文件（不写入动态）
        let needs_secret = static_cfg.secret.is_empty();
        if needs_secret {
            static_cfg.secret = uuid::Uuid::new_v4().simple().to_string();
        }
        static_cfg.ensure_dirs()?;
        if needs_secret || legacy_fields || !static_path.exists() {
            static_cfg.save_static_to(&static_path)?;
        }
        Self::secure_config_permissions(&static_path)?;

        // 动态：XDG_DATA_HOME/omash/config.json，JSON 覆盖静态
        let dynamic = Self::load_dynamic();
        let mut value = static_cfg;
        value.apply_dynamic(dynamic);

        // CLI / ENV 覆盖（最高优先级）
        if let Some(refresh_ms) = cli.refresh_ms {
            value.refresh_ms = refresh_ms;
        }
        value.controller = value.controller.trim_end_matches('/').to_owned();
        Ok(value)
    }

    fn load_dynamic() -> DynamicConfig {
        let path = Self::dynamic_path();
        if !path.exists() {
            return DynamicConfig::default();
        }
        fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn apply_dynamic(&mut self, patch: DynamicConfig) {
        if let Some(v) = patch.controller {
            self.controller = v;
        }
        if let Some(v) = patch.refresh_ms {
            self.refresh_ms = v;
        }
        if let Some(v) = patch.delay_test_url {
            self.delay_test_url = v;
        }
        if let Some(v) = patch.auto_start {
            self.auto_start = v;
        }
        if let Some(v) = patch.mixed_port {
            self.mixed_port = v;
        }
        if let Some(v) = patch.allow_lan {
            self.allow_lan = v;
        }
        if let Some(v) = patch.ipv6 {
            self.ipv6 = v;
        }
        if let Some(v) = patch.system_proxy {
            self.system_proxy = v;
        }
        if let Some(v) = patch.proxy_bypass {
            self.proxy_bypass = v;
        }
        if let Some(v) = patch.dns {
            self.dns = v;
        }
        if let Some(v) = patch.sniffer_enable {
            self.sniffer_enable = v;
        }
        if let Some(v) = patch.log_level {
            self.log_level = v;
        }
        if let Some(v) = patch.mihomo_path {
            self.mihomo_path = Some(v);
        }
    }

    pub fn refresh_interval(&self) -> Duration {
        Duration::from_millis(self.refresh_ms.max(250))
    }

    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("omash/config.toml")
    }

    pub fn data_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("omash")
    }

    pub fn mihomo_path() -> PathBuf {
        // Non-privileged friendly resolution order:
        // 1. $OMASH_MIHOMO / $OMASH_CORE_BIN env (explicit override)
        // 2. mihomo_path from dynamic config.json (runtime dialog)
        // 3. $HOME/.local/bin/mihomo (user-local install)
        // 4. $XDG_DATA_HOME/omash/bin/mihomo
        // 5. $PATH lookup (which mihomo)
        // 6. fallback /usr/bin/mihomo (system package)
        if let Some(path) = Self::env_mihomo_override() {
            return path;
        }
        if let Some(path) = Self::configured_mihomo_override() {
            return path;
        }
        if let Some(home) = dirs::home_dir() {
            let candidate = home.join(".local/bin/mihomo");
            if candidate.is_file() {
                return candidate;
            }
        }
        let data_bin = Self::data_dir().join("bin/mihomo");
        if data_bin.is_file() {
            return data_bin;
        }
        if let Ok(path) = which_mihomo() {
            return path;
        }
        PathBuf::from("/usr/bin/mihomo")
    }

    fn env_mihomo_override() -> Option<PathBuf> {
        let value = std::env::var("OMASH_MIHOMO")
            .or_else(|_| std::env::var("OMASH_CORE_BIN"))
            .ok()?;
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
    }

    /// `mihomo_path` persisted by the runtime core-missing dialog.
    pub fn configured_mihomo_override() -> Option<PathBuf> {
        let text = fs::read_to_string(Self::dynamic_path()).ok()?;
        let patch: DynamicConfig = serde_json::from_str(&text).ok()?;
        let value = patch.mihomo_path?;
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
    }

    /// Persist (or clear with `None`) the runtime mihomo binary location.
    pub fn set_mihomo_override(value: Option<&Path>) -> Result<()> {
        let path = Self::dynamic_path();
        let mut patch: DynamicConfig = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        patch.mihomo_path = value.map(|path| path.display().to_string());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if patch.is_empty() {
            let _ = fs::remove_file(&path);
            return Ok(());
        }
        let data =
            serde_json::to_string_pretty(&patch).context("failed to serialize dynamic config")?;
        fs::write(&path, data).with_context(|| format!("failed to write {}", path.display()))?;
        Self::secure_config_permissions(&path)?;
        Ok(())
    }

    pub fn mihomo_candidates() -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(path) = Self::env_mihomo_override() {
            candidates.push(path);
        }
        if let Some(path) = Self::configured_mihomo_override()
            && !candidates.contains(&path)
        {
            candidates.push(path);
        }
        if let Some(home) = dirs::home_dir() {
            candidates.push(home.join(".local/bin/mihomo"));
        }
        candidates.push(Self::data_dir().join("bin/mihomo"));
        if let Ok(path) = which_mihomo()
            && !candidates.contains(&path)
        {
            candidates.push(path);
        }
        candidates.push(PathBuf::from("/usr/bin/mihomo"));
        candidates
    }

    pub fn profiles_dir() -> PathBuf {
        Self::data_dir().join("profiles")
    }

    pub fn runtime_path() -> PathBuf {
        Self::data_dir().join("runtime.yaml")
    }

    pub fn proxy_group_order() -> Vec<String> {
        fs::read_to_string(Self::runtime_path())
            .ok()
            .and_then(|text| serde_yaml_ng::from_str::<RuntimeConfig>(&text).ok())
            .map(|config| {
                config
                    .proxy_groups
                    .into_iter()
                    .map(|group| group.name)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn profiles_path() -> PathBuf {
        Self::data_dir().join("profiles.yaml")
    }

    pub fn logs_dir() -> PathBuf {
        Self::data_dir().join("logs")
    }

    pub fn backups_dir() -> PathBuf {
        Self::data_dir().join("backups")
    }

    pub fn omash_log_path() -> PathBuf {
        Self::logs_dir().join(format!(
            "omash-{}.log",
            chrono::Local::now().format("%Y-%m-%d")
        ))
    }

    pub fn omash_log_dir() -> PathBuf {
        Self::logs_dir()
    }

    /// 静态路径：XDG_CONFIG_HOME/omash/config.toml
    pub fn static_path() -> PathBuf {
        Self::default_path()
    }

    /// 动态路径：XDG_DATA_HOME/omash/config.json（JSON，控制面可写）
    pub fn dynamic_path() -> PathBuf {
        Self::data_dir().join("config.json")
    }

    /// 动态保存：控制面变更写入 JSON，覆盖静态默认值
    pub fn save(&self) -> Result<()> {
        self.save_dynamic()
    }

    pub fn save_dynamic(&self) -> Result<()> {
        let patch = DynamicConfig {
            controller: Some(self.controller.clone()),
            refresh_ms: Some(self.refresh_ms),
            delay_test_url: Some(self.delay_test_url.clone()),
            auto_start: Some(self.auto_start),
            mixed_port: Some(self.mixed_port),
            allow_lan: Some(self.allow_lan),
            ipv6: Some(self.ipv6),
            system_proxy: Some(self.system_proxy),
            proxy_bypass: Some(self.proxy_bypass.clone()),
            dns: Some(self.dns.clone()),
            sniffer_enable: Some(self.sniffer_enable),
            log_level: Some(self.log_level.clone()),
            mihomo_path: self.mihomo_path.clone(),
        };
        let path = Self::dynamic_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // 清理空 patch 避免无意义文件
        if patch.is_empty() {
            let _ = fs::remove_file(&path);
            return Ok(());
        }
        let data =
            serde_json::to_string_pretty(&patch).context("failed to serialize dynamic config")?;
        fs::write(&path, data).with_context(|| format!("failed to write {}", path.display()))?;
        Self::secure_config_permissions(&path)?;
        Ok(())
    }

    /// 静态保存：仅用于首次初始化或显式静态编辑
    pub fn save_static_to(&self, path: &std::path::Path) -> Result<()> {
        self.save_to(path)
    }

    fn save_to(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(self)?)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Self::secure_config_permissions(path)?;
        Ok(())
    }

    fn secure_config_permissions(path: &std::path::Path) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            Self::data_dir(),
            Self::profiles_dir(),
            Self::logs_dir(),
            Self::backups_dir(),
        ] {
            fs::create_dir_all(&dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn dynamic_overrides_static() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        // Isolate XDG dirs
        let cfg_dir = dir.path().join("config");
        let data_dir = dir.path().join("data");
        let orig_cfg = std::env::var_os("XDG_CONFIG_HOME");
        let orig_data = std::env::var_os("XDG_DATA_HOME");
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &cfg_dir);
            std::env::set_var("XDG_DATA_HOME", &data_dir);
        }
        // Static
        let static_path = cfg_dir.join("omash/config.toml");
        fs::create_dir_all(static_path.parent().unwrap()).unwrap();
        fs::write(
            &static_path,
            "controller = 'http://static:9090'\nsecret = 's'\nmixed_port = 7897\n",
        )
        .unwrap();
        // Dynamic JSON overriding controller and mixed_port
        let dynamic_path = data_dir.join("omash/config.json");
        fs::create_dir_all(dynamic_path.parent().unwrap()).unwrap();
        fs::write(
            &dynamic_path,
            r#"{"controller":"http://dynamic:9090","mixed_port": 7898}"#,
        )
        .unwrap();
        let cfg = Config::load(&Cli {
            command: None,
            daemon: false,
            refresh_ms: None,
            config: Some(static_path.clone()),
        })
        .unwrap();
        assert_eq!(cfg.controller, "http://dynamic:9090");
        assert_eq!(cfg.mixed_port, 7898);
        // Static file should remain unchanged, dynamic holds override
        let static_text = fs::read_to_string(&static_path).unwrap();
        assert!(static_text.contains("static:9090"));
        let dyn_text = fs::read_to_string(&dynamic_path).unwrap();
        assert!(dyn_text.contains("dynamic:9090"));
        // Cleanup env
        unsafe {
            match orig_cfg {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
            match orig_data {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }

    #[test]
    fn refresh_cli_overrides_file_value() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let orig_data = std::env::var_os("XDG_DATA_HOME");
        unsafe { std::env::set_var("XDG_DATA_HOME", data_dir.path()) };
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "controller = 'http://old:1/'\nsecret = 'key'\nrefresh_ms = 99\n",
        )
        .unwrap();
        let cli = Cli {
            command: None,
            daemon: false,
            refresh_ms: None,
            config: Some(path),
        };
        let config = Config::load(&cli).unwrap();
        assert_eq!(config.controller, "http://old:1");
        assert_eq!(config.secret, "key");
        assert_eq!(config.refresh_ms, 99);
        assert_eq!(config.refresh_interval(), Duration::from_millis(250));
        assert!(config.system_proxy);
        unsafe {
            match orig_data {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }

    #[test]
    fn removes_legacy_external_core_fields() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let orig_data = std::env::var_os("XDG_DATA_HOME");
        unsafe { std::env::set_var("XDG_DATA_HOME", data_dir.path()) };
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "manage_core = false\nmihomo_path = '/tmp/mihomo'\ntun = true\nsecret = 'key'\n",
        )
        .unwrap();
        Config::load(&Cli {
            command: None,
            daemon: false,
            refresh_ms: None,
            config: Some(path.clone()),
        })
        .unwrap();
        let migrated = fs::read_to_string(path).unwrap();
        assert!(!migrated.contains("manage_core"));
        assert!(!migrated.contains("mihomo_path"));
        assert!(!migrated.contains("tun"));
        unsafe {
            match orig_data {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }
}
