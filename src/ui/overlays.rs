use super::layout::centered;
use super::widgets::{input_view, panel};
use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};

pub(crate) fn draw_input(frame: &mut Frame, app: &App) {
    match app.ui.input.as_ref() {
        Some(crate::app::InputMode::ImportProfile) => draw_import_input(frame, app),
        Some(crate::app::InputMode::SearchLogs) => draw_text_input(
            frame,
            app,
            " Search logs ",
            "Substring filter (case-insensitive) or empty to clear",
        ),
        Some(crate::app::InputMode::SearchRules) => draw_text_input(
            frame,
            app,
            " Search rules ",
            "Filter by type, payload or policy (case-insensitive)",
        ),
        Some(crate::app::InputMode::SearchNodes) => draw_text_input(
            frame,
            app,
            " Search nodes ",
            "Filter by node name (case-insensitive)",
        ),
        Some(crate::app::InputMode::CorePath) => draw_core_path_input(frame, app),
        Some(crate::app::InputMode::EditDnsListen) => draw_text_input(
            frame,
            app,
            " DNS listen ",
            "Enter DNS listen address (e.g. 0.0.0.0:1053)",
        ),
        Some(crate::app::InputMode::EditDnsServers) => draw_text_input(
            frame,
            app,
            " DNS servers ",
            "Enter comma-separated DNS servers (e.g. 223.5.5.5, 8.8.8.8, tls://9.9.9.9)",
        ),
        Some(crate::app::InputMode::EditDnsFakeIpRange) => draw_text_input(
            frame,
            app,
            " Fake IP range ",
            "Enter fake IP range (e.g. 198.18.0.1/16) or empty to clear",
        ),
        Some(crate::app::InputMode::EditDnsFakeIpFilter) => draw_text_input(
            frame,
            app,
            " Fake IP filter ",
            "Enter comma-separated filter entries or empty to clear",
        ),
        Some(crate::app::InputMode::EditDnsDefaultNs) => draw_text_input(
            frame,
            app,
            " Default nameserver ",
            "Enter comma-separated servers for bootstrapping DoT/DoH or empty to clear",
        ),
        Some(crate::app::InputMode::EditDnsDirectNs) => draw_text_input(
            frame,
            app,
            " Direct nameserver ",
            "Enter comma-separated servers for direct rules or empty to clear",
        ),
        Some(crate::app::InputMode::EditDnsProxyNs) => draw_text_input(
            frame,
            app,
            " Proxy nameserver ",
            "Enter comma-separated servers for proxy nodes or empty to clear",
        ),
        Some(crate::app::InputMode::EditDnsFallback) => draw_text_input(
            frame,
            app,
            " DNS fallback ",
            "Enter comma-separated fallback servers or empty to clear",
        ),
        Some(crate::app::InputMode::EditDnsFallbackGeoCode) => draw_text_input(
            frame,
            app,
            " Fallback GeoIP code ",
            "Enter GeoIP country code for fallback filter (e.g. CN) or empty to clear",
        ),
        Some(crate::app::InputMode::EditMixedPort) => draw_text_input(
            frame,
            app,
            " Mixed port ",
            "Enter HTTP/SOCKS mixed port (e.g. 7890) or empty to disable, core restarts",
        ),
        Some(crate::app::InputMode::EditController) => draw_text_input(
            frame,
            app,
            " Controller ",
            "Enter external controller URL (e.g. http://127.0.0.1:9090)",
        ),
        Some(crate::app::InputMode::EditSecret) => draw_text_input(
            frame,
            app,
            " Controller secret ",
            "Enter controller secret or empty for none",
        ),
        Some(crate::app::InputMode::EditProxyBypass) => draw_text_input(
            frame,
            app,
            " Proxy bypass ",
            "Enter comma-separated bypass hosts or empty for default",
        ),
        Some(crate::app::InputMode::EditDelayTestUrl) => draw_text_input(
            frame,
            app,
            " Delay test URL ",
            "Enter URL for node latency tests (e.g. https://www.gstatic.com/generate_204)",
        ),
        Some(crate::app::InputMode::EditSocksPort) => draw_text_input(
            frame,
            app,
            " Socks port ",
            "Enter SOCKS5 port (e.g. 7892) or empty to disable, core restarts",
        ),
        Some(crate::app::InputMode::EditHttpPort) => draw_text_input(
            frame,
            app,
            " HTTP port ",
            "Enter HTTP port (e.g. 7891) or empty to disable, core restarts",
        ),
        Some(crate::app::InputMode::EditRedirPort) => draw_text_input(
            frame,
            app,
            " Redir port ",
            "Enter redir port (e.g. 7893) or empty to disable, core restarts",
        ),
        Some(crate::app::InputMode::EditTproxyPort) => draw_text_input(
            frame,
            app,
            " Tproxy port ",
            "Enter tproxy port (e.g. 7894) or empty to disable, core restarts",
        ),
        Some(crate::app::InputMode::EditAuth) => draw_text_input(
            frame,
            app,
            " Authentication ",
            "Enter user:pass pairs comma-separated or empty to clear",
        ),
        Some(crate::app::InputMode::EditSkipAuth) => draw_text_input(
            frame,
            app,
            " Skip auth prefixes ",
            "Enter IP prefixes comma-separated or empty to clear",
        ),
        Some(crate::app::InputMode::EditLanAllowed) => draw_text_input(
            frame,
            app,
            " LAN allowed IPs ",
            "Enter allowed LAN IPs/CIDRs comma-separated or empty to clear",
        ),
        Some(crate::app::InputMode::EditLanDisallowed) => draw_text_input(
            frame,
            app,
            " LAN disallowed IPs ",
            "Enter blocked LAN IPs/CIDRs comma-separated or empty to clear",
        ),
        Some(crate::app::InputMode::EditTunDevice) => draw_text_input(
            frame,
            app,
            " TUN device ",
            "Enter TUN device name or empty for auto",
        ),
        Some(crate::app::InputMode::EditTunDnsHijack) => draw_text_input(
            frame,
            app,
            " TUN DNS hijack ",
            "Enter hijacked DNS servers comma-separated or empty to clear",
        ),
        Some(crate::app::InputMode::EditTunMtu) => draw_text_input(
            frame,
            app,
            " TUN MTU ",
            "Enter MTU 68-9000 or empty for auto",
        ),
        Some(crate::app::InputMode::EditTunRouteExclude) => draw_text_input(
            frame,
            app,
            " TUN route exclude ",
            "Enter IPs/CIDRs comma-separated (e.g. 192.168.0.0/16) or empty to clear",
        ),
        Some(crate::app::InputMode::EditSniffHttpPorts) => draw_text_input(
            frame,
            app,
            " Sniff HTTP ports ",
            "Enter ports/ranges comma-separated (e.g. 80, 8080-8880) or empty to clear",
        ),
        Some(crate::app::InputMode::EditSniffTlsPorts) => draw_text_input(
            frame,
            app,
            " Sniff TLS ports ",
            "Enter ports/ranges comma-separated (e.g. 443, 8443) or empty to clear",
        ),
        Some(crate::app::InputMode::EditGeoMirror) => draw_text_input(
            frame,
            app,
            " Geo mirror ",
            "Enter mirror prefix (e.g. https://gh-proxy.com) or empty for direct",
        ),
        Some(crate::app::InputMode::EditGeoIpUrl) => draw_text_input(
            frame,
            app,
            " GeoIP URL ",
            "Full geoip.dat URL or empty for mirror/direct",
        ),
        Some(crate::app::InputMode::EditGeositeUrl) => draw_text_input(
            frame,
            app,
            " Geosite URL ",
            "Full geosite.dat URL or empty for mirror/direct",
        ),
        Some(crate::app::InputMode::EditMmdbUrl) => draw_text_input(
            frame,
            app,
            " MMDB URL ",
            "Full geoip.metadb URL or empty for mirror/direct",
        ),
        Some(crate::app::InputMode::EditAsnUrl) => draw_text_input(
            frame,
            app,
            " ASN URL ",
            "Full ASN mmdb URL or empty for mirror/direct",
        ),
        Some(crate::app::InputMode::EditGeoProxy) => draw_text_input(
            frame,
            app,
            " Geo proxy ",
            "Enter proxy URL (e.g. http://127.0.0.1:7897) or empty for direct",
        ),
        Some(crate::app::InputMode::EditProfileName) => draw_text_input(
            frame,
            app,
            " Profile name ",
            "Display name, 1-100 characters",
        ),
        Some(crate::app::InputMode::EditProfileInterval) => draw_text_input(
            frame,
            app,
            " Update interval ",
            "Hours between auto-updates 1-8760, or empty for off",
        ),
        Some(crate::app::InputMode::EditProfileTimeout) => draw_text_input(
            frame,
            app,
            " Update timeout ",
            "Fetch timeout in seconds 1-600, or empty for default 30",
        ),
        Some(crate::app::InputMode::EditProfileAuth) => draw_text_input(
            frame,
            app,
            " Auth token ",
            "Sent as Authorization header, or empty to clear",
        ),
        Some(crate::app::InputMode::EditProfileUserAgent) => draw_text_input(
            frame,
            app,
            " User-Agent ",
            "Custom UA for subscription fetch, or empty for default",
        ),
        Some(crate::app::InputMode::RestoreBackup(path)) => {
            let area = centered(76, 7, frame.area());
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(format!(
                    "Overwrite current configuration with {}?",
                    path.display()
                ))
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(app.theme.foreground))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(app.theme.warning))
                        .padding(Padding::new(2, 2, 1, 1))
                        .title(Span::styled(
                            " Confirm restore · y Yes · n/Esc Cancel ",
                            Style::default()
                                .fg(app.theme.warning)
                                .add_modifier(Modifier::BOLD),
                        )),
                ),
                area,
            );
        }
        None => {}
    }
}

