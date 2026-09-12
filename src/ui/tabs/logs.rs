use super::super::widgets::panel;
use crate::app::{App, LogLevel, LogSource};
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

fn parse_level_word(word: &str) -> Option<LogLevel> {
    match word.to_ascii_uppercase().as_str() {
        "ERROR" | "ERR" | "FATAL" | "PANIC" => Some(LogLevel::Error),
        "WARN" | "WARNING" => Some(LogLevel::Warn),
        "INFO" => Some(LogLevel::Info),
        "DEBUG" | "TRACE" => Some(LogLevel::Debug),
        _ => None,
    }
}

/// `key="quoted value"` / `key=bare` lookup for logrus text lines.
fn extract_kv(line: &str, key: &str) -> Option<String> {
    let rest = line.find(&format!("{key}=")).map(|pos| &line[pos + key.len() + 1..])?;
    if let Some(quoted) = rest.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = quoted.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
                continue;
            }
            if c == '"' {
                break;
            }
            out.push(c);
        }
        Some(out)
    } else {
        Some(
            rest.split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches('"')
                .to_string(),
        )
    }
}

/// logrus `time="…" level=… msg="…"` -> compact parts, keeping the
/// full stamp for the expanded row.
fn split_logrus(line: &str) -> Option<(String, String, LogLevel, String)> {
    let level = match extract_kv(line, "level")?.to_lowercase().as_str() {
        "error" | "fatal" | "panic" => LogLevel::Error,
        "warn" | "warning" => LogLevel::Warn,
        "debug" | "trace" => LogLevel::Debug,
        _ => LogLevel::Info,
    };
    let body = extract_kv(line, "msg").filter(|msg| !msg.is_empty()).unwrap_or_else(|| line.to_string());
    let full = extract_kv(line, "time").unwrap_or_default();
    let time = crate::api::MihomoClient::short_time(&full);
    Some((time, full, level, body))
}

/// Raw line -> `(clock, full stamp, level, body)` for rendering.
/// Handles omash `[time] LEVEL body` and logrus text; anything else
/// keeps the full line as the body with no clock column.
pub(crate) fn split_line(line: &str) -> (String, String, LogLevel, String) {
    if let Some(rest) = line.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let after = rest[end + 1..].trim_start();
            let mut words = after.splitn(2, char::is_whitespace);
            if let Some(level) = words.next().and_then(parse_level_word) {
                let full = rest[..end].to_string();
                let time = crate::api::MihomoClient::short_time(&full);
                return (time, full, level, words.next().unwrap_or("").trim_start().to_string());
            }
        }
    }
    if line.contains("time=") {
        if let Some(parts) = split_logrus(line) {
            return parts;
        }
    }
    (String::new(), String::new(), level_of(line), line.to_string())
}

/// Lines surviving the source + level + query filters, oldest first.
pub(crate) fn filtered_view(app: &App) -> Vec<(LogSource, LogLevel, &str)> {
    let query = app.log_query.to_lowercase();
    app.logs
        .iter()
        .filter(|entry| {
            app.log_source == LogSource::All || entry.source == app.log_source
        })
        .map(|entry| (entry.source, level_of(&entry.text), entry.text.as_str()))
        .filter(|(_, level, _)| app.log_level_filter.is_none_or(|min| *level >= min))
        .filter(|(_, _, line)| query.is_empty() || line.to_lowercase().contains(&query))
        .collect()
}

fn highlight(line: &str, query: &str, base: Style) -> Line<'static> {
    if query.is_empty() {
        return Line::from(Span::styled(line.to_owned(), base));
    }
    let lower = line.to_lowercase();
    let mut spans = Vec::new();
    let mut rest = 0;
    let mark = Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    while let Some(found) = lower[rest..].find(query) {
        let start = rest + found;
        let end = start + query.len();
        // Case folding can shift byte offsets on exotic scripts; bail
        // out to plain text rather than panic on a boundary.
        if !line.is_char_boundary(start) || !line.is_char_boundary(end) {
            break;
        }
        if start > rest {
            spans.push(Span::styled(line[rest..start].to_owned(), base));
        }
        spans.push(Span::styled(line[start..end].to_owned(), mark));
        rest = end;
    }
    if rest < line.len() {
        spans.push(Span::styled(line[rest..].to_owned(), base));
    }
    Line::from(spans)
}

