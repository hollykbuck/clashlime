use super::types::{HitRegion, HitTarget};
use super::widgets::short_title;
use crate::app::Tab;
use ratatui::layout::{Constraint, Layout, Margin, Rect};

#[derive(Clone, Copy)]
pub(crate) struct ShellAreas {
    pub(crate) topbar: Rect,
    pub(crate) sidebar: Rect,
    pub(crate) header: Rect,
    pub(crate) content: Rect,
    pub(crate) status: Rect,
    pub(crate) wide: bool,
}

pub(crate) fn shell_areas(area: Rect) -> ShellAreas {
    let outer = area.inner(Margin::new(1, 0));
    if area.width >= 88 && area.height >= 24 {
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(outer);
        let columns = Layout::horizontal([
            Constraint::Length(23),
            Constraint::Length(2),
            Constraint::Min(40),
        ])
        .split(rows[1]);
        let main = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(4),
        ])
        .split(columns[2]);
        ShellAreas {
            topbar: rows[0],
            sidebar: columns[0],
            header: main[0],
            content: main[2].inner(Margin::new(1, 0)),
            status: rows[2],
            wide: true,
        }
    } else {
        let rows = Layout::vertical([Constraint::Min(8), Constraint::Length(3)]).split(outer);
        let main = Layout::vertical([
            Constraint::Length(4),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(4),
        ])
        .split(rows[0]);
        ShellAreas {
            topbar: main[0],
            sidebar: Rect::new(0, 0, 0, 0),
            header: main[1],
            content: main[3].inner(Margin::new(1, 0)),
            status: rows[1],
            wide: false,
        }
    }
}

/// Split a wide-mode topbar into brand and tab-strip areas.
pub(crate) fn topbar_areas(area: Rect) -> [Rect; 2] {
    let areas = Layout::horizontal([Constraint::Length(16), Constraint::Min(10)]).split(area);
    [areas[0], areas[1]]
}

/// Click regions for the tab strip, mirroring ratatui `Tabs` rendering
/// exactly: each tab occupies
/// `[padding_left(1)][title][padding_right(1)][divider(1)]`
/// starting at `area.x`. (The defaults are single-space padding and our
/// divider is `" "`.)
pub(crate) fn tab_regions(area: Rect) -> Vec<HitRegion> {
    let mut x = area.x;
    Tab::ALL
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, tab)| {
            let title_width = format!(" {} {} ", index + 1, short_title(tab))
                .chars()
                .count() as u16;
            let full = title_width + 3;
            let visible = full.min(area.right().saturating_sub(x));
            let region = (visible > 0).then_some(HitRegion {
                area: Rect::new(x, area.y, visible, 1),
                target: HitTarget::Tab(tab),
            });
            x = x.saturating_add(full);
            region
        })
        .collect()
}

pub(crate) fn list_regions(
    area: Rect,
    len: usize,
    selected: usize,
    has_header: bool,
    target: impl Fn(usize) -> HitTarget,
) -> Vec<HitRegion> {
    let inner = area.inner(Margin::new(1, 1));
    let header_height = if has_header { 2 } else { 0 };
    let capacity = inner.height.saturating_sub(header_height) as usize;
    if capacity == 0 || len == 0 {
        return Vec::new();
    }
    let start = visible_start(selected, len, capacity);
    let visible = (len - start).min(capacity);
    (0..visible)
        .map(|offset| HitRegion {
            area: Rect::new(
                inner.x,
                inner.y + header_height + offset as u16,
                inner.width,
                1,
            ),
            target: target(start + offset),
        })
        .collect()
}

pub(crate) fn visible_start(selected: usize, len: usize, capacity: usize) -> usize {
    if len <= capacity || selected < capacity {
        0
    } else {
        selected.saturating_add(1).saturating_sub(capacity)
    }
}

pub(crate) fn sidebar_mode_button_areas(area: Rect) -> Option<[Rect; 3]> {
    if area.height < 18 {
        return None;
    }
    let row = Rect::new(
        area.x + 3,
        area.bottom() - 2,
        area.width.saturating_sub(6),
        1,
    );
    let columns = Layout::horizontal([Constraint::Ratio(1, 3); 3]).split(row);
    Some([columns[0], columns[1], columns[2]])
}

pub(crate) fn proxy_columns(area: Rect) -> Vec<Rect> {
    Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
        .spacing(2)
        .split(area)
        .iter()
        .copied()
        .collect()
}

pub(crate) fn settings_areas(area: Rect) -> [Rect; 3] {
    let areas = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(6),
    ])
    .split(area);
    [areas[0], areas[1], areas[2]]
}

pub(crate) fn centered(percent: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent) / 2),
        Constraint::Percentage(percent),
        Constraint::Percentage((100 - percent) / 2),
    ])
    .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_selected_row_visible() {
        assert_eq!(visible_start(0, 20, 5), 0);
        assert_eq!(visible_start(4, 20, 5), 0);
        assert_eq!(visible_start(7, 20, 5), 3);
    }

    #[test]
    fn tab_regions_align_with_rendered_tabs_widget() {
        use ratatui::{backend::TestBackend, style::Modifier, text::Line, Terminal};
        use ratatui::widgets::Tabs;

        // Render the real Tabs widget with each tab selected in turn; every
        // highlighted title cell must fall inside that tab's click region.
        for (i, tab) in Tab::ALL.iter().copied().enumerate() {
            let backend = TestBackend::new(100, 3);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|frame| {
                    let area = Rect::new(0, 1, 100, 1);
                    let titles = Tab::ALL.iter().enumerate().map(|(n, t)| {
                        Line::from(format!(" {} {} ", n + 1, short_title(*t)))
                    });
                    frame.render_widget(
                        Tabs::new(titles).select(i).divider(" ").highlight_style(
                            ratatui::style::Style::default().add_modifier(Modifier::BOLD),
                        ),
                        area,
                    );
                })
                .unwrap();
            let regions = tab_regions(Rect::new(0, 1, 100, 1));
            assert_eq!(regions[i].target, HitTarget::Tab(tab));
            let region = regions[i].area;
            let buffer = terminal.backend().buffer().clone();
            let mut highlighted = 0;
            for x in 0..100 {
                let cell = buffer.cell((x, 1)).unwrap();
                if cell.modifier.contains(Modifier::BOLD) {
                    highlighted += 1;
                    assert!(
                        x >= region.x && x < region.right(),
                        "tab {i} highlight at x={x} outside {region:?}"
                    );
                }
            }
            assert!(highlighted > 0, "tab {i} has no highlighted cells");
        }
    }

    #[test]
    fn tab_regions_match_tabs_widget_geometry() {
        use super::super::types::HitTarget;
        use crate::app::Tab;

        // " 1 Home " is 8 wide -> 1 + 8 + 1 + 1 = 11 per the Tabs layout.
        let regions = tab_regions(Rect::new(10, 5, 100, 1));
        assert_eq!(regions.len(), Tab::ALL.len());
        assert_eq!(regions[0].area, Rect::new(10, 5, 11, 1));
        assert_eq!(regions[1].area, Rect::new(21, 5, 12, 1));
        assert!(matches!(regions[0].target, HitTarget::Tab(Tab::Dashboard)));
        assert!(matches!(regions[1].target, HitTarget::Tab(Tab::Proxies)));
        // Adjacent regions tile without gaps or overlap.
        for pair in regions.windows(2) {
            assert_eq!(pair[0].area.right(), pair[1].area.x);
        }
    }
}
