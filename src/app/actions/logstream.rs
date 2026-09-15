//! Live core logs via `GET /logs` (endless structured-JSON stream).
//!
//! A persistent background task owns the connection and reconnects with
//! backoff; parsed lines arrive as events. File tails stay only as the
//! startup backlog, so the Logs tab goes from 1.5s polling to real time.

use crate::api::MihomoClient;

/// Events streamed back from the background log task.
pub enum CoreLogEvent {
    /// The stream is (re)connected; file backlog no longer needed.
    Connected,
    /// Connection lost, retrying with backoff.
    Retrying,
    /// One parsed core log line (already in `[time] LEVEL msg` shape).
    Line { text: String },
}

impl crate::app::App {
    fn log_stream_id(&self) -> String {
        format!("{}|{}", self.config.controller, self.config.secret)
    }

    /// Keep the stream task aligned with connection state: run while the
    /// core is reachable, restart when controller/secret change.
    pub(crate) fn maintain_log_stream(&mut self) {
        let key = self.log_stream_id();
        let running = self.log_task.running() && self.log_stream_key == key;
        if self.online && !running {
            self.start_log_stream(key);
        } else if !self.online && self.log_task.running() {
            self.stop_log_stream();
        }
    }

    fn start_log_stream(&mut self, key: String) {
        crate::logger::debug("logstream", "starting stream");
        let client = self.api.clone();
        self.log_task.spawn(|tx| async move {
            run_log_stream(client, tx).await;
        });
        self.log_stream_key = key;
        // Optimistic: mihomo withholds /logs headers until the first event,
        // so `send()` idles on a healthy connection. A real failure surfaces
        // as `Retrying` and clears this.
        self.log_stream_live = true;
    }

    /// Stop the stream (offline or shutdown).
    pub(crate) fn stop_log_stream(&mut self) {
        self.log_task.stop();
        self.log_stream_live = false;
    }

    pub(crate) fn poll_log_events(&mut self) {
        let drain = self.log_task.drain();
        for event in drain.events {
            self.handle_log_event(event);
        }
        if drain.disconnected {
            self.log_stream_live = false;
        }
    }

    fn handle_log_event(&mut self, event: CoreLogEvent) {
        use crate::app::{LogEntry, LogSource};
        match event {
            CoreLogEvent::Connected => {
                self.log_stream_live = true;
                self.log_backlog_loaded = true;
            }
            CoreLogEvent::Retrying => {
                crate::logger::debug("logstream", "stream retrying");
                self.log_stream_live = false;
            }
            CoreLogEvent::Line { text } => {
                self.logs.push(LogEntry {
                    source: LogSource::Core,
                    text,
                });
                if self.logs.len() > 800 {
                    let drain = self.logs.len() - 800;
                    self.logs.drain(0..drain);
                }
            }
        }
    }
}

/// Connect, stream lines forever, reconnect on any failure. Backoff
/// resets after a connection that actually delivered lines.
async fn run_log_stream(
    client: MihomoClient,
    tx: tokio::sync::mpsc::UnboundedSender<CoreLogEvent>,
) {
    let mut backoff_secs = 1;
    loop {
        let delivered = match pump_log_stream(&client, &tx).await {
            Ok(delivered) => delivered,
            Err(error) => {
                crate::logger::debug("logstream", &format!("pump failed: {error:#}"));
                false
            }
        };
        let _ = tx.send(CoreLogEvent::Retrying);
        backoff_secs = if delivered {
            1
        } else {
            (backoff_secs * 2).min(30)
        };
        tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
    }
}

/// One connection lifetime: parse newline-delimited JSON until EOF/error.
async fn pump_log_stream(
    client: &MihomoClient,
    tx: &tokio::sync::mpsc::UnboundedSender<CoreLogEvent>,
) -> anyhow::Result<bool> {
    let mut response = client
        .log_stream_request()?
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("log stream connect failed: {e}"))?;
    if !response.status().is_success() {
        anyhow::bail!("log stream rejected: {}", response.status());
    }
    let _ = tx.send(CoreLogEvent::Connected);
    let mut delivered = false;
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|e| anyhow::anyhow!("log stream read failed: {e}"))?;
        let Some(chunk) = chunk else { break };
        pending.extend_from_slice(&chunk);
        while let Some(pos) = pending.iter().position(|b| *b == b'\n') {
            let raw: Vec<u8> = pending.drain(..=pos).collect();
            let line = raw
                .iter()
                .filter(|b| **b != b'\r')
                .copied()
                .collect::<Vec<_>>();
            if line.iter().all(|b| b.is_ascii_whitespace()) {
                continue;
            }
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&line)
                && let Some((_, text)) = MihomoClient::format_log_line(&value)
            {
                if tx.send(CoreLogEvent::Line { text }).is_err() {
                    return Ok(delivered);
                }
                delivered = true;
            }
        }
    }
    Ok(delivered)
}
