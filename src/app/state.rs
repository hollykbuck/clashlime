//! Split of the former `App` god object.
//!
//! Step 3 of the refactor. `App` keeps only the session roots
//! (`config`, `api`, `theme`, `remote`) plus three groups:
//!
//! - [`UiState`]: everything the renderer reads and the event loop
//!   mutates — tabs, cursors, filters, modals, text input, mouse.
//! - [`DataState`]: domain data from the API/daemon — snapshot, profiles,
//!   logs, status line.
//!
//! Background slots live in [`TaskHub`](super::tasks::TaskHub).
//! Each group derives `Default`, so tests construct only what they need
//! (`UiState { tab: Tab::Proxies, ..Default::default() }`) instead of a
//! 60-field `App` literal that breaks on every new field.

use super::{CoreMissingChoice, CoreMissingDialog, DnsMapEditor, GeoMissingChoice, GeoMissingDialog};
use super::{InputMode, LogEntry, LogLevel, LogSource, SettingSection, StatusKind, Tab};
use crate::api::Snapshot;
use crate::core::SupervisorState;
use crate::profiles::Profiles;
use crate::ui::HitRegion;
use crate::ui::HitTarget;
use crate::update::UpdateState;
use std::time::Instant;

/// Node ordering on the Proxies tab, cycled with `,` then `d` / `D` /
/// `n` / `o`. `Original` keeps the order the core reported.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum NodeSort {
    #[default]
    Original,
    /// Fastest first; nodes without a delay test sink to the bottom.
    DelayAsc,
    /// Slowest first; untested nodes sink to the bottom.
    DelayDesc,
    Name,
}

/// Which sub-tab the Proxies page shows: the group/node panes or the
/// detailed proxy-provider panes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProxyView {
    #[default]
    Groups,
    Providers,
}

impl NodeSort {
    /// Short label shown in the Nodes panel title.
    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "orig",
            Self::DelayAsc => "delay ↑",
            Self::DelayDesc => "delay ↓",
            Self::Name => "name",
        }
    }
}

/// Connection ordering on the Connections tab, chosen with `,` then a
/// key. `Original` keeps the order the core reported; the traffic and
/// start-time modes are descending because the interesting rows are the
/// busiest/newest ones.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum ConnectionSort {
    Original,
    #[default]
    StartDesc,
    UploadDesc,
    DownloadDesc,
    TrafficDesc,
    Host,
    Chain,
}

impl ConnectionSort {
    /// Short label shown in the connections panel title.
    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "orig",
            Self::StartDesc => "newest",
            Self::UploadDesc => "upload ↓",
            Self::DownloadDesc => "download ↓",
            Self::TrafficDesc => "traffic ↓",
            Self::Host => "host",
            Self::Chain => "chain",
        }
    }
}

/// Which connections the list shows. Closed rows are a local history:
/// mihomo drops them from `/connections` as soon as they close.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum ConnectionFilter {
    #[default]
    Active,
    All,
    Closed,
}

impl ConnectionFilter {
    /// Short label used in the panel title and status line.
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::All => "all",
            Self::Closed => "closed",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Active => Self::All,
            Self::All => Self::Closed,
            Self::Closed => Self::Active,
        }
    }
}

