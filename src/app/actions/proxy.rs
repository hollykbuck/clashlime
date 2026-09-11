impl crate::app::App {
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
                self.refresh().await;
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
                self.refresh().await;
            }
            Err(error) => self.say(format!("Selection failed: {error}")),
        }
    }

    pub(crate) async fn delay_selected(&mut self) {
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
                self.refresh().await;
            }
            Err(error) => self.say(format!("Close failed: {error}")),
        }
    }

    pub(crate) async fn close_all(&mut self) {
        match self.api.close_connection(None).await {
            Ok(()) => {
                self.say("All connections closed");
                self.refresh().await;
            }
            Err(error) => self.say(format!("Close failed: {error}")),
        }
    }
}
