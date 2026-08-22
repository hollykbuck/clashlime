use crate::{
    api::{MihomoClient, Snapshot},
    backup,
    config::Config,
    core::{self, SupervisorState},
    profiles::Profiles,
    theme::Theme,
    ui, update,
};
use anyhow::Result;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use futures_util::StreamExt;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    cmp::min,
    collections::HashSet,
    fs,
    io,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};
use tokio::time;

pub const SETTINGS_COUNT: usize = 9;

/// Severity of a status-bar message; drives color and how long the message
/// survives before periodic refreshes may replace it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusKind {
    #[default]
    Info,
    Success,
    Warning,
    Error,
    Busy,
}

impl StatusKind {
    /// Classify a message so action feedback survives refresh cycles.
    fn infer(text: &str) -> Self {
        let lower = text.to_ascii_lowercase();
        if lower.ends_with('…')
            || ["testing", "checking", "validating", "resolving", "connecting", "downloading"]
                .iter()
                .any(|k| lower.starts_with(k))
        {
            return Self::Busy;
        }
        if [
            "failed",
            "error",
            "rejected",
            "cannot",
            "no file",
            "not executable",
        ]
        .iter()
        .any(|k| lower.contains(k))
        {
            return Self::Error;
        }
        if ["cancelled", "skipped"].iter().any(|k| lower.contains(k)) {
            return Self::Warning;
        }
        if [
            "imported ",
            "installed at",
            " installed",
            "saved",
            "activated",
            "updated",
            "deleted",
            "backup created",
            "closed",
            "restored",
            "using mihomo",
            "hot patched",
            "up to date",
        ]
        .iter()
        .any(|k| lower.contains(k))
        {
            return Self::Success;
        }
        Self::Info
    }

    /// How long this message resists being overwritten by refresh().
    fn dwell(self) -> Option<Duration> {
        match self {
            Self::Busy => None,
            Self::Error => Some(Duration::from_secs(10)),
            Self::Success => Some(Duration::from_secs(4)),
            Self::Warning => Some(Duration::from_secs(6)),
            Self::Info => Some(Duration::from_secs(3)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Dashboard,
    Proxies,
    Profiles,
    Connections,
    Rules,
    Logs,
    Settings,
    Help,
}

impl Tab {
    pub const ALL: [Self; 8] = [
        Self::Dashboard,
        Self::Proxies,
        Self::Profiles,
        Self::Connections,
        Self::Rules,
        Self::Logs,
        Self::Settings,
        Self::Help,
    ];
    pub const fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Proxies => "Proxies",
            Self::Profiles => "Profiles",
            Self::Connections => "Connections",
            Self::Rules => "Rules",
            Self::Logs => "Logs",
            Self::Settings => "Settings",
            Self::Help => "Help",
        }
    }
}

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
    status_sticky_until: Option<Instant>,
    pub online: bool,
    pub last_refresh: Option<Instant>,
    pub last_profile_check: Option<Instant>,
    pub previous_totals: (u64, u64),
    pub speeds: (u64, u64),
    pub input: Option<InputMode>,
    pub input_buffer: String,
    pub help_open: bool,
    pub core_missing: Option<CoreMissingDialog>,
    core_download_rx: Option<tokio::sync::mpsc::UnboundedReceiver<CoreDownloadEvent>>,
    core_download_abort: Option<tokio::task::JoinHandle<()>>,
    pub mihomo_update: update::UpdateState,
    mouse_regions: Vec<ui::HitRegion>,
    last_click: Option<(ui::HitTarget, Instant)>,
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

fn core_missing_dialog() -> Option<CoreMissingDialog> {
    App::core_missing().then(|| CoreMissingDialog {
        choice: CoreMissingChoice::Download,
        busy: false,
        message: String::new(),
        progress: None,
    })
}

impl App {
    /// True when no mihomo core can be located; drives the startup dialog.
    pub fn core_missing() -> bool {
        !Config::mihomo_path().is_file()
    }