/// View and interaction state.
#[derive(Default)]
pub struct UiState {
    pub tab: Tab,
    pub group_index: usize,
    pub node_index: usize,
    /// Proxies tab substring filter over node names (set via `/`).
    pub node_query: String,
    /// Node ordering on the Proxies tab (`d` / `D` / `n` / `o`).
    pub node_sort: NodeSort,
    /// `,` pressed on Proxies: the next key picks a sort mode. Only
    /// armed while the flag is set; any key disarms it again.
    pub node_sort_prefix: bool,
    /// Connection ordering on the Connections tab.
    pub connection_sort: ConnectionSort,
    /// `,` pressed on Connections: the next key picks a sort mode.
    pub connection_sort_prefix: bool,
    /// Active/all/closed view, cycled with `f`.
    pub connection_filter: ConnectionFilter,
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
    /// Groups/node panes vs. the detailed provider panes (`p`).
    pub proxy_view: ProxyView,
    /// Selected provider in the provider view.
    pub provider_index: usize,
    /// Selected node inside the selected provider.
    pub provider_node_index: usize,
    /// Routing-mode menu open (`m`); index of the highlighted mode.
    pub mode_menu: bool,
    pub mode_menu_index: usize,
    /// Profile update-settings editor open (`e` on Profiles); index of
    /// the highlighted row. Text sub-edits reuse the shared `TextInput`.
    pub profile_editor: bool,
    pub profile_editor_index: usize,
    /// DNS key/value table editor (`nameserver-policy` / `hosts`); text
    /// sub-edits of a single entry reuse the shared `TextInput`.
    pub dns_map_editor: Option<DnsMapEditor>,
    /// Import dialog (`a` on Profiles) mirroring the update-settings
    /// editor, so a subscription can be added with the same knobs
    /// instead of being edited afterwards.
    pub import_editor: bool,
    pub import_editor_index: usize,
    pub import_draft: ImportDraft,
    pub(crate) input: Option<TextInput>,
    pub help_open: bool,
    pub notifications_open: bool,
    pub notifications_cursor: usize,
    pub core_missing: Option<CoreMissingDialog>,
    pub geo_missing: Option<GeoMissingDialog>,
    pub geo_missing_skipped: bool,
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
    /// Connection id backing the detail popup when it was opened from the
    /// Connections tab. The popup re-renders from the live snapshot so
    /// the rate rows keep updating while it stays open.
    pub detail_connection: Option<String>,
    /// First visible line of the detail popup, for long connection/log
    /// records that do not fit the fixed-height dialog.
    pub detail_scroll: u16,
    pub(crate) mouse_regions: Vec<HitRegion>,
    pub(crate) last_click: Option<(HitTarget, Instant)>,
}

/// Scratch state for the import dialog: the URL/path plus the same
/// per-profile update knobs the `e` editor exposes, so a subscription
/// lands fully configured instead of needing a second pass.
pub struct ImportDraft {
    pub source: String,
    pub name: String,
    pub auto_update: bool,
    pub interval: String,
    pub timeout: String,
    pub use_proxy: bool,
    pub auth_token: String,
    pub user_agent: String,
}

impl Default for ImportDraft {
    fn default() -> Self {
        Self {
            source: String::new(),
            name: String::new(),
            // Matches `Profile::auto_update_enabled`, which treats an
            // absent flag as enabled: the dialog must not silently
            // import a subscription with auto-update off.
            auto_update: true,
            interval: String::new(),
            timeout: String::new(),
            use_proxy: false,
            auth_token: String::new(),
            user_agent: String::new(),
        }
    }
}

impl ImportDraft {
    /// True once the source looks like a subscription, which is what
    /// makes the update rows meaningful (a local YAML path has none).
    pub(crate) fn source_is_remote(&self) -> bool {
        let source = self.source.trim();
        source.starts_with("http://") || source.starts_with("https://")
    }

    /// Validate the numeric fields and build the import overrides.
    /// Empty means "leave it to the server / the default".
    pub(crate) fn overrides(&self) -> Result<crate::profiles::ImportOverrides, String> {
        let parse = |raw: &str, label: &str, max: u64| -> Result<Option<u64>, String> {
            if raw.trim().is_empty() {
                return Ok(None);
            }
            raw.trim()
                .parse::<u64>()
                .ok()
                .filter(|value| (1..=max).contains(value))
                .map(Some)
                .ok_or_else(|| format!("Enter {label} 1-{max}, or empty for the default"))
        };
        let update_interval = parse(&self.interval, "hours", 8760)?;
        let update_timeout = parse(&self.timeout, "seconds", 600)?;
        Ok(crate::profiles::ImportOverrides {
            auto_update: self.auto_update,
            update_interval,
            update_timeout,
            use_proxy: self.use_proxy,
            auth_token: Some(self.auth_token.trim().to_owned()).filter(|token| !token.is_empty()),
            user_agent: Some(self.user_agent.trim().to_owned()).filter(|ua| !ua.is_empty()),
        })
    }
}