fn draw_import_input(frame: &mut Frame, app: &App) {
    let area = centered(82, 12, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.accent))
            .title(Span::styled(
                " Import profile ",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
        area,
    );

    let inner = area.inner(Margin::new(2, 1));
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new("Paste a subscription URL or enter an absolute local YAML path.")
            .style(Style::default().fg(app.theme.foreground)),
        rows[0],
    );

    let field_width = rows[1].width.saturating_sub(2) as usize;
    let (visible, cursor_offset) = input_view(&app.ui.input_buffer, app.ui.input_cursor, field_width);
    let field_content = if app.ui.input_buffer.is_empty() {
        Line::styled(
            "https://… or /home/you/Downloads/config.yaml",
            Style::default().fg(app.theme.muted),
        )
    } else {
        Line::styled(visible.clone(), Style::default().fg(app.theme.foreground))
    };
    frame.render_widget(
        Paragraph::new(field_content).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.accent)),
        ),
        rows[1],
    );
    let cursor_offset = if app.ui.input_buffer.is_empty() {
        0
    } else {
        cursor_offset as u16
    };
    frame.set_cursor_position((
        rows[1].x + 1 + cursor_offset.min(rows[1].width.saturating_sub(2)),
        rows[1].y + 1,
    ));

    frame.render_widget(
        Paragraph::new("The profile is validated before Mihomo starts.")
            .style(Style::default().fg(app.theme.muted)),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("URL   ", Style::default().fg(app.theme.muted)),
            Span::raw("https://example.com/subscription"),
        ])),
        rows[4],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("FILE  ", Style::default().fg(app.theme.muted)),
            Span::raw("/home/you/Downloads/config.yaml"),
        ])),
        rows[5],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " Enter ",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Import   ", Style::default().fg(app.theme.foreground)),
            Span::styled(
                " Esc ",
                Style::default()
                    .fg(app.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Cancel   ", Style::default().fg(app.theme.foreground)),
            Span::styled(
                " Ctrl+Shift+V ",
                Style::default()
                    .fg(app.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Paste", Style::default().fg(app.theme.foreground)),
        ])),
        rows[7],
    );
}