    /// Bring back the chooser while the machine still lacks a usable core.
    fn reopen_core_missing_dialog(&mut self, choice: CoreMissingChoice) {
        if Self::core_missing() {
            let previous = self.core_missing.take();
            self.core_missing = Some(previous.unwrap_or(CoreMissingDialog {
                choice,
                busy: false,
                message: String::new(),
                progress: None,
            }));
            if let Some(dialog) = self.core_missing.as_mut() {
                dialog.busy = false;
                dialog.choice = choice;
            }
        }
    }

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
            core_missing: core_missing_dialog(),
            core_download_rx: None,
            core_download_abort: None,
            mihomo_update: update::UpdateState::default(),
            mouse_regions: Vec::new(),
            last_click: None,
        })
    }

    /// Record a user-action message. The message stays pinned over periodic
    /// refresh statuses for a severity-dependent dwell time.
    pub fn say(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.status_kind = StatusKind::infer(&text);
        self.status_sticky_until = self
            .status_kind
            .dwell()
            .map(|dwell| Instant::now() + dwell);
        self.status = text;
    }

    /// Refresh-driven status ("Synced" / offline reason). Yields to any
    /// sticky user message that has not expired yet.
    fn set_default_status(&mut self, text: String) {
        if self.sticky_active() {
            return;
        }
        self.status_kind = StatusKind::infer(&text);
        self.status_sticky_until = None;
        self.status = text;
    }

    fn sticky_active(&self) -> bool {
        self.status_sticky_until
            .is_some_and(|until| Instant::now() < until)
    }

    /// True when the current status is transient work in progress.
    pub fn is_busy_status(&self) -> bool {
        self.status_kind == StatusKind::Busy
    }

    pub fn status_color_kind(&self) -> StatusKind {
        if self.is_busy_status() && !self.sticky_active() && self.online {
            StatusKind::Info
        } else {
            self.status_kind
        }
    }

    /// The active profile, for sidebar display.
    pub fn current_profile(&self) -> Option<&crate::profiles::Profile> {
        let uid = self.profiles.current.as_deref()?;
        self.profiles
            .items
            .iter()
            .find(|item| item.uid == uid)
    }

    pub async fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        self.refresh().await;
        let mut events = EventStream::new();
        let mut tick = time::interval(self.config.refresh_interval());
        loop {
            self.poll_core_download_events();
            let mut mouse_regions = Vec::new();
            terminal.draw(|frame| mouse_regions = ui::draw(frame, self))?;
            self.mouse_regions = mouse_regions;
            tokio::select! {
                _ = tick.tick() => self.refresh().await,
                event = events.next() => {
                    match event {
                        Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                            if self.handle_key(key).await? { break; }
                        }
                        Some(Ok(Event::Mouse(mouse))) => self.handle_mouse(mouse).await,
                        Some(Ok(Event::Paste(text)))
                            if matches!(self.input, Some(InputMode::ImportProfile)) =>
                        {
                            self.input_buffer.push_str(text.trim());
                        }
                        Some(Err(error)) => self.say(format!("input error: {error}")),
                        None => break,
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.input.is_some() {
            return;
        }
        let target = self
            .mouse_regions
            .iter()
            .find(|region| region.contains(mouse.column, mouse.row))
            .map(|region| region.target);
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let delta = if mouse.kind == MouseEventKind::ScrollUp {
                    -1
                } else {
                    1
                };
                if let Some(target) = target {
                    self.focus_mouse_target(target);
                }
                self.move_selection(delta);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(target) = target else { return };
                let now = Instant::now();
                let double_click = self.last_click.is_some_and(|(previous, then)| {
                    previous == target && now.duration_since(then).as_millis() <= 400
                });
                self.last_click = if double_click {
                    None
                } else {
                    Some((target, now))
                };
                self.activate_mouse_target(target, double_click).await;
            }
            _ => {}
        }
    }

    fn focus_mouse_target(&mut self, target: ui::HitTarget) {
        match target {
            ui::HitTarget::ProxyGroup(index) => {
                self.node_focus = false;
                self.group_index = index;
            }
            ui::HitTarget::ProxyNode(index) => {
                self.node_focus = true;
                self.node_index = index;
            }
            ui::HitTarget::Profile(index) => self.profile_index = index,
            ui::HitTarget::Connection(index) => self.connection_index = index,
            ui::HitTarget::Rule(index) => self.rule_index = index,
            ui::HitTarget::Setting(index) => self.setting_index = index,
            _ => {}
        }
    }

    async fn activate_mouse_target(&mut self, target: ui::HitTarget, double_click: bool) {
        self.focus_mouse_target(target);
        match target {
            ui::HitTarget::Tab(tab) => self.tab = tab,
            ui::HitTarget::CoreToggle => self.toggle_core().await,
            ui::HitTarget::RoutingMode(mode) => self.set_mode(mode).await,
            ui::HitTarget::ProxyGroup(_) => self.node_index = 0,
            ui::HitTarget::ProxyNode(_) if double_click => self.select_node().await,
            ui::HitTarget::Profile(_) if double_click => self.select_profile().await,
            ui::HitTarget::Setting(_) if double_click => self.toggle_setting().await,
            _ => {}
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(true);
        }
        if self.core_missing.is_some() && self.input.is_none() {
            self.handle_core_missing_key(key).await;
            return Ok(false);
        }
        if self.input.is_some() {
            self.handle_input(key).await;
            return Ok(false);
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(true);
        }
        if self.help_open {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => self.help_open = false,
                KeyCode::Char('q') => return Ok(true),
                _ => {}
            }
            return Ok(false);
        }
        if let Some(tab) = Self::tab_shortcut(&key.code) {
            self.open_tab(tab);
            return Ok(false);
        }
        if key.code == KeyCode::Char('?') {
            self.help_open = true;
            return Ok(false);
        }
        if key.code == KeyCode::Char('q') {
            return Ok(true);
        }
        match key.code {
            KeyCode::Tab | KeyCode::BackTab if self.tab == Tab::Proxies => {
                self.node_focus = !self.node_focus
            }
            KeyCode::Left | KeyCode::Char('h') if self.tab == Tab::Proxies => {
                self.node_focus = false
            }
            KeyCode::Right | KeyCode::Char('l') if self.tab == Tab::Proxies => {
                self.node_focus = true
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Char('r') => self.refresh().await,
            KeyCode::Char('s') if self.tab == Tab::Dashboard => self.toggle_core().await,
            KeyCode::Char('m') => self.cycle_mode().await,
            KeyCode::Char('a') if self.tab == Tab::Profiles => {
                self.input = Some(InputMode::ImportProfile);
                self.input_buffer.clear();
            }
            KeyCode::Char('u') if self.tab == Tab::Profiles => self.update_profile().await,
            KeyCode::Char('D') if self.tab == Tab::Profiles => self.delete_profile().await,
            KeyCode::Char('x') if self.tab == Tab::Connections => self.close_selected().await,
            KeyCode::Char('X') if self.tab == Tab::Connections => self.close_all().await,
            KeyCode::Char('d') if self.tab == Tab::Proxies => self.delay_selected().await,
            KeyCode::Enter if self.tab == Tab::Proxies => self.select_node().await,
            KeyCode::Enter if self.tab == Tab::Profiles => self.select_profile().await,
            KeyCode::Enter if self.tab == Tab::Settings => self.toggle_setting().await,
            KeyCode::Char('b') if self.tab == Tab::Settings => self.create_backup(),
            KeyCode::Char('R') if self.tab == Tab::Settings => self.confirm_restore_backup(),
            KeyCode::Char('u') if self.tab == Tab::Settings => {
                self.check_mihomo_update(false).await
            }
            KeyCode::Char('U') if self.tab == Tab::Settings => self.check_mihomo_update(true).await,
            KeyCode::Char('o') if self.tab == Tab::Settings => self.open_update_url(),
            _ => {}
        }
        Ok(false)
    }

    pub fn proxy_groups(&self) -> Vec<(&String, &crate::api::Proxy)> {
        let mut values: Vec<_> = self
            .proxy_group_order
            .iter()
            .filter_map(|name| self.snapshot.proxies.proxies.get_key_value(name))
            .filter(|(_, proxy)| !proxy.all.is_empty())
            .collect();
        let configured: HashSet<_> = self.proxy_group_order.iter().collect();
        let mut unconfigured: Vec<_> = self
            .snapshot
            .proxies
            .proxies
            .iter()
            .filter(|(name, proxy)| !proxy.all.is_empty() && !configured.contains(name))
            .collect();
        unconfigured.sort_by_key(|item| item.0.to_lowercase());
        values.extend(unconfigured);
        values
    }

    pub fn selected_group(&self) -> Option<(&String, &crate::api::Proxy)> {
        self.proxy_groups().get(self.group_index).copied()
    }

    pub fn selected_group_is_manual(&self) -> bool {
        self.selected_group()
            .is_some_and(|(_, group)| group.kind.eq_ignore_ascii_case("selector"))
    }

    fn open_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    fn tab_shortcut(code: &KeyCode) -> Option<Tab> {
        match code {
            KeyCode::Char('1') => Some(Tab::Dashboard),
            KeyCode::Char('2') => Some(Tab::Proxies),
            KeyCode::Char('3') => Some(Tab::Profiles),
            KeyCode::Char('4') => Some(Tab::Connections),
            KeyCode::Char('5') => Some(Tab::Rules),
            KeyCode::Char('6') => Some(Tab::Logs),
            KeyCode::Char('7') => Some(Tab::Settings),
            KeyCode::Char('8') => Some(Tab::Help),
            _ => None,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let group_len = self.proxy_groups().len();
        let (index, len) = match self.tab {
            Tab::Proxies if self.node_focus => {
                let len = self
                    .selected_group()
                    .map_or(0, |(_, proxy)| proxy.all.len());
                (&mut self.node_index, len)
            }
            Tab::Proxies => (&mut self.group_index, group_len),
            Tab::Profiles => (&mut self.profile_index, self.profiles.items.len()),
            Tab::Connections => (
                &mut self.connection_index,
                self.snapshot.connections.connections.len(),
            ),
            Tab::Rules => (&mut self.rule_index, self.snapshot.rules.rules.len()),
            Tab::Settings => (&mut self.setting_index, SETTINGS_COUNT),
            _ => return,
        };
        if len == 0 {
            *index = 0;
            return;
        }
        *index = ((*index as isize + delta).rem_euclid(len as isize)) as usize;
        if self.tab == Tab::Proxies && !self.node_focus {
            self.node_index = 0;
        }
    }

    async fn refresh(&mut self) {
        self.theme.refresh();
        self.proxy_group_order = Config::proxy_group_order();
        self.update_due_profiles().await;
        self.supervisor = core::supervisor_state().await;
        // Merge mihomo + omash logs (omash first, then mihomo), keep 200 latest
        let mihomo_logs = core::CoreManager::recent_logs(150).unwrap_or_default();
        let omash_logs = crate::logger::recent_logs(50);
        let mut combined = Vec::with_capacity(200);
        // Prefix omash logs for distinguish
        for line in omash_logs {
            combined.push(format!("[omash] {line}"));
        }
        combined.extend(mihomo_logs);
        // Keep last 200
        if combined.len() > 200 {
            let drain = combined.len() - 200;
            combined.drain(0..drain);
        }
        self.logs = combined;
        match self.api.snapshot().await {
            Ok(snapshot) => {
                let totals = (
                    snapshot.connections.upload_total,
                    snapshot.connections.download_total,
                );
                let elapsed = self
                    .last_refresh
                    .map_or(1.0, |then| then.elapsed().as_secs_f64())
                    .max(0.1);
                self.speeds = if self.last_refresh.is_some() {
                    (
                        (totals.0.saturating_sub(self.previous_totals.0) as f64 / elapsed) as u64,
                        (totals.1.saturating_sub(self.previous_totals.1) as f64 / elapsed) as u64,
                    )
                } else {
                    (0, 0)
                };
                self.previous_totals = totals;
                self.last_refresh = Some(Instant::now());
                self.snapshot = snapshot;
                self.online = true;
                self.set_default_status("Synced".into());
                self.clamp_selections();
            }
            Err(error) => {
                self.online = false;
                self.set_default_status(self.offline_status(&error.to_string()));
            }
        }
    }

    fn offline_status(&self, api_error: &str) -> String {
        if self.profiles.items.is_empty() {
            return "Mihomo is not running: no profile imported. Open Profiles and press a to import."
                .into();
        }
        if !self.supervisor.enabled {
            return "Mihomo is stopped: disabled in Settings.".into();
        }
        self.supervisor
            .error
            .as_ref()
            .map(|error| format!("Mihomo is not running: {error}"))
            .unwrap_or_else(|| format!("Mihomo API unavailable: {api_error}"))
    }

    async fn update_due_profiles(&mut self) {
        if self
            .last_profile_check
            .is_some_and(|last| last.elapsed().as_secs() < 60)
        {
            return;
        }
        self.last_profile_check = Some(Instant::now());
        let now = chrono::Utc::now().timestamp();
        let due: Vec<_> = self
            .profiles
            .items
            .iter()
            .filter(|profile| {
                profile.url.is_some()
                    && profile.update_interval.is_some_and(|hours| {
                        now.saturating_sub(profile.updated) >= (hours.saturating_mul(3600)) as i64
                    })
            })
            .map(|profile| profile.uid.clone())
            .collect();
        let current = self.profiles.current.clone();
        let mut reload = false;
        for uid in due {
            match self.profiles.update_validated(&uid, &self.config).await {
                Ok(()) => reload |= current.as_deref() == Some(&uid),
                Err(error) => {
                    self.say(format!("Auto-update {uid} failed: {error}"));
                    return;
                }
            }
        }
        if reload && let Err(error) = core::request_restart().await {
            self.say(format!("Auto-update applied, restart request failed: {error}"));
        }
    }

    fn clamp_selections(&mut self) {
        self.group_index = min(
            self.group_index,
            self.proxy_groups().len().saturating_sub(1),
        );
        let node_len = self.selected_group().map_or(0, |(_, p)| p.all.len());
        self.node_index = min(self.node_index, node_len.saturating_sub(1));
        self.connection_index = min(
            self.connection_index,
            self.snapshot
                .connections
                .connections
                .len()
                .saturating_sub(1),
        );
        self.rule_index = min(
            self.rule_index,
            self.snapshot.rules.rules.len().saturating_sub(1),
        );
        self.profile_index = min(
            self.profile_index,
            self.profiles.items.len().saturating_sub(1),
        );
    }

    async fn cycle_mode(&mut self) {
        let mode = match self.snapshot.config.mode.to_ascii_lowercase().as_str() {
            "rule" => "global",
            "global" => "direct",
            _ => "rule",
        };
        self.set_mode(mode).await;
    }

    async fn set_mode(&mut self, mode: &str) {
        if self.snapshot.config.mode.eq_ignore_ascii_case(mode) {
            return;
        }
        match self.api.set_mode(mode).await {
            Ok(()) => {
                self.say(format!("Mode changed to {mode}"));
                self.refresh().await;
            }
            Err(error) => self.say(format!("Mode change failed: {error}")),
        }
    }

    async fn select_node(&mut self) {
        let selected = self.selected_group().and_then(|(name, group)| {
            group.all.get(self.node_index).map(|node| {
                (
                    name.clone(),
                    node.clone(),
                    group.kind.eq_ignore_ascii_case("selector"),
                )
            })
        });
        let Some((group, node, manual)) = selected else {
            return;
        };
        if !manual {
            self.say(format!("{group} is managed automatically"));
            return;
        }
        match self.api.select_proxy(&group, &node).await {
            Ok(()) => {
                let message = match self.profiles.record_selection(&group, &node) {
                    Ok(()) => format!("{group} → {node}"),
                    Err(error) => format!("{group} → {node}; selection was not saved: {error}"),
                };
                self.say(message);
                self.refresh().await;
            }
            Err(error) => self.say(format!("Selection failed: {error}")),
        }
    }

    async fn delay_selected(&mut self) {
        let node = self
            .selected_group()
            .and_then(|(_, g)| g.all.get(self.node_index))
            .cloned();
        let Some(node) = node else { return };
        self.say(format!("Testing {node}…"));
        match self
            .api
            .test_delay(&node, &self.config.delay_test_url)
            .await
        {
            Ok(delay) => self.say(format!("{node}: {delay} ms")),
            Err(error) => self.say(format!("Delay test failed: {error}")),
        }
    }

    async fn close_selected(&mut self) {
        let id = self
            .snapshot
            .connections
            .connections
            .get(self.connection_index)
            .map(|c| c.id.clone());
        let Some(id) = id else { return };
        match self.api.close_connection(Some(&id)).await {
            Ok(()) => {
                self.say("Connection closed");
                self.refresh().await;
            }
            Err(error) => self.say(format!("Close failed: {error}")),
        }
    }

    async fn close_all(&mut self) {
        match self.api.close_connection(None).await {
            Ok(()) => {
                self.say("All connections closed");
                self.refresh().await;
            }
            Err(error) => self.say(format!("Close failed: {error}")),
        }
    }

    async fn handle_input(&mut self, key: KeyEvent) {
        if let Some(InputMode::RestoreBackup(path)) = self.input.clone() {
            match key.code {
                KeyCode::Char('y' | 'Y') => {
                    self.input = None;
                    match backup::restore(&path) {
                        Ok(()) => match Profiles::load() {
                            Ok(profiles) => {
                                self.profiles = profiles;
                                self.say(format!("Restored {}", path.display()));
                            }
                            Err(error) => {
                                self.say(format!("Restored, but reload failed: {error}"));
                            }
                        },
                        Err(error) => self.say(format!("Restore failed: {error}")),
                    }
                }
                KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                    self.input = None;
                    self.say("Restore cancelled");
                }
                _ => {}
            }
            return;
        }
        if let Some(InputMode::CorePath) = self.input.clone() {
            match key.code {
                KeyCode::Esc => {
                    self.input = None;
                    self.input_buffer.clear();
                    self.reopen_core_missing_dialog(CoreMissingChoice::ProvidePath);
                }
                KeyCode::Backspace => {
                    self.input_buffer.pop();
                }
                KeyCode::Char(c) => self.input_buffer.push(c),
                KeyCode::Enter => {
                    let value = self.input_buffer.trim().to_owned();
                    self.input_buffer.clear();
                    self.input = None;
                    if value.is_empty() {
                        self.say("Enter an absolute path to the mihomo binary");
                    } else {
                        self.apply_core_path(&value);
                    }
                    // Validation may have failed; give the user another chance
                    self.reopen_core_missing_dialog(CoreMissingChoice::Download);
                }
                _ => {}
            }
            return;
        }
        if let Some(InputMode::EditDnsListen) = self.input.clone() {
            match key.code {
                KeyCode::Esc => {
                    self.input = None;
                    self.input_buffer.clear();
                    self.say("DNS listen edit cancelled");
                }
                KeyCode::Backspace => {
                    self.input_buffer.pop();
                }
                KeyCode::Char(c) => self.input_buffer.push(c),
                KeyCode::Enter => {
                    let value = self.input_buffer.trim().to_owned();
                    if value.is_empty() {
                        self.say("DNS listen cannot be empty (e.g. 0.0.0.0:1053)");
                        return;
                    }
                    self.input_buffer.clear();
                    self.input = None;
                    self.config.dns.listen = value.clone();
                    // Auto-enable DNS when listen edited
                    if !self.config.dns.enable {
                        self.config.dns.enable = true;
                    }
                    if let Err(e) = self.config.save() {
                        self.say(format!("Save failed: {e}"));
                        return;
                    }
                    crate::logger::info("app", &format!("dns listen -> {value}"));
                    match self.api.update_dns(&self.config.dns).await {
                        Ok(()) => self.say(format!("DNS listen {value} (hot patched)")),
                        Err(e) => {
                            crate::logger::warn(
                                "app",
                                &format!("dns listen hot patch failed: {e}"),
                            );
                            match core::request_restart().await {
                                Ok(()) => {
                                    self.say(format!("DNS listen {value} saved, reload requested"))
                                }
                                Err(err) => {
                                    self.say(format!("Save ok but reload failed: {err} (hot: {e})"))
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some(InputMode::EditDnsServers) = self.input.clone() {
            match key.code {
                KeyCode::Esc => {
                    self.input = None;
                    self.input_buffer.clear();
                    self.say("DNS servers edit cancelled");
                }
                KeyCode::Backspace => {
                    self.input_buffer.pop();
                }
                KeyCode::Char(c) => self.input_buffer.push(c),
                KeyCode::Enter => {
                    let value = self.input_buffer.trim().to_owned();
                    if value.is_empty() {
                        self.say("Enter comma-separated DNS servers (e.g. 223.5.5.5, 8.8.8.8)");
                        return;
                    }
                    let servers: Vec<String> = value
                        .split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if servers.is_empty() {
                        self.say("No valid servers parsed");
                        return;
                    }
                    self.input_buffer.clear();
                    self.input = None;
                    self.config.dns.nameserver = servers.clone();
                    if !self.config.dns.enable {
                        self.config.dns.enable = true;
                    }
                    if let Err(e) = self.config.save() {
                        self.say(format!("Save failed: {e}"));
                        return;
                    }
                    crate::logger::info("app", &format!("dns servers -> {}", servers.join(", ")));
                    match self.api.update_dns(&self.config.dns).await {
                        Ok(()) => {
                            self.say(format!("DNS servers {} (hot patched)", servers.join(", ")));
                        }
                        Err(e) => {
                            crate::logger::warn(
                                "app",
                                &format!("dns servers hot patch failed: {e}"),
                            );
                            match core::request_restart().await {
                                Ok(()) => {
                                    self.say(format!("DNS servers saved, reload requested"))
                                }
                                Err(err) => {
                                    self.say(format!("Save ok but reload failed: {err} (hot: {e})"))
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.input = None;
                self.input_buffer.clear();
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(character) => self.input_buffer.push(character),
            KeyCode::Enter => {
                let value = self.input_buffer.trim().to_owned();
                if value.is_empty() {
                    self.say("Enter a subscription URL or an absolute YAML file path.");
                    return;
                }
                self.input_buffer.clear();
                self.input = None;
                let result = if value.starts_with("http://") || value.starts_with("https://") {
                    self.profiles
                        .import_remote(&value, None, &self.config)
                        .await
                } else {
                    self.profiles
                        .import_local(std::path::Path::new(&value), None, &self.config)
                        .await
                };
                match result {
                    Ok(uid) => {
                        self.say(format!("Imported {uid}"));
                        self.profile_index = self.profiles.items.len().saturating_sub(1);
                    }
                    Err(error) => self.say(format!("Import failed: {error}")),
                }
            }
            _ => {}
        }
    }

    async fn toggle_core(&mut self) {
        let enable = !core::core_desired_enabled().await;
        let result = core::request_core_enabled(enable).await.map(|()| {
            if enable && self.profiles.items.is_empty() {
                "Mihomo cannot start: no profile imported. Open Profiles and press a to import."
            } else if enable {
                "Mihomo start requested"
            } else {
                "Mihomo stop requested"
            }
            .to_owned()
        });
        match result {
            Ok(message) => self.say(message),
            Err(error) => self.say(format!("Core operation failed: {error}")),
        }
        self.refresh().await;
    }

    async fn handle_core_missing_key(&mut self, key: KeyEvent) {
        if self.core_missing.as_ref().is_some_and(|dialog| dialog.busy) {
            // Only Esc is honored mid-download; it cancels the task.
            if key.code == KeyCode::Esc {
                self.cancel_core_download();
            }
            return;
        }
        match key.code {
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Char('h')
            | KeyCode::Char('l')
            | KeyCode::Tab => {
                if let Some(dialog) = self.core_missing.as_mut() {
                    dialog.choice = match dialog.choice {
                        CoreMissingChoice::Download => CoreMissingChoice::ProvidePath,
                        CoreMissingChoice::ProvidePath => CoreMissingChoice::Download,
                    };
                }
            }
            KeyCode::Enter => {
                let choice = self
                    .core_missing
                    .as_ref()
                    .map(|dialog| dialog.choice)
                    .unwrap_or(CoreMissingChoice::Download);
                match choice {
                    CoreMissingChoice::Download => self.download_core().await,
                    CoreMissingChoice::ProvidePath => {
                        self.input = Some(InputMode::CorePath);
                        self.input_buffer.clear();
                        self.core_missing = None;
                    }
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => self.download_core().await,
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.input = Some(InputMode::CorePath);
                self.input_buffer.clear();
                self.core_missing = None;
            }
            KeyCode::Esc => {
                // Keep running without a core; supervisor retries in background
                self.core_missing = None;
                self.say("Skipped: no mihomo core. Settings will keep showing the warning.");
            }
            _ => {}
        }
    }

    async fn download_core(&mut self) {
        if self.core_download_rx.is_some() {
            return; // already running
        }
        let Some(dialog) = self.core_missing.as_mut() else {
            return;
        };
        dialog.busy = true;
        dialog.progress = None;
        dialog.message = "resolving latest release…".into();
        let (event_tx, rx) = tokio::sync::mpsc::unbounded_channel::<CoreDownloadEvent>();
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();

        let destination = Config::data_dir().join("bin/mihomo");
        let handle = tokio::spawn(async move {
            let report = |event| {
                let _ = event_tx.send(event);
            };
            report(CoreDownloadEvent::Stage("resolving latest release…".into()));
            match update::fetch_latest_release(true).await {
                Ok(release) => match update::pick_asset(&release) {
                    Ok(asset) => {
                        report(CoreDownloadEvent::Stage(format!(
                            "downloading {} ({})…",
                            asset.name,
                            update::format_size(asset.size as usize)
                        )));
                        // Forward byte progress to the UI as it arrives.
                        let forwarder_tx = event_tx.clone();
                        let forwarder = tokio::spawn(async move {
                            while let Some(progress) = progress_rx.recv().await {
                                if forwarder_tx
                                    .send(CoreDownloadEvent::Progress(progress))
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        });
                        let result =
                            update::download_core(&asset, &destination, Some(&progress_tx)).await;
                        drop(progress_tx);
                        let _ = forwarder.await;
                        match result {
                            Ok(path) => report(CoreDownloadEvent::Done(path)),
                            Err(error) => report(CoreDownloadEvent::Failed(error.to_string())),
                        }
                    }
                    Err(error) => report(CoreDownloadEvent::Failed(error.to_string())),
                },
                Err(error) => report(CoreDownloadEvent::Failed(error.to_string())),
            }
        });
        self.core_download_abort = Some(handle);
        self.core_download_rx = Some(rx);
    }

    /// Apply one background-download event to the dialog and status line.
    fn handle_core_download_event(&mut self, event: CoreDownloadEvent) {
        match event {
            CoreDownloadEvent::Stage(stage) => {
                if let Some(dialog) = self.core_missing.as_mut() {
                    dialog.message = stage;
                    dialog.busy = true;
                }
            }
            CoreDownloadEvent::Progress(progress) => {
                if let Some(dialog) = self.core_missing.as_mut() {
                    dialog.progress = Some((progress.downloaded, progress.total));
                }
            }
            CoreDownloadEvent::Done(path) => {
                self.core_download_rx = None;
                self.core_download_abort = None;
                self.core_missing = None;
                self.say(format!("Mihomo installed at {}", path.display()));
                crate::logger::info("update", &format!("core installed at {}", path.display()));
            }
            CoreDownloadEvent::Failed(error) => {
                self.core_download_rx = None;
                self.core_download_abort = None;
                crate::logger::warn("update", &format!("download failed: {error}"));
                if let Some(dialog) = self.core_missing.as_mut() {
                    dialog.busy = false;
                    dialog.message = format!("failed: {error}");
                }
                self.say(format!("Mihomo download failed: {error}"));
            }
        }
    }

    /// Cancel an in-flight core download (dialog Esc).
    fn cancel_core_download(&mut self) {
        if let Some(handle) = self.core_download_abort.take() {
            handle.abort();
        }
        self.core_download_rx = None;
        if let Some(dialog) = self.core_missing.as_mut() {
            dialog.busy = false;
            dialog.progress = None;
            dialog.message = "download cancelled".into();
        }
        self.say("Mihomo download cancelled");
    }

    fn poll_core_download_events(&mut self) {
        while let Some(event) = self
            .core_download_rx
            .as_mut()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.handle_core_download_event(event);
        }
    }

    /// Validate a user-provided core binary and persist it in the dynamic
    /// config so the daemon picks it up on its next tick.
    fn apply_core_path(&mut self, value: &str) {
        let path = PathBuf::from(value.trim());
        if !path.is_file() {
            self.say(format!("No file at {}", path.display()));
            return;
        }
        #[cfg(unix)]
        if let Ok(meta) = fs::metadata(&path) {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o111 == 0
                && let Err(error) = fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            {
                self.say(format!(
                    "{} is not executable (chmod failed: {error})",
                    path.display()
                ));
                return;
            }
        }
        // Sanity check: it must print a version string
        if let Err(error) = update::version_from_binary_at(&path) {
            self.say(format!("{} rejected: {error}", path.display()));
            return;
        }
        match Config::set_mihomo_override(Some(&path)) {
            Ok(()) => {
                self.say(format!("Using mihomo at {}", path.display()));
                crate::logger::info("app", &format!("core path override -> {}", path.display()));
            }
            Err(error) => self.say(format!("Binary ok but save failed: {error}")),
        }
    }

    async fn select_profile(&mut self) {
        let Some(uid) = self
            .profiles
            .items
            .get(self.profile_index)
            .map(|item| item.uid.clone())
        else {
            return;
        };
        self.say(format!("Validating profile {uid}…"));
        let mut candidate = self.profiles.clone();
        candidate.current = Some(uid.clone());
        match core::CoreManager::new()
            .validate_only(&self.config, &candidate)
            .await
        {
            Ok(()) => match candidate.save() {
                Ok(()) => match core::request_restart().await {
                    Ok(()) => {
                        self.profiles = candidate;
                        self.say(format!("Profile {uid} activated"));
                    }
                    Err(error) => {
                        self.say(format!(
                            "Profile was valid but could not be activated: {error}"
                        ));
                    }
                },
                Err(error) => self.say(format!(
                    "Profile was valid but could not be activated: {error}"
                )),
            },
            Err(error) => self.say(format!("Profile rejected; current core kept: {error}")),
        }
        self.refresh().await;
    }

    async fn update_profile(&mut self) {
        let Some(uid) = self
            .profiles
            .items
            .get(self.profile_index)
            .map(|item| item.uid.clone())
        else {
            return;
        };
        match self.profiles.update_validated(&uid, &self.config).await {
            Ok(()) => self.say(format!("Profile {uid} updated")),
            Err(error) => self.say(format!("Update failed: {error}")),
        }
    }

    async fn delete_profile(&mut self) {
        let Some(uid) = self
            .profiles
            .items
            .get(self.profile_index)
            .map(|item| item.uid.clone())
        else {
            return;
        };
        match self.profiles.delete(&uid) {
            Ok(()) => self.say(format!("Profile {uid} deleted")),
            Err(error) => self.say(format!("Delete failed: {error}")),
        }
    }

    async fn toggle_setting(&mut self) {
        let mut restart = false;
        let mut dns_hot_patch = false;
        match self.setting_index {
            0 => {
                let enable = !core::core_desired_enabled().await;
                if let Err(error) = core::request_core_enabled(enable).await {
                    self.say(format!("Core state change failed: {error}"));
                    return;
                }
            }
            1 => {
                let previous = self.config.auto_start;
                self.config.auto_start = !previous;
                if let Err(error) = self.config.save() {
                    self.config.auto_start = previous;
                    self.say(format!("Save failed: {error}"));
                    return;
                }
                if let Err(error) = core::set_supervisor_autostart(self.config.auto_start).await {
                    self.config.auto_start = previous;
                    let rollback = self.config.save().err();
                    self.say(format!("Autostart change failed: {error}"));
                    if let Some(rollback) = rollback {
                        let rollback_text = format!("; rollback failed: {rollback}");
                        self.status.push_str(&rollback_text);
                    }
                    return;
                }
                self.say("Autostart setting saved");
                return;
            }
            2 => {
                self.config.system_proxy = !self.config.system_proxy;
                restart = true;
            }
            3 => {
                self.config.allow_lan = !self.config.allow_lan;
                restart = true;
            }
            4 => {
                self.config.ipv6 = !self.config.ipv6;
                restart = true;
            }
            5 => {
                self.config.refresh_ms = if self.config.refresh_ms >= 5000 {
                    500
                } else {
                    self.config.refresh_ms + 500
                };
            }
            6 => {
                self.config.dns.enable = !self.config.dns.enable;
                dns_hot_patch = true;
            }
            7 => {
                // Edit DNS listen address
                self.input = Some(InputMode::EditDnsListen);
                self.input_buffer = self.config.dns.listen.clone();
                return;
            }
            8 => {
                // Edit DNS nameservers (comma separated)
                self.input = Some(InputMode::EditDnsServers);
                self.input_buffer = self.config.dns.nameserver.join(", ");
                return;
            }
            _ => {}
        }
        if let Err(error) = self.config.save() {
            self.say(format!("Save failed: {error}"));
            return;
        }
        if dns_hot_patch {
            crate::logger::info("app", &format!("dns enable -> {}", self.config.dns.enable));
            match self.api.update_dns(&self.config.dns).await {
                Ok(()) => {
                    let state = if self.config.dns.enable {
                        "enabled"
                    } else {
                        "disabled"
                    };
                    self.say(format!("DNS {state} (hot patched)"));
                    return;
                }
                Err(e) => {
                    crate::logger::warn(
                        "app",
                        &format!("dns hot patch failed, fallback to reload: {e}"),
                    );
                    // fallback to runtime rebuild + reload
                    if let Err(err) = core::request_restart().await {
                        self.say(format!("DNS saved but reload failed: {err} (hot patch: {e})"));
                        return;
                    }
                    self.say("DNS saved, reload requested");
                    return;
                }
            }
        }
        if restart && let Err(error) = core::request_restart().await {
            self.say(format!("Saved, restart request failed: {error}"));
            return;
        }
        self.say("Setting saved");
    }

    fn create_backup(&mut self) {
        match backup::create() {
            Ok(path) => self.say(format!("Backup created: {}", path.display())),
            Err(error) => self.say(format!("Backup failed: {error}")),
        }
    }

    fn confirm_restore_backup(&mut self) {
        match backup::list() {
            Ok(files) if files.is_empty() => self.say("No local backups"),
            Ok(files) => self.input = Some(InputMode::RestoreBackup(files[0].clone())),
            Err(error) => self.say(format!("Cannot list backups: {error}")),
        }
    }

    async fn check_mihomo_update(&mut self, force: bool) {
        if self.mihomo_update.checking {
            self.say("Update check already in progress");
            return;
        }
        self.mihomo_update.checking = true;
        self.mihomo_update.message = "checking…".into();
        self.say("Checking mihomo update via GitHub…");
        // Prefer binary version, fallback to snapshot version, fallback to "unknown"
        let current = update::current_version_from_binary()
            .ok()
            .or_else(|| {
                let v = self.snapshot.version.version.clone();
                if v.is_empty() || v == "—" {
                    None
                } else {
                    Some(v)
                }
            })
            .unwrap_or_else(|| "unknown".into());
        self.mihomo_update.current = current.clone();
        match update::check_update(&current, force).await {
            Ok((release, available)) => {
                self.mihomo_update.latest = Some(release.tag_name.clone());
                self.mihomo_update.html_url = Some(release.html_url.clone());
                self.mihomo_update.available = Some(available);
                self.mihomo_update.prerelease = release.prerelease;
                self.mihomo_update.checked_at = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                );
                self.mihomo_update.checking = false;
                self.mihomo_update.message = if available {
                    format!("{} → {} available", current, release.tag_name)
                } else {
                    format!("{} up to date", current)
                };
                crate::logger::info(
                    "update",
                    &format!(
                        "check {} -> {} available={}",
                        current, release.tag_name, available
                    ),
                );
                self.say(if available {
                    format!(
                        "Update available: {} → {} ({})",
                        current, release.tag_name, release.html_url
                    )
                } else {
                    format!("Mihomo {} is up to date", current)
                });
            }
            Err(e) => {
                self.mihomo_update.checking = false;
                self.mihomo_update.available = None;
                self.mihomo_update.message = format!("failed: {e}");
                crate::logger::warn("update", &format!("check failed: {e}"));
                self.say(format!("Update check failed: {e}"));
            }
        }
    }

    fn open_update_url(&mut self) {
        let Some(url) = self.mihomo_update.html_url.clone().or_else(|| {
            // fallback to releases page
            Some("https://github.com/MetaCubeX/mihomo/releases".to_owned())
        }) else {
            self.say("No update URL");
            return;
        };
        let result = std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .or_else(|_| std::process::Command::new("open").arg(&url).spawn())
            .or_else(|_| {
                std::process::Command::new("gio")
                    .args(["open", &url])
                    .spawn()
            });
        match result {
            Ok(_) => self.say(format!("Opening {url}")),
            Err(e) => self.say(format!("Failed to open {url}: {e}")),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_kind_infers_severity_from_text() {
        assert_eq!(StatusKind::infer("Testing node…"), StatusKind::Busy);
        assert_eq!(
            StatusKind::infer("Import failed: timeout"),
            StatusKind::Error
        );
        assert_eq!(StatusKind::infer("Imported abc123"), StatusKind::Success);
        assert_eq!(
            StatusKind::infer("Restore cancelled"),
            StatusKind::Warning
        );
        assert_eq!(StatusKind::infer("Synced"), StatusKind::Info);
    }

    #[test]
    fn status_kind_error_dwells_longer_than_info() {
        let error_dwell = StatusKind::Error.dwell();
        let info_dwell = StatusKind::Info.dwell();
        assert!(error_dwell.unwrap() > info_dwell.unwrap());
        assert_eq!(StatusKind::Busy.dwell(), None);
    }
}
