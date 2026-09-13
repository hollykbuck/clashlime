use crate::app::Tab;
use crate::theme::Theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Padding},
};

pub(crate) fn inset_panel(area: Rect) -> Rect {
    if area.width > 4 {
        area.inner(ratatui::layout::Margin::new(1, 0))
    } else {
        area
    }
}

pub(crate) fn panel<'a>(title: &'a str, theme: &Theme) -> Block<'a> {
    Block::default()
        .padding(Padding::new(0, 1, 1, 1))
        .title(Span::styled(
            title,
            Style::default()
                .fg(theme.muted)
                .add_modifier(Modifier::BOLD),
        ))
}

pub(crate) fn focus_panel<'a>(title: &'a str, focused: bool, theme: &Theme) -> Block<'a> {
    Block::default()
        .padding(Padding::new(0, 1, 1, 1))
        .title(Span::styled(
            title,
            Style::default()
                .fg(if focused { theme.accent } else { theme.muted })
                .add_modifier(Modifier::BOLD),
        ))
}

pub(crate) fn selection_style(focused: bool, theme: &Theme) -> Style {
    if focused {
        // No filled background: the selected row speaks through accent
        // foreground + bold, terminal background untouched.
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.muted)
    }
}

pub(crate) fn status_badge(value: &'static str, theme: &Theme) -> Span<'static> {
    let color = if value == "on" {
        theme.success
    } else {
        theme.muted
    };
    Span::styled(
        value.to_uppercase(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn section_line(title: &'static str, theme: &Theme) -> Line<'static> {
    Line::styled(
        title,
        Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn help_binding(
    key: &'static str,
    description: &'static str,
    theme: &Theme,
) -> Line<'static> {
    let key_color = if matches!(key, "D" | "X" | "R") {
        theme.danger
    } else {
        theme.accent
    };
    Line::from(vec![
        Span::styled(
            format!(" {key:<9}"),
            Style::default().fg(key_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {description}"),
            Style::default().fg(theme.foreground),
        ),
    ])
}

pub(crate) fn short_title(tab: Tab) -> &'static str {
    match tab {
        Tab::Dashboard => "Home",
        Tab::Proxies => "Proxy",
        Tab::Profiles => "Profiles",
        Tab::Connections => "Conns",
        Tab::Rules => "Rules",
        Tab::Logs => "Logs",
        Tab::Settings => "Settings",
        Tab::Help => "Help",
    }
}

pub(crate) fn truncate_tail(text: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// Drop emoji presentation selectors (U+FE0F) for display.
///
/// Terminals with old Unicode tables ignore VS16 and draw `✈️` one
/// column wide while ratatui reserves two; every cell after it lands
/// one off, leaving a stale glyph at the row tail that the diff
/// renderer then never repaints (`space == space` is skipped), so the
/// smear survives scrolling and even tab switches. Bare `✈` is text
/// presentation everywhere (width 1), and chars that are emoji by
/// default (🎯, flags) don't need the selector, so removing it is
/// display-safe. Borrowed when there is nothing to strip (hot path).
pub(crate) fn strip_vs16(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains('\u{FE0F}') {
        std::borrow::Cow::Owned(text.replace('\u{FE0F}', ""))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// Cursor-aware viewport for single-line text inputs.
///
/// Returns `(visible_text, cursor_offset_in_chars)` where `visible_text`
/// fits into `width` terminal cells (char count approximation) and always
/// contains the cursor. When the whole value fits, it is returned as-is.
/// When it overflows, a sliding window around the cursor is shown with a
/// `…` marker on the truncated side(s), so long values like `proxy_bypass`
/// stay editable with Left/Right/Home/End.
pub(crate) fn input_view(value: &str, cursor: usize, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }
    if width == 1 {
        return ("…".into(), 0);
    }
    let chars: Vec<char> = value.chars().collect();
    let total = chars.len();
    let cursor = cursor.min(total);
    if total <= width {
        return (value.to_owned(), cursor);
    }
    // Near the start: head + trailing marker.
    if cursor < width - 1 {
        let head: String = chars[..width - 1].iter().collect();
        return (format!("{head}…"), cursor);
    }
    // Near the end (including the very end): leading marker + tail.
    // This preserves the historical tail view when the cursor is at the end.
    if cursor > total - (width - 1) {
        let tail: String = chars[total - (width - 1)..].iter().collect();
        let offset = cursor - (total - (width - 1)) + 1;
        return (format!("…{tail}"), offset.min(width));
    }
    // Middle: marker on both sides, cursor centered in the window.
    let inner = width - 2;
    let mut start = cursor.saturating_sub(inner / 2);
    let max_start = total.saturating_sub(inner);
    if start > max_start {
        start = max_start;
    }
    let end = (start + inner).min(total);
    let start = end.saturating_sub(inner);
    let middle: String = chars[start..end].iter().collect();
    (format!("…{middle}…"), cursor - start + 1)
}

pub(crate) fn fit_column(value: &str, width: usize, align_right: bool) -> String {
    if width == 0 {
        return String::new();
    }
    let value_width = Line::from(value).width();
    if value_width <= width {
        let padding = " ".repeat(width - value_width);
        return if align_right {
            format!("{padding}{value}")
        } else {
            format!("{value}{padding}")
        };
    }
    if width == 1 {
        return "…".into();
    }

    let content_width = width - 1;
    let mut result = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = Line::from(character.to_string()).width();
        if used + character_width > content_width {
            break;
        }
        result.push(character);
        used += character_width;
    }
    result.push('…');
    result.push_str(&" ".repeat(content_width - used));
    result
}

pub(crate) fn bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", value, UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

pub(crate) fn bool_text(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "on",
        Some(false) => "off",
        None => "—",
    }
}

pub(crate) fn value_or_dash(value: &str) -> &str {
    if value.is_empty() { "—" } else { value }
}

pub(crate) fn on_off(value: bool) -> String {
    if value { "on".into() } else { "off".into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1536), "1.5 KiB");
    }

    #[test]
    fn strip_vs16_normalizes_ambiguous_emoji() {
        use ratatui::text::Span;
        use std::borrow::Cow;
        // VS16-emoji -> text style, width 1 everywhere.
        assert_eq!(&*strip_vs16("\u{2708}\u{FE0F}Final"), "\u{2708}Final");
        assert_eq!(Span::raw("\u{2708}Final").width(), 6);
        // Already-emoji chars are untouched.
        assert_eq!(&*strip_vs16("\u{1F3AF}Direct"), "\u{1F3AF}Direct");
        assert_eq!(Span::raw("\u{1F3AF}Direct").width(), 8);
        // Plain text borrows (hot path: no allocation).
        assert!(matches!(strip_vs16("plain"), Cow::Borrowed(_)));
    }

    #[test]
    fn panel_title_aligns_with_list_gutter() {
        use ratatui::{
            Terminal,
            backend::TestBackend,
            layout::Rect,
            widgets::{HighlightSpacing, List, ListItem, ListState},
        };

        // Borderless panels carry no left padding, and titles ignore
        // padding (`title x = area.left + border`), so the title text
        // must start at the same x as the list highlight gutter.
        // Titles passed to panel()/focus_panel() therefore carry no
        // leading space; this test pins that alignment.
        let backend = TestBackend::new(40, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let mut state = ListState::default().with_selected(Some(0));
                frame.render_stateful_widget(
                    List::new(vec![ListItem::new("row")])
                        .highlight_symbol("▎ ")
                        .highlight_spacing(HighlightSpacing::Always)
                        .block(focus_panel("Title ", true, &Theme::default())),
                    Rect::new(0, 0, 40, 5),
                    &mut state,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        assert_eq!(buffer.cell((0, 0)).unwrap().symbol(), "T");
        assert_eq!(buffer.cell((0, 2)).unwrap().symbol(), "▎");
    }

    #[test]
    fn input_view_keeps_cursor_visible() {
        // Fits: whole value, cursor passes through.
        assert_eq!(input_view("abc", 1, 10), ("abc".into(), 1));
        // End: historical tail view.
        let (visible, offset) = input_view("localhost,127.0.0.1,::1", 23, 10);
        assert_eq!(visible.chars().count(), 10);
        assert_eq!(offset, 10);
        assert!(visible.starts_with('…'));
        // Near end but not at end: still tail view, cursor one left.
        let (visible, offset) = input_view("localhost,127.0.0.1,::1", 22, 10);
        assert_eq!(visible.chars().count(), 10);
        assert_eq!(offset, 9);
        // Middle: window contains the cursor with markers.
        let (visible, offset) = input_view("localhost,127.0.0.1,::1", 12, 10);
        assert_eq!(visible.chars().count(), 10);
        assert!(visible.starts_with('…'));
        assert!(visible.ends_with('…'));
        let before: String = visible.chars().take(offset).collect();
        assert_eq!(before.chars().count(), offset);
        // Start: no leading marker, cursor at 0.
        let (visible, offset) = input_view("localhost,127.0.0.1,::1", 0, 10);
        assert_eq!(offset, 0);
        assert!(!visible.starts_with('…'));
    }
}
