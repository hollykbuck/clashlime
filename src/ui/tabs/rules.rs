use super::super::widgets::{panel, selection_style, strip_vs16};
use crate::api::{Rule, RuleProvider};
use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState},
};

/// Rules surviving the substring filter, with their original indices so
/// the cursor, mouse regions and detail view all address the same rows.
pub(crate) fn filtered_rules(app: &App) -> Vec<(usize, &Rule)> {
    let query = app.rule_query.to_lowercase();
    app.snapshot
        .rules
        .rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| {
            query.is_empty()
                || rule.kind.to_lowercase().contains(&query)
                || rule.payload.to_lowercase().contains(&query)
                || rule.proxy.to_lowercase().contains(&query)
        })
        .collect()
}

/// Sorted rule providers for the header panel.
pub(crate) fn sorted_providers(app: &App) -> Vec<(&String, &RuleProvider)> {
    let mut providers: Vec<_> = app.snapshot.rule_providers.providers.iter().collect();
    providers.sort_by_key(|(name, _)| name.to_lowercase());
    providers
}

/// Table area below the providers panel (`None` panel when no providers).
/// `shell.rs` uses the same split so mouse regions match rendered rows.
pub(crate) fn rules_table_area(area: Rect, provider_count: usize) -> Rect {
    if provider_count == 0 {
        return area;
    }
    let panel_h = (provider_count as u16 + 2).min(8).min(area.height);
    Rect::new(
        area.x,
        area.y + panel_h,
        area.width,
        area.height.saturating_sub(panel_h),
    )
}

fn provider_line(name: &str, provider: &RuleProvider) -> Line<'static> {
    let mut updated = provider.updated_at.replace('T', " ");
    updated.truncate(19);
    Line::from(vec![
        Span::raw(strip_vs16(name).into_owned()),
        Span::raw(format!(
            " · {} · {} rules · {} · {}",
            if provider.behavior.is_empty() {
                "?"
            } else {
                &provider.behavior
            },
            provider.rule_count,
            if updated.is_empty() { "never updated" } else { &updated },
            if provider.vehicle_type.is_empty() {
                "?"
            } else {
                &provider.vehicle_type
            },
        )),
    ])
}

pub(crate) fn rules(frame: &mut Frame, app: &mut App, area: Rect) {
    let providers = sorted_providers(app);
    if !providers.is_empty() {
        let panel_h = (providers.len() as u16 + 2).min(8).min(area.height);
        let [panel_area, _] =
            Layout::vertical([Constraint::Length(panel_h), Constraint::Fill(1)]).areas(area);
        let lines: Vec<_> = providers
            .iter()
            .map(|(name, provider)| provider_line(name, provider))
            .collect();
        frame.render_widget(
            Paragraph::new(lines).block(panel(" Rule providers · u to update ", &app.theme)),
            panel_area,
        );
    }
    let table_area = rules_table_area(area, providers.len());
    let view = filtered_rules(app);
    let total_rules = app.snapshot.rules.rules.len();
    let rows = view.iter().map(|(_, rule)| {
        let match_badge = rule.kind.eq_ignore_ascii_case("match");
        let policy_text = strip_vs16(&rule.proxy).into_owned();
        let policy = if match_badge {
            Cell::from(policy_text).style(
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Cell::from(policy_text)
        };
        let mut row = Row::new([
            Cell::from(rule.kind.as_str()),
            Cell::from(rule.extra.hit_count.to_string()),
            Cell::from(strip_vs16(&rule.payload).into_owned()),
            policy,
        ]);
        if match_badge {
            row = row.style(Style::default().add_modifier(Modifier::BOLD));
        }
        row
    });
    let query = if app.rule_query.is_empty() {
        String::new()
    } else {
        format!(" · /{}", app.rule_query)
    };
    let title = format!(
        " Rules · {}/{} · {} providers{} ",
        view.len(),
        total_rules,
        providers.len(),
        query,
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Length(9),
            Constraint::Fill(1),
            Constraint::Length(20),
        ],
    )
    .header(
        Row::new(["Type", "Hits", "Payload", "Policy"])
            .style(
                Style::default()
                    .fg(app.theme.muted)
                    .add_modifier(Modifier::BOLD),
            )
            .bottom_margin(1),
    )
    .row_highlight_style(selection_style(true, &app.theme))
    .highlight_symbol("▎ ")
    .block(panel(&title, &app.theme));
    let mut state = TableState::default().with_selected(Some(app.rule_index));
    frame.render_stateful_widget(table, table_area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_line_formats_missing_fields() {
        let line = provider_line(
            "p",
            &RuleProvider {
                name: "p".into(),
                ..Default::default()
            },
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("never updated"), "{text}");
    }

    #[test]
    fn rule_response_keeps_hit_stats() {
        use crate::api::RuleResponse;
        let response: RuleResponse = serde_json::from_str(
            r#"{"rules":[{"type":"DomainSuffix","payload":"google.com","proxy":"Auto","size":128,"extra":{"hitCount":42,"hitAt":"2026-09-12T16:02:11+08:00","missCount":3}}]}"#,
        )
        .unwrap();
        let rule = &response.rules[0];
        assert_eq!(rule.extra.hit_count, 42);
        assert_eq!(rule.size, 128);
        assert_eq!(rule.proxy, "Auto");
    }
}
