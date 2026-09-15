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

use super::{InputMode, LogEntry, LogLevel, LogSource, SettingSection, StatusKind, Tab};
use super::{CoreMissingChoice, CoreMissingDialog};
use crate::api::Snapshot;
use crate::core::SupervisorState;
use crate::profiles::Profiles;
use crate::update::UpdateState;
use crate::ui::HitRegion;
use crate::ui::HitTarget;
use std::time::Instant;

/// View and interaction state.
#[derive(Default)]
pub struct UiState {
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
    pub input: Option<InputMode>,
    pub input_buffer: String,
    /// Cursor position inside `input_buffer` as a character index
    /// (`0` = before first char, `len` = after last char). Needed for
    /// Left/Right editing of long values like `proxy_bypass`.
    pub input_cursor: usize,
    pub help_open: bool,
    pub core_missing: Option<CoreMissingDialog>,
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
    pub(crate) mouse_regions: Vec<HitRegion>,
    pub(crate) last_click: Option<(HitTarget, Instant)>,
}

/// Domain data: API snapshot, profiles, logs, status line.
#[derive(Default)]
pub struct DataState {
    pub snapshot: Snapshot,
    pub profiles: Profiles,
    pub proxy_group_order: Vec<String>,
    pub supervisor: SupervisorState,
    pub logs: Vec<LogEntry>,
    pub online: bool,
    pub last_slow_refresh: Option<Instant>,
    pub last_profile_check: Option<Instant>,
    /// Rules/providers fetched at least once. The slow fetch runs only
    /// while the Rules tab is visible (lazy load), so this gates the
    /// fetch-on-open when switching to the tab.
    pub(crate) rules_loaded: bool,
    /// Latest `/traffic` sample, refreshed every second by the stream task.
    pub speeds: (u64, u64),
    pub status: String,
    pub status_kind: StatusKind,
    pub(crate) status_sticky_until: Option<Instant>,
    pub mihomo_update: UpdateState,
    /// Tag of the release being installed as a core upgrade (`i` in
    /// Settings); distinguishes upgrade downloads from the first-run
    /// core-missing flow sharing the same channel.
    pub(crate) core_upgrade: Option<String>,
}

impl UiState {
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

    /// Cursor navigation shared by every text input (Left/Right/Home/End/
    /// Delete plus Ctrl-B/F/A/E/D). Returns true when the key was consumed.
    /// Without this, long values like `proxy_bypass` can only be edited
    /// at the end because Left/Right fall into `_ => {}`.
    pub(crate) fn input_nav(&mut self, key: &crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Left => {
                self.input_move_left();
                true
            }
            KeyCode::Right => {
                self.input_move_right();
                true
            }
            KeyCode::Home => {
                self.input_move_home();
                true
            }
            KeyCode::End => {
                self.input_move_end();
                true
            }
            KeyCode::Delete => {
                self.input_delete();
                true
            }
            KeyCode::Char('b') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                self.input_move_left();
                true
            }
            KeyCode::Char('f') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                self.input_move_right();
                true
            }
            KeyCode::Char('a') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                self.input_move_home();
                true
            }
            KeyCode::Char('e') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                self.input_move_end();
                true
            }
            KeyCode::Char('d') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                self.input_delete();
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
        !crate::config::Config::mihomo_path().is_file()
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
