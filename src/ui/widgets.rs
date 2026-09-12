use crate::app::Tab;
use crate::theme::Theme;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

pub(crate) fn card(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    value: &str,
    color: Color,
    theme: &Theme,
) {
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::styled(
                title,
                Style::default()
                    .fg(theme.muted)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                value,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ])
        .alignment(ratatui::layout::Alignment::Center),
        area,
    );
}

pub(crate) fn inset_panel(area: Rect) -> Rect {
    if area.width > 4 {
        area.inner(ratatui::layout::Margin::new(1, 0))
    } else {
        area
    }
}

pub(crate) fn panel<'a>(title: &'a str, theme: &Theme) -> Block<'a> {
    Block::default()
        .padding(Padding::new(1, 1, 1, 1))
        .title(Span::styled(
            title,
            Style::default()
                .fg(theme.muted)
                .add_modifier(Modifier::BOLD),
        ))
}

pub(crate) fn focus_panel<'a>(title: &'a str, focused: bool, theme: &Theme) -> Block<'a> {
    Block::default()
        .padding(Padding::new(1, 1, 1, 1))
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

pub(crate) fn input_tail(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    let tail = value
        .chars()
        .rev()
        .take(width.saturating_sub(1))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("…{tail}")
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
}