/// Domain data: API snapshot, profiles, logs, status line.
#[derive(Default)]
pub struct DataState {
    pub snapshot: Snapshot,
    pub profiles: Profiles,
    pub proxy_group_order: Vec<String>,
    /// Names declared under `proxy-providers:` in the generated config.
    /// Empty for the common profile that inlines `proxies:`, which is what
    /// keeps the provider sub-tab from costing a request.
    pub proxy_provider_order: Vec<String>,
    pub supervisor: SupervisorState,
    pub logs: Vec<LogEntry>,
    pub online: bool,
    pub last_slow_refresh: Option<Instant>,
    pub last_profile_check: Option<Instant>,
    /// Rules/rule providers fetched at least once. The slow fetch runs only
    /// while the Rules tab is visible (lazy load), so this gates the
    /// fetch-on-open when switching to the tab.
    pub(crate) rules_loaded: bool,
    /// Proxy providers fetched at least once, same lazy-load contract for
    /// the Proxies tab. Set eagerly (without a request) when the profile
    /// declares no providers, so it stops gating.
    pub(crate) proxy_providers_loaded: bool,
    pub(crate) dynamic_config_loaded: bool,
    /// Latest `/traffic` sample, refreshed every second by the stream task.
    pub speeds: (u64, u64),
    /// Per-connection throughput (upload, download) in bytes/s, keyed by
    /// connection id. Derived from the previous `/connections` snapshot:
    /// mihomo only reports cumulative counters, so a delta over elapsed
    /// time is the only way to show a live rate.
    pub connection_rates: std::collections::HashMap<String, (u64, u64)>,
    /// When [`Self::snapshot`]'s connection counters were sampled. `None`
    /// until the first successful refresh, which yields no rate yet.
    pub(crate) connection_sample_at: Option<Instant>,
    /// Connections that disappeared from `/connections`, newest last.
    /// mihomo keeps no history, so this is the only way to inspect a
    /// closed connection; capped to avoid unbounded growth.
    pub closed_connections: Vec<crate::api::Connection>,
    pub status: String,
    pub status_kind: StatusKind,
    pub(crate) status_sticky_until: Option<Instant>,
    pub notifications: Vec<crate::app::status::Notification>,
    pub unread_notifications: usize,
    pub last_supervisor_notified_error: Option<String>,
    pub mihomo_update: UpdateState,
    /// Tag of the release being installed as a core upgrade (`i` in
    /// Settings); distinguishes upgrade downloads from the first-run
    /// core-missing flow sharing the same channel.
    pub(crate) core_upgrade: Option<String>,
}

impl DataState {
    pub fn push_notification(&mut self, kind: StatusKind, message: String, time: String) {
        if let Some(first) = self.notifications.first()
            && first.message == message
        {
            return;
        }
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.notifications.insert(0, crate::app::status::Notification {
            id,
            kind,
            message,
            time,
            read: false,
        });
        if self.notifications.len() > 100 {
            self.notifications.truncate(100);
        }
        self.unread_notifications = self.unread_notifications.saturating_add(1);
    }

    pub fn mark_notifications_read(&mut self) {
        for n in &mut self.notifications {
            n.read = true;
        }
        self.unread_notifications = 0;
    }

    pub fn clear_notifications(&mut self) {
        self.notifications.clear();
        self.unread_notifications = 0;
    }

    pub fn has_unread_error(&self) -> bool {
        self.notifications
            .iter()
            .any(|n| !n.read && matches!(n.kind, StatusKind::Error))
    }
}

pub(crate) struct TextInput {
    mode: InputMode,
    buffer: String,
    cursor: usize,
}

impl TextInput {
    fn new(mode: InputMode, value: String) -> Self {
        let cursor = value.chars().count();
        Self {
            mode,
            buffer: value,
            cursor,
        }
    }

    /// Byte offset of the char-based cursor inside the buffer.
    fn byte_index(&self) -> usize {
        self.buffer
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or_else(|| self.buffer.len())
    }

    fn insert(&mut self, c: char) {
        let idx = self.byte_index();
        self.buffer.insert(idx, c);
        self.cursor += 1;
    }

    fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let idx = self.byte_index();
        self.buffer.insert_str(idx, text);
        self.cursor += text.chars().count();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let end = self.byte_index();
        let start = self
            .buffer
            .char_indices()
            .nth(self.cursor - 1)
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.buffer.drain(start..end);
        self.cursor -= 1;
    }

    fn delete(&mut self) {
        let len = self.buffer.chars().count();
        if self.cursor >= len {
            return;
        }
        let start = self.byte_index();
        let end = self
            .buffer
            .char_indices()
            .nth(self.cursor + 1)
            .map(|(i, _)| i)
            .unwrap_or_else(|| self.buffer.len());
        self.buffer.drain(start..end);
    }

    fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        let len = self.buffer.chars().count();
        if self.cursor < len {
            self.cursor += 1;
        }
    }

    fn move_home(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.buffer.chars().count();
    }
}

