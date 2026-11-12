//! Profile management CLI commands (`clashlime profile ...`).

use crate::{
    cmd::table::{Align, Cell, Table, active_indicator, format_relative_time, success},
    config::ProfileCommand,
    profiles::{ImportOverrides, Profile, ProfileKind, Profiles},
};
use anyhow::{Result, bail};
use serde::Serialize;

#[derive(Serialize)]
struct JsonProfileItem {
    uid: String,
    name: String,
    active: bool,
    #[serde(rename = "type")]
    kind: ProfileKind,
    file: String,
    url: Option<String>,
    updated: i64,
    update_interval: Option<u64>,
}

#[derive(Serialize)]
struct JsonProfileList {
    current: Option<String>,
    items: Vec<JsonProfileItem>,
}

pub async fn run(args: &crate::config::ProfileArgs) -> Result<()> {
    match &args.command {
        None => run_list(args.json).await,
        Some(ProfileCommand::List { json }) => run_list(*json || args.json).await,
        Some(ProfileCommand::Switch { target }) => run_switch(target).await,
        Some(ProfileCommand::Update { target }) => run_update(target.as_deref()).await,
        Some(ProfileCommand::Import { source, name }) => run_import(source, name.as_deref()).await,
        Some(ProfileCommand::Delete { target }) => run_delete(target).await,
    }
}

async fn load_profiles() -> Result<Profiles> {
    if crate::ipc::daemon_alive().await {
        match crate::ipc::profiles().await {
            Ok(p) => return Ok(p),
            Err(e) => {
                crate::logger::warn("cli", &format!("ipc profiles failed: {e}; falling back to file"));
            }
        }
    }
    Profiles::load()
}

fn resolve_profile<'a>(profiles: &'a Profiles, target: &str) -> Result<&'a Profile> {
    let target = target.trim();
    // 1. Exact UID match
    if let Some(p) = profiles.items.iter().find(|p| p.uid == target) {
        return Ok(p);
    }
    // 2. Exact Name match (case-insensitive)
    let exact_name: Vec<&Profile> = profiles
        .items
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(target))
        .collect();
    if exact_name.len() == 1 {
        return Ok(exact_name[0]);
    }
    // 3. Substring Name match (case-insensitive)
    let lower_target = target.to_lowercase();
    let partial: Vec<&Profile> = profiles
        .items
        .iter()
        .filter(|p| p.name.to_lowercase().contains(&lower_target))
        .collect();
    if partial.len() == 1 {
        return Ok(partial[0]);
    }
    if partial.len() > 1 {
        let names: Vec<String> = partial.iter().map(|p| format!("\"{}\"", p.name)).collect();
        bail!("multiple profiles match '{target}': {}", names.join(", "));
    }
    bail!("profile '{target}' not found");
}

async fn run_list(json: bool) -> Result<()> {
    let profiles = load_profiles().await?;
    let current_uid = profiles.current.as_deref();

    if json {
        let items: Vec<JsonProfileItem> = profiles
            .items
            .iter()
            .map(|p| JsonProfileItem {
                uid: p.uid.clone(),
                name: p.name.clone(),
                active: current_uid == Some(&p.uid),
                kind: p.kind,
                file: p.file.clone(),
                url: p.url.clone(),
                updated: p.updated,
                update_interval: p.update_interval,
            })
            .collect();
        let payload = JsonProfileList {
            current: profiles.current.clone(),
            items,
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    if profiles.items.is_empty() {
        println!("no profiles found");
        return Ok(());
    }

    let mut table = Table::new(vec![
        ("ACTIVE", Align::Left),
        ("NAME", Align::Left),
        ("TYPE", Align::Left),
        ("UPDATED", Align::Left),
        ("INTERVAL", Align::Right),
        ("URL/PATH", Align::Left),
    ]);

    for item in &profiles.items {
        let is_active = current_uid == Some(&item.uid);
        let (active_display, active_raw) = active_indicator(is_active);
        let (updated_display, updated_raw) = format_relative_time(item.updated);

        let kind_str = match item.kind {
            ProfileKind::Remote => "remote",
            ProfileKind::Local => "local",
        };

        let interval_str = item
            .update_interval
            .map(|h| format!("{h}h"))
            .unwrap_or_else(|| "-".to_string());

        let source_str = item.url.as_deref().unwrap_or(&item.file);

        table.add_row(vec![
            Cell::styled(active_display, active_raw),
            Cell::new(&item.name),
            Cell::new(kind_str),
            Cell::styled(updated_display, updated_raw),
            Cell::new(interval_str).right(),
            Cell::new(source_str),
        ]);
    }

    print!("{}", table.render());
    Ok(())
}

async fn run_switch(target: &str) -> Result<()> {
    let profiles = load_profiles().await?;
    let target_profile = resolve_profile(&profiles, target)?;
    let uid = target_profile.uid.clone();
    let name = target_profile.name.clone();

    crate::ipc::activate_profile(&uid).await?;
    success(&format!("Switched to profile \"{name}\" ({uid})"));
    Ok(())
}

async fn run_update(target: Option<&str>) -> Result<()> {
    let profiles = load_profiles().await?;
    let target_profile = match target {
        Some(t) => resolve_profile(&profiles, t)?,
        None => {
            let Some(cur) = profiles.current.as_deref() else {
                bail!("no profile is currently active; please specify a profile to update");
            };
            profiles
                .items
                .iter()
                .find(|p| p.uid == cur)
                .ok_or_else(|| anyhow::anyhow!("active profile '{cur}' not found"))?
        }
    };

    let uid = target_profile.uid.clone();
    let name = target_profile.name.clone();

    crate::ipc::update_profile(&uid).await?;
    success(&format!("Updated profile \"{name}\""));
    Ok(())
}

async fn run_import(source: &str, name: Option<&str>) -> Result<()> {
    let overrides = ImportOverrides::default();
    let (_profiles, uid) = crate::ipc::import_profile(source, name, overrides).await?;
    let display_name = name.unwrap_or(source);
    success(&format!("Imported profile \"{display_name}\" ({uid})"));
    Ok(())
}

async fn run_delete(target: &str) -> Result<()> {
    let profiles = load_profiles().await?;
    let target_profile = resolve_profile(&profiles, target)?;
    let uid = target_profile.uid.clone();
    let name = target_profile.name.clone();

    crate::ipc::delete_profile(&uid).await?;
    success(&format!("Deleted profile \"{name}\" ({uid})"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_profile_matches_uid_exact_name_and_substring() {
        let mut profiles = Profiles::default();
        profiles.items.push(Profile {
            uid: "uid-123".into(),
            name: "MySubscription".into(),
            ..Default::default()
        });
        profiles.items.push(Profile {
            uid: "uid-456".into(),
            name: "Backup-Nodes".into(),
            ..Default::default()
        });

        // 1. By UID
        let res = resolve_profile(&profiles, "uid-123").unwrap();
        assert_eq!(res.name, "MySubscription");

        // 2. By exact name (case-insensitive)
        let res = resolve_profile(&profiles, "mysubscription").unwrap();
        assert_eq!(res.uid, "uid-123");

        // 3. By substring
        let res = resolve_profile(&profiles, "backup").unwrap();
        assert_eq!(res.uid, "uid-456");

        // 4. Not found
        assert!(resolve_profile(&profiles, "nonexistent").is_err());
    }
}
