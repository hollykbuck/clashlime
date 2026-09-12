use super::super::widgets::panel;
use crate::app::{App, LogLevel};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState},
};

/// Severity guess for one log line. mihomo uses logrus `level=…`;
/// omash's own lines look like `[ts] LEVEL [target] msg`.
pub(crate) fn level_of(line: &str) -> LogLevel {
    let lower = line.to_lowercase();
    if let Some(rest) = lower.split_once("level=").map(|(_, rest)| rest) {
        if rest.starts_with("error") || rest.starts_with("fatal") || rest.starts_with("panic") {
            return LogLevel::Error;
        }
        if rest.starts_with("warn") {
            return LogLevel::Warn;
        }
        if rest.starts_with("debug") || rest.starts_with("trace") {
            return LogLevel::Debug;
        }
        return LogLevel::Info;
    }
    // omash pads levels to 5 chars (`] WARN  [`), so only the left
    // space is structural.
    for (marker, level) in [
        ("] error ", LogLevel::Error),
        ("] warn ", LogLevel::Warn),
        ("] info ", LogLevel::Info),
        ("] debug ", LogLevel::Debug),
    ] {
        if lower.contains(marker) {
            return level;
        }
    }
    LogLevel::Info
}

/// Lines surviving the level + query filters, oldest first.
pub(crate) fn filtered_view(app: &App) -> Vec<(LogLevel, &str)> {
    let query = app.log_query.to_lowercase();
    app.logs
        .iter()
        .map(|line| (level_of(line), line.as_str()))
        .filter(|(level, _)| app.log_level_filter.is_none_or(|min| *level >= min))
        .filter(|(_, line)| query.is_empty() || line.to_lowercase().contains(&query))
        .collect()
}

fn highlight<'a>(line: &'a str, query: &str, base: Style) -> Line<'a> {
    if query.is_empty() {
        return Line::from(Span::styled(line, base));
    }
    let lower = line.to_lowercase();
    let mut spans = Vec::new();
    let mut rest = 0;
    let mark = Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    while let Some(found) = lower[rest..].find(query) {
        let start = rest + found;
        let end = start + query.len();
        if start > rest {
            spans.push(Span::styled(&line[rest..start], base));
        }
        spans.push(Span::styled(&line[start..end], mark));
        rest = end;
    }
    if rest < line.len() {
        spans.push(Span::styled(&line[rest..], base));
    }
    Line::from(spans)
}

pub(crate) fn logs(frame: &mut Frame, app: &mut App, area: Rect) {
    let height = area.height.saturating_sub(2) as usize;
    app.log_height = height.max(1);
    let total = filtered_view(app).len();
    let offset = if app.log_follow || height == 0 {
        total.saturating_sub(height)
    } else {
        app.log_scroll.min(total.saturating_sub(1))
    };
    app.log_scroll = offset;
    let query_lower = app.log_query.to_lowercase();
    let items: Vec<_> = filtered_view(app)
        .into_iter()
        .map(|(level, line)| {
            let base = match level {
                LogLevel::Error => Style::default().fg(app.theme.danger),
                LogLevel::Warn => Style::default().fg(app.theme.warning),
                LogLevel::Debug => Style::default().fg(app.theme.muted),
                LogLevel::Info => Style::default(),
            };
            ListItem::new(highlight(line, &query_lower, base))
        })
        .collect();
    let follow = if app.log_follow { "FOLLOW" } else { "···" };
    let filter = app
        .log_level_filter
        .map_or("all".into(), |level| level.label().to_owned());
    let query = if app.log_query.is_empty() {
        String::new()
    } else {
        format!(" · /{}", app.log_query)
    };
    let title = format!(" Mihomo logs · {follow} · {filter}{query} · {total} ");
    let mut state = ListState::default();
    if total > 0 {
        state.select(Some(offset));
    }
    frame.render_stateful_widget(
        List::new(items).block(panel(&title, &app.theme)),
        area,
        &mut state,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_both_log_formats() {
        assert_eq!(
            level_of(r#"time="2026-01-01T00:00:00Z" level=error msg="boom""#),
            LogLevel::Error
        );
        assert_eq!(
            level_of(r#"time="2026-01-01T00:00:00Z" level=warning msg="slow""#),
            LogLevel::Warn
        );
        assert_eq!(
            level_of("[2026-01-01 00:00:00] ERROR [geo] failed"),
            LogLevel::Error
        );
        assert_eq!(
            level_of("[2026-01-01 00:00:00] WARN  [app] hmm"),
            LogLevel::Warn
        );
        assert_eq!(level_of("plain line without a level"), LogLevel::Info);
    }
}
