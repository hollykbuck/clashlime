//! One generic background-task slot for the TUI event loop.
//!
//! Step 2 of the `App` de-god-object refactor. Every background job in the
//! TUI used to be an `Option<JoinHandle>` + `Option<UnboundedReceiver<E>>`
//! pair with its own copy of spawn/poll/stop/disconnect handling (9 pairs).
//! [`BackgroundTask`] owns both halves; call sites keep only their event
//! type and their `handle_*` interpretation.

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use futures_util::FutureExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

/// All background job slots owned by the TUI event loop: one-shot jobs
/// (core download, geo, import, profile, update check, delay test) plus
/// the persistent API streams (logs, memory, traffic) with their
/// controller keys and liveness flags.
#[derive(Default)]
pub(crate) struct TaskHub {
    pub core_download: BackgroundTask<crate::app::CoreDownloadEvent>,
    pub geo: BackgroundTask<crate::app::actions::geo::GeoEvent>,
    pub import: BackgroundTask<crate::app::actions::import::ImportEvent>,
    pub profile: BackgroundTask<crate::app::actions::profile::ProfileEvent>,
    pub update: BackgroundTask<crate::app::actions::update::UpdateCheckEvent>,
    pub delay: BackgroundTask<crate::app::actions::proxy::DelayEvent>,
    pub log: BackgroundTask<crate::app::actions::logstream::CoreLogEvent>,
    pub log_stream_key: String,
    pub log_stream_live: bool,
    pub log_backlog_loaded: bool,
    pub mem: BackgroundTask<crate::api::MemoryInfo>,
    pub mem_stream_key: String,
    pub traffic: BackgroundTask<crate::api::TrafficInfo>,
    pub traffic_stream_key: String,
}

/// Result of [`BackgroundTask::drain`]: queued events, whether the sender
/// went away, and the panic message when the task died that way.
///
/// `disconnected` alone does *not* mean failure: dropping the sender is how
/// a task ends normally, so every successful run reports it too. Callers
/// must key failure messages off [`panic`](Self::panic) instead.
pub(crate) struct TaskDrain<E> {
    pub events: Vec<E>,
    pub disconnected: bool,
    /// Set when the task unwound. The TUI suppresses stderr, so this is the
    /// only place the real cause of a dead background task can surface.
    pub panic: Option<String>,
}

/// A spawned task plus its event channel. At most one run at a time:
/// [`spawn`](Self::spawn) stops any previous run first.
pub(crate) struct BackgroundTask<E> {
    handle: Option<JoinHandle<()>>,
    rx: Option<UnboundedReceiver<E>>,
    /// Filled by the spawned wrapper if the task unwinds.
    panic: Option<Arc<Mutex<Option<String>>>>,
}

impl<E> BackgroundTask<E> {
    pub const fn new() -> Self {
        Self {
            handle: None,
            rx: None,
            panic: None,
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
        let panic: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        self.panic = Some(Arc::clone(&panic));
        let task = run(tx);
        self.handle = Some(tokio::spawn(async move {
            // Catch the unwind inside the task so the cause is attributable
            // to *this* run; a process-wide "last panic" would let a later,
            // successful run report an unrelated earlier panic as its own.
            // The panic hook still runs first and records the location.
            if let Err(payload) = AssertUnwindSafe(task).catch_unwind().await {
                let message = panic_message(&payload);
                if let Ok(mut guard) = panic.lock() {
                    *guard = Some(message);
                }
            }
        }));
    }

    /// Abort the task (if any) and drop the channel.
    pub fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
        self.rx = None;
        self.panic = None;
    }

    /// Drain all queued events. When the sender is gone the slot is cleared
    /// and `disconnected` is set so the caller can react; a silent drop used
    /// to leave a dead `Some(rx)` behind forever. A recorded panic, if any,
    /// is moved into the result.
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
        }
        // Read the recorded panic before `stop`, which clears the slot. Only
        // a finished task can have one: the wrapper fills it as it unwinds,
        // and unwinding drops the sender.
        let panic = self
            .panic
            .as_ref()
            .and_then(|slot| slot.lock().ok().and_then(|mut guard| guard.take()));
        if disconnected {
            self.stop();
        }
        TaskDrain {
            events,
            disconnected,
            panic,
        }
    }
}

/// Best-effort rendering of a panic payload.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned())
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
        // Normal completion still disconnects: success is not failure.
        assert!(drain.disconnected);
        assert!(drain.panic.is_none());
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
        assert!(drain.panic.is_none());
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

    #[tokio::test]
    async fn panic_is_captured_and_reported() {
        let mut task: BackgroundTask<i32> = BackgroundTask::new();
        task.spawn(|_tx| async move {
            panic!("boom from the task");
        });
        // Let the task start and unwind.
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        let drain = task.drain();
        assert!(drain.events.is_empty());
        assert!(drain.disconnected);
        let panic = drain.panic.expect("panic payload should be captured");
        assert!(panic.contains("boom from the task"), "{panic}");
    }

    #[tokio::test]
    async fn panic_is_not_reported_for_a_later_successful_run() {
        let mut task: BackgroundTask<i32> = BackgroundTask::new();
        task.spawn(|_tx| async move {
            panic!("earlier boom");
        });
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        let first = task.drain();
        assert!(first.panic.is_some());

        // A fresh run that succeeds must not inherit the old panic.
        task.spawn(|tx| async move {
            let _ = tx.send(1);
        });
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        let second = task.drain();
        assert_eq!(second.events, vec![1]);
        assert!(second.panic.is_none(), "{:?}", second.panic);
    }

    #[tokio::test]
    async fn panic_after_a_terminal_event_is_still_reported() {
        let mut task: BackgroundTask<i32> = BackgroundTask::new();
        task.spawn(|tx| async move {
            let _ = tx.send(1);
            panic!("boom after sending");
        });
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        let drain = task.drain();
        // A terminal event does not excuse a panic that happened after it:
        // callers key failure off `panic`, not off event presence.
        assert_eq!(drain.events, vec![1]);
        let panic = drain.panic.expect("panic must not be swallowed");
        assert!(panic.contains("boom after sending"), "{panic}");
    }
}
