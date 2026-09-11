use super::types::{HitRegion, HitTarget};
use super::widgets::short_title;
use crate::app::Tab;
use ratatui::layout::{Constraint, Layout, Margin, Rect};

#[derive(Clone, Copy)]
pub(crate) struct ShellAreas {
    pub(crate) navigation: Rect,
    pub(crate) header: Rect,
    pub(crate) content: Rect,
    pub(crate) status: Rect,
    pub(crate) wide: bool,
}

pub(crate) fn shell_areas(area: Rect) -> ShellAreas {
    let outer = area.inner(Margin::new(1, 0));
    if area.width >= 88 && area.height >= 24 {
        let rows = Layout::vertical([Constraint::Min(8), Constraint::Length(2)]).split(outer);
        let columns = Layout::horizontal([
            Constraint::Length(23),
            Constraint::Length(2),
            Constraint::Min(40),
        ])
        .split(rows[0]);
        let main = Layout::vertical([
            Constraint::Length(5),
            Constraint::Length(1),
            Constraint::Min(4),
        ])
        .split(columns[2]);
        ShellAreas {
            navigation: columns[0],
            header: main[0],
            content: main[2].inner(Margin::new(1, 0)),
            status: rows[1],
            wide: true,
        }
    } else {
        let rows = Layout::vertical([Constraint::Min(8), Constraint::Length(3)]).split(outer);
        let main = Layout::vertical([
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Length(1),
            Constraint::Min(4),
        ])
        .split(rows[0]);
        ShellAreas {
            navigation: main[0],
            header: main[1],
            content: main[3].inner(Margin::new(1, 0)),
            status: rows[1],
            wide: false,
        }
    }
}

pub(crate) fn tab_regions(area: Rect, wide: bool) -> Vec<HitRegion> {
    if wide {
        Tab::ALL
            .iter()
            .copied()
            .enumerate()
            .map(|(index, tab)| HitRegion {
                area: Rect::new(
                    area.x + 1,
                    area.y + 5 + index as u16,
                    area.width.saturating_sub(2),
                    1,
                ),
                target: HitTarget::Tab(tab),
            })
            .collect()
    } else {
        let tabs = Rect::new(area.x, area.y + 2, area.width, 2);
        let mut x = tabs.x.saturating_add(1);
        Tab::ALL
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(index, tab)| {
                let width = format!(" {} {} ", index + 1, short_title(tab))
                    .chars()
                    .count() as u16;
                let visible = width.min(tabs.right().saturating_sub(x));
                let region = (visible > 0).then_some(HitRegion {
                    area: Rect::new(x, tabs.y, visible, 1),
                    target: HitTarget::Tab(tab),
                });
                x = x.saturating_add(width).saturating_add(1);
                region
            })
            .collect()
    }
}

pub(crate) fn list_regions(
    area: Rect,
    len: usize,
    selected: usize,
    has_header: bool,
    target: fn(usize) -> HitTarget,
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
    if area.height < 28 {
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

pub(crate) fn dashboard_card_areas(area: Rect) -> Vec<Rect> {
    if area.width >= 72 {
        Layout::horizontal([Constraint::Ratio(1, 4); 4])
            .spacing(2)
            .split(Rect::new(area.x, area.y, area.width, area.height.min(5)))
            .iter()
            .copied()
            .collect()
    } else {
        let rows = Layout::vertical([Constraint::Length(5), Constraint::Length(5)])
            .split(Rect::new(area.x, area.y, area.width, area.height.min(10)));
        let top = Layout::horizontal([Constraint::Ratio(1, 2); 2])
            .spacing(2)
            .split(rows[0]);
        let bottom = Layout::horizontal([Constraint::Ratio(1, 2); 2])
            .spacing(2)
            .split(rows[1]);
        vec![top[0], top[1], bottom[0], bottom[1]]
    }
}

pub(crate) fn proxy_columns(area: Rect) -> Vec<Rect> {
    Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
        .spacing(2)
        .split(area)
        .iter()
        .copied()
        .collect()
}

pub(crate) fn settings_areas(area: Rect) -> [Rect; 2] {
    let areas = Layout::vertical([Constraint::Min(5), Constraint::Length(7)]).split(area);
    [areas[0], areas[1]]
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
}
