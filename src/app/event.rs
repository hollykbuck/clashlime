use crate::ui;
use anyhow::Result;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use futures_util::StreamExt;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{io, time::Instant};
use tokio::time;

use super::{InputMode, Tab};

impl super::App {
    pub async fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        self.refresh_full().await;
        let mut events = EventStream::new();
        let mut tick = time::interval(self.config.refresh_interval());
        loop {
            self.poll_core_download_events();
            self.poll_geo_events();
            self.poll_import_events().await;
            self.poll_profile_events().await;
            self.poll_update_check_events();
            self.poll_delay_events();
            self.poll_log_events();
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
                        Some(Ok(Event::Paste(text))) if self.input.is_some() => {
                            self.handle_paste(text);
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

    /// Bracketed-paste content goes straight into the input buffer for every
    /// text input. The restore-confirm dialog answers y/n/Esc and ignores it.
    pub(crate) fn handle_paste(&mut self, text: String) {
        match self.input {
            Some(InputMode::ImportProfile) => self.input_buffer.push_str(text.trim()),
            Some(_) if !matches!(self.input, Some(InputMode::RestoreBackup(_))) => {
                self.input_buffer.push_str(&text);
            }
            _ => {}
        }
    }

    pub(crate) async fn handle_mouse(&mut self, mouse: MouseEvent) {        if self.input.is_some() {
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

    async fn activate_mouse_target(&mut self, target: ui::HitTarget, double_click: bool) {
        self.focus_mouse_target(target);
        match target {
            ui::HitTarget::Tab(tab) => self.tab = tab,
            ui::HitTarget::CoreToggle => self.toggle_core().await,
            ui::HitTarget::RoutingMode(mode) => self.set_mode(mode).await,
            ui::HitTarget::ProxyGroup(_) => self.node_index = 0,
            ui::HitTarget::ProxyNode(_) if double_click => self.select_node().await,
            ui::HitTarget::Profile(_) if double_click => self.start_select_profile(),
            ui::HitTarget::Setting(_, _) if double_click => self.toggle_setting().await,
            _ => {}
        }
    }

    pub(crate) async fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
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
        if key.code == KeyCode::Esc
            && (self.geo_updating()
                || self.import_running()
                || self.profile_task_running()
                || self.update_check_running()
                || self.delay_running())
        {
            // Background geo / import / profile / update-check / delay work
            // is the only cancellable work here.
            if self.geo_updating() {
                self.cancel_geo_update();
            }
            if self.import_running() {
                self.cancel_import();
            }
            if self.profile_task_running() {
                self.cancel_profile_task();
            }
            if self.update_check_running() {
                self.cancel_update_check();
            }
            if self.delay_running() {
                self.cancel_delay_test();
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
            KeyCode::Left | KeyCode::Char('h') if self.tab == Tab::Settings => {
                self.move_setting_section(-1)
            }
            KeyCode::Right | KeyCode::Char('l') if self.tab == Tab::Settings => {
                self.move_setting_section(1)
            }
            KeyCode::Down | KeyCode::Char('j') if self.tab == Tab::Logs => self.scroll_logs(1),
            KeyCode::Up | KeyCode::Char('k') if self.tab == Tab::Logs => self.scroll_logs(-1),
            KeyCode::PageDown if self.tab == Tab::Logs => self.page_logs(1),
            KeyCode::PageUp if self.tab == Tab::Logs => self.page_logs(-1),
            KeyCode::End | KeyCode::Char('G') if self.tab == Tab::Logs => self.follow_logs(),
            KeyCode::Home | KeyCode::Char('g') if self.tab == Tab::Logs => self.top_logs(),
            KeyCode::Char('f') if self.tab == Tab::Logs => self.cycle_log_filter(),
            KeyCode::Char('v') if self.tab == Tab::Logs => self.cycle_log_source(),
            KeyCode::Char('/') if self.tab == Tab::Logs => {
                self.input = Some(InputMode::SearchLogs);
                self.input_buffer = self.log_query.clone();
            }
            KeyCode::Esc if self.tab == Tab::Logs && !self.log_query.is_empty() => {
                self.log_query.clear();
                self.follow_logs();
                self.say("Log search cleared");
            }
            KeyCode::Left | KeyCode::Char('h') if self.tab == Tab::Logs => {
                self.scroll_logs_horizontal(-1)
            }
            KeyCode::Right | KeyCode::Char('l') if self.tab == Tab::Logs => {
                self.scroll_logs_horizontal(1)
            }
            KeyCode::Enter if self.tab == Tab::Logs => self.open_log_detail(),
            KeyCode::Esc if self.tab == Tab::Logs && self.log_detail.is_some() => {
                self.close_log_detail()
            }
            KeyCode::Char('c') if self.tab == Tab::Logs => {
                self.log_query.clear();
                self.log_level_filter = None;
                self.log_hscroll = 0;
                self.follow_logs();
                self.say("Log filters cleared");
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Char('r') => self.refresh_full().await,
            KeyCode::Char('s') if self.tab == Tab::Dashboard => self.toggle_core().await,
            KeyCode::Char('m') => self.cycle_mode().await,
            KeyCode::Char('a') if self.tab == Tab::Profiles => {
                self.input = Some(InputMode::ImportProfile);
                self.input_buffer.clear();
            }
            KeyCode::Char('u') if self.tab == Tab::Profiles => self.start_update_profile(),
            KeyCode::Char('D') if self.tab == Tab::Profiles => self.delete_profile().await,
            KeyCode::Char('x') if self.tab == Tab::Connections => self.close_selected().await,
            KeyCode::Char('X') if self.tab == Tab::Connections => self.close_all().await,
            KeyCode::Char('d') if self.tab == Tab::Proxies => self.start_delay_selected(),
            KeyCode::Enter if self.tab == Tab::Proxies => self.select_node().await,
            KeyCode::Enter if self.tab == Tab::Profiles => self.start_select_profile(),
            KeyCode::Enter if self.tab == Tab::Settings => self.toggle_setting().await,
            KeyCode::Char('b') if self.tab == Tab::Settings => self.create_backup(),
            KeyCode::Char('R') if self.tab == Tab::Settings => self.confirm_restore_backup(),
            KeyCode::Char('g') if self.tab == Tab::Settings => {
                self.setting_section = crate::app::SettingSection::Geo;
                self.setting_index = self.section_cursor
                    [crate::app::SettingSection::Geo.index()]
                .min(
                    crate::app::SettingSection::Geo
                        .row_count()
                        .saturating_sub(1),
                );
            }
            KeyCode::Char('u') if self.tab == Tab::Settings => {
                self.start_mihomo_update_check(false)
            }
            KeyCode::Char('U') if self.tab == Tab::Settings => {
                self.start_mihomo_update_check(true)
            }
            KeyCode::Char('o') if self.tab == Tab::Settings => self.open_update_url(),
            _ => {}
        }
        Ok(false)
    }
}