fn draw_text_input(frame: &mut Frame, app: &App, title: &str, hint: &str) {
    let area = centered(82, 10, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.accent))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
        area,
    );
    let inner = area.inner(Margin::new(2, 1));
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(app.theme.foreground)),
        rows[0],
    );
    let field_width = rows[1].width.saturating_sub(2) as usize;
    let (visible, cursor_offset) = input_view(&app.ui.input_buffer, app.ui.input_cursor, field_width);
    let field_content = if app.ui.input_buffer.is_empty() {
        Line::styled("…", Style::default().fg(app.theme.muted))
    } else {
        Line::styled(visible.clone(), Style::default().fg(app.theme.foreground))
    };
    frame.render_widget(
        Paragraph::new(field_content).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.accent)),
        ),
        rows[1],
    );
    let cursor_offset = cursor_offset as u16;
    frame.set_cursor_position((
        rows[1].x + 1 + cursor_offset.min(rows[1].width.saturating_sub(2)),
        rows[1].y + 1,
    ));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " Enter ",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Save   ", Style::default().fg(app.theme.foreground)),
            Span::styled(
                " Esc ",
                Style::default()
                    .fg(app.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Cancel", Style::default().fg(app.theme.foreground)),
        ])),
        rows[3],
    );
}

/// Full text of one log line (or rule), wrapped. Opened with Enter on
/// the Logs/Rules tabs; Esc closes it.
pub(crate) fn draw_log_detail(frame: &mut Frame, app: &App) {
    let Some(line) = app.ui.log_detail.as_deref() else {
        return;
    };
    let title = if app.ui.tab == crate::app::Tab::Rules {
        " Rule "
    } else {
        " Log line "
    };
    let height = (frame.area().height * 60 / 100).clamp(8, 30);
    let area = centered(84, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.accent))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
        area,
    );
    let inner = area.inner(Margin::new(2, 1));
    let rows = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).split(inner);
    frame.render_widget(
        Paragraph::new(super::widgets::strip_vs16(line).into_owned())
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(app.theme.foreground)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " Esc ",
                Style::default()
                    .fg(app.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Close", Style::default().fg(app.theme.foreground)),
        ])),
        rows[1],
    );
}

