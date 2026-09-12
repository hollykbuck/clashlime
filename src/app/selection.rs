use super::{SettingSection, Tab};
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
        // Filtered length first: the view borrows all of `self`, which
        // would clash with the `&mut` index below.
        let rules_len = crate::ui::tabs::rules::filtered_rules(self).len();
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
            Tab::Rules => (&mut self.rule_index, rules_len),
            Tab::Settings => (&mut self.setting_index, self.setting_section.row_count()),
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
        if self.tab == Tab::Settings {
            self.section_cursor[self.setting_section.index()] = self.setting_index;
        }
    }

    /// Switch the Settings sub-page, restoring that section's cursor.
    pub(crate) fn move_setting_section(&mut self, delta: isize) {
        self.section_cursor[self.setting_section.index()] = self.setting_index;
        let position = SettingSection::ALL
            .iter()
            .position(|section| *section == self.setting_section)
            .unwrap_or(0);
        let next =
            ((position as isize + delta).rem_euclid(SettingSection::ALL.len() as isize)) as usize;
        self.setting_section = SettingSection::ALL[next];
        self.setting_index =
            self.section_cursor[next].min(self.setting_section.row_count().saturating_sub(1));
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
            crate::ui::tabs::rules::filtered_rules(self)
                .len()
                .saturating_sub(1),
        );
        self.profile_index = min(
            self.profile_index,
            self.profiles.items.len().saturating_sub(1),
        );
        self.setting_index = min(
            self.setting_index,
            self.setting_section.row_count().saturating_sub(1),
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
            ui::HitTarget::Setting(section, row) => {
                if let Some(target) = SettingSection::ALL.get(section) {
                    self.setting_section = *target;
                    self.setting_index = row.min(target.row_count().saturating_sub(1));
                    self.section_cursor[target.index()] = self.setting_index;
                }
            }
            _ => {}
        }
    }

    /// The active profile, for sidebar display.
    pub fn current_profile(&self) -> Option<&crate::profiles::Profile> {
        let uid = self.profiles.current.as_deref()?;
        self.profiles.items.iter().find(|item| item.uid == uid)
    }
}
