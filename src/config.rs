use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime},
};

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
    #[arg(long, env = "CLASHLIME_REFRESH_MS")]
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
    /// Manage the supervisor daemon (e.g. `server status`, `server stop`)
    Server(ServerArgs),
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

#[derive(Debug, Args)]
pub struct ServerArgs {
    #[command(subcommand)]
    pub command: ServerCommand,
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

#[derive(Debug, Subcommand)]
pub enum ServerCommand {
    /// Query daemon/supervisor status over IPC (exit 0 when core is running)
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Stop the supervisor daemon and the core (idempotent, exit 0 when stopped)
    Stop {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct DnsConfig {
    pub enable: bool,
    /// Override the profile's own `dns` section with the settings above.
    /// Off keeps the subscription DNS untouched (no injection, no hot
    /// patch); defaults to true to preserve existing behavior.
    #[serde(default = "default_true")]
    pub override_profile: bool,
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
    /// fake-ip-filter-mode: blacklist | whitelist (fake-ip mode only).
    #[serde(
        rename = "fake-ip-filter-mode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fake_ip_filter_mode: Option<String>,
    #[serde(
        rename = "fake-ip-filter",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub fake_ip_filter: Vec<String>,
    #[serde(
        rename = "respect-rules",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub respect_rules: Option<bool>,
    #[serde(
        rename = "default-nameserver",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub default_nameserver: Vec<String>,
    #[serde(
        rename = "proxy-server-nameserver",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub proxy_server_nameserver: Vec<String>,
    #[serde(
        rename = "direct-nameserver",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub direct_nameserver: Vec<String>,
    #[serde(
        rename = "fallback-filter",
        default,
        skip_serializing_if = "FallbackFilter::is_empty"
    )]
    pub fallback_filter: FallbackFilter,
    #[serde(
        rename = "nameserver-policy",
        default,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub nameserver_policy: std::collections::BTreeMap<String, StringList>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub hosts: std::collections::BTreeMap<String, StringList>,
    #[serde(
        rename = "use-system-hosts",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub use_system_hosts: Option<bool>,
}

/// A config value that is either one string or a list of strings
/// (mihomo `hosts` / `nameserver-policy` values). Deserializes from both
/// shapes so old single-string configs keep loading.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum StringList {
    Single(String),
    Multiple(Vec<String>),
}

impl StringList {
    pub fn values(&self) -> Vec<String> {
        match self {
            Self::Single(value) => vec![value.clone()],
            Self::Multiple(values) => values.clone(),
        }
    }

    /// Test-only pretty printer (used by the round-trip tests below).
    #[cfg(test)]
    pub fn display(&self) -> String {
        self.values().join(", ")
    }
}

/// fallback-filter sub-object (cf. clash-party fallback filter card).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct FallbackFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geoip: Option<bool>,
    #[serde(
        rename = "geoip-code",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub geoip_code: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ipcidr: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domain: Vec<String>,
}

impl FallbackFilter {
    pub fn is_empty(&self) -> bool {
        self.geoip.is_none()
            && self.geoip_code.is_none()
            && self.ipcidr.is_empty()
            && self.domain.is_empty()
    }
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            enable: false,
            override_profile: true,
            listen: "0.0.0.0:1053".into(),
            ipv6: false,
            nameserver: vec!["223.5.5.5".into(), "119.29.29.29".into()],
            fallback: vec!["tls://8.8.4.4".into()],
            enhanced_mode: Some("fake-ip".into()),
            fake_ip_range: Some("198.18.0.1/16".into()),
            fake_ip_filter_mode: None,
            fake_ip_filter: vec![],
            respect_rules: None,
            default_nameserver: vec![],
            proxy_server_nameserver: vec![],
            direct_nameserver: vec![],
            fallback_filter: FallbackFilter::default(),
            nameserver_policy: Default::default(),
            hosts: Default::default(),
            use_system_hosts: None,
        }
    }
}