pub(crate) fn draw_mode_menu(frame: &mut Frame, app: &App) {
    use crate::app::App as AppType;
    let area = centered(58, 10, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        panel("Routing mode ", &app.theme).border_style(Style::default().fg(app.theme.accent)),
        area,
    );
    let inner = area.inner(Margin::new(2, 1));
    let mut lines = Vec::new();
    for (index, (mode, description)) in AppType::MODES.iter().enumerate() {
        let selected = index == app.ui.mode_menu_index;
        let current = app.data.snapshot.config.mode.eq_ignore_ascii_case(mode);
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▸ " } else { "  " },
                Style::default().fg(app.theme.accent),
            ),
            Span::styled(
                format!("{mode:<7}"),
                Style::default()
                    .fg(if selected {
                        app.theme.accent
                    } else {
                        app.theme.foreground
                    })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            Span::styled(
                (*description).to_string(),
                Style::default().fg(app.theme.muted),
            ),
            Span::styled(
                if current { " ●" } else { "" },
                Style::default().fg(app.theme.success),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "r/g/d select · j/k move · Enter confirm · Esc close",
        Style::default().fg(app.theme.muted),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Per-profile update settings (`e` on Profiles). Toggles apply on
/// Enter/Space, text rows drop into a text sub-input; everything saves
/// to profiles.yaml immediately, so closing never loses anything.
pub(crate) fn draw_profile_editor(frame: &mut Frame, app: &App) {
    use crate::profiles::DEFAULT_UPDATE_TIMEOUT_SECS;
    let Some(profile) = app.data.profiles.items.get(app.ui.profile_index) else {
        return;
    };
    let remote = profile.url.is_some();
    let on_off = |enabled: bool| {
        if enabled {
            Span::styled("on", Style::default().fg(app.theme.success))
        } else {
            Span::styled("off", Style::default().fg(app.theme.muted))
        }
    };
    // Local profiles have no subscription fetch: update rows show a dash.
    let dash = Span::styled("—", Style::default().fg(app.theme.muted));
    let rows: Vec<(String, Span, String)> = vec![
        (
            "Name".into(),
            Span::styled(
                profile.name.clone(),
                Style::default().fg(app.theme.foreground),
            ),
            "display name".into(),
        ),
        (
            "Auto update".into(),
            if remote {
                on_off(profile.auto_update_enabled())
            } else {
                dash.clone()
            },
            "automatic refresh when due".into(),
        ),
        (
            "Update interval".into(),
            if remote {
                Span::styled(
                    profile
                        .update_interval
                        .map(|hours| format!("{hours} h"))
                        .unwrap_or_else(|| "off".into()),
                    Style::default().fg(app.theme.foreground),
                )
            } else {
                dash.clone()
            },
            "hours between auto-updates".into(),
        ),
        (
            "Pin interval".into(),
            if remote {
                on_off(profile.interval_pinned())
            } else {
                dash.clone()
            },
            "server header cannot change it".into(),
        ),
        (
            "Update timeout".into(),
            if remote {
                Span::styled(
                    profile
                        .update_timeout
                        .map(|secs| format!("{secs} s"))
                        .unwrap_or_else(|| format!("{DEFAULT_UPDATE_TIMEOUT_SECS} s default")),
                    Style::default().fg(app.theme.foreground),
                )
            } else {
                dash.clone()
            },
            "fetch timeout".into(),
        ),
        (
            "Fetch via proxy".into(),
            if remote {
                on_off(profile.use_proxy.unwrap_or(false))
            } else {
                dash.clone()
            },
            "through the core mixed port".into(),
        ),
        (
            "Auth token".into(),
            if remote {
                Span::styled(
                    if profile.auth_token.as_deref().is_some_and(|t| !t.is_empty()) {
                        "set •••"
                    } else {
                        "unset"
                    },
                    Style::default().fg(app.theme.foreground),
                )
            } else {
                dash.clone()
            },
            "Authorization header".into(),
        ),
        (
            "User-Agent".into(),
            if remote {
                Span::styled(
                    profile
                        .user_agent
                        .clone()
                        .unwrap_or_else(|| "default".into()),
                    Style::default().fg(app.theme.foreground),
                )
            } else {
                dash.clone()
            },
            "subscription fetch UA".into(),
        ),
    ];
    let area = centered(64, (rows.len() + 5) as u16, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        panel(&format!("Update settings · {} ", profile.name), &app.theme)
            .border_style(Style::default().fg(app.theme.accent)),
        area,
    );
    let inner = area.inner(Margin::new(2, 1));
    let mut lines = Vec::new();
    for (index, (label, value, hint)) in rows.iter().enumerate() {
        let selected = index == app.ui.profile_editor_index;
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▸ " } else { "  " },
                Style::default().fg(app.theme.accent),
            ),
            Span::styled(
                format!("{label:<16}"),
                Style::default()
                    .fg(if selected {
                        app.theme.accent
                    } else {
                        app.theme.foreground
                    })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            value.clone(),
            Span::styled(format!("  {hint}"), Style::default().fg(app.theme.muted)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter/Space toggle or edit · j/k move · Esc close",
        Style::default().fg(app.theme.muted),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

pub(crate) fn draw_core_missing(frame: &mut Frame, app: &App) {
    let Some(dialog) = app.ui.core_missing.as_ref() else {
        return;
    };
    let area = centered(72, 11, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.warning))
        .title(Span::styled(
            " Mihomo core not found ",
            Style::default()
                .fg(app.theme.warning)
                .add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(block, area);

    let inner = area.inner(Margin::new(2, 1));
    let rows = Layout::vertical([
        Constraint::Length(2), // explanation
        Constraint::Length(1), // tried path
        Constraint::Length(1), // spacing
        Constraint::Length(1), // download option
        Constraint::Length(1), // provide-path option
        Constraint::Fill(1),   // status message
        Constraint::Length(1), // key hints
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(
            "clashlime could not find a mihomo binary. Download the latest \
             release from GitHub, or point clashlime at an existing binary.",
        )
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(app.theme.foreground)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "Tried: {}",
            crate::config::Config::mihomo_path().display()
        ))
        .style(Style::default().fg(app.theme.muted)),
        rows[1],
    );

    let option = |selected: bool, app: &App| {
        if selected && !dialog.busy {
            (
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
                "▸",
            )
        } else {
            (Style::default().fg(app.theme.muted), " ")
        }
    };

    let (download_style, marker) = option(
        dialog.choice == crate::app::CoreMissingChoice::Download,
        app,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {marker} "), download_style),
            Span::styled("[D] Download latest release  ", download_style),
            Span::styled(
                "installs to ~/.local/share/clashlime/bin/mihomo",
                Style::default().fg(app.theme.muted),
            ),
        ])),
        rows[3],
    );
    let (path_style, marker) = option(
        dialog.choice == crate::app::CoreMissingChoice::ProvidePath,
        app,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {marker} "), path_style),
            Span::styled("[P] Use an existing mihomo binary  ", path_style),
            Span::styled(
                "copies to ~/.local/share/clashlime/bin/mihomo",
                Style::default().fg(app.theme.muted),
            ),
        ])),
        rows[4],
    );

    if dialog.busy || !dialog.message.is_empty() {
        let style = if dialog.busy {
            Style::default().fg(app.theme.accent)
        } else {
            Style::default().fg(app.theme.danger)
        };
        frame.render_widget(
            Paragraph::new(dialog.message.clone())
                .style(style)
                .wrap(Wrap { trim: false }),
            rows[5],
        );
    }
    if let Some((downloaded, total)) = dialog.progress
        && dialog.busy
    {
        let width = rows[5].width.saturating_sub(2) as usize;
        if let Some(total) = total.filter(|total| *total > 0) {
            let ratio = (downloaded.min(total)) as f64 / total as f64;
            let filled = ((width as f64) * ratio).round() as usize;
            let label = format!(
                " {:>7} / {:<7} {:>3.0}%",
                crate::update::format_size(downloaded as usize),
                crate::update::format_size(total as usize),
                ratio * 100.0
            );
            let bar_width = width.saturating_sub(label.chars().count());
            let bar: String =
                "━".repeat(filled.min(bar_width)) + &"─".repeat(bar_width.saturating_sub(filled));
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(bar, Style::default().fg(app.theme.accent)),
                    Span::styled(label, Style::default().fg(app.theme.muted)),
                ])),
                Rect::new(rows[5].x, rows[5].y + 1, rows[5].width, 1),
            );
        } else {
            frame.render_widget(
                Paragraph::new(format!(
                    " {} received",
                    crate::update::format_size(downloaded as usize)
                ))
                .style(Style::default().fg(app.theme.muted)),
                Rect::new(rows[5].x, rows[5].y + 1, rows[5].width, 1),
            );
        }
    }

    let hint_row = |busy| {
        if busy {
            Line::from(vec![
                Span::styled(
                    " Esc ",
                    Style::default()
                        .fg(app.theme.danger)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " Cancel download",
                    Style::default().fg(app.theme.foreground),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled(" ←/→ ", Style::default().fg(app.theme.foreground)),
                Span::styled(" Switch   ", Style::default().fg(app.theme.foreground)),
                Span::styled(" Enter ", Style::default().fg(app.theme.accent)),
                Span::styled(" Confirm   ", Style::default().fg(app.theme.foreground)),
                Span::styled(" Esc ", Style::default().fg(app.theme.foreground)),
                Span::styled(" Skip", Style::default().fg(app.theme.foreground)),
            ])
        }
    };
    frame.render_widget(Paragraph::new(hint_row(dialog.busy)), rows[6]);
}

