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
        self.refresh().await;
        let mut events = EventStream::new();
        let mut tick = time::interval(self.config.refresh_interval());
        loop {
            self.poll_core_download_events();
            self.poll_geo_events();
            self.poll_import_events();
            self.poll_profile_events().await;
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

    pub(crate) async fn handle_mouse(&mut self, mouse: MouseEvent) {
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

    async fn activate_mouse_target(&mut self, target: ui::HitTarget, double_click: bool) {
        self.focus_mouse_target(target);
        match target {
            ui::HitTarget::Tab(tab) => self.tab = tab,
            ui::HitTarget::CoreToggle => self.toggle_core().await,
            ui::HitTarget::RoutingMode(mode) => self.set_mode(mode).await,
            ui::HitTarget::ProxyGroup(_) => self.node_index = 0,
            ui::HitTarget::ProxyNode(_) if double_click => self.select_node().await,
            ui::HitTarget::Profile(_) if double_click => self.start_select_profile(),
            ui::HitTarget::Setting(_) if double_click => self.toggle_setting().await,
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
            && (self.geo_updating() || self.import_running() || self.profile_task_running())
        {
            // Background geo / import / profile work is the only cancellable
            // work here.
            if self.geo_updating() {
                self.cancel_geo_update();
            }
            if self.import_running() {
                self.cancel_import();
            }
            if self.profile_task_running() {
                self.cancel_profile_task();
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
            KeyCode::Char('u') if self.tab == Tab::Profiles => self.start_update_profile(),
            KeyCode::Char('D') if self.tab == Tab::Profiles => self.delete_profile().await,
            KeyCode::Char('x') if self.tab == Tab::Connections => self.close_selected().await,
            KeyCode::Char('X') if self.tab == Tab::Connections => self.close_all().await,
            KeyCode::Char('d') if self.tab == Tab::Proxies => self.delay_selected().await,
            KeyCode::Enter if self.tab == Tab::Proxies => self.select_node().await,
            KeyCode::Enter if self.tab == Tab::Profiles => self.start_select_profile(),
            KeyCode::Enter if self.tab == Tab::Settings => self.toggle_setting().await,
            KeyCode::Char('b') if self.tab == Tab::Settings => self.create_backup(),
            KeyCode::Char('R') if self.tab == Tab::Settings => self.confirm_restore_backup(),
            KeyCode::Char('g') if self.tab == Tab::Settings => self.start_geo_update(),
            KeyCode::Char('u') if self.tab == Tab::Settings => {
                self.check_mihomo_update(false).await
            }
            KeyCode::Char('U') if self.tab == Tab::Settings => self.check_mihomo_update(true).await,
            KeyCode::Char('o') if self.tab == Tab::Settings => self.open_update_url(),
            _ => {}
        }
        Ok(false)
    }
}
