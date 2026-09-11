use std::time::Duration;

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

impl super::App {
    /// Record a user-action message. The message stays pinned over periodic
    /// refresh statuses for a severity-dependent dwell time.
    pub fn say(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.status_kind = StatusKind::infer(&text);
        self.status_sticky_until = self
            .status_kind
            .dwell()
            .map(|dwell| std::time::Instant::now() + dwell);
        self.status = text;
    }

    /// Refresh-driven status ("Synced" / offline reason). Yields to any
    /// sticky user message that has not expired yet.
    pub(crate) fn set_default_status(&mut self, text: String) {
        if self.sticky_active() {
            return;
        }
        self.status_kind = StatusKind::infer(&text);
        self.status_sticky_until = None;
        self.status = text;
    }

    pub(crate) fn sticky_active(&self) -> bool {
        self.status_sticky_until
            .is_some_and(|until| std::time::Instant::now() < until)
    }

    /// True when the current status is transient work in progress.
    pub fn is_busy_status(&self) -> bool {
        self.status_kind == StatusKind::Busy
    }

    pub fn status_color_kind(&self) -> StatusKind {
        if self.is_busy_status() && !self.sticky_active() && self.online {
            StatusKind::Info
        } else {
            self.status_kind
        }
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
        assert_eq!(
            StatusKind::infer("Restore cancelled"),
            StatusKind::Warning
        );
        assert_eq!(StatusKind::infer("Synced"), StatusKind::Info);
        assert_eq!(
            StatusKind::infer("Importing https://example.com/sub…"),
            StatusKind::Busy
        );
    }

    #[test]
    fn status_kind_error_dwells_longer_than_info() {
        let error_dwell = StatusKind::Error.dwell();
        let info_dwell = StatusKind::Info.dwell();
        assert!(error_dwell.unwrap() > info_dwell.unwrap());
        assert_eq!(StatusKind::Busy.dwell(), None);
    }
}
