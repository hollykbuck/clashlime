pub mod actions;
pub mod event;
pub mod input;
pub mod refresh;
pub mod selection;
pub mod setting_section;
pub mod status;
pub mod tab;

pub use setting_section::SettingSection;
pub use status::StatusKind;
pub use tab::Tab;

use crate::{
    api::{MihomoClient, Snapshot},
    config::Config,
    core::SupervisorState,
    profiles::Profiles,
    theme::Theme,
    ui, update,
};
use anyhow::Result;
use std::{path::PathBuf, process::Command, time::Instant};

pub struct App {
    pub config: Config,
    pub api: MihomoClient,
    pub snapshot: Snapshot,
    pub profiles: Profiles,
    pub proxy_group_order: Vec<String>,
    pub theme: Theme,
    pub supervisor: SupervisorState,
    pub logs: Vec<LogEntry>,
    /// Logs tab source filter.
    pub log_source: LogSource,
    /// Logs tab: first visible row of the filtered view.
    pub log_scroll: usize,
    /// Follow tail on new output; any manual scroll turns it off.
    pub log_follow: bool,
    /// Minimum level shown (`None` = all).
    pub log_level_filter: Option<LogLevel>,
    /// Substring filter (set via `/`).
    pub log_query: String,
    /// Visible height of the log list, recorded at render for paging.
    pub(crate) log_height: usize,
    /// Horizontal character offset for long log lines.
    pub log_hscroll: usize,
    /// Full text of the log line opened in the detail popup (`None` = closed).
    pub log_detail: Option<String>,
    pub geoip_version: String,
    pub tab: Tab,
    pub group_index: usize,
    pub node_index: usize,
    pub connection_index: usize,
    pub rule_index: usize,
    /// Rules tab substring filter over type/payload/policy (set via `/`).
    pub rule_query: String,
    pub profile_index: usize,
    pub setting_index: usize,
    pub setting_section: SettingSection,
    /// Per-section cursor memory, indexed by `SettingSection::index`.
    pub section_cursor: [usize; 6],
    pub node_focus: bool,
    /// Routing-mode menu open (`m`); index of the highlighted mode.
    pub mode_menu: bool,
    pub mode_menu_index: usize,
    /// Profile update-settings editor open (`e` on Profiles); index of
    /// the highlighted row. Text sub-edits reuse `input`/`input_buffer`.
    pub profile_editor: bool,
    pub profile_editor_index: usize,
    pub status: String,
    pub status_kind: StatusKind,
    pub(crate) status_sticky_until: Option<Instant>,
    pub online: bool,
    pub last_refresh: Option<Instant>,
    pub last_slow_refresh: Option<Instant>,
    pub last_profile_check: Option<Instant>,
    pub previous_totals: (u64, u64),
    pub speeds: (u64, u64),
    pub input: Option<InputMode>,
    pub input_buffer: String,
    pub help_open: bool,
    pub core_missing: Option<CoreMissingDialog>,
    pub(crate) core_download_rx: Option<tokio::sync::mpsc::UnboundedReceiver<CoreDownloadEvent>>,
    pub(crate) core_download_abort: Option<tokio::task::JoinHandle<()>>,
    pub(crate) geo_rx:
        Option<tokio::sync::mpsc::UnboundedReceiver<crate::app::actions::geo::GeoEvent>>,
    pub(crate) geo_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) import_rx:
        Option<tokio::sync::mpsc::UnboundedReceiver<crate::app::actions::import::ImportEvent>>,
    pub(crate) import_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) profile_rx:
        Option<tokio::sync::mpsc::UnboundedReceiver<crate::app::actions::profile::ProfileEvent>>,
    pub(crate) profile_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) update_rx:
        Option<tokio::sync::mpsc::UnboundedReceiver<crate::app::actions::update::UpdateCheckEvent>>,
    pub(crate) update_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) delay_rx:
        Option<tokio::sync::mpsc::UnboundedReceiver<crate::app::actions::proxy::DelayEvent>>,
    pub(crate) delay_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) log_rx:
        Option<tokio::sync::mpsc::UnboundedReceiver<crate::app::actions::logstream::CoreLogEvent>>,
    pub(crate) log_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) log_stream_key: String,
    pub log_stream_live: bool,
    pub(crate) log_backlog_loaded: bool,
    pub mihomo_update: update::UpdateState,
    pub(crate) mouse_regions: Vec<ui::HitRegion>,
    pub(crate) last_click: Option<(ui::HitTarget, Instant)>,
}

/// Where one log line came from. The TUI merges three files; the
/// filter picks which sources stay visible.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogSource {
    All,
    Core,
    Daemon,
    Tui,
}

impl LogSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Core => "core",
            Self::Daemon => "daemon",
            Self::Tui => "tui",
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::All => "",
            Self::Core => "[core] ",
            Self::Daemon => "[daemon] ",
            Self::Tui => "[tui] ",
        }
    }
}

/// One merged log line with its origin attached.
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub source: LogSource,
    pub text: String,
}