pub(crate) fn logs(frame: &mut Frame, app: &mut App, area: Rect) {
    let height = area.height.saturating_sub(2) as usize;
    app.log_height = height.max(1);
    let width = area.width.saturating_sub(2) as usize;
    let total = filtered_view(app).len();
    let offset = if app.log_follow || height == 0 {
        total.saturating_sub(height)
    } else {
        app.log_scroll.min(total.saturating_sub(1))
    };
    app.log_scroll = offset;
    let query_lower = app.log_query.to_lowercase();
    let hscroll = app.log_hscroll;
    let time_style = Style::default().fg(app.theme.muted);
    let level_badge = |level: LogLevel| match level {
        LogLevel::Error => "ERROR",
        LogLevel::Warn => "WARN ",
        LogLevel::Info => "INFO ",
        LogLevel::Debug => "DEBUG",
    };
    let items: Vec<_> = filtered_view(app)
        .into_iter()
        .enumerate()
        .map(|(i, (source, _, line))| {
            let (time, full, level, body) = split_line(line);
            let base = match level {
                LogLevel::Error => Style::default().fg(app.theme.danger),
                LogLevel::Warn => Style::default().fg(app.theme.warning),
                LogLevel::Debug => Style::default().fg(app.theme.muted),
                LogLevel::Info => Style::default(),
            };
            let badge = Style::from(base).add_modifier(Modifier::BOLD);
            // Fixed clock + level columns; only the body scrolls, so the
            // `→N` indicator now refers to the message, not the timestamp.
            let prefix_width = (if time.is_empty() { 0 } else { 9 }) + 6;
            let body_width = width.saturating_sub(prefix_width).max(1);
            // Horizontal window into long messages; CJK-safe via char slicing.
            let visible: String = body.chars().skip(hscroll).take(body_width).collect();
            let mut header = Vec::new();
            if !time.is_empty() {
                header.push(Span::styled(time, time_style));
                header.push(Span::raw(" "));
            }
            header.push(Span::styled(level_badge(level), badge));
            header.push(Span::raw(" "));
            header.extend(highlight(&visible, &query_lower, base).spans);
            // The cursor row expands into provenance the header dropped:
            // full timestamp + origin, instead of repeating the message.
            if i == offset {
                let meta = if full.is_empty() {
                    source.label().to_string()
                } else {
                    format!("{full} · {}", source.label())
                };
                let wide: String = meta
                    .chars()
                    .skip(hscroll)
                    .take(width.saturating_sub(2).max(1))
                    .collect();
                let mut second = vec![Span::styled("▸ ", time_style)];
                second.extend(highlight(&wide, &query_lower, time_style).spans);
                ListItem::new(vec![Line::from(header), Line::from(second)])
            } else {
                ListItem::new(Line::from(header))
            }
        })
        .collect();
    let follow = if app.log_follow {
        if app.log_stream_live { "FOLLOW●" } else { "FOLLOW" }
    } else {
        "···"
    };
    let filter = app
        .log_level_filter
        .map_or("all".into(), |level| level.label().to_owned());
    let query = if app.log_query.is_empty() {
        String::new()
    } else {
        format!(" · /{}", app.log_query)
    };
    let hscroll = if app.log_hscroll == 0 {
        String::new()
    } else {
        format!(" · →{}", app.log_hscroll)
    };
    let source = app.log_source.label();
    let title =
        format!(" Mihomo logs · {follow} · {source} · {filter}{query}{hscroll} · {total} ");
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

    #[test]
    fn split_line_compacts_both_formats() {
        let (time, full, level, body) = split_line("[16:10:01] WARN  dial failed [proxy=x]");
        assert_eq!((time.as_str(), full.as_str(), level, body.as_str()), ("16:10:01", "16:10:01", LogLevel::Warn, "dial failed [proxy=x]"));
        let (time, full, level, body) = split_line("[2026-09-12 16:14:59] DEBUG [logstream] starting stream");
        assert_eq!((time.as_str(), full.as_str(), level, body.as_str()), ("16:14:59", "2026-09-12 16:14:59", LogLevel::Debug, "[logstream] starting stream"));
        let (time, full, level, body) = split_line(
            r#"time="2026-09-12T16:03:13.806135756+08:00" level=info msg="Start initial provider""#,
        );
        assert_eq!((time.as_str(), full.as_str(), level, body.as_str()), ("16:03:13", "2026-09-12T16:03:13.806135756+08:00", LogLevel::Info, "Start initial provider"));
        let (time, full, level, body) = split_line("plain line without a level");
        assert_eq!((time.as_str(), full.as_str(), level, body.as_str()), ("", "", LogLevel::Info, "plain line without a level"));
    }
}
