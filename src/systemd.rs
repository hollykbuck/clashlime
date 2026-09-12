//! systemd user manager integration (the only supervisor backend).
//!
//! The daemon is always a child of the user manager: either the packaged
//! `omash-supervisor.service` unit or a unit file this module writes to
//! `~/.config/systemd/user/`. All state queries and lifecycle operations go
//! through the user bus (`org.freedesktop.systemd1`) — no `systemctl`
//! subprocesses, no silent fallbacks.

use anyhow::{Context, Result};
use std::path::PathBuf;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

pub const SERVICE: &str = "omash-supervisor.service";
const PACKAGED_UNIT: &str = "/usr/lib/systemd/user/omash-supervisor.service";

#[zbus::proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait Manager {
    fn get_unit(&self, name: &str) -> Result<OwnedObjectPath>;
    fn start_unit(&self, name: &str, mode: &str) -> Result<OwnedObjectPath>;
    fn stop_unit(&self, name: &str, mode: &str) -> Result<OwnedObjectPath>;
    fn restart_unit(&self, name: &str, mode: &str) -> Result<OwnedObjectPath>;
    fn reload(&self) -> Result<()>;
    fn set_environment(&self, assignments: Vec<String>) -> Result<()>;
    fn unset_environment(&self, names: Vec<String>) -> Result<()>;
    fn enable_unit_files(
        &self,
        files: Vec<String>,
        runtime: bool,
        force: bool,
    ) -> Result<(bool, Vec<(String, String, String)>)>;
    fn disable_unit_files(
        &self,
        files: Vec<String>,
        runtime: bool,
    ) -> Result<Vec<(String, String, String)>>;
}

#[zbus::proxy(
    interface = "org.freedesktop.DBus.Properties",
    default_service = "org.freedesktop.systemd1"
)]
trait Properties {
    fn get(&self, interface: &str, property: &str) -> Result<OwnedValue>;
}

async fn get_string(
    props: &PropertiesProxy<'_>,
    interface: &str,
    property: &str,
) -> Option<String> {
    props
        .get(interface, property)
        .await
        .ok()
        .and_then(|value| String::try_from(value).ok())
}

async fn manager() -> Result<ManagerProxy<'static>> {
    let connection = zbus::Connection::session()
        .await
        .context("cannot connect to systemd user bus")?;
    ManagerProxy::new(&connection)
        .await
        .context("cannot talk to systemd user manager")
}

/// Live state of our supervisor unit.
pub struct UnitStatus {
    pub active: String,
    pub sub: String,
    pub main_pid: u32,
}

/// `None` when the unit is not loaded (never installed/started).
pub async fn unit_status() -> Result<Option<UnitStatus>> {
    let manager = manager().await?;
    let path = match manager.get_unit(SERVICE).await {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let connection = zbus::Connection::session()
        .await
        .context("cannot connect to systemd user bus")?;
    let props = PropertiesProxy::builder(&connection)
        .path(path)
        .context("bad unit object path")?
        .build()
        .await
        .context("cannot talk to unit properties")?;
    let unit = "org.freedesktop.systemd1.Unit";
    let service = "org.freedesktop.systemd1.Service";
    let active = get_string(&props, unit, "ActiveState")
        .await
        .unwrap_or_else(|| "unknown".into());
    if active == "inactive" {
        // Unit file known but never started (or stopped): no live state.
        return Ok(None);
    }
    let sub = get_string(&props, unit, "SubState")
        .await
        .unwrap_or_default();
    let main_pid = props
        .get(service, "MainPID")
        .await
        .ok()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    Ok(Some(UnitStatus {
        active,
        sub,
        main_pid,
    }))
}

fn user_unit_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("systemd/user")
        .join(SERVICE)
}

