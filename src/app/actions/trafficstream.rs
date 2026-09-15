//! Live throughput via `GET /traffic` (endless JSON stream, one
//! `{up, down}` object per second already expressed as bytes per second).
//!
//! A persistent background task owns the connection and reconnects with
//! backoff; the UI keeps only the latest sample. This replaces differencing
//! the cumulative `/connections` totals on the refresh tick, which capped
//! resolution at the tick interval and re-fetched the (potentially large)
//! connections dump just to draw two numbers.

use crate::api::{MihomoClient, TrafficInfo};

impl crate::app::App {
    fn traffic_stream_id(&self) -> String {
        format!("{}|{}", self.config.controller, self.config.secret)
    }

    /// Keep the stream task aligned with connection state: run while the
    /// core is reachable, restart when controller/secret change.
    pub(crate) fn maintain_traffic_stream(&mut self) {
        let key = self.traffic_stream_id();
        let running = self.tasks.traffic.running() && self.tasks.traffic_stream_key == key;
        if self.data.online && !running {
            self.start_traffic_stream(key);
        } else if !self.data.online && self.tasks.traffic.running() {
            self.stop_traffic_stream();
        }
    }

    fn start_traffic_stream(&mut self, key: String) {
        crate::logger::debug("trafficstream", "starting stream");
        let client = self.api.clone();
        self.tasks.traffic.spawn(|tx| async move {
            run_traffic_stream(client, tx).await;
        });
        self.tasks.traffic_stream_key = key;
    }

    /// Stop the stream (offline or shutdown).
    pub(crate) fn stop_traffic_stream(&mut self) {
        self.tasks.traffic.stop();
    }

    /// Drain to the latest sample; the sidebar renders it as ↑/↓.
    pub(crate) fn poll_traffic_events(&mut self) {
        for sample in self.tasks.traffic.drain().events {
            self.data.speeds = (sample.up, sample.down);
        }
    }
}

/// Connect, stream samples forever, reconnect on any failure. Backoff
/// resets after a connection that actually delivered samples.
async fn run_traffic_stream(
    client: MihomoClient,
    tx: tokio::sync::mpsc::UnboundedSender<TrafficInfo>,
) {
    let mut backoff_secs = 1;
    loop {
        let delivered = match pump_traffic_stream(&client, &tx).await {
            Ok(delivered) => delivered,
            Err(error) => {
                crate::logger::debug("trafficstream", &format!("pump failed: {error:#}"));
                false
            }
        };
        backoff_secs = if delivered {
            1
        } else {
            (backoff_secs * 2).min(30)
        };
        tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
    }
}

/// One connection lifetime: forward every valid sample until EOF/error.
async fn pump_traffic_stream(
    client: &MihomoClient,
    tx: &tokio::sync::mpsc::UnboundedSender<TrafficInfo>,
) -> anyhow::Result<bool> {
    let mut response = client
        .traffic_stream_request()?
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("traffic stream connect failed: {e}"))?;
    if !response.status().is_success() {
        anyhow::bail!("traffic stream rejected: {}", response.status());
    }
    let mut delivered = false;
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|e| anyhow::anyhow!("traffic stream read failed: {e}"))?;
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
            let Ok(sample) = serde_json::from_slice::<TrafficInfo>(&line) else {
                continue;
            };
            if tx.send(sample).is_err() {
                return Ok(delivered);
            }
            delivered = true;
        }
    }
    Ok(delivered)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed the pump two samples, then EOF: both are forwarded in order.
    #[tokio::test]
    async fn pump_forwards_samples_in_order() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut head = vec![0u8; 1024];
            let _ = stream.read(&mut head).await;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"up\":12,\"down\":34}\n{\"up\":56,\"down\":78}\n",
                )
                .await
                .unwrap();
            let _ = stream.shutdown().await;
        });
        let client = MihomoClient::new(&format!("http://127.0.0.1:{port}"), String::new()).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TrafficInfo>();
        let delivered = pump_traffic_stream(&client, &tx).await.unwrap();
        assert!(delivered);
        let first = rx.try_recv().unwrap();
        assert_eq!((first.up, first.down), (12, 34));
        let second = rx.try_recv().unwrap();
        assert_eq!((second.up, second.down), (56, 78));
        server.abort();
    }
}
