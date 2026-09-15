pub mod actions;
pub mod backend;
pub mod event;
pub mod input;
pub mod refresh;
pub mod selection;
pub mod setting_section;
pub mod state;
pub mod status;
pub mod tab;
pub mod tasks;

pub use setting_section::SettingSection;
pub use state::{DataState, UiState};
pub use status::StatusKind;
pub use tab::Tab;
pub(crate) use tasks::TaskHub;

use crate::{
    api::MihomoClient, config::Config, core::SupervisorState, profiles::Profiles,
    theme::Theme, update,
};
use anyhow::Result;
use std::path::PathBuf;

/// Session roots plus three state groups ([`UiState`], [`DataState`],
/// [`TaskHub`](tasks::TaskHub)). New state goes into a group — never back
/// onto this struct — so tests keep constructing only what they need.
pub struct App {
    pub config: Config,
    pub api: MihomoClient,
    /// Pure remote TUI: no daemon IPC, no local core/profile management.
    /// Only remote Mihomo API endpoints are used.
    pub remote: bool,
    pub theme: Theme,
    pub ui: UiState,
    pub data: DataState,
    pub tasks: TaskHub,
}

/// Where one log line came from. The TUI merges three files; the
/// filter picks which sources stay visible.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LogSource {
    #[default]
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
            remote: false,
            theme: Theme::load(),
            ui: UiState {
                core_missing: UiState::core_missing_dialog(),
                ..Default::default()
            },
            data: DataState {
                profiles,
                proxy_group_order: Config::proxy_group_order(),
                // Info, not Busy: a sticky Busy would pin "Connecting…"
                // forever and block the first successful refresh from
                // reporting "Synced".
                status: "Connecting…".into(),
                ..Default::default()
            },
            tasks: TaskHub::default(),
        })
    }

    /// Switch into pure remote TUI mode: drop local profile/supervisor
    /// state, never show the local core-missing dialog. Only the remote
    /// Mihomo API (`config.controller` + `config.secret`) is used.
    pub fn enter_remote(&mut self) {
        self.remote = true;
        self.data.profiles = Profiles::default();
        self.data.proxy_group_order = Vec::new();
        self.data.supervisor = SupervisorState {
            enabled: true,
            ..SupervisorState::default()
        };
        self.ui.core_missing = None;
        self.data.status = "Connecting… (remote)".into();
    }

    /// Backend strategy for this instance (local daemon vs pure remote).
    pub(crate) fn backend(&self) -> backend::Backend {
        backend::Backend::of(self.remote)
    }

    /// Guard a local-only action. Returns true when allowed; in remote
    /// mode says why it is unavailable and returns false.
    pub(crate) fn require(&mut self, capability: backend::Capability) -> bool {
        use backend::Capability::*;
        if self.backend().allows(capability) {
            return true;
        }
        self.say(match capability {
            ManageCore => "Not available in remote mode (no local core to start/stop)",
            ManageProfiles => {
                "Not available in remote mode (profiles are managed on the remote core)"
            }
            ManageGeo => "Not available in remote mode (geo data lives on the remote core)",
            ManageCoreBinary => {
                "Not available in remote mode (remote core is managed elsewhere)"
            }
            ManageBackups => {
                "Not available in remote mode (no local profiles to back up or restore)"
            }
        });
        false
    }
}
