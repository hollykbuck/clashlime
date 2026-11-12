//! Proxy and node management CLI commands (`clashlime proxy ...`).

use crate::{
    api::{MihomoClient, Proxy, ProxyResponse},
    cmd::table::{Align, Cell, Table, active_indicator, format_delay, success},
    config::{Config, ProxyCommand},
};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{collections::HashMap, io::IsTerminal};

#[derive(Serialize)]
struct JsonGroupItem {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    now: String,
    count: usize,
}

#[derive(Serialize)]
struct JsonNodeItem {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    active: bool,
    delay: Option<u32>,
}

pub async fn get_api(config: &Config) -> Result<MihomoClient> {
    let mut controller = config.controller.clone();
    let mut secret = config.secret.clone();
    if crate::config::is_system_mode() || crate::ipc::daemon_alive().await {
        if let Ok(sup) = crate::ipc::state().await {
            if let Some(s) = sup.secret
                && !s.is_empty()
            {
                secret = s;
            }
            if let Some(c) = sup.controller
                && !c.is_empty()
            {
                controller = c;
            }
        }
    }
    MihomoClient::new(&controller, secret)
}

pub async fn run(config: &Config, args: &crate::config::ProxyArgs) -> Result<()> {
    match &args.command {
        None => run_list_groups(config, args.json).await,
        Some(ProxyCommand::List { group: None, json }) => run_list_groups(config, *json || args.json).await,
        Some(ProxyCommand::List {
            group: Some(group),
            json,
        }) => run_list_nodes(config, group, *json || args.json).await,
        Some(ProxyCommand::Select { group, node }) => run_select(config, group, node).await,
        Some(ProxyCommand::Test { group, json }) => run_test(config, group.as_deref(), *json || args.json).await,
    }
}

fn group_order_list(_config: &Config, sup_order: &[String]) -> Vec<String> {
    let mut order = Config::proxy_group_order();
    if order.is_empty() {
        order = sup_order.to_vec();
    }
    order
}