/// Log severity for the Logs tab filter. Ordering is significant:
/// a filter shows its level and everything above it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Debug => "debug+",
            Self::Info => "info+",
            Self::Warn => "warn+",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug)]
pub enum InputMode {
    ImportProfile,
    RestoreBackup(PathBuf),
    SearchLogs,
    SearchRules,
    EditDnsListen,
    EditDnsServers,
    EditDnsFakeIpRange,
    EditDnsFakeIpFilter,
    EditDnsDefaultNs,
    EditDnsDirectNs,
    EditDnsProxyNs,
    EditDnsFallback,
    EditDnsFallbackGeoCode,
    EditMixedPort,
    EditController,
    EditSecret,
    EditProxyBypass,
    EditDelayTestUrl,
    EditSocksPort,
    EditHttpPort,
    EditRedirPort,
    EditTproxyPort,
    EditAuth,
    EditSkipAuth,
    EditLanAllowed,
    EditLanDisallowed,
    EditTunDevice,
    EditTunDnsHijack,
    EditTunMtu,
    EditTunRouteExclude,
    EditSniffHttpPorts,
    EditSniffTlsPorts,
    EditGeoMirror,
    EditGeoProxy,
    EditGeoIpUrl,
    EditGeositeUrl,
    EditMmdbUrl,
    EditAsnUrl,
    EditProfileName,
    EditProfileInterval,
    EditProfileTimeout,
    EditProfileAuth,
    EditProfileUserAgent,
    CorePath,
}

/// Modal shown when no mihomo core is found at startup.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CoreMissingChoice {
    Download,
    ProvidePath,
}

pub struct CoreMissingDialog {
    pub choice: CoreMissingChoice,
    pub busy: bool,
    pub message: String,
    /// Live download progress: (bytes received, total bytes when known).
    pub progress: Option<(u64, Option<u64>)>,
}

/// Events streamed back from the background core-download task.
pub enum CoreDownloadEvent {
    Stage(String),
    Progress(update::DownloadProgress),
    Done(PathBuf),
    Failed(String),
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        let api = MihomoClient::new(&config.controller, config.secret.clone())?;
        let profiles = Profiles::load()?;
        Ok(Self {
            config,
            api,
            snapshot: Snapshot::default(),
            profiles,
            proxy_group_order: Config::proxy_group_order(),
            theme: Theme::load(),
            supervisor: SupervisorState::default(),
            logs: vec![],
            log_source: LogSource::All,
            log_scroll: 0,
            log_follow: true,
            log_level_filter: None,
            log_query: String::new(),
            log_height: 0,
            log_hscroll: 0,
            log_detail: None,
            log_rx: None,
            log_task: None,
            log_stream_key: String::new(),
            log_stream_live: false,
            log_backlog_loaded: false,
            geoip_version: installed_package_version("clash-geoip"),
            tab: Tab::default(),
            group_index: 0,
            node_index: 0,
            connection_index: 0,
            rule_index: 0,
            rule_query: String::new(),
            profile_index: 0,
            setting_index: 0,
            setting_section: SettingSection::Core,
            section_cursor: [0; 6],
            node_focus: false,
            mode_menu: false,
            mode_menu_index: 0,
            profile_editor: false,
            profile_editor_index: 0,
            status: "Connecting…".into(),
            // Info, not Busy: a sticky Busy would pin "Connecting…" forever
            // and block the first successful refresh from reporting "Synced".
            status_kind: StatusKind::Info,
            status_sticky_until: None,
            online: false,
            last_refresh: None,
            last_slow_refresh: None,
            last_profile_check: None,
            previous_totals: (0, 0),
            speeds: (0, 0),
            input: None,
            input_buffer: String::new(),
            help_open: false,
            core_missing: Self::core_missing_dialog(),
            core_download_rx: None,
            core_download_abort: None,
            geo_rx: None,
            geo_task: None,
            import_rx: None,
            import_task: None,
            profile_rx: None,
            profile_task: None,
            update_rx: None,
            update_task: None,
            delay_rx: None,
            delay_task: None,
            mihomo_update: update::UpdateState::default(),
            mouse_regions: Vec::new(),
            last_click: None,
        })
    }
}

fn installed_package_version(name: &str) -> String {
    // Fast path: parse pacman's local db directly. Forking `pacman -Q`
    // costs ~300ms on every TUI startup and blocks the first frame.
    if let Ok(entries) = std::fs::read_dir("/var/lib/pacman/local") {
        let prefix = format!("{name}-");
        let mut found = false;
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let Some(rest) = file_name.strip_prefix(&prefix) else {
                continue;
            };
            // Dir layout is `{pkgname}-{pkgver}-{pkgrel}`; `rest` is exactly
            // what `pacman -Q` prints after the name.
            if !rest.is_empty() {
                return format!("{name} {rest}");
            }
            found = true;
        }
        if !found {
            // DB readable and package absent: no need to fork pacman.
            return "not installed".into();
        }
    }
    Command::new("pacman")
        .args(["-Q", name])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|line| {
            line.split_once(' ')
                .map(|(_, version)| version.trim().to_owned())
        })
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| "not installed".into())
}
