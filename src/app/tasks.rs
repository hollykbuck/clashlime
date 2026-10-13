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

/// Upper bound on a panic message shown in the UI. The status bar is one
/// line, so anything longer only pushes useful text off-screen.
const MAX_PANIC_MESSAGE: usize = 200;

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

/// Whether an event reports a task's *result*, as opposed to streaming
/// progress on the way there.
///
/// A one-shot job whose event enum only has `Done`/`Failed` needs no
/// override: every event is terminal by default. A job that also emits
/// progress events (the core download reports `Stage`/`Progress`) overrides
/// it, so callers never have to re-derive "did this run report a result?"
/// from `!events.is_empty()` at each call site.
pub(crate) trait TaskEvent {
    fn is_terminal(&self) -> bool {
        true
    }
}

/// Every event type that flows through a [`BackgroundTask`] implements
/// [`TaskEvent`] here, so the DTO modules (`api`) and the action modules
/// never have to know this trait exists — the dependency only points from
/// this file outward to the types it serves.
impl TaskEvent for crate::app::CoreDownloadEvent {
    /// `Stage` and `Progress` stream the download's progress; only `Done`
    /// and `Failed` report a result. Without this, a run cancelled midway
    /// would look like a normal end just because it had sent progress.
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Done { .. } | Self::Failed(_))
    }
}
impl TaskEvent for crate::app::actions::geo::GeoEvent {}
impl TaskEvent for crate::app::actions::import::ImportEvent {}
impl TaskEvent for crate::app::actions::profile::ProfileEvent {}
impl TaskEvent for crate::app::actions::proxy::DelayEvent {}
impl TaskEvent for crate::app::actions::update::UpdateCheckEvent {}

/// Stream event types get no terminal event: they loop until
/// [`BackgroundTask::stop`] or the task dies, so every sample is progress
/// rather than a result. Keeping the default (`true`) here would be a lie
/// that only shows up if a poller ever starts consulting `TaskDrain::terminal`.
impl TaskEvent for crate::app::actions::logstream::CoreLogEvent {
    fn is_terminal(&self) -> bool {
        false
    }
}
impl TaskEvent for crate::api::MemoryInfo {
    fn is_terminal(&self) -> bool {
        false
    }
}
impl TaskEvent for crate::api::TrafficInfo {
    fn is_terminal(&self) -> bool {
        false
    }
}

/// Result of [`BackgroundTask::drain`]: queued events, whether the sender
/// went away, whether a terminal event arrived, and the panic message when
/// the task died that way.
///
/// `disconnected` alone does *not* mean failure: dropping the sender is how
/// a task ends normally, so every successful run reports it too. Callers
/// must key failure messages off [`panic`](Self::panic) instead.
pub(crate) struct TaskDrain<E> {
    pub events: Vec<E>,
    pub disconnected: bool,
    /// At least one queued event reported a result for this run. A task that
    /// ends without one neither succeeded nor reported why it stopped.
    pub terminal: bool,
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

impl<E: TaskEvent> BackgroundTask<E> {
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
    ///
    /// A job that reports progress events (the core download) keeps sending
    /// them until it is [`stop`](Self::stop)ped or ends; only a terminal
    /// event reports a result, and the wrapper fills the panic slot if the
    /// task unwinds instead.
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
        let terminal = events.iter().any(TaskEvent::is_terminal);
        if disconnected {
            self.stop();
        }
        TaskDrain {
            events,
            disconnected,
            terminal,
            panic,
        }
    }
}

/// Best-effort rendering of a panic payload.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    let raw = payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned());
    // The payload can carry remote text (an error string from the core, a
    // profile name). Control characters would land verbatim in the
    // single-line status bar and garble the layout, so fold them into
    // spaces and clamp the length.
    let flattened = crate::app::status::flatten_status(&raw);
    let trimmed = flattened.trim();
    if trimmed.is_empty() {
        return "background task panicked".to_owned();
    }
    if trimmed.chars().count() <= MAX_PANIC_MESSAGE {
        return trimmed.to_owned();
    }
    let mut cut: String = trimmed.chars().take(MAX_PANIC_MESSAGE - 1).collect();
    cut.push('…');
    cut
}

impl<E: TaskEvent> Default for BackgroundTask<E> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-in for a Done/Failed-only event enum (the default impl).
    impl TaskEvent for i32 {}

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
        // The default TaskEvent impl treats every event as terminal.
        assert!(drain.terminal);
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

    #[derive(Debug, PartialEq)]
    enum Streamy {
        Progress(u32),
        Done,
    }

    impl TaskEvent for Streamy {
        fn is_terminal(&self) -> bool {
            matches!(self, Self::Done)
        }
    }

    #[tokio::test]
    async fn progress_events_alone_are_not_terminal() {
        let mut task: BackgroundTask<Streamy> = BackgroundTask::new();
        task.spawn(|tx| async move {
            let _ = tx.send(Streamy::Progress(50));
            // End without ever reporting a result.
        });
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        let drain = task.drain();
        assert_eq!(drain.events, vec![Streamy::Progress(50)]);
        assert!(drain.disconnected);
        assert!(drain.panic.is_none());
        // A progress event must not read as "the run reported a result",
        // otherwise a cancelled download looks like a normal finish.
        assert!(!drain.terminal);
    }

    #[tokio::test]
    async fn stream_events_are_never_terminal() {
        // The API stream types must report "no result", not inherit the
        // one-shot default: a poll loop that consulted `terminal` would
        // otherwise treat the first sample as a finished run.
        assert!(!crate::api::MemoryInfo::default().is_terminal());
        assert!(!crate::api::TrafficInfo::default().is_terminal());
        assert!(!crate::app::actions::logstream::CoreLogEvent::Connected.is_terminal());
        // One-shot events keep the all-terminal default.
        assert!(i32::default().is_terminal());
    }

    #[tokio::test]
    async fn done_event_after_progress_is_terminal() {
        let mut task: BackgroundTask<Streamy> = BackgroundTask::new();
        task.spawn(|tx| async move {
            let _ = tx.send(Streamy::Progress(50));
            let _ = tx.send(Streamy::Done);
        });
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        let drain = task.drain();
        assert!(drain.terminal);
    }

    #[test]
    fn panic_message_flattens_control_chars_and_clamps() {
        let boxed: Box<dyn std::any::Any + Send> = Box::new("a\nb\rc\u{1b}[31m");
        let message = panic_message(&boxed);
        assert!(!message.contains('\n'));
        assert!(!message.contains('\r'));
        assert!(!message.contains('\u{1b}'));
        assert!(message.contains("a b c"), "{message}");

        let long: Box<dyn std::any::Any + Send> = Box::new("x".repeat(MAX_PANIC_MESSAGE * 2));
        let clamped = panic_message(&long);
        assert_eq!(clamped.chars().count(), MAX_PANIC_MESSAGE);
        assert!(clamped.ends_with('\u{2026}'));
    }

    #[test]
    fn panic_message_handles_blank_and_non_string_payloads() {
        let blank: Box<dyn std::any::Any + Send> = Box::new("   \n\t  ");
        assert_eq!(panic_message(&blank), "background task panicked");
        let opaque: Box<dyn std::any::Any + Send> = Box::new(42u8);
        assert_eq!(panic_message(&opaque), "non-string panic payload");
    }
}
