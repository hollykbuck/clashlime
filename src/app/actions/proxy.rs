use std::collections::{HashMap, HashSet};

/// Events streamed back from the background delay-test task.
pub enum DelayEvent {
    Done((String, u32)),
    GroupDone {
        group: String,
        members: Vec<String>,
        delays: HashMap<String, u32>,
    },
    Failed(String),
}

/// Events streamed back from the background provider health-check task.
pub enum ProviderCheckEvent {
    Done((usize, usize, Vec<String>)),
}

impl crate::app::App {
    /// Handle a key while the `,` sort prefix is armed. Returns `None`
    /// when the key was not a sort choice, so the caller can fall
    /// through to its normal handling. Esc never reaches here: the
    /// caller handles it so it cannot double as "clear filter".
    pub(crate) fn handle_node_sort_prefix_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<()> {
        use crate::app::NodeSort;
        use crossterm::event::KeyCode;
        self.ui.node_sort_prefix = false;
        self.ui.node_sort = match key.code {
            KeyCode::Char('d') => NodeSort::DelayAsc,
            KeyCode::Char('D') => NodeSort::DelayDesc,
            KeyCode::Char('n' | 'N') => NodeSort::Name,
            KeyCode::Char('o' | 'O') => NodeSort::Original,
            _ => return None,
        };
        // The cursor addresses a row, not a name: keep it in range and
        // let the caller re-clamp against the new ordering.
        self.ui.node_index = 0;
        self.say(format!("Nodes sorted by {}", self.ui.node_sort.label()));
        Some(())
    }

    /// Start a delay test for the focused pane: the selected node when
    /// the node list owns the focus, otherwise every node in the group.
    pub(crate) fn start_delay_selected(&mut self) {
        if self.tasks.delay.running() {
            self.say("Delay test already in progress");
            return;
        }
        if self.ui.node_focus {
            self.start_node_delay();
        } else {
            self.start_group_delay();
        }
    }

    fn start_node_delay(&mut self) {
        let view = crate::ui::tabs::proxies::filtered_nodes(self);
        let node = view
            .get(self.ui.node_index)
            .map(|(_, node)| (*node).clone());
        let Some(node) = node else { return };
        self.say(format!("Testing {node}…"));
        let provider = self.provider_for_node(&node);
        let api = self.api.clone();
        let url = self.config.delay_test_url.clone();
        self.tasks.delay.spawn(|tx| async move {
            let result = match provider {
                Some(provider) => api.test_provider_delay(&provider, &node, &url).await,
                None => api.test_delay(&node, &url).await,
            };
            match result {
                Ok(delay) => {
                    let _ = tx.send(DelayEvent::Done((node, delay)));
                }
                Err(error) => {
                    let _ = tx.send(DelayEvent::Failed(format!("{error:#}")));
                }
            }
        });
    }

    /// Probe every member of the focused group through
    /// `GET /group/{name}/delay`. The core runs the probes concurrently
    /// and returns only the nodes that answered; the group's member list
    /// fills the rest in as failures.
    fn start_group_delay(&mut self) {
        let Some((group, proxy)) = self.selected_group() else {
            return;
        };
        let (group, members) = (group.clone(), proxy.all.clone());
        if members.is_empty() {
            self.say(format!("{group} has no nodes"));
            return;
        }
        self.say(format!("Testing group {group} ({} nodes)…", members.len()));
        let api = self.api.clone();
        let url = self.config.delay_test_url.clone();
        self.tasks.delay.spawn(|tx| async move {
            match api.test_group_delay(&group, &url).await {
                Ok(delays) => {
                    let _ = tx.send(DelayEvent::GroupDone {
                        group,
                        members,
                        delays,
                    });
                }
                Err(error) => {
                    let _ = tx.send(DelayEvent::Failed(format!("{error:#}")));
                }
            }
        });
    }

