//! Geo database downloads run in a background task so slow networks
//! never freeze the TUI event loop (cf. the core-download pattern).

use crate::app::backend::Capability;

/// Events streamed back from the background geo-download task.
pub enum GeoEvent {
    Done(Vec<String>),
    Failed(String),
}

impl crate::app::App {
    /// Start ensuring geo files in the background. Returns immediately so
    /// the UI keeps painting; completion arrives via [`Self::poll_geo_events`].
    pub(crate) fn start_geo_update(&mut self) {
        if !self.require(Capability::ManageGeo) {
            return;
        }        if self.tasks.geo.running() {
            self.say("Geo update already in progress");
            return;
        }
        let geo = self.config.geo.clone();
        self.tasks.geo.spawn(|tx| async move {
            match crate::geo::ensure_all(&geo).await {
                Ok(fetched) => {
                    let _ = tx.send(GeoEvent::Done(fetched));
                }
                Err(error) => {
                    let _ = tx.send(GeoEvent::Failed(format!("{error:#}")));
                }
            }
        });
        self.say("Updating Geo data…");
        crate::logger::info("geo", "background geo update started");
    }

    pub(crate) fn poll_geo_events(&mut self) {
        let drain = self.tasks.geo.drain();
        for event in drain.events {
            self.handle_geo_event(event);
        }
        if drain.disconnected {
            crate::logger::warn("geo", "background task ended unexpectedly");
            self.say("Geo update failed: background task ended unexpectedly");
        }
    }

    fn handle_geo_event(&mut self, event: GeoEvent) {
        match event {
            GeoEvent::Done(fetched) => {
                self.tasks.geo.stop();
                if fetched.is_empty() {
                    self.say(format!("Geo data ready ({})", crate::geo::summary()));
                } else {
                    crate::logger::info("geo", &format!("updated: {}", fetched.join(", ")));
                    self.say(format!("Geo data updated: {}", fetched.join(", ")));
                }
            }
            GeoEvent::Failed(error) => {
                self.tasks.geo.stop();
                crate::logger::warn("geo", &format!("update failed: {error}"));
                self.say(format!("Geo update failed: {error}"));
            }
        }
    }

    /// Cancel an in-flight geo download (Settings Esc).
    pub(crate) fn cancel_geo_update(&mut self) {
        self.tasks.geo.stop();
        self.say("Geo update cancelled");
    }

    pub(crate) fn geo_updating(&self) -> bool {
        self.tasks.geo.running()
    }
}
