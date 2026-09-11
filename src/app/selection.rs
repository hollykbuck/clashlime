use super::{SETTINGS_COUNT, Tab};
use crate::{api, ui};
use std::{cmp::min, collections::HashSet};

impl super::App {
    pub fn proxy_groups(&self) -> Vec<(&String, &api::Proxy)> {
        let mut values: Vec<_> = self
            .proxy_group_order
            .iter()
            .filter_map(|name| self.snapshot.proxies.proxies.get_key_value(name))
            .filter(|(_, proxy)| !proxy.all.is_empty())
            .collect();
        let configured: HashSet<_> = self.proxy_group_order.iter().collect();
        let mut unconfigured: Vec<_> = self
            .snapshot
            .proxies
            .proxies
            .iter()
            .filter(|(name, proxy)| !proxy.all.is_empty() && !configured.contains(name))
            .collect();
        unconfigured.sort_by_key(|item| item.0.to_lowercase());
        values.extend(unconfigured);
        values
    }

    pub fn selected_group(&self) -> Option<(&String, &api::Proxy)> {
        self.proxy_groups().get(self.group_index).copied()
    }

    pub fn selected_group_is_manual(&self) -> bool {
        self.selected_group()
            .is_some_and(|(_, group)| group.kind.eq_ignore_ascii_case("selector"))
    }

    pub(crate) fn open_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    pub(crate) fn tab_shortcut(code: &crossterm::event::KeyCode) -> Option<Tab> {
        Tab::shortcut(code)
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let group_len = self.proxy_groups().len();
        let (index, len) = match self.tab {
            Tab::Proxies if self.node_focus => {
                let len = self
                    .selected_group()
                    .map_or(0, |(_, proxy)| proxy.all.len());
                (&mut self.node_index, len)
            }
            Tab::Proxies => (&mut self.group_index, group_len),
            Tab::Profiles => (&mut self.profile_index, self.profiles.items.len()),
            Tab::Connections => (
                &mut self.connection_index,
                self.snapshot.connections.connections.len(),
            ),
            Tab::Rules => (&mut self.rule_index, self.snapshot.rules.rules.len()),
            Tab::Settings => (&mut self.setting_index, SETTINGS_COUNT),
            _ => return,
        };
        if len == 0 {
            *index = 0;
            return;
        }
        *index = ((*index as isize + delta).rem_euclid(len as isize)) as usize;
        if self.tab == Tab::Proxies && !self.node_focus {
            self.node_index = 0;
        }
    }

    pub(crate) fn clamp_selections(&mut self) {
        self.group_index = min(
            self.group_index,
            self.proxy_groups().len().saturating_sub(1),
        );
        let node_len = self.selected_group().map_or(0, |(_, p)| p.all.len());
        self.node_index = min(self.node_index, node_len.saturating_sub(1));
        self.connection_index = min(
            self.connection_index,
            self.snapshot
                .connections
                .connections
                .len()
                .saturating_sub(1),
        );
        self.rule_index = min(
            self.rule_index,
            self.snapshot.rules.rules.len().saturating_sub(1),
        );
        self.profile_index = min(
            self.profile_index,
            self.profiles.items.len().saturating_sub(1),
        );
    }

    pub(crate) fn focus_mouse_target(&mut self, target: ui::HitTarget) {
        match target {
            ui::HitTarget::ProxyGroup(index) => {
                self.node_focus = false;
                self.group_index = index;
            }
            ui::HitTarget::ProxyNode(index) => {
                self.node_focus = true;
                self.node_index = index;
            }
            ui::HitTarget::Profile(index) => self.profile_index = index,
            ui::HitTarget::Connection(index) => self.connection_index = index,
            ui::HitTarget::Rule(index) => self.rule_index = index,
            ui::HitTarget::Setting(index) => self.setting_index = index,
            _ => {}
        }
    }

    /// The active profile, for sidebar display.
    pub fn current_profile(&self) -> Option<&crate::profiles::Profile> {
        let uid = self.profiles.current.as_deref()?;
        self.profiles
            .items
            .iter()
            .find(|item| item.uid == uid)
    }
}