async fn run_list_groups(config: &Config, json: bool) -> Result<()> {
    let api = get_api(config).await?;
    let proxies = api.proxies().await.context("failed to fetch proxies from mihomo")?;

    let sup_order = if let Ok(s) = crate::ipc::state().await {
        s.proxy_group_order
    } else {
        Vec::new()
    };
    let order = group_order_list(config, &sup_order);

    let mut groups: Vec<(&String, &Proxy)> = proxies
        .proxies
        .iter()
        .filter(|(_, p)| !p.all.is_empty())
        .collect();

    groups.sort_by(|(a_name, _), (b_name, _)| {
        let a_pos = order.iter().position(|o| o.eq_ignore_ascii_case(a_name));
        let b_pos = order.iter().position(|o| o.eq_ignore_ascii_case(b_name));
        match (a_pos, b_pos) {
            (Some(ia), Some(ib)) => ia.cmp(&ib),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a_name.cmp(b_name),
        }
    });

    if json {
        let items: Vec<JsonGroupItem> = groups
            .iter()
            .map(|(name, p)| JsonGroupItem {
                name: (*name).clone(),
                kind: p.kind.clone(),
                now: p.now.clone(),
                count: p.all.len(),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    if groups.is_empty() {
        println!("no proxy groups found");
        return Ok(());
    }

    let mut table = Table::new(vec![
        ("GROUP", Align::Left),
        ("TYPE", Align::Left),
        ("CURRENT", Align::Left),
        ("NODES", Align::Right),
    ]);

    for (name, p) in groups {
        table.add_row(vec![
            Cell::new(name.as_str()),
            Cell::new(p.kind.as_str()),
            Cell::new(p.now.as_str()),
            Cell::new(p.all.len().to_string()).right(),
        ]);
    }

    print!("{}", table.render());
    Ok(())
}

fn collect_delays(proxies: &ProxyResponse, providers_delays: HashMap<String, u32>) -> HashMap<String, u32> {
    let mut map = providers_delays;
    for (name, p) in &proxies.proxies {
        if let Some(h) = p.history.last()
            && h.delay > 0
        {
            map.entry(name.clone()).or_insert(h.delay);
        }
    }
    map
}

async fn run_list_nodes(config: &Config, target_group: &str, json: bool) -> Result<()> {
    let api = get_api(config).await?;
    let proxies = api.proxies().await.context("failed to fetch proxies from mihomo")?;

    let (group_name, group) = proxies
        .proxies
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(target_group))
        .ok_or_else(|| anyhow::anyhow!("proxy group '{target_group}' not found"))?;

    let mut provider_delays = HashMap::new();
    let mut provider_types = HashMap::new();
    if let Ok(providers) = api.all_proxy_providers().await {
        for provider in &providers {
            for node in &provider.proxies {
                if !node.kind.is_empty() {
                    provider_types.entry(node.name.clone()).or_insert_with(|| node.kind.clone());
                }
                if let Some(h) = node.history.last()
                    && h.delay > 0
                {
                    provider_delays.entry(node.name.clone()).or_insert(h.delay);
                }
            }
        }
    }
    let delays = collect_delays(&proxies, provider_delays);

    if json {
        let items: Vec<JsonNodeItem> = group
            .all
            .iter()
            .map(|node_name| {
                let kind = proxies
                    .proxies
                    .get(node_name)
                    .map(|p| p.kind.clone())
                    .or_else(|| provider_types.get(node_name).cloned())
                    .unwrap_or_else(|| "-".to_string());
                JsonNodeItem {
                    name: node_name.clone(),
                    kind,
                    active: group.now == *node_name,
                    delay: delays.get(node_name).copied(),
                }
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    let mut table = Table::new(vec![
        ("ACTIVE", Align::Left),
        ("NAME", Align::Left),
        ("TYPE", Align::Left),
        ("DELAY", Align::Right),
    ]);

    for node_name in &group.all {
        let is_active = group.now == *node_name;
        let (active_disp, active_raw) = active_indicator(is_active);
        let kind = proxies
            .proxies
            .get(node_name)
            .map(|p| p.kind.as_str())
            .or_else(|| provider_types.get(node_name).map(|s| s.as_str()))
            .unwrap_or("-");
        let delay = delays.get(node_name).copied();
        let (delay_disp, delay_raw) = format_delay(delay);

        table.add_row(vec![
            Cell::styled(active_disp, active_raw),
            Cell::new(node_name.as_str()),
            Cell::new(kind),
            Cell::styled(delay_disp, delay_raw).right(),
        ]);
    }

    if std::io::stdout().is_terminal() {
        println!("\x1b[2mGroup: {}\x1b[0m\n", group_name);
    }
    print!("{}", table.render());
    Ok(())
}

async fn run_select(config: &Config, target_group: &str, target_node: &str) -> Result<()> {
    let api = get_api(config).await?;
    let proxies = api.proxies().await.context("failed to fetch proxies from mihomo")?;

    let (group_name, group) = proxies
        .proxies
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(target_group))
        .ok_or_else(|| anyhow::anyhow!("proxy group '{target_group}' not found"))?;

    // Find node: exact match -> case-insensitive exact -> substring match
    let matched_node = if let Some(n) = group.all.iter().find(|n| **n == target_node) {
        n.clone()
    } else if let Some(n) = group.all.iter().find(|n| n.eq_ignore_ascii_case(target_node)) {
        n.clone()
    } else {
        let lower = target_node.to_lowercase();
        let matches: Vec<&String> = group
            .all
            .iter()
            .filter(|n| n.to_lowercase().contains(&lower))
            .collect();
        if matches.len() == 1 {
            matches[0].clone()
        } else if matches.len() > 1 {
            let names: Vec<String> = matches.iter().map(|n| format!("\"{n}\"")).collect();
            bail!("multiple nodes match '{target_node}': {}", names.join(", "));
        } else {
            bail!("node '{target_node}' not found in group '{group_name}'");
        }
    };

    api.select_proxy(group_name, &matched_node).await?;
    if let Err(e) = crate::ipc::select_proxy(group_name, &matched_node).await {
        crate::logger::warn("cli", &format!("proxy selected in runtime but IPC persistence failed: {e}"));
    }

    success(&format!("Selected \"{matched_node}\" for group \"{group_name}\""));
    Ok(())
}

async fn run_test(config: &Config, target_group: Option<&str>, json: bool) -> Result<()> {
    let api = get_api(config).await?;
    let proxies = api.proxies().await.context("failed to fetch proxies from mihomo")?;

    let group_name = match target_group {
        Some(name) => {
            let (k, _) = proxies
                .proxies
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .ok_or_else(|| anyhow::anyhow!("proxy group '{name}' not found"))?;
            k.clone()
        }
        None => {
            // Find default group: "PROXY", or first group with nodes
            if proxies.proxies.contains_key("PROXY") {
                "PROXY".to_string()
            } else if let Some((k, _)) = proxies.proxies.iter().find(|(_, p)| !p.all.is_empty()) {
                k.clone()
            } else {
                bail!("no proxy groups available to test");
            }
        }
    };

    let group = proxies
        .proxies
        .get(&group_name)
        .ok_or_else(|| anyhow::anyhow!("proxy group '{group_name}' not found"))?;

    let test_url = &config.delay_test_url;
    let delay_map = api
        .test_group_delay(&group_name, test_url)
        .await
        .context("delay test request failed")?;

    let mut provider_types = HashMap::new();
    if let Ok(providers) = api.all_proxy_providers().await {
        for provider in &providers {
            for node in &provider.proxies {
                if !node.kind.is_empty() {
                    provider_types.entry(node.name.clone()).or_insert_with(|| node.kind.clone());
                }
            }
        }
    }

    struct TestedNode {
        name: String,
        kind: String,
        active: bool,
        delay: Option<u32>,
    }

    let mut nodes: Vec<TestedNode> = group
        .all
        .iter()
        .map(|node_name| {
            let kind = proxies
                .proxies
                .get(node_name)
                .map(|p| p.kind.clone())
                .or_else(|| provider_types.get(node_name).cloned())
                .unwrap_or_else(|| "-".to_string());
            let delay = delay_map.get(node_name).copied();
            TestedNode {
                name: node_name.clone(),
                kind,
                active: group.now == *node_name,
                delay,
            }
        })
        .collect();

    // Sort by delay: fastest first (>0), then timeouts (==0), then untestable
    nodes.sort_by(|a, b| match (a.delay, b.delay) {
        (Some(da), Some(db)) => {
            if da > 0 && db > 0 {
                da.cmp(&db)
            } else if da > 0 {
                std::cmp::Ordering::Less
            } else if db > 0 {
                std::cmp::Ordering::Greater
            } else {
                a.name.cmp(&b.name)
            }
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.name.cmp(&b.name),
    });

    if json {
        let items: Vec<JsonNodeItem> = nodes
            .into_iter()
            .map(|n| JsonNodeItem {
                name: n.name,
                kind: n.kind,
                active: n.active,
                delay: n.delay,
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    let mut table = Table::new(vec![
        ("ACTIVE", Align::Left),
        ("NAME", Align::Left),
        ("TYPE", Align::Left),
        ("DELAY", Align::Right),
    ]);

    for node in &nodes {
        let (active_disp, active_raw) = active_indicator(node.active);
        let (delay_disp, delay_raw) = format_delay(node.delay);

        table.add_row(vec![
            Cell::styled(active_disp, active_raw),
            Cell::new(node.name.as_str()),
            Cell::new(node.kind.as_str()),
            Cell::styled(delay_disp, delay_raw).right(),
        ]);
    }

    if std::io::stdout().is_terminal() {
        println!("\x1b[2mTested Group: {} (url: {})\x1b[0m\n", group_name, test_url);
    }
    print!("{}", table.render());
    Ok(())
}