fn draw_core_path_input(frame: &mut Frame, app: &App) {
    let area = centered(82, 9, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.accent))
            .title(Span::styled(
                " Use existing mihomo binary ",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
        area,
    );
    let inner = area.inner(Margin::new(2, 1));
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new("Enter the absolute path of an existing mihomo binary.")
            .style(Style::default().fg(app.theme.foreground)),
        rows[0],
    );
    let field_width = rows[1].width.saturating_sub(2) as usize;
    let (visible, cursor_offset) = input_view(&app.ui.input_buffer, app.ui.input_cursor, field_width);
    let field_content = if app.ui.input_buffer.is_empty() {
        Line::styled("/usr/bin/mihomo", Style::default().fg(app.theme.muted))
    } else {
        Line::styled(visible.clone(), Style::default().fg(app.theme.foreground))
    };
    frame.render_widget(
        Paragraph::new(field_content).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.accent)),
        ),
        rows[1],
    );
    let cursor_offset = cursor_offset as u16;
    frame.set_cursor_position((
        rows[1].x + 1 + cursor_offset.min(rows[1].width.saturating_sub(2)),
        rows[1].y + 1,
    ));
    frame.render_widget(
        Paragraph::new("The binary is verified with `mihomo -v` before use.")
            .style(Style::default().fg(app.theme.muted)),
        rows[2],
    );
}
