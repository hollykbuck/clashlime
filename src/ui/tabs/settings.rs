use super::super::layout::settings_areas;
use super::super::widgets::{on_off, panel, selection_style, value_or_dash};
use crate::app::App;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
};

pub(crate) fn settings(frame: &mut Frame, app: &App, area: Rect) {
    let [settings_area, updates_area] = settings_areas(area);
    let dns_servers = if app.config.dns.nameserver.is_empty() {
        "—".into()
    } else {
        app.config.dns.nameserver.join(", ")
    };
    let values = [
        ("Keep Mihomo running", on_off(app.supervisor.enabled)),
        ("Start on login", on_off(app.config.auto_start)),
        ("System proxy", on_off(app.config.system_proxy)),
        ("Allow LAN", on_off(app.config.allow_lan)),
        ("IPv6", on_off(app.config.ipv6)),
        ("Refresh interval", format!("{} ms", app.config.refresh_ms)),
        ("DNS enable", on_off(app.config.dns.enable)),
        (
            "DNS listen",
            if app.config.dns.enable {
                app.config.dns.listen.clone()
            } else {
                "— (enable DNS first)".into()
            },
        ),
        ("DNS servers", dns_servers),
        (
            "Geo data",
            if app.geo_updating() {
                "Updating… · Esc cancels".into()
            } else {
                crate::geo::summary()
            },
        ),
        (
            "Geo mirror",
            app.config
                .geo
                .mirror
                .clone()
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| "— (direct GitHub)".into()),
        ),
        ("Geo auto update", on_off(app.config.geo.auto_update)),
        (
            "Geo interval",
            if app.config.geo.auto_update {
                format!("{} h", app.config.geo.update_interval)
            } else {
                "— (auto update off)".into()
            },
        ),
        (
            "Geo proxy",
            app.config
                .geo
                .proxy
                .clone()
                .filter(|p| !p.trim().is_empty())
                .unwrap_or_else(|| "— (direct access)".into()),
        ),
    ];
    let items: Vec<_> = values
        .into_iter()
        .map(|(name, value)| {
            ListItem::new(Line::from(vec![
                Span::raw(format!("{name:<28}")),
                Span::styled(value, Style::default().fg(app.theme.accent)),
            ]))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.setting_index));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("▎ ")
            .highlight_style(selection_style(true, &app.theme))
            .block(panel(" Settings ", &app.theme)),
        settings_area,
        &mut state,
    );
    // Mihomo update panel (GitHub Releases)
    let current = if !app.mihomo_update.current.is_empty() {
        app.mihomo_update.current.clone()
    } else {
        value_or_dash(&app.snapshot.version.version).to_owned()
    };
    let (latest_text, latest_style) = if app.mihomo_update.checking {
        ("checking…".to_owned(), Style::default().fg(app.theme.muted))
    } else if let Some(latest) = &app.mihomo_update.latest {
        let available = app.mihomo_update.available.unwrap_or(false);
        let color = if available {
            app.theme.warning
        } else {
            app.theme.success
        };
        let suffix = if available {
            " → update"
        } else {
            " ✓ up to date"
        };
        (
            format!("{latest}{suffix}"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )
    } else if !app.mihomo_update.message.is_empty() && app.mihomo_update.message.contains("failed")
    {
        (
            app.mihomo_update.message.clone(),
            Style::default().fg(app.theme.danger),
        )
    } else {
        (
            "not checked · press u".to_owned(),
            Style::default().fg(app.theme.muted),
        )
    };
    let url_line = if let Some(url) = &app.mihomo_update.html_url {
        Line::from(vec![
            Span::styled("URL     ", Style::default().fg(app.theme.muted)),
            Span::styled(url.clone(), Style::default().fg(app.theme.accent)),
        ])
    } else {
        Line::from(vec![
            Span::styled("Tip     ", Style::default().fg(app.theme.muted)),
            Span::styled(
                "u check · U force · o open releases",
                Style::default().fg(app.theme.muted),
            ),
        ])
    };
    let prerelease_marker = if app.mihomo_update.prerelease {
        Span::styled(" (pre)", Style::default().fg(app.theme.warning))
    } else {
        Span::raw("")
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Mihomo  ", Style::default().fg(app.theme.muted)),
                Span::styled(current, Style::default().fg(app.theme.foreground)),
                prerelease_marker,
            ]),
            Line::from(vec![
                Span::styled("Latest  ", Style::default().fg(app.theme.muted)),
                Span::styled(latest_text, latest_style),
            ]),
            Line::from(vec![
                Span::styled("GeoIP   ", Style::default().fg(app.theme.muted)),
                Span::styled(
                    &app.geoip_version,
                    Style::default().fg(app.theme.foreground),
                ),
            ]),
            url_line,
        ])
        .block(panel(" Mihomo update (GitHub) ", &app.theme)),
        updates_area,
    );
}
