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
        // First frame before any network/IPC: `refresh_full` awaits the
        // Mihomo API + supervisor socket and would otherwise leave a black
        // screen for seconds when the core/daemon is down.
        {
            let mut mouse_regions = Vec::new();
            terminal.draw(|frame| mouse_regions = ui::draw(frame, self))?;
            self.mouse_regions = mouse_regions;
        }
        self.refresh_full().await;
        let mut events = EventStream::new();
        let mut tick = time::interval(self.config.refresh_interval());
        loop {
            self.poll_core_download_events().await;
            self.poll_geo_events();
            self.poll_import_events().await;
            self.poll_profile_events().await;
            self.poll_update_check_events();
            self.poll_delay_events();
            self.poll_log_events();
            self.poll_mem_events();
            self.poll_traffic_events();
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
            Some(InputMode::ImportProfile) => self.input_insert_str(text.trim()),
            Some(_) if !matches!(self.input, Some(InputMode::RestoreBackup(_))) => {
                self.input_insert_str(&text);
            }
            _ => {}
        }
        self.clamp_input_cursor();
    }

    pub(crate) async fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.input.is_some() {
            return;
        }
        // Clicking anywhere dismisses the mode menu instead of hitting
        // whatever sits behind the modal.
        if self.mode_menu {
            self.mode_menu = false;
            return;
        }
        // Same for the profile update editor: edits save immediately,
        // so dismissing never loses anything.
        if self.profile_editor {
            self.profile_editor = false;
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
                // The wheel steps the cursor like j/k from its current spot.
                // Never focus the hovered row first: that teleported the
                // proxy group cursor across the whole list in one tick.
                if self.tab == Tab::Logs {
                    self.scroll_logs(delta);
                } else {
                    self.move_selection(delta);
                }
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
            ui::HitTarget::Tab(tab) => self.open_tab(tab).await,
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
        if self.mode_menu {
            self.handle_mode_menu_key(key).await;
            return Ok(false);
        }
        if self.profile_editor {
            self.handle_profile_editor_key(key);
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
            if self.core_download_rx.is_some() {
                self.cancel_core_download();
            }
            if self.delay_running() {
                self.cancel_delay_test();
            }
            return Ok(false);
        }
        if let Some(tab) = Self::tab_shortcut(&key.code) {
            self.open_tab(tab).await;
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
                let initial = self.log_query.clone();
                self.set_input(initial);
            }
            KeyCode::Char('/') if self.tab == Tab::Rules => {
                self.input = Some(InputMode::SearchRules);
                let initial = self.rule_query.clone();
                self.set_input(initial);
            }
            KeyCode::Char('/') if self.tab == Tab::Proxies => {
                self.input = Some(InputMode::SearchNodes);
                let initial = self.node_query.clone();
                self.set_input(initial);
                self.node_focus = true;
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
            KeyCode::Enter if self.tab == Tab::Rules => self.open_rule_detail(),
            KeyCode::Esc if self.tab == Tab::Logs && self.log_detail.is_some() => {
                self.close_log_detail()
            }
            KeyCode::Esc if self.tab == Tab::Rules && self.log_detail.is_some() => {
                self.close_log_detail()
            }
            KeyCode::Esc if self.tab == Tab::Rules && !self.rule_query.is_empty() => {
                self.rule_query.clear();
                self.rule_index = 0;
                self.say("Rule search cleared");
            }
            KeyCode::Esc if self.tab == Tab::Proxies && !self.node_query.is_empty() => {
                self.node_query.clear();
                self.node_index = 0;
                self.say("Node search cleared");
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
            KeyCode::Char('m') => self.open_mode_menu(),
            KeyCode::Char('a') if self.tab == Tab::Profiles => {
                self.input = Some(InputMode::ImportProfile);
                self.clear_input();
            }
            KeyCode::Char('u') if self.tab == Tab::Profiles => self.start_update_profile(),
            KeyCode::Char('e') if self.tab == Tab::Profiles => self.open_profile_editor(),
            KeyCode::Char('u') if self.tab == Tab::Rules => self.update_rule_providers().await,
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
                self.setting_index = self.section_cursor[crate::app::SettingSection::Geo.index()]
                    .min(
                        crate::app::SettingSection::Geo
                            .row_count()
                            .saturating_sub(1),
                    );
            }
            KeyCode::Char('u') if self.tab == Tab::Settings => {
                self.start_mihomo_update_check(false)
            }
            KeyCode::Char('U') if self.tab == Tab::Settings => self.start_mihomo_update_check(true),
            KeyCode::Char('i') if self.tab == Tab::Settings => self.start_core_upgrade(),
            KeyCode::Char('I') if self.tab == Tab::Settings => self.start_core_reinstall(),
            KeyCode::Char('o') if self.tab == Tab::Settings => self.open_update_url(),
            _ => {}
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{LogSource, SettingSection, StatusKind};
    use super::*;
    use crate::api::{MihomoClient, Proxy};
    use crate::config::Config;
    use crate::core::SupervisorState;
    use crate::profiles::Profiles;
    use crate::theme::Theme;
    use crate::ui::{HitRegion, HitTarget};
    use ratatui::layout::Rect;
    use std::collections::HashMap;

    /// Minimal App with 10 proxy groups; no env or filesystem involved.
    fn wheel_test_app() -> super::super::App {
        let mut proxies = HashMap::new();
        for i in 0..10 {
            proxies.insert(
                format!("g{i:02}"),
                Proxy {
                    kind: "Selector".into(),
                    now: "n".into(),
                    all: vec!["n".into()],
                    ..Default::default()
                },
            );
        }
        let mut app = super::super::App {
            config: Config::default(),
            api: MihomoClient::new("http://127.0.0.1:9090", String::new()).unwrap(),
            snapshot: Default::default(),
            profiles: Profiles::default(),
            proxy_group_order: Vec::new(),
            theme: Theme::default(),
            supervisor: SupervisorState::default(),
            logs: Vec::new(),
            log_source: LogSource::All,
            log_scroll: 0,
            log_follow: true,
            log_level_filter: None,
            log_query: String::new(),
            log_height: 0,
            log_hscroll: 0,
            log_detail: None,
            tab: Tab::Proxies,
            group_index: 1,
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
            status: String::new(),
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
            core_missing: None,
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
            mihomo_update: Default::default(),
            mouse_regions: Vec::new(),
            last_click: None,
        };
        app.snapshot.proxies.proxies = proxies;
        app
    }

    fn wheel(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        }
    }

    /// The wheel must step the cursor like j/k from its current position,
    /// even when hovering a far-away row: no teleport to the pointer.
    #[tokio::test]
    async fn wheel_steps_from_cursor_instead_of_teleporting() {
        let mut app = wheel_test_app();
        assert_eq!(app.proxy_groups().len(), 10);
        // Pointer sits on group 8 while the cursor is on group 1.
        app.mouse_regions = vec![HitRegion {
            area: Rect::new(0, 8, 40, 1),
            target: HitTarget::ProxyGroup(8),
        }];
        app.handle_mouse(wheel(MouseEventKind::ScrollDown, 5, 8))
            .await;
        assert_eq!(app.group_index, 2);
        app.handle_mouse(wheel(MouseEventKind::ScrollUp, 5, 8))
            .await;
        assert_eq!(app.group_index, 1);
    }

    /// Wheeling over a node row must not steal group focus either.
    #[tokio::test]
    async fn wheel_over_node_keeps_group_focus() {
        let mut app = wheel_test_app();
        app.mouse_regions = vec![HitRegion {
            area: Rect::new(0, 12, 40, 1),
            target: HitTarget::ProxyNode(3),
        }];
        app.handle_mouse(wheel(MouseEventKind::ScrollDown, 5, 12))
            .await;
        assert!(!app.node_focus);
        assert_eq!((app.group_index, app.node_index), (2, 0));
    }

    /// Cancelling a core upgrade (Settings Esc) resets the upgrade
    /// markers so the panel stops showing progress and `i` can retry.
    #[test]
    fn cancel_core_upgrade_resets_state() {
        let mut app = wheel_test_app();
        app.core_upgrade = Some("v9.9.99".into());
        app.mihomo_update.download = Some((1024, Some(2048)));
        app.mihomo_update.message = "downloading v9.9.99…".into();
        app.cancel_core_download();
        assert!(app.core_upgrade.is_none());
        assert!(app.mihomo_update.download.is_none());
        assert_eq!(app.mihomo_update.message, "download cancelled");
    }

    /// Rule search filters across type/payload/policy and keeps the
    /// original indices so cursor and detail stay aligned.
    #[test]
    fn rule_search_filters_all_columns() {
        use crate::api::Rule;
        use crate::ui::tabs::rules::filtered_rules;
        let mut app = wheel_test_app();
        app.snapshot.rules.rules = vec![
            Rule {
                kind: "DomainSuffix".into(),
                payload: "google.com".into(),
                proxy: "Auto".into(),
                ..Default::default()
            },
            Rule {
                kind: "GeoIP".into(),
                payload: "cn".into(),
                proxy: "Direct".into(),
                ..Default::default()
            },
            Rule {
                kind: "Match".into(),
                proxy: "Final".into(),
                ..Default::default()
            },
        ];
        assert_eq!(filtered_rules(&app).len(), 3);
        app.rule_query = "auto".into();
        let view = filtered_rules(&app);
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].0, 0);
        app.rule_query = "GEOIP".into();
        assert_eq!(filtered_rules(&app).len(), 1);
        app.rule_query = "nope".into();
        assert!(filtered_rules(&app).is_empty());
    }

    /// Node search filters the selected group's names case-insensitively
    /// and keeps the original indices so delay tests and selection act on
    /// the right node.
    #[test]
    fn node_search_filters_selected_group() {
        use crate::ui::tabs::proxies::filtered_nodes;
        let mut app = wheel_test_app();
        app.group_index = 0;
        let group = app.snapshot.proxies.proxies.get_mut("g00").unwrap();
        group.all = vec!["alpha".into(), "Beta-node".into(), "gamma".into()];
        assert_eq!(filtered_nodes(&app).len(), 3);
        app.node_query = "BETA".into();
        let view = filtered_nodes(&app);
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].0, 1);
        assert_eq!(view[0].1.as_str(), "Beta-node");
        app.node_query = "nope".into();
        assert!(filtered_nodes(&app).is_empty());
    }

    fn menu_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    /// The mode menu opens on the current mode, wraps with j/k, picks
    /// instantly with r/g/d and closes with Esc/Enter.
    #[tokio::test]
    async fn mode_menu_selects_without_api() {
        let mut app = wheel_test_app();
        app.snapshot.config.mode = "global".into();
        app.open_mode_menu();
        assert!(app.mode_menu);
        assert_eq!(app.mode_menu_index, 1);
        app.handle_mode_menu_key(menu_key(KeyCode::Char('j'))).await;
        assert_eq!(app.mode_menu_index, 2);
        app.handle_mode_menu_key(menu_key(KeyCode::Char('j'))).await;
        assert_eq!(app.mode_menu_index, 0);
        app.handle_mode_menu_key(menu_key(KeyCode::Char('k'))).await;
        assert_eq!(app.mode_menu_index, 2);
        // Instant key: same-mode set is a no-op besides closing.
        app.handle_mode_menu_key(menu_key(KeyCode::Char('g'))).await;
        assert!(!app.mode_menu);
        assert_eq!(app.mode_menu_index, 1);
        // Esc just closes.
        app.open_mode_menu();
        app.handle_mode_menu_key(menu_key(KeyCode::Esc)).await;
        assert!(!app.mode_menu);
    }
}
