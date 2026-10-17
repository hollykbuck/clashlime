//! systemd user manager integration (the only supervisor backend).
//!
//! The daemon is always a child of the user manager: either the packaged
//! `clashlime-supervisor.service` unit or a unit file this module writes to
//! `~/.config/systemd/user/`. All state queries and lifecycle operations go
//! through the user bus (`org.freedesktop.systemd1`) — no `systemctl`
//! subprocesses, no silent fallbacks.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

pub const SERVICE: &str = "clashlime-supervisor.service";
const PACKAGED_UNIT: &str = "/usr/lib/systemd/user/clashlime-supervisor.service";

/// Master switch from `systemd = false` / `CLASHLIME_SYSTEMD=0`.
///
/// Defaults to on until `Config::load` stores the resolved value, so the
/// read-only `server` probes — which deliberately never load a config — keep
/// working (they consult `CLASHLIME_SYSTEMD` directly, see below).
static SYSTEMD_ENABLED: OnceLock<AtomicBool> = OnceLock::new();

fn systemd_enabled() -> &'static AtomicBool {
    SYSTEMD_ENABLED.get_or_init(|| AtomicBool::new(true))
}

/// Apply the resolved config switch (static file plus `CLASHLIME_SYSTEMD`).
pub fn set_enabled(enabled: bool) {
    systemd_enabled().store(enabled, Ordering::Relaxed);
}

/// Whether a systemd *user* manager is reachable at all.
///
/// The unit backend is the only supervisor on a desktop, but containers and
/// headless shells have no session bus at all. Probing the bus before talking
/// to it keeps those environments from logging a connection failure on every
/// single start. Deliberately not cached: the bus can appear later in a login
/// session (the daemon starts before the compositor does), and the check is
/// only a couple of `stat` calls.
pub fn user_manager_available() -> bool {
    if !systemd_enabled().load(Ordering::Relaxed) {
        return false;
    }
    if env_systemd_override() == Some(false) {
        return false;
    }
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        return true;
    }
    dirs::runtime_dir().is_some_and(|runtime| runtime.join("bus").exists())
}

/// `CLASHLIME_SYSTEMD` as a boolean; unset/invalid means "no opinion". The
/// config loader merges the same variable into `Config::systemd`, so this is
/// only what read-only paths see.
fn env_systemd_override() -> Option<bool> {
    crate::config::env_flag("CLASHLIME_SYSTEMD")
}

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

async fn manager() -> Result<ManagerProxy<'static>> {
    let connection = zbus::Connection::session()
        .await
        .context("cannot connect to systemd user bus")?;
    ManagerProxy::new(&connection)
        .await
        .context("cannot talk to systemd user manager")
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
         Description=Clashlime Mihomo Supervisor\n\
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
        .unwrap_or_else(|_| String::from("clashlime"));
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

/// Stop the supervisor unit. Unlike `start`, this never creates a unit
/// file: stopping must stay side-effect free (and work when nothing was
/// ever installed).
pub async fn stop() -> Result<()> {
    // Nothing to stop without a user bus: the container's daemon is PID 1
    // and exits with it.
    if !user_manager_available() {
        return Ok(());
    }
    stop_unit(SERVICE).await
}

/// Stop an arbitrary user unit.
pub async fn stop_unit(unit: &str) -> Result<()> {
    if !user_manager_available() {
        return Ok(());
    }
    manager()
        .await?
        .stop_unit(unit, "replace")
        .await
        .with_context(|| format!("failed to stop {unit}"))?;
    Ok(())
}

/// Restart an arbitrary user unit (used for desktop integration).
pub async fn restart_unit(unit: &str) -> Result<()> {
    if !user_manager_available() {
        return Ok(());
    }
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
    if !user_manager_available() {
        return Ok(());
    }
    manager()
        .await?
        .set_environment(assignments)
        .await
        .context("failed to set user manager environment")?;
    Ok(())
}

/// Sync proxy environment into the user manager (and UWSM marker).
/// `None` disables setup (teardown still runs): without a mixed listener
/// there is nothing to point the environment at.
pub async fn set_proxy_environment(
    enabled: bool,
    mixed_port: Option<u16>,
    bypass: &str,
) -> Result<()> {
    // Nothing to sync into: without a user manager there is no environment
    // that newly launched applications would inherit. Callers treat this as
    // "proxy environment handled", which is true here.
    if !user_manager_available() {
        return Ok(());
    }
    let manager = manager().await?;
    if !enabled || mixed_port.is_none() {
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
    let mixed_port = mixed_port.unwrap_or(0);
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
    // No unit to enable: in a container the daemon is PID 1 and the runtime
    // owns its restart policy, so autostart is not ours to manage. Report
    // success (nothing failed) but say so once in the log, instead of
    // surfacing a D-Bus failure for a feature the environment does not have.
    if !user_manager_available() {
        crate::logger::info(
            "systemd",
            "autostart not applied: no systemd user manager in this environment",
        );
        return Ok(());
    }
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
