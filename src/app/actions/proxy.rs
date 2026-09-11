/// Events streamed back from the background delay-test task.
pub enum DelayEvent {
    Done((String, u32)),
    Failed(String),
}

impl crate::app::App {
    /// Events streamed back from the background delay-test task.
    pub(crate) fn start_delay_selected(&mut self) {
        if self.delay_task.is_some() {
            self.say("Delay test already in progress");
            return;
        }
        let node = self
            .selected_group()
            .and_then(|(_, g)| g.all.get(self.node_index))
            .cloned();
        let Some(node) = node else { return };
        self.say(format!("Testing {node}…"));
        let api = self.api.clone();
        let url = self.config.delay_test_url.clone();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<DelayEvent>();
        let handle = tokio::spawn(async move {
            match api.test_delay(&node, &url).await {
                Ok(delay) => {
                    let _ = tx.send(DelayEvent::Done((node, delay)));
                }
                Err(error) => {
                    let _ = tx.send(DelayEvent::Failed(format!("{error:#}")));
                }
            }
        });
        self.delay_task = Some(handle);
        self.delay_rx = Some(rx);
    }

    pub(crate) fn poll_delay_events(&mut self) {
        use tokio::sync::mpsc::error::TryRecvError;
        loop {
            let next = self.delay_rx.as_mut().map(|rx| rx.try_recv());
            match next {
                Some(Ok(event)) => self.handle_delay_event(event),
                Some(Err(TryRecvError::Empty)) | None => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    self.delay_rx = None;
                    self.delay_task = None;
                    self.say("Delay test failed: background task ended unexpectedly");
                    break;
                }
            }
        }
    }

    fn handle_delay_event(&mut self, event: DelayEvent) {
        self.delay_rx = None;
        self.delay_task = None;
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
        if let Some(handle) = self.delay_task.take() {
            handle.abort();
        }
        self.delay_rx = None;
        self.say("Delay test cancelled");
    }

    pub(crate) fn delay_running(&self) -> bool {
        self.delay_task.is_some()
    }
    pub(crate) async fn cycle_mode(&mut self) {
        let mode = match self.snapshot.config.mode.to_ascii_lowercase().as_str() {
            "rule" => "global",
            "global" => "direct",
            _ => "rule",
        };
        self.set_mode(mode).await;
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
