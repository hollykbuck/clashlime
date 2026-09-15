/// Events streamed back from the background delay-test task.
pub enum DelayEvent {
    Done((String, u32)),
    Failed(String),
}

impl crate::app::App {
    /// Events streamed back from the background delay-test task.
    pub(crate) fn start_delay_selected(&mut self) {
        if self.delay_task.running() {
            self.say("Delay test already in progress");
            return;
        }
        let view = crate::ui::tabs::proxies::filtered_nodes(self);
        let node = view
            .get(self.node_index)
            .map(|(_, node)| (*node).clone());
        let Some(node) = node else { return };
        self.say(format!("Testing {node}…"));
        let api = self.api.clone();
        let url = self.config.delay_test_url.clone();
        self.delay_task.spawn(|tx| async move {
            match api.test_delay(&node, &url).await {
                Ok(delay) => {
                    let _ = tx.send(DelayEvent::Done((node, delay)));
                }
                Err(error) => {
                    let _ = tx.send(DelayEvent::Failed(format!("{error:#}")));
                }
            }
        });
    }

    pub(crate) fn poll_delay_events(&mut self) {
        let drain = self.delay_task.drain();
        for event in drain.events {
            self.handle_delay_event(event);
        }
        if drain.disconnected {
            self.say("Delay test failed: background task ended unexpectedly");
        }
    }

    fn handle_delay_event(&mut self, event: DelayEvent) {
        self.delay_task.stop();
        match event {
            DelayEvent::Done((node, delay)) => self.say(format!("{node}: {delay} ms")),
            DelayEvent::Failed(error) => {
                crate::logger::warn("app", &format!("delay test failed: {error}"));
                self.say(format!("Delay test failed: {error}"));
            }
        }
    }

    /// Cancel an in-flight delay test (Esc).
    pub(crate) fn cancel_delay_test(&mut self) {
        self.delay_task.stop();
        self.say("Delay test cancelled");
    }

    pub(crate) fn delay_running(&self) -> bool {
        self.delay_task.running()
    }
    /// Routing modes in menu order, with one-line explanations.
    pub(crate) const MODES: [(&'static str, &'static str); 3] = [
        ("rule", "Match traffic against policies (default)"),
        ("global", "Send everything through the proxy"),
        ("direct", "Send everything direct, no proxy"),
    ];

    /// Open the routing-mode menu (`m`); the index starts at the current mode.
    pub(crate) fn open_mode_menu(&mut self) {
        self.mode_menu_index = Self::MODES
            .iter()
            .position(|(mode, _)| self.snapshot.config.mode.eq_ignore_ascii_case(mode))
            .unwrap_or(0);
        self.mode_menu = true;
    }

    /// Keys while the mode menu is open. Single-key `r/g/d` (or `1/2/3`)
    /// selects immediately; `j/k` + Enter confirms; Esc closes.
    pub(crate) async fn handle_mode_menu_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        let instant = match key.code {
            KeyCode::Char('r') | KeyCode::Char('R') | KeyCode::Char('1') => Some(0),
            KeyCode::Char('g') | KeyCode::Char('G') | KeyCode::Char('2') => Some(1),
            KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Char('3') => Some(2),
            _ => None,
        };
        if let Some(index) = instant {
            self.mode_menu_index = index;
            self.mode_menu = false;
            self.set_mode(Self::MODES[index].0).await;
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('m') => self.mode_menu = false,
            KeyCode::Down | KeyCode::Char('j') => {
                self.mode_menu_index = (self.mode_menu_index + 1) % Self::MODES.len();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.mode_menu_index =
                    (self.mode_menu_index + Self::MODES.len() - 1) % Self::MODES.len();
            }
            KeyCode::Enter => {
                let mode = Self::MODES[self.mode_menu_index].0;
                self.mode_menu = false;
                self.set_mode(mode).await;
            }
            _ => {}
        }
    }

    pub(crate) async fn set_mode(&mut self, mode: &str) {
        if self.snapshot.config.mode.eq_ignore_ascii_case(mode) {
            return;
        }
        match self.api.set_mode(mode).await {
            Ok(()) => {
                self.say(format!("Mode changed to {mode}"));
                self.refresh_full().await;
            }
            Err(error) => self.say(format!("Mode change failed: {error}")),
        }
    }

    pub(crate) async fn select_node(&mut self) {
        let view = crate::ui::tabs::proxies::filtered_nodes(self);
        let selected = self.selected_group().and_then(|(name, group)| {
            view.get(self.node_index).map(|(_, node)| {
                (
                    name.clone(),
                    (*node).clone(),
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
                self.refresh_full().await;
            }
            Err(error) => self.say(format!("Selection failed: {error}")),
        }
    }

    pub(crate) async fn close_selected(&mut self) {
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
                self.refresh_full().await;
            }
            Err(error) => self.say(format!("Close failed: {error}")),
        }
    }

    pub(crate) async fn close_all(&mut self) {
        match self.api.close_connection(None).await {
            Ok(()) => {
                self.say("All connections closed");
                self.refresh_full().await;
            }
            Err(error) => self.say(format!("Close failed: {error}")),
        }
    }
}