/// GeoIP / GeoSite database settings (cf. clash-party 外部资源面板).
/// `mirror` is a gh-proxy style URL prefix prepended to the upstream asset
/// URL, e.g. `https://gh-proxy.com/`. `$CLASHLIME_GEO_MIRROR` overrides it.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct GeoConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<String>,
    /// HTTP(S) proxy for downloads (geo databases AND mihomo core
    /// check/install), e.g. `http://127.0.0.1:7897` (mihomo's own mixed
    /// port works once the core runs). `$CLASHLIME_GEO_PROXY` overrides it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
    /// Per-asset full URLs (cf. clash-party `geox-url`). When set, the
    /// entry wins over `mirror` for both clashlime-side downloads and the
    /// core's `geox-url`; empty falls back to mirror/direct.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geoip_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geosite_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mmdb_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asn_url: Option<String>,
    pub auto_update: bool,
    pub update_interval: u64,
}

impl Default for GeoConfig {
    fn default() -> Self {
        Self {
            mirror: None,
            proxy: None,
            geoip_url: None,
            geosite_url: None,
            mmdb_url: None,
            asn_url: None,
            auto_update: true,
            update_interval: 24,
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
    /// Static default when set, explicit off when null. Absent keeps static.
    #[serde(
        default,
        deserialize_with = "nullable_port",
        skip_serializing_if = "Option::is_none"
    )]
    pub mixed_port: Option<Option<u16>>,
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
    pub sniffer: Option<SnifferConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mihomo_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo: Option<GeoConfig>,
    /// Dedicated listener ports. Number overrides, null disables, absent
    /// keeps the static default.
    #[serde(
        default,
        deserialize_with = "nullable_port",
        skip_serializing_if = "Option::is_none"
    )]
    pub socks_port: Option<Option<u16>>,
    #[serde(
        default,
        deserialize_with = "nullable_port",
        skip_serializing_if = "Option::is_none"
    )]
    pub http_port: Option<Option<u16>>,
    #[serde(
        default,
        deserialize_with = "nullable_port",
        skip_serializing_if = "Option::is_none"
    )]
    pub redir_port: Option<Option<u16>>,
    #[serde(
        default,
        deserialize_with = "nullable_port",
        skip_serializing_if = "Option::is_none"
    )]
    pub tproxy_port: Option<Option<u16>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_auth_prefixes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lan_allowed_ips: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lan_disallowed_ips: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_concurrent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unified_delay: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub find_process_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tun: Option<TunConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub controller: String,
    pub secret: String,
    pub refresh_ms: u64,
    pub delay_test_url: String,
    pub auto_start: bool,
    /// Mixed listener. `None` disables it (profile value, if any, passes
    /// through untouched); system-proxy setup is skipped without it.
    #[serde(default = "default_mixed_port", skip_serializing_if = "Option::is_none")]
    pub mixed_port: Option<u16>,
    pub allow_lan: bool,
    pub ipv6: bool,
    pub system_proxy: bool,
    pub proxy_bypass: String,
    #[serde(default)]
    pub dns: DnsConfig,
    /// Sniffer override injected into the generated runtime config.
    #[serde(default)]
    pub sniffer_enable: bool,
    /// Sniffer details (force-dns-mapping / ports…). Empty leaves the
    /// profile values untouched.
    #[serde(default, skip_serializing_if = "SnifferConfig::is_empty")]
    pub sniffer: SnifferConfig,
    /// Geo database management (mirror / core self-update).
    #[serde(default)]
    pub geo: GeoConfig,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// Explicit mihomo binary location chosen at runtime (dynamic JSON only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mihomo_path: Option<String>,
    /// Dedicated listener ports. `None` leaves the profile value untouched.
    #[serde(
        default = "default_socks_port",
        skip_serializing_if = "Option::is_none"
    )]
    pub socks_port: Option<u16>,
    #[serde(
        default = "default_http_port",
        skip_serializing_if = "Option::is_none"
    )]
    pub http_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redir_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tproxy_port: Option<u16>,
    /// `authentication` entries (`user:pass`), empty leaves profile alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authentication: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_auth_prefixes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lan_allowed_ips: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lan_disallowed_ips: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tcp_concurrent: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unified_delay: Option<bool>,
    /// Process matching (`strict`/`off`/`always`). `None` leaves the
    /// profile value untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub find_process_mode: Option<String>,
    /// TUN interface settings (cf. clash-party tun page).
    #[serde(default, skip_serializing_if = "TunConfig::is_empty")]
    pub tun: TunConfig,
}

