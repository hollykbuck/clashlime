use crate::app::Tab;
use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitTarget {
    Tab(Tab),
    CoreToggle,
    RoutingMode(&'static str),
    ProxyGroup(usize),
    ProxyNode(usize),
    Profile(usize),
    Connection(usize),
    Rule(usize),
    Setting(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct HitRegion {
    pub area: Rect,
    pub target: HitTarget,
}

impl HitRegion {
    pub fn contains(self, x: u16, y: u16) -> bool {
        x >= self.area.x && x < self.area.right() && y >= self.area.y && y < self.area.bottom()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_region_excludes_right_and_bottom_edges() {
        let region = HitRegion {
            area: Rect::new(2, 3, 4, 2),
            target: HitTarget::CoreToggle,
        };
        assert!(region.contains(2, 3));
        assert!(region.contains(5, 4));
        assert!(!region.contains(6, 4));
        assert!(!region.contains(5, 5));
    }
}
