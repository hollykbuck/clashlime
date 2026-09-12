//! Rules tab interactions: detail popup and rule provider updates.

use crate::ui::tabs::rules::{filtered_rules, sorted_providers};

impl crate::app::App {
    /// Open the selected rule in the wrapped detail popup (shared with
    /// the Logs tab; Esc closes it).
    pub(crate) fn open_rule_detail(&mut self) {
        let view = filtered_rules(self);
        let Some((_, rule)) = view.get(self.rule_index) else {
            return;
        };
        // Clone out before touching `self` again: the view borrows it.
        let (kind, payload, proxy, size) = (
            rule.kind.clone(),
            rule.payload.clone(),
            rule.proxy.clone(),
            rule.size,
        );
        let (hits, hit_at, misses) = (
            rule.extra.hit_count,
            rule.extra.hit_at.clone(),
            rule.extra.miss_count,
        );
        drop(view);
        let last_hit = if hits == 0 {
            "never".to_string()
        } else if hit_at.is_empty() {
            hits.to_string()
        } else {
            format!("{hits} ({hit_at})")
        };
        self.log_detail = Some(format!(
            "{kind}\n{}\n→ Policy: {proxy}\nSize: {} · Hits: {last_hit} · Misses: {misses}",
            if payload.is_empty() {
                "(catch-all)"
            } else {
                &payload
            },
            if size < 0 {
                "∞".to_string()
            } else {
                size.to_string()
            },
        ));
    }

    /// Refresh every rule provider (`PUT /providers/rules/{name}`), then
    /// reload the slow snapshot so new counts show up.
    pub(crate) async fn update_rule_providers(&mut self) {
        let names: Vec<String> = sorted_providers(self)
            .iter()
            .map(|(name, _)| (*name).clone())
            .collect();
        if names.is_empty() {
            self.say("No rule providers: profile inlines all rules");
            return;
        }
        let mut ok = 0;
        for name in &names {
            if self.api.update_rule_provider(name).await.is_ok() {
                ok += 1;
            }
        }
        self.say(format!("Rule providers updated: {ok}/{}", names.len()));
        self.refresh_full().await;
    }
}
