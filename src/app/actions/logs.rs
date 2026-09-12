//! Logs tab interactions: scrolling, follow mode, level filter and
//! text search over the pulled mihomo + omash log buffer.

use crate::app::LogLevel;
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
        self.log_scroll = (self.log_scroll as isize + delta).clamp(0, max as isize) as usize;
        self.log_follow = false;
    }

    /// Page by the last rendered list height.
    pub(crate) fn page_logs(&mut self, delta: isize) {
        let step = self.log_height.max(1) as isize * delta;
        self.scroll_logs(step);
    }

    /// Jump to the tail and resume following new output.
    pub(crate) fn follow_logs(&mut self) {
        self.log_follow = true;
    }

    /// Jump to the head of the filtered view.
    pub(crate) fn top_logs(&mut self) {
        self.log_scroll = 0;
        self.log_follow = false;
    }

    /// Cycle the severity filter: all -> error -> warn -> info -> all.
    pub(crate) fn cycle_log_filter(&mut self) {
        self.log_level_filter = match self.log_level_filter {
            None => Some(LogLevel::Error),
            Some(LogLevel::Error) => Some(LogLevel::Warn),
            Some(LogLevel::Warn) => Some(LogLevel::Info),
            Some(LogLevel::Info) => None,
            Some(LogLevel::Debug) => None,
        };
        self.log_follow = true;
        let active = self
            .log_level_filter
            .map_or("all".into(), |level| level.label().to_owned());
        self.say(format!("Log filter: {active}"));
    }
}