    /// The proxy provider owning `node`, when the node comes from a
    /// subscription rather than an inline `proxies:` entry.
    ///
    /// Provider nodes never appear in `GET /proxies`, so probing them
    /// through `/proxies/{name}/delay` answers 404; the provider-scoped
    /// health-check route is the only way to address them.
    fn provider_for_node(&self, node: &str) -> Option<String> {
        if self.data.snapshot.proxies.proxies.contains_key(node) {
            return None;
        }
        self.data
            .snapshot
            .proxy_providers
            .iter()
            .find(|provider| {
                !provider.is_compatible() && provider.proxies.iter().any(|proxy| proxy.name == node)
            })
            .map(|provider| provider.name.clone())
    }

    /// Reflect a finished probe in the cached snapshot. Inline nodes live
    /// in `/proxies`; provider nodes are absent from it and only exist in
    /// the provider snapshot, so both are checked.
    fn record_delay(&mut self, node: &str, delay: u32) {
        if let Some(proxy) = self.data.snapshot.proxies.proxies.get_mut(node) {
            proxy.alive = Some(delay > 0);
            push_history(&mut proxy.history, delay);
            return;
        }
        for provider in &mut self.data.snapshot.proxy_providers {
            let Some(proxy) = provider.proxies.iter_mut().find(|proxy| proxy.name == node) else {
                continue;
            };
            proxy.alive = delay > 0;
            push_history(&mut proxy.history, delay);
            return;
        }
    }

    /// Apply a group probe: the core omits nodes that timed out, so the
    /// member list turns the missing names into `0 ms` failures.
    fn record_group_delays(
        &mut self,
        group: &str,
        members: &[String],
        delays: &HashMap<String, u32>,
    ) {
        let results = group_delay_results(members, delays);
        if results.is_empty() {
            self.say(format!("{group}: no nodes to test"));
            return;
        }
        let (mut ok, mut failed) = (0, 0);
        for (node, delay) in &results {
            if *delay > 0 {
                ok += 1;
            } else {
                failed += 1;
            }
            self.record_delay(node, *delay);
        }
        if failed == 0 {
            self.say(format!("{group}: {ok} nodes checked"));
        } else {
            self.say(format!("{group}: {ok} ok, {failed} failed"));
        }
    }

    pub(crate) fn poll_delay_events(&mut self) {
        let drain = self.tasks.delay.drain();
        for event in drain.events {
            self.handle_delay_event(event);
        }
        // A panic is reported even when a terminal event already arrived.
        if let Some(panic) = drain.panic {
            self.say(format!("Delay test failed: {panic}"));
        } else if drain.disconnected && !drain.terminal {
            self.say("Delay test failed: background task ended unexpectedly");
        }
    }

    fn handle_delay_event(&mut self, event: DelayEvent) {
        self.tasks.delay.stop();
        match event {
            DelayEvent::Done((node, delay)) => {
                self.record_delay(&node, delay);
                self.say(format!("{node}: {delay} ms"));
            }
            DelayEvent::GroupDone {
                group,
                members,
                delays,
            } => self.record_group_delays(&group, &members, &delays),
            DelayEvent::Failed(error) => {
                crate::logger::warn("app", &format!("delay test failed: {error}"));
                self.say(format!("Delay test failed: {error}"));
            }
        }
    }

    /// Cancel an in-flight delay test (Esc).
    pub(crate) fn cancel_delay_test(&mut self) {
        self.tasks.delay.stop();
        self.say("Delay test cancelled");
    }

    pub(crate) fn delay_running(&self) -> bool {
        self.tasks.delay.running()
    }

    /// Refresh every proxy provider (`PUT /providers/proxies/{name}`),
    /// then reload the slow snapshot so the panel shows new counts.
    ///
    /// Mirrors `update_rule_providers`, which is a plain sequential loop
    /// because each request is a local round-trip.
    pub(crate) async fn update_proxy_providers(&mut self) {
        let names = self.data.proxy_provider_order.clone();
        if names.is_empty() {
            self.say("No proxy providers: profile inlines all proxies");
            return;
        }
        let mut ok = 0;
        for name in &names {
            if self.api.update_proxy_provider(name).await.is_ok() {
                ok += 1;
            }
        }
        crate::logger::info(
            "app",
            &format!("proxy providers updated: {ok}/{}", names.len()),
        );
        self.say(format!("Proxy providers updated: {ok}/{}", names.len()));
        self.refresh_full().await;
    }

