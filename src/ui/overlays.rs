use super::layout::centered;
use super::widgets::input_tail;
use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};

pub(crate) fn draw_input(frame: &mut Frame, app: &App) {
    match app.input.as_ref() {
        Some(crate::app::InputMode::ImportProfile) => draw_import_input(frame, app),
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
            "Enter HTTP/SOCKS mixed port (e.g. 7890), core restarts",
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
            "Enter SOCKS5 port (e.g. 7891), core restarts",
        ),
        Some(crate::app::InputMode::EditHttpPort) => draw_text_input(
            frame,
            app,
            " HTTP port ",
            "Enter HTTP port (e.g. 7892), core restarts",
        ),
        Some(crate::app::InputMode::EditRedirPort) => draw_text_input(
            frame,
            app,
            " Redir port ",
            "Enter redir port (e.g. 7893), core restarts",
        ),
        Some(crate::app::InputMode::EditTproxyPort) => draw_text_input(
            frame,
            app,
            " Tproxy port ",
            "Enter tproxy port (e.g. 7894), core restarts",
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
        Some(crate::app::InputMode::EditGeoProxy) => draw_text_input(
            frame,
            app,
            " Geo proxy ",
            "Enter proxy URL (e.g. http://127.0.0.1:7897) or empty for direct",
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
                        .style(Style::default().bg(app.theme.surface))
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
            .style(Style::default().bg(app.theme.surface))
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
    let visible = input_tail(&app.input_buffer, field_width);
    let field_content = if app.input_buffer.is_empty() {
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
                .border_style(Style::default().fg(app.theme.accent))
                .style(Style::default().bg(app.theme.surface_active)),
        ),
        rows[1],
    );
    let cursor_offset = if app.input_buffer.is_empty() {
        0
    } else {
        visible.chars().count() as u16
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
                    .fg(app.theme.background)
                    .bg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Import   ", Style::default().fg(app.theme.foreground)),
            Span::styled(
                " Esc ",
                Style::default()
                    .fg(app.theme.foreground)
                    .bg(app.theme.surface_active)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Cancel   ", Style::default().fg(app.theme.foreground)),
            Span::styled(
                " Ctrl+Shift+V ",
                Style::default()
                    .fg(app.theme.foreground)
                    .bg(app.theme.surface_active)
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
            .style(Style::default().bg(app.theme.surface))
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
    let visible = input_tail(&app.input_buffer, field_width);
    let field_content = if app.input_buffer.is_empty() {
        Line::styled("…", Style::default().fg(app.theme.muted))
    } else {
        Line::styled(visible.clone(), Style::default().fg(app.theme.foreground))
    };
    frame.render_widget(
        Paragraph::new(field_content).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.accent))
                .style(Style::default().bg(app.theme.surface_active)),
        ),
        rows[1],
    );
    let cursor_offset = visible.chars().count() as u16;
    frame.set_cursor_position((
        rows[1].x + 1 + cursor_offset.min(rows[1].width.saturating_sub(2)),
        rows[1].y + 1,
    ));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " Enter ",
                Style::default()
                    .fg(app.theme.background)
                    .bg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Save   ", Style::default().fg(app.theme.foreground)),
            Span::styled(
                " Esc ",
                Style::default()
                    .fg(app.theme.foreground)
                    .bg(app.theme.surface_active)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Cancel", Style::default().fg(app.theme.foreground)),
        ])),
        rows[3],
    );
}

pub(crate) fn draw_core_missing(frame: &mut Frame, app: &App) {
    let Some(dialog) = app.core_missing.as_ref() else {
        return;
    };
    let area = centered(72, 11, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.warning))
        .style(Style::default().bg(app.theme.surface))
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
            "omash could not find a mihomo binary. Download the latest \
             release from GitHub, or point omash at an existing binary.",
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
                    .fg(app.theme.background)
                    .bg(app.theme.accent)
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
                "installs to ~/.local/share/omash/bin/mihomo",
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
            Span::styled("[P] Use an existing mihomo binary", path_style),
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
            let bar: String = "━".repeat(filled.min(bar_width))
                + &"─".repeat(bar_width.saturating_sub(filled));
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        bar,
                        Style::default().fg(app.theme.accent),
                    ),
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
                        .fg(app.theme.background)
                        .bg(app.theme.danger)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Cancel download", Style::default().fg(app.theme.foreground)),
            ])
        } else {
            Line::from(vec![
                Span::styled(
                    " ←/→ ",
                    Style::default()
                        .fg(app.theme.foreground)
                        .bg(app.theme.surface_active),
                ),
                Span::styled(" Switch   ", Style::default().fg(app.theme.foreground)),
                Span::styled(
                    " Enter ",
                    Style::default()
                        .fg(app.theme.background)
                        .bg(app.theme.accent),
                ),
                Span::styled(" Confirm   ", Style::default().fg(app.theme.foreground)),
                Span::styled(
                    " Esc ",
                    Style::default()
                        .fg(app.theme.foreground)
                        .bg(app.theme.surface_active),
                ),
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
            .style(Style::default().bg(app.theme.surface))
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
    let visible = input_tail(&app.input_buffer, field_width);
    let field_content = if app.input_buffer.is_empty() {
        Line::styled("/usr/bin/mihomo", Style::default().fg(app.theme.muted))
    } else {
        Line::styled(visible.clone(), Style::default().fg(app.theme.foreground))
    };
    frame.render_widget(
        Paragraph::new(field_content).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.accent))
                .style(Style::default().bg(app.theme.surface_active)),
        ),
        rows[1],
    );
    let cursor_offset = visible.chars().count() as u16;
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
