//! One generic background-task slot for the TUI event loop.
//!
//! Step 2 of the `App` de-god-object refactor. Every background job in the
//! TUI used to be an `Option<JoinHandle>` + `Option<UnboundedReceiver<E>>`
//! pair with its own copy of spawn/poll/stop/disconnect handling (9 pairs).
//! [`BackgroundTask`] owns both halves; call sites keep only their event
//! type and their `handle_*` interpretation.

use std::future::Future;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

/// Result of [`BackgroundTask::drain`]: queued events plus whether the
/// sender went away (task died without a terminal event).
pub(crate) struct TaskDrain<E> {
    pub events: Vec<E>,
    pub disconnected: bool,
}

/// A spawned task plus its event channel. At most one run at a time:
/// [`spawn`](Self::spawn) stops any previous run first.
pub(crate) struct BackgroundTask<E> {
    handle: Option<JoinHandle<()>>,
    rx: Option<UnboundedReceiver<E>>,
}

impl<E> BackgroundTask<E> {
    pub const fn new() -> Self {
        Self {
            handle: None,
            rx: None,
        }
    }

    pub fn running(&self) -> bool {
        self.handle.is_some()
    }

    /// Spawn `run` (which owns the fresh sender) after stopping any
    /// previous run. One-shot jobs check [`running`](Self::running) first
    /// and return early; keyed streams call this unconditionally.
    pub fn spawn<Fut>(&mut self, run: impl FnOnce(UnboundedSender<E>) -> Fut)
    where
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.stop();
        let (tx, rx) = unbounded_channel();
        self.rx = Some(rx);
        self.handle = Some(tokio::spawn(run(tx)));
    }

    /// Abort the task (if any) and drop the channel.
    pub fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
        self.rx = None;
    }

    /// Drain all queued events. When the sender is gone the slot is
    /// cleared and `disconnected` is set so the caller can report it;
    /// a silent drop used to leave a dead `Some(rx)` behind forever.
    pub fn drain(&mut self) -> TaskDrain<E> {
        use tokio::sync::mpsc::error::TryRecvError;
        let mut events = Vec::new();
        let mut disconnected = false;
        if let Some(rx) = self.rx.as_mut() {
            loop {
                match rx.try_recv() {
                    Ok(event) => events.push(event),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
            if disconnected {
                self.stop();
            }
        }
        TaskDrain {
            events,
            disconnected,
        }
    }
}

impl<E> Default for BackgroundTask<E> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn drain_collects_events_in_order() {
        let mut task: BackgroundTask<i32> = BackgroundTask::new();
        assert!(!task.running());
        task.spawn(|tx| async move {
            let _ = tx.send(1);
            let _ = tx.send(2);
        });
        assert!(task.running());
        // Let the spawned future run to completion.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let drain = task.drain();
        assert_eq!(drain.events, vec![1, 2]);
        assert!(drain.disconnected);
        // Disconnect clears the slot so `running` never sticks.
        assert!(!task.running());
    }

    #[tokio::test]
    async fn stop_clears_a_live_task() {
        let mut task: BackgroundTask<i32> = BackgroundTask::new();
        task.spawn(|_tx| async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        assert!(task.running());
        task.stop();
        assert!(!task.running());
        let drain = task.drain();
        assert!(drain.events.is_empty());
        assert!(!drain.disconnected);
    }

    #[tokio::test]
    async fn spawn_replaces_previous_run() {
        let mut task: BackgroundTask<i32> = BackgroundTask::new();
        task.spawn(|_tx| async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        task.spawn(|tx| async move {
            let _ = tx.send(7);
        });
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let drain = task.drain();
        assert_eq!(drain.events, vec![7]);
        task.stop();
    }
}