fn unit_content(exe: &str) -> String {
    format!(
        "[Unit]\n\
         Description=Omash Mihomo Supervisor\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exe} --daemon\n\
         Restart=on-failure\n\
         RestartSec=2\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

/// Ensure a unit file exists: prefer the packaged one, otherwise manage our
/// own copy. Returns true when the manager needs a `reload`.
fn ensure_unit_file() -> Result<bool> {
    if PathBuf::from(PACKAGED_UNIT).is_file() {
        return Ok(false);
    }
    let path = user_unit_path();
    let exe = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| String::from("omash"));
    let content = unit_content(&exe);
    if std::fs::read_to_string(&path).ok().as_deref() == Some(content.as_str()) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(true)
}

/// Start the supervisor unit (fails loudly — there is no other backend).
pub async fn start() -> Result<()> {
    if ensure_unit_file()? {
        manager()
            .await?
            .reload()
            .await
            .context("systemd reload failed")?;
    }
    manager()
        .await?
        .start_unit(SERVICE, "replace")
        .await
        .with_context(|| format!("failed to start {SERVICE}"))?;
    Ok(())
}

/// Stop the supervisor unit.
pub async fn stop() -> Result<()> {
    manager()
        .await?
        .stop_unit(SERVICE, "replace")
        .await
        .with_context(|| format!("failed to stop {SERVICE}"))?;
    Ok(())
}

/// Restart an arbitrary user unit (used for desktop integration).
pub async fn restart_unit(unit: &str) -> Result<()> {
    manager()
        .await?
        .restart_unit(unit, "replace")
        .await
        .with_context(|| format!("failed to restart {unit}"))?;
    Ok(())
}

/// Query ActiveState of an arbitrary user unit.
pub async fn is_unit_active(unit: &str) -> bool {
    let Ok(manager) = manager().await else {
        return false;
    };
    let Ok(path) = manager.get_unit(unit).await else {
        return false;
    };
    let Ok(connection) = zbus::Connection::session().await else {
        return false;
    };
    let builder = match PropertiesProxy::builder(&connection).path(path) {
        Ok(builder) => builder,
        Err(_) => return false,
    };
    let Ok(props) = builder.build().await else {
        return false;
    };
    props
        .get("org.freedesktop.systemd1.Unit", "ActiveState")
        .await
        .ok()
        .and_then(|value| String::try_from(value).ok())
        .is_some_and(|state| state == "active")
}

/// Set arbitrary user-manager environment entries (`KEY=value`).
pub async fn set_environment(assignments: Vec<String>) -> Result<()> {
    manager()
        .await?
        .set_environment(assignments)
        .await
        .context("failed to set user manager environment")?;
    Ok(())
}

/// Sync proxy environment into the user manager (and UWSM marker).
pub async fn set_proxy_environment(enabled: bool, mixed_port: u16, bypass: &str) -> Result<()> {
    let manager = manager().await?;
    if !enabled {
        manager
            .unset_environment(
                [
                    "http_proxy",
                    "https_proxy",
                    "all_proxy",
                    "no_proxy",
                    "HTTP_PROXY",
                    "HTTPS_PROXY",
                    "ALL_PROXY",
                    "NO_PROXY",
                ]
                .into_iter()
                .map(String::from)
                .collect(),
            )
            .await
            .context("failed to unset proxy environment")?;
        return Ok(());
    }
    let http = format!("http://127.0.0.1:{mixed_port}");
    let socks = format!("socks5://127.0.0.1:{mixed_port}");
    manager
        .set_environment(vec![
            format!("http_proxy={http}"),
            format!("https_proxy={http}"),
            format!("all_proxy={socks}"),
            format!("no_proxy={bypass}"),
            format!("HTTP_PROXY={http}"),
            format!("HTTPS_PROXY={http}"),
            format!("ALL_PROXY={socks}"),
            format!("NO_PROXY={bypass}"),
        ])
        .await
        .context("failed to set proxy environment")?;
    Ok(())
}

/// Enable/disable autostart. No linger: the unit lives with login sessions.
pub async fn set_autostart(enabled: bool) -> Result<()> {
    if enabled && ensure_unit_file()? {
        manager()
            .await?
            .reload()
            .await
            .context("systemd reload failed")?;
    }
    let manager = manager().await?;
    if enabled {
        manager
            .enable_unit_files(vec![SERVICE.into()], false, true)
            .await
            .with_context(|| format!("failed to enable {SERVICE}"))?;
    } else {
        // Disabling autostart must not interrupt the currently running daemon.
        let _ = manager
            .disable_unit_files(vec![SERVICE.into()], false)
            .await;
    }
    Ok(())
}

/// Remove a stale self-managed unit file (e.g. after the binary moved and
/// the packaged unit took over). Keeps user edits to other units alone.
pub fn cleanup_stale_unit_file() -> Result<bool> {
    if PathBuf::from(PACKAGED_UNIT).is_file() {
        return Ok(false);
    }
    let path = user_unit_path();
    if path.is_file() {
        std::fs::remove_file(&path)
            .with_context(|| format!("failed to remove {}", path.display()))?;
        return Ok(true);
    }
    Ok(false)
}
