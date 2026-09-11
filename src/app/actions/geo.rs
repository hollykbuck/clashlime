//! Geo database downloads run in a background task so slow networks
//! never freeze the TUI event loop (cf. the core-download pattern).

/// Events streamed back from the background geo-download task.
pub enum GeoEvent {
    Done(Vec<String>),
    Failed(String),
}

impl crate::app::App {
    /// Start ensuring geo files in the background. Returns immediately so
    /// the UI keeps painting; completion arrives via [`Self::poll_geo_events`].
    pub(crate) fn start_geo_update(&mut self) {
        if self.geo_task.is_some() {
            self.say("Geo update already in progress");
            return;
        }
        let mirror = crate::geo::effective_mirror(self.config.geo.mirror.as_deref());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<GeoEvent>();
        let handle = tokio::spawn(async move {
            match crate::geo::ensure_all(mirror.as_deref()).await {
                Ok(fetched) => {
                    let _ = tx.send(GeoEvent::Done(fetched));
                }
                Err(error) => {
                    let _ = tx.send(GeoEvent::Failed(format!("{error:#}")));
                }
            }
        });
        self.geo_task = Some(handle);
        self.geo_rx = Some(rx);
        self.say("Updating Geo data…");
        crate::logger::info("geo", "background geo update started");
    }

    pub(crate) fn poll_geo_events(&mut self) {
        while let Some(event) = self
            .geo_rx
            .as_mut()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.handle_geo_event(event);
        }
    }

    fn handle_geo_event(&mut self, event: GeoEvent) {
        match event {
            GeoEvent::Done(fetched) => {
                self.geo_rx = None;
                self.geo_task = None;
                if fetched.is_empty() {
                    self.say(format!("Geo data ready ({})", crate::geo::summary()));
                } else {
                    crate::logger::info("geo", &format!("updated: {}", fetched.join(", ")));
                    self.say(format!("Geo data updated: {}", fetched.join(", ")));
                }
            }
            GeoEvent::Failed(error) => {
                self.geo_rx = None;
                self.geo_task = None;
                crate::logger::warn("geo", &format!("update failed: {error}"));
                self.say(format!("Geo update failed: {error}"));
            }
        }
    }

    /// Cancel an in-flight geo download (Settings Esc).
    pub(crate) fn cancel_geo_update(&mut self) {
        if let Some(handle) = self.geo_task.take() {
            handle.abort();
        }
        self.geo_rx = None;
        self.say("Geo update cancelled");
    }

    pub(crate) fn geo_updating(&self) -> bool {
        self.geo_task.is_some()
    }
}