impl UiState {
    pub(crate) fn input_mode(&self) -> Option<&InputMode> {
        self.input.as_ref().map(|input| &input.mode)
    }

    pub(crate) fn input_mode_cloned(&self) -> Option<InputMode> {
        self.input.as_ref().map(|input| input.mode.clone())
    }

    pub(crate) fn input_value(&self) -> &str {
        self.input
            .as_ref()
            .map(|input| input.buffer.as_str())
            .unwrap_or("")
    }

    pub(crate) fn input_cursor(&self) -> usize {
        self.input.as_ref().map(|input| input.cursor).unwrap_or(0)
    }

    pub(crate) fn has_input(&self) -> bool {
        self.input.is_some()
    }

    /// Open the text editor with its mode and initial value in one step.
    pub(crate) fn begin_input(&mut self, mode: InputMode, value: String) {
        self.input = Some(TextInput::new(mode, value));
    }

    /// Close the text editor and drop its buffer/cursor.
    pub(crate) fn close_input(&mut self) {
        self.input = None;
    }

    pub(crate) fn clamp_input_cursor(&mut self) {
        if let Some(input) = self.input.as_mut() {
            input.cursor = input.cursor.min(input.buffer.chars().count());
        }
    }

    /// Insert a character at the cursor (Left/Right editable).
    pub(crate) fn input_insert(&mut self, c: char) {
        if let Some(input) = self.input.as_mut() {
            input.insert(c);
        }
    }

    /// Insert a pasted string at the cursor.
    pub(crate) fn input_insert_str(&mut self, text: &str) {
        if let Some(input) = self.input.as_mut() {
            input.insert_str(text);
        }
    }

    /// Backspace: delete the character before the cursor.
    pub(crate) fn input_backspace(&mut self) {
        if let Some(input) = self.input.as_mut() {
            input.backspace();
        }
    }

    /// Cursor navigation shared by every text input (Left/Right/Home/End/
    /// Delete plus Ctrl-B/F/A/E/D). Returns true when the key was consumed.
    /// Without this, long values like `proxy_bypass` can only be edited
    /// at the end because Left/Right fall into `_ => {}`.
    pub(crate) fn input_nav(&mut self, key: &crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;
        let Some(input) = self.input.as_mut() else {
            return false;
        };
        match key.code {
            KeyCode::Left => {
                input.move_left();
                true
            }
            KeyCode::Right => {
                input.move_right();
                true
            }
            KeyCode::Home => {
                input.move_home();
                true
            }
            KeyCode::End => {
                input.move_end();
                true
            }
            KeyCode::Delete => {
                input.delete();
                true
            }
            KeyCode::Char('b')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                input.move_left();
                true
            }
            KeyCode::Char('f')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                input.move_right();
                true
            }
            KeyCode::Char('a')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                input.move_home();
                true
            }
            KeyCode::Char('e')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                input.move_end();
                true
            }
            KeyCode::Char('d')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                input.delete();
                true
            }
            _ => false,
        }
    }

    /// Plain typing character, if any. Ctrl/Alt combos are not text
    /// (Ctrl-C quits one layer up); only empty or Shift modifiers insert.
    pub(crate) fn input_typing(key: &crossterm::event::KeyEvent) -> Option<char> {
        use crossterm::event::KeyModifiers;
        match key.code {
            crossterm::event::KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                    && !key.modifiers.contains(KeyModifiers::SUPER)
                    && !key.modifiers.contains(KeyModifiers::HYPER)
                    && !key.modifiers.contains(KeyModifiers::META) =>
            {
                Some(c)
            }
            _ => None,
        }
    }

    /// True when no mihomo core can be located; drives the startup dialog.
    pub fn core_missing() -> bool {
        if crate::config::is_system_mode() {
            // In system mode, client delegates core presence check to supervisor via IPC.
            return false;
        }
        !crate::config::Config::mihomo_path().is_file()
    }

    /// True when required geodata files are missing; drives the geodata modal.
    pub fn geo_missing() -> bool {
        if crate::config::is_system_mode() {
            // In system mode, client delegates geo presence check to supervisor via IPC.
            return false;
        }
        !crate::geo::present("geoip.metadb") || !crate::geo::present("geosite.dat")
    }

    pub fn required_geo_missing_files(sup: &SupervisorState) -> Vec<String> {
        if crate::config::is_system_mode() {
            sup.geo_missing_files.clone()
        } else {
            ["geoip.metadb", "geosite.dat"]
                .into_iter()
                .filter(|name| !crate::geo::present(name))
                .map(String::from)
                .collect()
        }
    }

    pub(crate) fn geo_missing_dialog() -> Option<GeoMissingDialog> {
        (Self::geo_missing() && !Self::core_missing()).then(|| GeoMissingDialog {
            choice: GeoMissingChoice::Download,
            busy: false,
            message: String::new(),
            progress: None,
        })
    }

    /// Bring back the geodata chooser while required geodata files are missing.
    pub(crate) fn reopen_geo_missing_dialog(&mut self, choice: GeoMissingChoice) {
        if Self::geo_missing() {
            let previous = self.geo_missing.take();
            self.geo_missing = Some(previous.unwrap_or(GeoMissingDialog {
                choice,
                busy: false,
                message: String::new(),
                progress: None,
            }));
            if let Some(dialog) = self.geo_missing.as_mut() {
                dialog.busy = false;
                dialog.choice = choice;
            }
        }
    }

    pub(crate) fn core_missing_dialog() -> Option<CoreMissingDialog> {
        Self::core_missing().then(|| CoreMissingDialog {
            choice: CoreMissingChoice::Download,
            busy: false,
            message: String::new(),
            progress: None,
        })
    }

    /// Bring back the chooser while the machine still lacks a usable core.
    pub(crate) fn reopen_core_missing_dialog(&mut self, choice: CoreMissingChoice) {
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
}

