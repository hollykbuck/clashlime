//! Routing mode CLI command (`clashlime mode [mode]`).

use crate::{
    cmd::{
        proxy::get_api,
        table::{Align, Cell, Table, success},
    },
    config::{Config, ModeArgs},
};
use anyhow::Result;
use serde::Serialize;

#[derive(Serialize)]
struct JsonModeInfo {
    mode: String,
    mixed_port: Option<u16>,
    allow_lan: Option<bool>,
}

pub async fn run(config: &Config, args: &ModeArgs) -> Result<()> {
    let api = get_api(config).await?;

    if let Some(mode) = args.mode {
        api.set_mode(mode.as_str()).await?;
        success(&format!("Routing mode set to {}", mode.as_str()));
        return Ok(());
    }

    let runtime = api.runtime_config().await?;

    if args.json {
        let payload = JsonModeInfo {
            mode: runtime.mode.clone(),
            mixed_port: runtime.mixed_port,
            allow_lan: runtime.allow_lan,
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    let mut table = Table::new(vec![
        ("MODE", Align::Left),
        ("MIXED PORT", Align::Right),
        ("ALLOW LAN", Align::Left),
    ]);

    let port_str = runtime
        .mixed_port
        .map(|p| p.to_string())
        .unwrap_or_else(|| "-".to_string());
    let lan_str = match runtime.allow_lan {
        Some(true) => "yes",
        Some(false) => "no",
        None => "-",
    };

    table.add_row(vec![
        Cell::new(runtime.mode.as_str()),
        Cell::new(port_str).right(),
        Cell::new(lan_str),
    ]);

    print!("{}", table.render());
    Ok(())
}
