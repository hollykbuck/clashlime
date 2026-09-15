use super::{SettingSection, Tab};
use crate::{api, ui};
use std::{cmp::min, collections::HashSet};

impl super::App {
    pub fn proxy_groups(&self) -> Vec<(&String, &api::Proxy)> {
        let mut values: Vec<_> = self
            .data.proxy_group_order
            .iter()
            .filter_map(|name| self.data.snapshot.proxies.proxies.get_key_value(name))
            .filter(|(_, proxy)| !proxy.all.is_empty())
            .collect();
        let configured: HashSet<_> = self.data.proxy_group_order.iter().collect();
        let mut unconfigured: Vec<_> = self
            .data.snapshot
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
        self.proxy_groups().get(self.ui.group_index).copied()
    }

    pub fn selected_group_is_manual(&self) -> bool {
        self.selected_group()
            .is_some_and(|(_, group)| group.kind.eq_ignore_ascii_case("selector"))
    }

    pub(crate) async fn open_tab(&mut self, tab: Tab) {
        self.ui.tab = tab;
        // Rules are lazy-loaded: fetch on open instead of waiting for the
        // next slow tick, so the first paint already has rows. (Don't hint
        // this with a `say("…")` message: `…` infers Busy, which pins
        // forever and no refresh status can replace it.)
        if tab == Tab::Rules && self.data.online && !self.data.rules_loaded {
            self.fetch_rules().await;
        }
    }

    pub(crate) fn tab_shortcut(code: &crossterm::event::KeyCode) -> Option<Tab> {
        Tab::shortcut(code)
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let group_len = self.proxy_groups().len();
        // Filtered length first: the view borrows all of `self`, which
        // would clash with the `&mut` index below.
        let rules_len = crate::ui::tabs::rules::filtered_rules(self).len();
        let nodes_len = crate::ui::tabs::proxies::filtered_nodes(self).len();
        let (index, len) = match self.ui.tab {
            Tab::Proxies if self.ui.node_focus => (&mut self.ui.node_index, nodes_len),
            Tab::Proxies => (&mut self.ui.group_index, group_len),
            Tab::Profiles => (&mut self.ui.profile_index, self.data.profiles.items.len()),
            Tab::Connections => (
                &mut self.ui.connection_index,
                self.data.snapshot.connections.connections.len(),
            ),
            Tab::Rules => (&mut self.ui.rule_index, rules_len),
            Tab::Settings => (&mut self.ui.setting_index, self.ui.setting_section.row_count()),
            _ => return,
        };
        if len == 0 {
            *index = 0;
            return;
        }
        *index = ((*index as isize + delta).rem_euclid(len as isize)) as usize;
        if self.ui.tab == Tab::Proxies && !self.ui.node_focus {
            self.ui.node_index = 0;
        }
        if self.ui.tab == Tab::Settings {
            self.ui.section_cursor[self.ui.setting_section.index()] = self.ui.setting_index;
        }
    }

    /// Switch the Settings sub-page, restoring that section's cursor.
    pub(crate) fn move_setting_section(&mut self, delta: isize) {
        self.ui.section_cursor[self.ui.setting_section.index()] = self.ui.setting_index;
        let position = SettingSection::ALL
            .iter()
            .position(|section| *section == self.ui.setting_section)
            .unwrap_or(0);
        let next =
            ((position as isize + delta).rem_euclid(SettingSection::ALL.len() as isize)) as usize;
        self.ui.setting_section = SettingSection::ALL[next];
        self.ui.setting_index =
            self.ui.section_cursor[next].min(self.ui.setting_section.row_count().saturating_sub(1));
    }

    pub(crate) fn clamp_selections(&mut self) {
        self.ui.group_index = min(
            self.ui.group_index,
            self.proxy_groups().len().saturating_sub(1),
        );
        let node_len = crate::ui::tabs::proxies::filtered_nodes(self).len();
        self.ui.node_index = min(self.ui.node_index, node_len.saturating_sub(1));
        self.ui.connection_index = min(
            self.ui.connection_index,
            self.data.snapshot
                .connections
                .connections
                .len()
                .saturating_sub(1),
        );
        self.ui.rule_index = min(
            self.ui.rule_index,
            crate::ui::tabs::rules::filtered_rules(self)
                .len()
                .saturating_sub(1),
        );
        self.ui.profile_index = min(
            self.ui.profile_index,
            self.data.profiles.items.len().saturating_sub(1),
        );
        self.ui.setting_index = min(
            self.ui.setting_index,
            self.ui.setting_section.row_count().saturating_sub(1),
        );
    }

    pub(crate) fn focus_mouse_target(&mut self, target: ui::HitTarget) {
        match target {
            ui::HitTarget::ProxyGroup(index) => {
                self.ui.node_focus = false;
                self.ui.group_index = index;
            }
            ui::HitTarget::ProxyNode(index) => {
                self.ui.node_focus = true;
                self.ui.node_index = index;
            }
            ui::HitTarget::Profile(index) => self.ui.profile_index = index,
            ui::HitTarget::Connection(index) => self.ui.connection_index = index,
            ui::HitTarget::Rule(index) => self.ui.rule_index = index,
            ui::HitTarget::Setting(section, row) => {
                if let Some(target) = SettingSection::ALL.get(section) {
                    self.ui.setting_section = *target;
                    self.ui.setting_index = row.min(target.row_count().saturating_sub(1));
                    self.ui.section_cursor[target.index()] = self.ui.setting_index;
                }
            }
            _ => {}
        }
    }

    /// The active profile, for sidebar display.
    pub fn current_profile(&self) -> Option<&crate::profiles::Profile> {
        let uid = self.data.profiles.current.as_deref()?;
        self.data.profiles.items.iter().find(|item| item.uid == uid)
    }
}