/// TUN interface settings. Only applied to the runtime config when
/// `enable` is set; otherwise any profile-provided `tun` section is
/// stripped as before.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct TunConfig {
    pub enable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_route: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_detect_interface: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict_route: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_redirect: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub route_exclude_address: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dns_hijack: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u16>,
}

impl TunConfig {
    pub fn is_empty(&self) -> bool {
        !self.enable
            && self.stack.is_none()
            && self.device.is_none()
            && self.auto_route.is_none()
            && self.auto_detect_interface.is_none()
            && self.strict_route.is_none()
            && self.auto_redirect.is_none()
            && self.route_exclude_address.is_empty()
            && self.dns_hijack.is_empty()
            && self.mtu.is_none()
    }
}

fn default_log_level() -> String {
    "info".into()
}

fn default_mixed_port() -> Option<u16> {
    Some(7890)
}

fn default_socks_port() -> Option<u16> {
    Some(7892)
}

fn default_http_port() -> Option<u16> {
    Some(7891)
}

/// Dynamic override for an optional port: absent keeps the static value,
/// `null` disables, a number sets. Plain `Option<Option<u16>>` would read
/// JSON `null` as absent, so this wrapper is needed.
fn nullable_port<'de, D>(deserializer: D) -> Result<Option<Option<u16>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<u16>::deserialize(deserializer).map(Some)
}

fn default_true() -> bool {
    true
}

/// Sniffer details. Only `enable` (via `sniffer_enable`) is required;
/// every other key is injected only when set, otherwise the profile
/// value survives. `override_profile` (default true) gates the whole
/// injection: off keeps the subscription `sniffer` section untouched.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct SnifferConfig {
    #[serde(default = "default_true")]
    pub override_profile: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_dns_mapping: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse_pure_ip: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_destination: Option<bool>,
    /// `sniff.HTTP.ports` entries (`80`, `8080-8880`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub http_ports: Vec<String>,
    /// `sniff.TLS.ports` entries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tls_ports: Vec<String>,
}

impl SnifferConfig {
    pub fn is_empty(&self) -> bool {
        // `override_profile = false` is a real choice worth persisting;
        // the default true contributes nothing on its own.
        self.override_profile
            && self.force_dns_mapping.is_none()
            && self.parse_pure_ip.is_none()
            && self.override_destination.is_none()
            && self.http_ports.is_empty()
            && self.tls_ports.is_empty()
    }
}

