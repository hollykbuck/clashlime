use std::time::Duration;

/// Collapse control characters into spaces so a message stays on one line.
/// The status bar renders with `Paragraph`, so a stray `\n` would otherwise
/// spill text into the rows below and shift the rest of the frame.
pub(crate) fn flatten_status(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

/// Severity of a status-bar message; drives color and how long the message
/// survives before periodic refreshes may replace it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusKind {
    #[default]
    Info,
    Success,
    Warning,
    Error,
    Busy,
}

impl StatusKind {
    /// Classify a message so action feedback survives refresh cycles.
    pub(crate) fn infer(text: &str) -> Self {
        let lower = text.to_ascii_lowercase();
        if lower.ends_with('…')
            || [
                "testing",
                "checking",
                "validating",
                "resolving",
                "connecting",
                "downloading",
                "importing",
                "fetching",
            ]
            .iter()
            .any(|k| lower.starts_with(k))
        {
            return Self::Busy;
        }
        if [
            "failed",
            "error",
            "rejected",
            "cannot",
            "no file",
            "not executable",
        ]
        .iter()
        .any(|k| lower.contains(k))
        {
            return Self::Error;
        }
        if ["cancelled", "skipped"].iter().any(|k| lower.contains(k)) {
            return Self::Warning;
        }
        if [
            "imported ",
            "installed at",
            " installed",
            "saved",
            "activated",
            "updated",
            "checked",
            "deleted",
            "backup created",
            "closed",
            "restored",
            "using mihomo",
            "hot patched",
            "up to date",
        ]
        .iter()
        .any(|k| lower.contains(k))
        {
            return Self::Success;
        }
        Self::Info
    }

    /// How long this message resists being overwritten by refresh().
    pub(crate) fn dwell(self) -> Option<Duration> {
        match self {
            Self::Busy => None,
            Self::Error => Some(Duration::from_secs(10)),
            Self::Success => Some(Duration::from_secs(4)),
            Self::Warning => Some(Duration::from_secs(6)),
            Self::Info => Some(Duration::from_secs(3)),
        }
    }
}

/// A recorded event in the notification history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub id: u64,
    pub kind: StatusKind,
    pub message: String,
    pub time: String,
    pub read: bool,
}

impl super::App {
    /// Record a user-action message and add it to the notification history.
    pub fn say(&mut self, text: impl Into<String>) {
        let text = flatten_status(&text.into());
        let kind = StatusKind::infer(&text);
        self.notify(kind, text);
    }

    /// Explicitly notify with a given severity level.
    pub fn notify(&mut self, kind: StatusKind, text: impl Into<String>) {
        let text = flatten_status(&text.into());
        self.data.status_kind = kind;
        self.data.status_sticky_until = self
            .data
            .status_kind
            .dwell()
            .map(|dwell| std::time::Instant::now() + dwell);
        self.data.status = text.clone();
        if kind != StatusKind::Busy && !text.is_empty() {
            let time = chrono::Local::now().format("%H:%M:%S").to_string();
            self.data.push_notification(kind, text, time);
        }
    }

    pub fn toggle_notifications(&mut self) {
        self.ui.notifications_open = !self.ui.notifications_open;
        if self.ui.notifications_open {
            self.ui.notifications_cursor = 0;
            self.data.mark_notifications_read();
        }
    }

    pub fn move_notifications_cursor(&mut self, delta: isize) {
        if self.data.notifications.is_empty() {
            self.ui.notifications_cursor = 0;
            return;
        }
        let len = self.data.notifications.len();
        let next = (self.ui.notifications_cursor as isize + delta)
            .clamp(0, len.saturating_sub(1) as isize);
        self.ui.notifications_cursor = next as usize;
    }

    /// Refresh-driven status ("Synced" / offline reason). Yields to any
    /// sticky user message that has not expired yet.
    pub(crate) fn set_default_status(&mut self, text: String) {
        if self.sticky_active() {
            return;
        }
        self.data.status_kind = StatusKind::infer(&text);
        self.data.status_sticky_until = None;
        self.data.status = text;
    }

    pub(crate) fn sticky_active(&self) -> bool {
        // Busy has no dwell expiry: work-in-progress stays pinned until the
        // next `say()` replaces it. Anything else follows its dwell time.
        if self.data.status_kind == StatusKind::Busy {
            return true;
        }
        self.data
            .status_sticky_until
            .is_some_and(|until| std::time::Instant::now() < until)
    }

    /// Severity color for the status bar.
    pub fn status_color_kind(&self) -> StatusKind {
        self.data.status_kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_kind_infers_severity_from_text() {
        assert_eq!(StatusKind::infer("Testing node…"), StatusKind::Busy);
        assert_eq!(
            StatusKind::infer("Import failed: timeout"),
            StatusKind::Error
        );
        assert_eq!(StatusKind::infer("Imported abc123"), StatusKind::Success);
        assert_eq!(StatusKind::infer("Restore cancelled"), StatusKind::Warning);
        assert_eq!(StatusKind::infer("Synced"), StatusKind::Info);
        assert_eq!(
            StatusKind::infer("Importing https://example.com/sub…"),
            StatusKind::Busy
        );
        // The provider actions rely on these classifications: the running
        // check must not expire (Busy has no dwell) while the result does.
        assert_eq!(StatusKind::infer("Checking 2 providers…"), StatusKind::Busy);
        assert_eq!(StatusKind::infer("Checking 1 provider…"), StatusKind::Busy);
        assert_eq!(
            StatusKind::infer("Proxy providers updated: 1/1"),
            StatusKind::Success
        );
        assert_eq!(
            StatusKind::infer("Providers checked: 2/2"),
            StatusKind::Success
        );
        assert_eq!(
            StatusKind::infer("No proxy providers: profile inlines all proxies"),
            StatusKind::Info
        );
    }

    #[test]
    fn status_kind_error_dwells_longer_than_info() {
        let error_dwell = StatusKind::Error.dwell();
        let info_dwell = StatusKind::Info.dwell();
        assert!(error_dwell.unwrap() > info_dwell.unwrap());
        assert_eq!(StatusKind::Busy.dwell(), None);
    }

    #[test]
    fn flatten_status_keeps_messages_on_one_line() {
        assert_eq!(flatten_status("a\nb\rc\u{1b}d"), "a b c d");
        // Text with no control characters is untouched.
        assert_eq!(flatten_status("Imported abc123"), "Imported abc123");
    }

    #[test]
    fn notification_history_tracks_events_and_deduplicates() {
        let mut data = crate::app::DataState::default();
        data.push_notification(StatusKind::Info, "First event".into(), "10:00:00".into());
        assert_eq!(data.notifications.len(), 1);
        assert_eq!(data.unread_notifications, 1);

        // Immediate duplicate ignored
        data.push_notification(StatusKind::Info, "First event".into(), "10:00:01".into());
        assert_eq!(data.notifications.len(), 1);
        assert_eq!(data.unread_notifications, 1);

        data.push_notification(StatusKind::Error, "Core failed".into(), "10:00:02".into());
        assert_eq!(data.notifications.len(), 2);
        assert_eq!(data.unread_notifications, 2);
        assert!(data.has_unread_error());

        data.mark_notifications_read();
        assert_eq!(data.unread_notifications, 0);
        assert!(!data.has_unread_error());
        assert_eq!(data.notifications.len(), 2);

        data.clear_notifications();
        assert_eq!(data.notifications.len(), 0);
    }
}