#[cfg(test)]
mod tests {
    use super::{ImportDraft, InputMode, UiState};

    #[test]
    fn begin_input_keeps_mode_buffer_and_cursor_in_sync() {
        let mut ui = UiState::default();
        ui.begin_input(InputMode::EditProxyBypass, "stale".into());
        ui.begin_input(InputMode::EditController, "http://127.0.0.1:9090".into());
        assert!(matches!(ui.input_mode(), Some(InputMode::EditController)));
        assert_eq!(ui.input_value(), "http://127.0.0.1:9090");
        assert_eq!(ui.input_cursor(), ui.input_value().chars().count());
        ui.close_input();
        assert!(!ui.has_input());
        assert!(ui.input_value().is_empty());
        assert_eq!(ui.input_cursor(), 0);
    }

    #[test]
    fn import_draft_defaults_to_auto_update_on() {
        // `Profile::auto_update_enabled` treats an absent flag as
        // enabled, so the dialog must not disagree with it.
        assert!(ImportDraft::default().auto_update);
    }

    #[test]
    fn import_draft_detects_remote_sources() {
        let mut draft = ImportDraft::default();
        assert!(!draft.source_is_remote());
        draft.source = "  /home/you/config.yaml ".into();
        assert!(!draft.source_is_remote());
        draft.source = "https://example.com/sub".into();
        assert!(draft.source_is_remote());
        draft.source = "http://example.com/sub".into();
        assert!(draft.source_is_remote());
    }

    #[test]
    fn import_draft_empty_numeric_fields_mean_default() {
        let overrides = ImportDraft::default().overrides().unwrap();
        assert_eq!(overrides.update_interval, None);
        assert_eq!(overrides.update_timeout, None);
        assert_eq!(overrides.auth_token, None);
        assert_eq!(overrides.user_agent, None);
    }

    #[test]
    fn import_draft_rejects_out_of_range_numbers() {
        for (interval, timeout) in [("0", ""), ("8761", ""), ("", "0"), ("", "601")] {
            let draft = ImportDraft {
                interval: interval.into(),
                timeout: timeout.into(),
                ..Default::default()
            };
            assert!(
                draft.overrides().is_err(),
                "interval={interval:?} timeout={timeout:?} must be rejected"
            );
        }
    }

    #[test]
    fn import_draft_trims_and_keeps_valid_numbers() {
        let draft = ImportDraft {
            interval: " 24 ".into(),
            timeout: "45".into(),
            auth_token: "  Bearer x  ".into(),
            user_agent: "   ".into(),
            ..Default::default()
        };
        let overrides = draft.overrides().unwrap();
        assert_eq!(overrides.update_interval, Some(24));
        assert_eq!(overrides.update_timeout, Some(45));
        assert_eq!(overrides.auth_token.as_deref(), Some("Bearer x"));
        // Whitespace-only UA means "use the default".
        assert_eq!(overrides.user_agent, None);
    }
}