impl Default for SnifferConfig {
    fn default() -> Self {
        Self {
            override_profile: true,
            force_dns_mapping: None,
            parse_pure_ip: None,
            override_destination: None,
            http_ports: vec![],
            tls_ports: vec![],
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            controller: "http://127.0.0.1:9090".into(),
            secret: String::new(),
            refresh_ms: 1500,
            delay_test_url: "https://www.gstatic.com/generate_204".into(),
            auto_start: true,
            mixed_port: default_mixed_port(),
            allow_lan: false,
            ipv6: true,
            system_proxy: true,
            proxy_bypass: "localhost,127.0.0.1,::1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12".into(),
            dns: DnsConfig::default(),
            sniffer_enable: false,
            sniffer: SnifferConfig::default(),
            geo: GeoConfig::default(),
            log_level: default_log_level(),
            mihomo_path: None,
            socks_port: default_socks_port(),
            http_port: default_http_port(),
            redir_port: None,
            tproxy_port: None,
            authentication: vec![],
            skip_auth_prefixes: vec![],
            lan_allowed_ips: vec![],
            lan_disallowed_ips: vec![],
            tcp_concurrent: None,
            unified_delay: None,
            find_process_mode: None,
            tun: TunConfig::default(),
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
            && self.sniffer.is_none()
            && self.log_level.is_none()
            && self.mihomo_path.is_none()
            && self.geo.is_none()
            && self.socks_port.is_none()
            && self.http_port.is_none()
            && self.redir_port.is_none()
            && self.tproxy_port.is_none()
            && self.authentication.is_none()
            && self.skip_auth_prefixes.is_none()
            && self.lan_allowed_ips.is_none()
            && self.lan_disallowed_ips.is_none()
            && self.tcp_concurrent.is_none()
            && self.unified_delay.is_none()
            && self.tun.is_none()
    }
}

impl Config {
    pub fn load(cli: &Cli) -> Result<Self> {
        // 静态：XDG_CONFIG_HOME/clashlime/config.toml（或 --config 指定），提供初始默认值
        let static_path = cli.config.clone().unwrap_or_else(Self::default_path);
        let mut static_cfg = if static_path.exists() {
            let text = fs::read_to_string(&static_path)
                .with_context(|| format!("failed to read {}", static_path.display()))?;
            let document: toml::Value = toml::from_str(&text)
                .with_context(|| format!("invalid config in {}", static_path.display()))?;
            document
                .try_into()
                .with_context(|| format!("invalid config in {}", static_path.display()))?
        } else {
            Self::default()
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
        if needs_secret || !static_path.exists() {
            static_cfg.save_static_to(&static_path)?;
        }
        Self::secure_config_permissions(&static_path)?;

        // 动态：XDG_DATA_HOME/clashlime/config.json，JSON 覆盖静态
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
        if let Some(v) = patch.sniffer {
            self.sniffer = v;
        }
        if let Some(v) = patch.log_level {
            self.log_level = v;
        }
        if let Some(v) = patch.mihomo_path {
            self.mihomo_path = Some(v);
        }
        if let Some(v) = patch.geo {
            self.geo = v;
        }
        if let Some(v) = patch.socks_port {
            self.socks_port = v;
        }
        if let Some(v) = patch.http_port {
            self.http_port = v;
        }
        if let Some(v) = patch.redir_port {
            self.redir_port = v;
        }
        if let Some(v) = patch.tproxy_port {
            self.tproxy_port = v;
        }
        if let Some(v) = patch.authentication {
            self.authentication = v;
        }
        if let Some(v) = patch.skip_auth_prefixes {
            self.skip_auth_prefixes = v;
        }
        if let Some(v) = patch.lan_allowed_ips {
            self.lan_allowed_ips = v;
        }
        if let Some(v) = patch.lan_disallowed_ips {
            self.lan_disallowed_ips = v;
        }
        if let Some(v) = patch.tcp_concurrent {
            self.tcp_concurrent = Some(v);
        }
        if let Some(v) = patch.unified_delay {
            self.unified_delay = Some(v);
        }
        if let Some(v) = patch.find_process_mode {
            self.find_process_mode = Some(v);
        }
        if let Some(v) = patch.tun {
            self.tun = v;
        }
    }

    pub fn refresh_interval(&self) -> Duration {
        Duration::from_millis(self.refresh_ms.max(250))
    }

    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("clashlime/config.toml")
    }

    pub fn data_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("clashlime")
    }

    pub fn mihomo_path() -> PathBuf {
        // Self-managed core only: the daemon never picks up ambient system
        // binaries from $PATH / /usr/bin / ~/.local/bin. Resolution order:
        // 1. $CLASHLIME_MIHOMO / $CLASHLIME_CORE_BIN env (explicit override)
        // 2. mihomo_path from dynamic config.json (first-run dialog choice)
        // 3. $XDG_DATA_HOME/clashlime/bin/mihomo (the TUI "Download" target)
        // A system binary is used only when the user explicitly points the
        // dialog at it, which then persists via (2).
        if let Some(path) = Self::env_mihomo_override() {
            return path;
        }
        if let Some(path) = Self::configured_mihomo_override() {
            return path;
        }
        Self::data_dir().join("bin/mihomo")
    }

    fn env_mihomo_override() -> Option<PathBuf> {
        let value = std::env::var("CLASHLIME_MIHOMO")
            .or_else(|_| std::env::var("CLASHLIME_CORE_BIN"))
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
        candidates.push(Self::data_dir().join("bin/mihomo"));
        candidates
    }

    pub fn profiles_dir() -> PathBuf {
        Self::data_dir().join("profiles")
    }

    pub fn runtime_path() -> PathBuf {
        Self::data_dir().join("runtime.yaml")
    }

