//! Logs tab interactions: scrolling, follow mode, level filter and
//! text search over the pulled mihomo + clashlime log buffer.

use crate::app::{LogLevel, LogSource};
use crate::ui::tabs::logs::filtered_view;

impl crate::app::App {
    fn log_view_len(&self) -> usize {
        filtered_view(self).len()
    }

    /// Scroll by lines; any manual scroll leaves follow mode.
    pub(crate) fn scroll_logs(&mut self, delta: isize) {
        let len = self.log_view_len();
        if len == 0 {
            return;
        }
        let max = len.saturating_sub(1);
        self.ui.log_scroll = (self.ui.log_scroll as isize + delta).clamp(0, max as isize) as usize;
        self.ui.log_follow = false;
    }

    /// Page by the last rendered list height.
    pub(crate) fn page_logs(&mut self, delta: isize) {
        let step = self.ui.log_height.max(1) as isize * delta;
        self.scroll_logs(step);
    }

    /// Jump to the tail and resume following new output.
    pub(crate) fn follow_logs(&mut self) {
        self.ui.log_follow = true;
    }

    /// Jump to the head of the filtered view.
    pub(crate) fn top_logs(&mut self) {
        self.ui.log_scroll = 0;
        self.ui.log_follow = false;
    }

    /// Horizontal scroll through long lines, in 8-column steps.
    pub(crate) fn scroll_logs_horizontal(&mut self, delta: isize) {
        self.ui.log_hscroll = (self.ui.log_hscroll as isize + delta * 8).max(0) as usize;
    }

    /// Open the current line in a wrapped detail popup.
    pub(crate) fn open_log_detail(&mut self) {
        let view = filtered_view(self);
        if let Some((_, _, line)) = view.get(self.ui.log_scroll) {
            self.ui.log_detail = Some((*line).to_owned());
        }
    }

    /// Close the log detail popup.
    pub(crate) fn close_log_detail(&mut self) {
        self.ui.log_detail = None;
    }

    /// Cycle the severity filter: all -> error -> warn -> info -> all.
    pub(crate) fn cycle_log_filter(&mut self) {
        self.ui.log_level_filter = match self.ui.log_level_filter {
            None => Some(LogLevel::Error),
            Some(LogLevel::Error) => Some(LogLevel::Warn),
            Some(LogLevel::Warn) => Some(LogLevel::Info),
            Some(LogLevel::Info) => None,
            Some(LogLevel::Debug) => None,
        };
        self.ui.log_follow = true;
        self.ui.log_hscroll = 0;
        let active = self
            .ui.log_level_filter
            .map_or("all".into(), |level| level.label().to_owned());
        self.say(format!("Log filter: {active}"));
    }
}

impl crate::app::App {
    /// Cycle the log source filter: all -> core -> daemon -> tui -> all.
    pub(crate) fn cycle_log_source(&mut self) {
        self.ui.log_source = match self.ui.log_source {
            LogSource::All => LogSource::Core,
            LogSource::Core => LogSource::Daemon,
            LogSource::Daemon => LogSource::Tui,
            LogSource::Tui => LogSource::All,
        };
        self.ui.log_follow = true;
        self.ui.log_hscroll = 0;
        self.say(format!("Log source: {}", self.ui.log_source.label()));
    }
}