    /// Run each proxy provider's own health check
    /// (`GET /providers/proxies/{name}/healthcheck`).
    ///
    /// A provider's check probes every node it owns, so several providers
    /// take seconds; it runs as a background task and reports progress so
    /// the TUI keeps painting.
    pub(crate) fn start_provider_check(&mut self) {
        if self.tasks.provider_check.running() {
            self.say("Provider health check already in progress");
            return;
        }
        let names = self.data.proxy_provider_order.clone();
        if names.is_empty() {
            self.say("No proxy providers: profile inlines all proxies");
            return;
        }
        self.say(format!(
            "Checking {} provider{}…",
            names.len(),
            if names.len() == 1 { "" } else { "s" }
        ));
        let api = self.api.clone();
        self.tasks.provider_check.spawn(|tx| async move {
            let total = names.len();
            let mut ok = 0;
            let mut failed = Vec::new();
            for name in names {
                match api.healthcheck_proxy_provider(&name).await {
                    Ok(()) => ok += 1,
                    Err(error) => {
                        crate::logger::warn(
                            "app",
                            &format!("provider health check failed for {name}: {error:#}"),
                        );
                        failed.push(name);
                    }
                }
            }
            let _ = tx.send(ProviderCheckEvent::Done((ok, total, failed)));
        });
    }

    pub(crate) async fn poll_provider_check_events(&mut self) {
        let drain = self.tasks.provider_check.drain();
        for event in drain.events {
            self.handle_provider_check_event(event).await;
        }
        if let Some(panic) = drain.panic {
            self.say(format!("Provider health check failed: {panic}"));
        } else if drain.disconnected && !drain.terminal {
            self.say("Provider health check failed: background task ended unexpectedly");
        }
    }

    async fn handle_provider_check_event(&mut self, event: ProviderCheckEvent) {
        self.tasks.provider_check.stop();
        match event {
            ProviderCheckEvent::Done((ok, total, failed)) if failed.is_empty() => {
                self.say(format!("Providers checked: {ok}/{total}"));
                // The panel's alive counts live in the slow snapshot, so
                // force one now instead of leaving stale numbers for 30s.
                self.refresh_full().await;
            }
            ProviderCheckEvent::Done((ok, total, failed)) => {
                self.say(format!(
                    "Providers checked: {ok}/{total}; failed {}",
                    failed.join(", ")
                ));
            }
        }
    }

    /// Cancel an in-flight provider health check (Esc).
    pub(crate) fn cancel_provider_check(&mut self) {
        self.tasks.provider_check.stop();
        self.say("Provider health check cancelled");
    }

    pub(crate) fn provider_check_running(&self) -> bool {
        self.tasks.provider_check.running()
    }
    /// Routing modes in menu order, with one-line explanations.
    pub(crate) const MODES: [(&'static str, &'static str); 3] = [
        ("rule", "Match traffic against policies (default)"),
        ("global", "Send everything through the proxy"),
        ("direct", "Send everything direct, no proxy"),
    ];

