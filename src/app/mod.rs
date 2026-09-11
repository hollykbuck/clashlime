pub mod actions;
pub mod event;
pub mod input;
pub mod refresh;
pub mod selection;
pub mod status;
pub mod tab;

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

pub const SETTINGS_COUNT: usize = 10;

pub struct App {
    pub config: Config,
    pub api: MihomoClient,
    pub snapshot: Snapshot,
    pub profiles: Profiles,
    pub proxy_group_order: Vec<String>,
    pub theme: Theme,
    pub supervisor: SupervisorState,
    pub logs: Vec<String>,
    pub geoip_version: String,
    pub tab: Tab,
    pub group_index: usize,
    pub node_index: usize,
    pub connection_index: usize,
    pub rule_index: usize,
    pub profile_index: usize,
    pub setting_index: usize,
    pub node_focus: bool,
    pub status: String,
    pub status_kind: StatusKind,
    pub(crate) status_sticky_until: Option<Instant>,
    pub online: bool,
    pub last_refresh: Option<Instant>,
    pub last_profile_check: Option<Instant>,
    pub previous_totals: (u64, u64),
    pub speeds: (u64, u64),
    pub input: Option<InputMode>,
    pub input_buffer: String,
    pub help_open: bool,
    pub core_missing: Option<CoreMissingDialog>,
    pub(crate) core_download_rx: Option<tokio::sync::mpsc::UnboundedReceiver<CoreDownloadEvent>>,
    pub(crate) core_download_abort: Option<tokio::task::JoinHandle<()>>,
    pub(crate) geo_rx: Option<tokio::sync::mpsc::UnboundedReceiver<crate::app::actions::geo::GeoEvent>>,
    pub(crate) geo_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) import_rx: Option<tokio::sync::mpsc::UnboundedReceiver<crate::app::actions::import::ImportEvent>>,
    pub(crate) import_task: Option<tokio::task::JoinHandle<()>>,
    pub mihomo_update: update::UpdateState,
    pub(crate) mouse_regions: Vec<ui::HitRegion>,
    pub(crate) last_click: Option<(ui::HitTarget, Instant)>,
}

#[derive(Clone, Debug)]
pub enum InputMode {
    ImportProfile,
    RestoreBackup(PathBuf),
    EditDnsListen,
    EditDnsServers,
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
            geoip_version: installed_package_version("clash-geoip"),
            tab: Tab::default(),
            group_index: 0,
            node_index: 0,
            connection_index: 0,
            rule_index: 0,
            profile_index: 0,
            setting_index: 0,
            node_focus: false,
            status: "Connecting…".into(),
            status_kind: StatusKind::Busy,
            status_sticky_until: None,
            online: false,
            last_refresh: None,
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
            mihomo_update: update::UpdateState::default(),
            mouse_regions: Vec::new(),
            last_click: None,
        })
    }

}

fn installed_package_version(name: &str) -> String {
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