    pub fn proxy_group_order() -> Vec<String> {
        let path = Self::runtime_path();
        let mtime = fs::metadata(&path).and_then(|meta| meta.modified()).ok();
        static CACHE: OnceLock<Mutex<Option<(PathBuf, Option<SystemTime>, Vec<String>)>>> =
            OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(None));
        // Runs on every refresh tick; re-parsing the multi-MB generated
        // config each time burned ~80% of TUI CPU (samply). The order only
        // changes when the daemon rebuilds the file, so a stat-guarded
        // cache is exact.
        if let Ok(guard) = cache.lock()
            && let Some((cached_path, cached_mtime, cached_order)) = guard.as_ref()
            && *cached_path == path
            && *cached_mtime == mtime
        {
            return cached_order.clone();
        }
        let order: Vec<String> = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_yaml_ng::from_str::<RuntimeConfig>(&text).ok())
            .map(|config| {
                config
                    .proxy_groups
                    .into_iter()
                    .map(|group| group.name)
                    .collect()
            })
            .unwrap_or_default();
        if let Ok(mut guard) = cache.lock() {
            *guard = Some((path, mtime, order.clone()));
        }
        order
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

    /// This process's own log file (`clashlime-tui-<date>.log`).
    pub fn clashlime_log_path() -> PathBuf {
        Self::logs_dir().join(format!(
            "clashlime-tui-{}.log",
            chrono::Local::now().format("%Y-%m-%d")
        ))
    }

    /// The supervisor daemon's log file (`clashlime-daemon-<date>.log`).
    pub fn clashlime_daemon_log_path() -> PathBuf {
        Self::logs_dir().join(format!(
            "clashlime-daemon-{}.log",
            chrono::Local::now().format("%Y-%m-%d")
        ))
    }

    /// 动态路径：XDG_DATA_HOME/clashlime/config.json（JSON，控制面可写）
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
            sniffer: Some(self.sniffer.clone()),
            log_level: Some(self.log_level.clone()),
            mihomo_path: self.mihomo_path.clone(),
            geo: Some(self.geo.clone()),
            socks_port: Some(self.socks_port),
            http_port: Some(self.http_port),
            redir_port: Some(self.redir_port),
            tproxy_port: Some(self.tproxy_port),
            authentication: Some(self.authentication.clone()),
            skip_auth_prefixes: Some(self.skip_auth_prefixes.clone()),
            lan_allowed_ips: Some(self.lan_allowed_ips.clone()),
            lan_disallowed_ips: Some(self.lan_disallowed_ips.clone()),
            tcp_concurrent: self.tcp_concurrent,
            unified_delay: self.unified_delay,
            find_process_mode: self.find_process_mode.clone(),
            tun: Some(self.tun.clone()),
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
pub(crate) mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    /// Serialize environment-mutating tests across modules (geo tests
    /// redirect `XDG_DATA_HOME` too). Only compiled for tests.
    pub(crate) fn test_env_lock() -> &'static Mutex<()> {
        env_lock()
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
        let static_path = cfg_dir.join("clashlime/config.toml");
        fs::create_dir_all(static_path.parent().unwrap()).unwrap();
        fs::write(
            &static_path,
            "controller = 'http://static:9090'\nsecret = 's'\nmixed_port = 7897\n",
        )
        .unwrap();
        // Dynamic JSON overriding controller and mixed_port
        let dynamic_path = data_dir.join("clashlime/config.json");
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
        assert_eq!(cfg.mixed_port, Some(7898));
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
    fn port_defaults_and_explicit_null_disables() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cfg_dir = dir.path().join("config");
        let data_dir = dir.path().join("data");
        let orig_cfg = std::env::var_os("XDG_CONFIG_HOME");
        let orig_data = std::env::var_os("XDG_DATA_HOME");
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &cfg_dir);
            std::env::set_var("XDG_DATA_HOME", &data_dir);
        }
        let static_path = cfg_dir.join("clashlime/config.toml");
        fs::create_dir_all(static_path.parent().unwrap()).unwrap();
        fs::write(
            &static_path,
            "controller = 'http://static:9090'\nsecret = 's'\n",
        )
        .unwrap();
        let load = || {
            Config::load(&Cli {
                command: None,
                daemon: false,
                refresh_ms: None,
                config: Some(static_path.clone()),
            })
            .unwrap()
        };
        // Fresh defaults: mixed 7890, http 7891, socks 7892.
        let cfg = load();
        assert_eq!(cfg.mixed_port, Some(7890));
        assert_eq!(cfg.http_port, Some(7891));
        assert_eq!(cfg.socks_port, Some(7892));
        // Explicit null disables and survives save/reload.
        let dynamic_path = data_dir.join("clashlime/config.json");
        fs::create_dir_all(dynamic_path.parent().unwrap()).unwrap();
        fs::write(
            &dynamic_path,
            r#"{"mixed_port": null, "http_port": null, "socks_port": null}"#,
        )
        .unwrap();
        let cfg = load();
        assert_eq!(cfg.mixed_port, None);
        assert_eq!(cfg.http_port, None);
        assert_eq!(cfg.socks_port, None);
        let mut cfg = cfg;
        cfg.save().unwrap();
        let dyn_text = fs::read_to_string(&dynamic_path).unwrap();
        assert!(dyn_text.contains("\"mixed_port\": null"), "{dyn_text}");
        let reloaded = load();
        assert_eq!(reloaded.mixed_port, None);
        assert_eq!(reloaded.http_port, None);
        assert_eq!(reloaded.socks_port, None);
        // A number re-enables.
        fs::write(&dynamic_path, r#"{"mixed_port": 7895}"#).unwrap();
        assert_eq!(load().mixed_port, Some(7895));
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
    fn geo_mirror_survives_dynamic_save_and_reload() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cfg_dir = dir.path().join("config");
        let data_dir = dir.path().join("data");
        let orig_cfg = std::env::var_os("XDG_CONFIG_HOME");
        let orig_data = std::env::var_os("XDG_DATA_HOME");
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &cfg_dir);
            std::env::set_var("XDG_DATA_HOME", &data_dir);
        }
        let static_path = cfg_dir.join("clashlime/config.toml");
        fs::create_dir_all(static_path.parent().unwrap()).unwrap();
        fs::write(
            &static_path,
            "controller = 'http://static:9090'\nsecret = 's'\nmixed_port = 7897\n",
        )
        .unwrap();
        let load = || {
            Config::load(&Cli {
                command: None,
                daemon: false,
                refresh_ms: None,
                config: Some(static_path.clone()),
            })
            .unwrap()
        };
        // Defaults when nothing configured.
        let cfg = load();
        assert_eq!(cfg.geo.mirror, None);
        assert!(cfg.geo.auto_update);
        // TUI edit path: set mirror + save (dynamic JSON), reload keeps it.
        let mut edited = cfg;
        edited.geo.mirror = Some("https://gh-proxy.com".into());
        edited.geo.auto_update = false;
        edited.geo.update_interval = 48;
        edited.save().unwrap();
        let reloaded = load();
        assert_eq!(reloaded.geo.mirror.as_deref(), Some("https://gh-proxy.com"));
        assert!(!reloaded.geo.auto_update);
        assert_eq!(reloaded.geo.update_interval, 48);
        // Static TOML untouched by the TUI edit.
        let static_text = fs::read_to_string(&static_path).unwrap();
        assert!(!static_text.contains("gh-proxy"));
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
    fn proxy_group_order_caches_by_mtime() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let orig_data = std::env::var_os("XDG_DATA_HOME");
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &data_dir);
        }
        // Missing runtime.yaml: empty, and cached as such.
        assert!(Config::proxy_group_order().is_empty());
        assert!(Config::proxy_group_order().is_empty());
        let runtime = data_dir.join("clashlime/runtime.yaml");
        fs::create_dir_all(runtime.parent().unwrap()).unwrap();
        fs::write(&runtime, "proxy-groups:\n  - name: alpha\n  - name: beta\n").unwrap();
        assert_eq!(Config::proxy_group_order(), vec!["alpha", "beta"]);
        // Rewrite: mtime changes, cache refreshes (no stale order).
        fs::write(&runtime, "proxy-groups:\n  - name: gamma\n").unwrap();
        assert_eq!(Config::proxy_group_order(), vec!["gamma"]);
        unsafe {
            match orig_data {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }

    #[test]
    fn dns_advanced_fields_roundtrip_with_mihomo_key_names() {
        let mut dns = DnsConfig::default();
        dns.respect_rules = Some(true);
        dns.default_nameserver = vec!["8.8.8.8".into()];
        dns.direct_nameserver = vec!["223.5.5.5".into()];
        dns.proxy_server_nameserver = vec!["tls://8.8.8.8".into()];
        dns.fake_ip_filter_mode = Some("blacklist".into());
        dns.fake_ip_filter = vec!["*.example.com".into()];
        dns.fallback_filter.geoip = Some(true);
        dns.fallback_filter.geoip_code = Some("CN".into());
        dns.fallback_filter.ipcidr = vec!["240.0.0.0/4".into()];
        dns.nameserver_policy = [(
            "geosite:cn".to_owned(),
            StringList::Single("223.5.5.5".into()),
        )]
        .into();
        dns.hosts = [(
            "example.com".to_owned(),
            StringList::Multiple(vec!["1.2.3.4".into(), "5.6.7.8".into()]),
        )]
        .into();
        dns.use_system_hosts = Some(true);

        let text = serde_json::to_string(&dns).unwrap();
        for key in [
            "respect-rules",
            "default-nameserver",
            "proxy-server-nameserver",
            "direct-nameserver",
            "fake-ip-filter-mode",
            "fake-ip-filter",
            "fallback-filter",
            "geoip-code",
            "nameserver-policy",
            "use-system-hosts",
        ] {
            assert!(text.contains(key), "missing {key} in {text}");
        }
        let back: DnsConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(back.respect_rules, Some(true));
        assert_eq!(back.default_nameserver, vec!["8.8.8.8"]);
        assert_eq!(back.fallback_filter.geoip_code.as_deref(), Some("CN"));
        assert_eq!(
            back.nameserver_policy
                .get("geosite:cn")
                .map(StringList::display)
                .as_deref(),
            Some("223.5.5.5")
        );
        // Single strings and arrays both deserialize.
        assert_eq!(
            back.hosts
                .get("example.com")
                .map(StringList::display)
                .as_deref(),
            Some("1.2.3.4, 5.6.7.8")
        );
        let legacy: DnsConfig = serde_json::from_str(
            r#"{"nameserver-policy": {"geosite:cn": "223.5.5.5"}, "hosts": {"a.com": "1.1.1.1"}}"#,
        )
        .unwrap();
        assert_eq!(
            legacy
                .nameserver_policy
                .get("geosite:cn")
                .map(StringList::display)
                .as_deref(),
            Some("223.5.5.5")
        );
        // Unset advanced fields stay empty and serialize away.
        let minimal = serde_json::to_string(&DnsConfig::default()).unwrap();
        assert!(!minimal.contains("nameserver-policy"));
        assert!(!minimal.contains("fallback-filter"));
    }

    #[test]
    fn override_flags_default_true_and_survive_round_trip() {
        // Old files without the keys keep overriding (behavior unchanged).
        let legacy_dns: DnsConfig = serde_json::from_str(r#"{"enable":true}"#).unwrap();
        assert!(legacy_dns.override_profile);
        let legacy_sniffer: SnifferConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert!(legacy_sniffer.override_profile);
        assert!(SnifferConfig::default().override_profile);
        // `override = false` is a real choice: it must persist through
        // JSON and TOML even when every other sniffer key is empty.
        let mut sniffer = SnifferConfig::default();
        sniffer.override_profile = false;
        assert!(!sniffer.is_empty());
        let back: SnifferConfig =
            serde_json::from_str(&serde_json::to_string(&sniffer).unwrap()).unwrap();
        assert!(!back.override_profile);
        let back: SnifferConfig = toml::from_str(&toml::to_string(&sniffer).unwrap()).unwrap();
        assert!(!back.override_profile);
        let mut dns = DnsConfig::default();
        dns.override_profile = false;
        let back: DnsConfig = serde_json::from_str(&serde_json::to_string(&dns).unwrap()).unwrap();
        assert!(!back.override_profile);
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
}