    /// Open the routing-mode menu (`m`); the index starts at the current mode.
    pub(crate) fn open_mode_menu(&mut self) {
        self.ui.mode_menu_index = Self::MODES
            .iter()
            .position(|(mode, _)| self.data.snapshot.config.mode.eq_ignore_ascii_case(mode))
            .unwrap_or(0);
        self.ui.mode_menu = true;
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
            self.ui.mode_menu_index = index;
            self.ui.mode_menu = false;
            self.set_mode(Self::MODES[index].0).await;
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('m') => self.ui.mode_menu = false,
            KeyCode::Down | KeyCode::Char('j') => {
                self.ui.mode_menu_index = (self.ui.mode_menu_index + 1) % Self::MODES.len();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.ui.mode_menu_index =
                    (self.ui.mode_menu_index + Self::MODES.len() - 1) % Self::MODES.len();
            }
            KeyCode::Enter => {
                let mode = Self::MODES[self.ui.mode_menu_index].0;
                self.ui.mode_menu = false;
                self.set_mode(mode).await;
            }
            _ => {}
        }
    }

    pub(crate) async fn set_mode(&mut self, mode: &str) {
        if self.data.snapshot.config.mode.eq_ignore_ascii_case(mode) {
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
            view.get(self.ui.node_index).map(|(_, node)| {
                (
                    name.clone(),
                    (*node).clone(),
                    group.is_selector(),
                    group.supports_fixed(),
                    group.fixed.clone(),
                )
            })
        });
        let Some((group, node, selector, supports_fixed, fixed)) = selected else {
            return;
        };
        if !selector && !supports_fixed {
            self.say(format!("{group} is managed automatically"));
            return;
        }
        // URLTest/Fallback pin the selected node; pressing Enter on the
        // pinned node again releases it back to automatic selection.
        if supports_fixed && fixed == node {
            match self.api.unselect_proxy(&group).await {
                Ok(()) => {
                    let message = match self.data.profiles.clear_selection(&group) {
                        Ok(()) => format!("{group} unfixed"),
                        Err(error) => {
                            format!("{group} unfixed; selection was not saved: {error}")
                        }
                    };
                    self.say(message);
                    self.refresh_full().await;
                }
                Err(error) => self.say(format!("Unfix failed: {error}")),
            }
            return;
        }
        match self.api.select_proxy(&group, &node).await {
            Ok(()) => {
                let label = if supports_fixed {
                    format!("{group} fixed")
                } else {
                    group.clone()
                };
                let message = match self.data.profiles.record_selection(&group, &node) {
                    Ok(()) => format!("{label} → {node}"),
                    Err(error) => {
                        format!("{label} → {node}; selection was not saved: {error}")
                    }
                };
                self.say(message);
                self.refresh_full().await;
            }
            Err(error) => self.say(format!("Selection failed: {error}")),
        }
    }

    pub(crate) async fn close_selected(&mut self) {
        let id = crate::ui::tabs::connections::sorted_connections(self)
            .get(self.ui.connection_index)
            .map(|c| c.id.clone());
        let Some(id) = id else { return };
        if self
            .data
            .closed_connections
            .iter()
            .any(|connection| connection.id == id)
        {
            self.say("Connection already closed");
            return;
        }
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

/// Append one probe result, keeping the same short history window the
/// core uses so the cached snapshot does not grow without bound.
fn push_history(history: &mut Vec<crate::api::DelayHistory>, delay: u32) {
    history.push(crate::api::DelayHistory { delay });
    if history.len() > 10 {
        history.remove(0);
    }
}

/// Combine a group's declared members with the core's probe map. The
/// endpoint only returns nodes that answered, so missing members are
/// failures (`0 ms`); extra names returned by the core are kept too.
fn group_delay_results(members: &[String], delays: &HashMap<String, u32>) -> Vec<(String, u32)> {
    let mut seen = HashSet::new();
    let mut results = Vec::with_capacity(members.len());
    for member in members {
        if seen.insert(member.as_str()) {
            results.push((member.clone(), delays.get(member).copied().unwrap_or(0)));
        }
    }
    for (name, delay) in delays {
        if seen.insert(name.as_str()) {
            results.push((name.clone(), *delay));
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::{group_delay_results, push_history};
    use std::collections::HashMap;

    #[test]
    fn group_results_turn_missing_members_into_failures() {
        let members = vec!["a".to_string(), "b".to_string()];
        let delays = HashMap::from([("a".to_string(), 120), ("c".to_string(), 80)]);
        assert_eq!(
            group_delay_results(&members, &delays),
            vec![
                ("a".to_string(), 120),
                ("b".to_string(), 0),
                ("c".to_string(), 80),
            ]
        );
    }

    #[test]
    fn push_history_keeps_the_last_ten_probes() {
        let mut history = Vec::new();
        for delay in 1..=11 {
            push_history(&mut history, delay);
        }
        assert_eq!(history.len(), 10);
        assert_eq!(history.first().map(|entry| entry.delay), Some(2));
        assert_eq!(history.last().map(|entry| entry.delay), Some(11));
    }
}
