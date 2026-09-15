//! Live core memory via `GET /memory` (endless JSON stream, one object
//! per second, opening with an `inuse: 0` sentinel).
//!
//! A persistent background task owns the connection and reconnects with
//! backoff; the UI keeps only the latest sample. This replaces polling
//! the stream on the 30s slow-refresh path, which either returned the
//! sentinel forever (first line only) or blocked the tick for seconds.

use crate::api::{MemoryInfo, MihomoClient};

impl crate::app::App {
    fn mem_stream_id(&self) -> String {
        format!("{}|{}", self.config.controller, self.config.secret)
    }

    /// Keep the stream task aligned with connection state: run while the
    /// core is reachable, restart when controller/secret change.
    pub(crate) fn maintain_mem_stream(&mut self) {
        let key = self.mem_stream_id();
        let running = self.mem_task.running() && self.mem_stream_key == key;
        if self.online && !running {
            self.start_mem_stream(key);
        } else if !self.online && self.mem_task.running() {
            self.stop_mem_stream();
        }
    }

    fn start_mem_stream(&mut self, key: String) {
        crate::logger::debug("memstream", "starting stream");
        let client = self.api.clone();
        self.mem_task.spawn(|tx| async move {
            run_mem_stream(client, tx).await;
        });
        self.mem_stream_key = key;
    }

    /// Stop the stream (offline or shutdown).
    pub(crate) fn stop_mem_stream(&mut self) {
        self.mem_task.stop();
    }

    /// Drain to the latest sample; the sidebar renders whatever is current.
    pub(crate) fn poll_mem_events(&mut self) {
        for sample in self.mem_task.drain().events {
            self.snapshot.memory = Some(sample);
        }
    }
}

/// Connect, stream samples forever, reconnect on any failure. Backoff
/// resets after a connection that actually delivered a real sample.
async fn run_mem_stream(
    client: MihomoClient,
    tx: tokio::sync::mpsc::UnboundedSender<MemoryInfo>,
) {
    let mut backoff_secs = 1;
    loop {
        let delivered = match pump_mem_stream(&client, &tx).await {
            Ok(delivered) => delivered,
            Err(error) => {
                crate::logger::debug("memstream", &format!("pump failed: {error:#}"));
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

/// One connection lifetime: forward every nonzero sample until EOF/error.
/// The `inuse: 0` opener is a sentinel, never displayed.
async fn pump_mem_stream(
    client: &MihomoClient,
    tx: &tokio::sync::mpsc::UnboundedSender<MemoryInfo>,
) -> anyhow::Result<bool> {
    let mut response = client
        .memory_stream_request()?
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("memory stream connect failed: {e}"))?;
    if !response.status().is_success() {
        anyhow::bail!("memory stream rejected: {}", response.status());
    }
    let mut delivered = false;
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|e| anyhow::anyhow!("memory stream read failed: {e}"))?;
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
            let Ok(sample) = serde_json::from_slice::<MemoryInfo>(&line) else {
                continue;
            };
            if sample.inuse == 0 {
                continue;
            }
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

    /// Feed the pump a sentinel + real sample, then EOF: only the real
    /// sample is forwarded and the pump reports delivery.
    #[tokio::test]
    async fn pump_skips_sentinel_and_forwards_sample() {
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
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"inuse\":0,\"oslimit\":0}\n{\"inuse\":73007104,\"oslimit\":0}\n",
                )
                .await
                .unwrap();
            let _ = stream.shutdown().await;
        });
        let client =
            MihomoClient::new(&format!("http://127.0.0.1:{port}"), String::new()).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<MemoryInfo>();
        let delivered = pump_mem_stream(&client, &tx).await.unwrap();
        assert!(delivered);
        let sample = rx.try_recv().unwrap();
        assert_eq!(sample.inuse, 73007104);
        // Sentinel never forwarded: exactly one sample queued.
        assert!(rx.try_recv().is_err());
        server.abort();
    }
}
