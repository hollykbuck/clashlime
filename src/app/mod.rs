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
use std::{path::PathBuf, time::Instant};

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
    pub tab: Tab,
    pub group_index: usize,
    pub node_index: usize,
    /// Proxies tab substring filter over node names (set via `/`).
    pub node_query: String,
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
    pub last_slow_refresh: Option<Instant>,
    pub last_profile_check: Option<Instant>,
    /// Rules/providers fetched at least once. The slow fetch runs only
    /// while the Rules tab is visible (lazy load), so this gates the
    /// fetch-on-open when switching to the tab.
    pub(crate) rules_loaded: bool,
    /// Latest `/traffic` sample, refreshed every second by the stream task.
    pub speeds: (u64, u64),
    pub input: Option<InputMode>,
    pub input_buffer: String,
    /// Cursor position inside `input_buffer` as a character index
    /// (`0` = before first char, `len` = after last char). Needed for
    /// Left/Right editing of long values like `proxy_bypass`.
    pub input_cursor: usize,
    pub help_open: bool,
    pub core_missing: Option<CoreMissingDialog>,
    pub(crate) core_download_rx: Option<tokio::sync::mpsc::UnboundedReceiver<CoreDownloadEvent>>,
    pub(crate) core_download_abort: Option<tokio::task::JoinHandle<()>>,
    /// Tag of the release being installed as a core upgrade (`i` in
    /// Settings); distinguishes upgrade downloads from the first-run
    /// core-missing flow sharing the same channel.
    pub(crate) core_upgrade: Option<String>,
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
    pub(crate) mem_rx: Option<tokio::sync::mpsc::UnboundedReceiver<crate::api::MemoryInfo>>,
    pub(crate) mem_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) mem_stream_key: String,
    pub(crate) traffic_rx: Option<tokio::sync::mpsc::UnboundedReceiver<crate::api::TrafficInfo>>,
    pub(crate) traffic_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) traffic_stream_key: String,
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
    SearchNodes,
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
    Done { tag: String, path: PathBuf },
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
            mem_rx: None,
            mem_task: None,
            mem_stream_key: String::new(),
            traffic_rx: None,
            traffic_task: None,
            traffic_stream_key: String::new(),
            tab: Tab::default(),
            group_index: 0,
            node_index: 0,
            node_query: String::new(),
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
            last_slow_refresh: None,
            last_profile_check: None,
            rules_loaded: false,
            speeds: (0, 0),
            input: None,
            input_buffer: String::new(),
            input_cursor: 0,
            help_open: false,
            core_missing: Self::core_missing_dialog(),
            core_download_rx: None,
            core_download_abort: None,
            core_upgrade: None,
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

    /// Set the text input buffer and place the cursor at the end.
    /// Use for every `input = Some(..)` entry so Left/Right editing
    /// starts from a consistent position.
    pub(crate) fn set_input(&mut self, value: String) {
        self.input_cursor = value.chars().count();
        self.input_buffer = value;
    }

    /// Clear the text input buffer and reset the cursor.
    pub(crate) fn clear_input(&mut self) {
        self.input_buffer.clear();
        self.input_cursor = 0;
    }

    /// Byte offset of the char-based `input_cursor` inside `input_buffer`.
    fn input_byte_index(&self) -> usize {
        self.input_buffer
            .char_indices()
            .nth(self.input_cursor)
            .map(|(i, _)| i)
            .unwrap_or_else(|| self.input_buffer.len())
    }

    pub(crate) fn clamp_input_cursor(&mut self) {
        let len = self.input_buffer.chars().count();
        if self.input_cursor > len {
            self.input_cursor = len;
        }
    }

    /// Insert a character at the cursor (Left/Right editable).
    pub(crate) fn input_insert(&mut self, c: char) {
        let idx = self.input_byte_index();
        self.input_buffer.insert(idx, c);
        self.input_cursor += 1;
    }

    /// Insert a pasted string at the cursor.
    pub(crate) fn input_insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let idx = self.input_byte_index();
        self.input_buffer.insert_str(idx, text);
        self.input_cursor += text.chars().count();
    }

    /// Backspace: delete the character before the cursor.
    pub(crate) fn input_backspace(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let end = self.input_byte_index();
        let start = self
            .input_buffer
            .char_indices()
            .nth(self.input_cursor - 1)
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.input_buffer.drain(start..end);
        self.input_cursor -= 1;
    }

    /// Delete: remove the character under the cursor.
    pub(crate) fn input_delete(&mut self) {
        let len = self.input_buffer.chars().count();
        if self.input_cursor >= len {
            return;
        }
        let start = self.input_byte_index();
        let end = self
            .input_buffer
            .char_indices()
            .nth(self.input_cursor + 1)
            .map(|(i, _)| i)
            .unwrap_or_else(|| self.input_buffer.len());
        self.input_buffer.drain(start..end);
    }

    pub(crate) fn input_move_left(&mut self) {
        self.input_cursor = self.input_cursor.saturating_sub(1);
    }

    pub(crate) fn input_move_right(&mut self) {
        let len = self.input_buffer.chars().count();
        if self.input_cursor < len {
            self.input_cursor += 1;
        }
    }

    pub(crate) fn input_move_home(&mut self) {
        self.input_cursor = 0;
    }

    pub(crate) fn input_move_end(&mut self) {
        self.input_cursor = self.input_buffer.chars().count();
    }
}
