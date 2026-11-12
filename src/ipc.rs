//! Unix-socket IPC between the clashlime TUI and the supervisor daemon.
//!
//! The socket lives in `$XDG_RUNTIME_DIR` (per-user tmpfs, wiped at logout);
//! when the variable is unset the daemon data directory is used instead.
//! Protocol: newline-delimited JSON, one request per connection.

use crate::core::SupervisorState;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
#[cfg(unix)]
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(400);
const CALL_TIMEOUT: Duration = Duration::from_secs(3);

#[cfg(unix)]
const SOCKET_FILE_MODE: u32 = 0o600;

pub const SOCKET_NAME: &str = "clashlime.sock";
pub const SYSTEM_SOCKET_PATH: &str = "/run/clashlime/clashlime.sock";

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    State,
    Restart,
    /// Stop the core process and start it again (new binary, TUN device,
    /// controller secret…). Unlike `Restart`, never settles for a hot
    /// PUT /configs reload.
    RestartProcess,
    SetEnabled {
        enabled: bool,
    },
    /// Ask the supervisor daemon to stop the core and exit cleanly
    /// (`server stop`). A clean exit never trips `Restart=on-failure`.
    Shutdown,
    /// Query dynamic configuration from the supervisor daemon.
    GetConfig,
    /// Save dynamic configuration on behalf of a client.
    SaveConfig(Box<crate::config::DynamicConfig>),
    /// Record proxy selection in profiles on behalf of a client.
    SelectProxy {
        group: String,
        proxy: String,
    },
    /// Clear proxy selection/pin in profiles on behalf of a client.
    UnselectProxy {
        group: String,
    },
    /// Query profiles state from the supervisor daemon.
    GetProfiles,
    /// Activate a profile and reload the core.
    ActivateProfile { uid: String },
    /// Delete a profile and reload the core.
    DeleteProfile { uid: String },
    /// Update a profile from its upstream source.
    UpdateProfile { uid: String },
    /// Import a profile (remote or local).
    ImportProfile {
        source: String,
        name: Option<String>,
        overrides: Box<crate::profiles::ImportOverrides>,
    },
    /// Save profiles index on behalf of a client (e.g. metadata/settings edits).
    SaveProfiles(Box<crate::profiles::Profiles>),
    /// Adopt a core binary into the managed bin directory on behalf of a client.
    AdoptCore { path: PathBuf },
    /// Adopt a geo database file into the managed data directory on behalf of a client.
    AdoptGeo { name: String, path: PathBuf },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "res", rename_all = "snake_case")]
pub enum Response {
    State(Box<SupervisorState>),
    Config(Box<crate::config::DynamicConfig>),
    Profiles(Box<crate::profiles::Profiles>),
    Imported {
        profiles: Box<crate::profiles::Profiles>,
        uid: String,
    },
    Ok,
    Err { error: String },
}

/// Mutable flags the IPC server flips on behalf of TUI clients and the
/// supervisor loop consumes each tick.
#[derive(Debug, Default)]
pub struct Flags {
    pub restart: AtomicBool,
    pub process_restart: AtomicBool,
    pub enabled: AtomicBool,
    pub shutdown: AtomicBool,
    pub notify: tokio::sync::Notify,
}

impl Flags {
    pub fn wake(&self) {
        self.notify.notify_waiters();
    }

    pub fn take_restart(&self) -> bool {
        self.restart.swap(false, Ordering::SeqCst)
    }

    pub fn take_process_restart(&self) -> bool {
        self.process_restart.swap(false, Ordering::SeqCst)
    }

    pub fn take_shutdown(&self) -> bool {
        self.shutdown.swap(false, Ordering::SeqCst)
    }

    pub fn set_enabled(&self, value: bool) {
        self.enabled.store(value, Ordering::SeqCst);
    }

    pub fn desired_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
}

pub type SharedState = Arc<Mutex<SupervisorState>>;

/// `--daemon-socket` override, installed once at startup from the CLI.
/// `None` means "use the platform default"; a `Some` path is used verbatim
/// (relative paths included) by both the daemon and its clients.
static SOCKET_OVERRIDE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

/// Point this process at a specific daemon socket. Called from `main` before
/// any IPC happens; later calls are ignored so the first (CLI) value wins.
pub fn set_socket_override(path: Option<PathBuf>) {
    let _ = SOCKET_OVERRIDE.set(path);
}

pub fn socket_path() -> PathBuf {
    resolve_socket_path(SOCKET_OVERRIDE.get().and_then(|path| path.as_ref()))
}

fn resolve_socket_path(override_path: Option<&PathBuf>) -> PathBuf {
    if let Some(path) = override_path {
        return path.clone();
    }
    if let Some(env_path) = std::env::var_os("CLASHLIME_DAEMON_SOCKET")
        && !env_path.is_empty()
    {
        return PathBuf::from(env_path);
    }
    if crate::config::is_system_mode() {
        return PathBuf::from(SYSTEM_SOCKET_PATH);
    }
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir).join(SOCKET_NAME),
        _ => crate::config::Config::data_dir().join(SOCKET_NAME),
    }
}

/// Bind at the default [`socket_path`], cleaning up a stale file left by a
/// crashed daemon. Fails when another live daemon already owns the endpoint.
#[cfg(unix)]
pub async fn bind() -> Result<UnixListener> {
    bind_at(socket_path()).await
}

/// Bind at an explicit path with stale-socket recovery.
#[cfg(unix)]
pub async fn bind_at(path: PathBuf) -> Result<UnixListener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        #[cfg(unix)]
        if crate::config::is_system_mode() {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o750)) {
                crate::logger::warn("ipc", &format!("failed to chmod 0750 {}: {e}", parent.display()));
            }
            if let Some(gid) = clashlime_gid() {
                if let Err(e) = std::os::unix::fs::chown(parent, None, Some(gid)) {
                    crate::logger::warn("ipc", &format!("failed to chown {} to gid {gid}: {e}", parent.display()));
                }
            }
        }
    }
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            if daemon_alive_at(&path).await {
                bail!("another clashlime daemon owns {}", path.display());
            }
            // Stale socket from a crashed daemon
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to unlink stale socket {}", path.display()))?;
            UnixListener::bind(&path)?
        }
        Err(error) => return Err(error.into()),
    };
    harden(&path)?;
    crate::logger::info("ipc", &format!("listening on {}", path.display()));
    Ok(listener)
}

#[cfg(unix)]
fn clashlime_gid() -> Option<u32> {
    use std::ffi::CString;
    let name = CString::new("clashlime").ok()?;
    let grp = unsafe { libc::getgrnam(name.as_ptr()) };
    if grp.is_null() {
        None
    } else {
        Some(unsafe { (*grp).gr_gid })
    }
}

#[cfg(unix)]
fn harden(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if crate::config::is_system_mode() {
        0o660
    } else {
        SOCKET_FILE_MODE
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("failed to chmod {}", path.display()))?;

    if crate::config::is_system_mode() {
        if let Some(gid) = clashlime_gid() {
            if let Err(e) = std::os::unix::fs::chown(path, None, Some(gid)) {
                crate::logger::warn("ipc", &format!("failed to chown {} to gid {gid}: {e}", path.display()));
            }
        }
    }
    Ok(())
}

/// True when a daemon currently answers on the IPC socket.
#[cfg(unix)]
pub async fn daemon_alive() -> bool {
    daemon_alive_at(&socket_path()).await
}

/// Liveness probe against an explicit socket path. `bind_at` must check
/// the path it is binding, not the global default: probing the default
/// makes the stale-socket test (and any custom path) depend on whatever
/// unrelated daemon happens to be alive on the machine.
#[cfg(unix)]
pub async fn daemon_alive_at(path: &std::path::Path) -> bool {
    match path.try_exists() {
        Ok(true) => {}
        _ => return false,
    }
    let attempt = tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(path)).await;
    matches!(attempt, Ok(Ok(_)))
}

/// Accept loop: one request per connection, newline-delimited JSON.
#[cfg(unix)]
pub async fn serve(listener: UnixListener, state: SharedState, flags: std::sync::Arc<Flags>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let state = state.clone();
                let flags = flags.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle(stream, state, flags).await {
                        crate::logger::warn("ipc", &format!("request failed: {error}"));
                    }
                });
            }
            Err(error) => {
                crate::logger::warn("ipc", &format!("accept failed: {error}"));
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

#[cfg(unix)]
async fn handle(
    stream: UnixStream,
    state: SharedState,
    flags: std::sync::Arc<Flags>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let bytes = reader.read_line(&mut line).await?;
    // Liveness probes (`daemon_alive`) connect and go away without
    // sending anything; closing quietly keeps them out of the logs.
    if bytes == 0 || line.trim().is_empty() {
        return Ok(());
    }
    let response = match serde_json::from_str::<Request>(&line) {
        Ok(request) => dispatch(request, state, flags).await,
        Err(error) => Response::Err {
            error: format!("{error:#}"),
        },
    };
    let mut reply = serde_json::to_string(&response)?;
    reply.push('\n');
    // The client timing out or going away mid-reply is routine socket
    // churn, not a daemon problem; swallow it instead of warning.
    match writer.write_all(reply.as_bytes()).await {
        Ok(()) => writer.flush().await.map_err(anyhow::Error::from),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(unix)]
async fn dispatch(request: Request, state: SharedState, flags: std::sync::Arc<Flags>) -> Response {
    match request {
        Request::State => {
            let guard = state.lock().map(|guard| guard.clone());
            match guard {
                Ok(state) => Response::State(Box::new(state)),
                Err(error) => Response::Err {
                    error: format!("{error:#}"),
                },
            }
        }
        Request::Restart => {
            flags.restart.store(true, Ordering::SeqCst);
            flags.wake();
            crate::logger::info("ipc", "restart requested");
            Response::Ok
        }
        Request::RestartProcess => {
            flags.process_restart.store(true, Ordering::SeqCst);
            flags.wake();
            crate::logger::info("ipc", "process restart requested");
            Response::Ok
        }
        Request::SetEnabled { enabled } => {
            flags.set_enabled(enabled);
            flags.wake();
            crate::logger::info("ipc", &format!("core {} requested", verb(enabled)));
            Response::Ok
        }
        Request::Shutdown => {
            flags.shutdown.store(true, Ordering::SeqCst);
            flags.wake();
            crate::logger::info("ipc", "shutdown requested");
            Response::Ok
        }
        Request::GetConfig => {
            let path = crate::config::Config::dynamic_path();
            match crate::persist::read_optional(&path) {
                Ok(Some(text)) => match serde_json::from_str::<crate::config::DynamicConfig>(&text) {
                    Ok(cfg) => Response::Config(Box::new(cfg)),
                    Err(error) => {
                        crate::logger::error("ipc", &format!("failed to parse dynamic config: {error:#}"));
                        Response::Err {
                            error: format!("{error:#}"),
                        }
                    }
                },
                Ok(None) => Response::Config(Box::default()),
                Err(error) => {
                    crate::logger::error("ipc", &format!("failed to read dynamic config: {error:#}"));
                    Response::Err {
                        error: format!("{error:#}"),
                    }
                }
            }
        }
        Request::SaveConfig(patch) => {
            match patch.save() {
                Ok(()) => {
                    flags.restart.store(true, Ordering::SeqCst);
                    flags.wake();
                    crate::logger::info("ipc", "dynamic config saved, restart requested");
                    Response::Ok
                }
                Err(error) => {
                    crate::logger::error("ipc", &format!("failed to save dynamic config: {error:#}"));
                    Response::Err {
                        error: format!("{error:#}"),
                    }
                }
            }
        }
        Request::SelectProxy { group, proxy } => {
            match crate::profiles::Profiles::load() {
                Ok(mut profiles) => match profiles.record_selection(&group, &proxy) {
                    Ok(()) => {
                        crate::logger::info("ipc", &format!("proxy selected: {group} -> {proxy}"));
                        Response::Ok
                    }
                    Err(error) => {
                        crate::logger::error("ipc", &format!("failed to record selection: {error:#}"));
                        Response::Err {
                            error: format!("{error:#}"),
                        }
                    }
                },
                Err(error) => {
                    crate::logger::error("ipc", &format!("failed to load profiles: {error:#}"));
                    Response::Err {
                        error: format!("{error:#}"),
                    }
                }
            }
        }
        Request::UnselectProxy { group } => {
            match crate::profiles::Profiles::load() {
                Ok(mut profiles) => match profiles.clear_selection(&group) {
                    Ok(()) => {
                        crate::logger::info("ipc", &format!("proxy unselected: {group}"));
                        Response::Ok
                    }
                    Err(error) => {
                        crate::logger::error("ipc", &format!("failed to clear selection: {error:#}"));
                        Response::Err {
                            error: format!("{error:#}"),
                        }
                    }
                },
                Err(error) => {
                    crate::logger::error("ipc", &format!("failed to load profiles: {error:#}"));
                    Response::Err {
                        error: format!("{error:#}"),
                    }
                }
            }
        }
        Request::GetProfiles => match crate::profiles::Profiles::load() {
            Ok(profiles) => Response::Profiles(Box::new(profiles)),
            Err(error) => Response::Err {
                error: format!("{error:#}"),
            },
        },
        Request::ActivateProfile { uid } => match crate::profiles::Profiles::load() {
            Ok(mut profiles) => {
                profiles.current = Some(uid.clone());
                let config = match crate::core::load_daemon_config() {
                    Ok(cfg) => cfg,
                    Err(e) => return Response::Err { error: format!("{e:#}") },
                };
                match crate::core::CoreManager::new().validate_only(&config, &profiles).await {
                    Ok(()) => match profiles.save() {
                        Ok(()) => {
                            flags.restart.store(true, Ordering::SeqCst);
                            flags.wake();
                            crate::logger::info("ipc", &format!("profile activated: {uid}"));
                            Response::Profiles(Box::new(profiles))
                        }
                        Err(e) => Response::Err { error: format!("{e:#}") },
                    },
                    Err(e) => Response::Err { error: format!("{e:#}") },
                }
            }
            Err(e) => Response::Err { error: format!("{e:#}") },
        },
        Request::DeleteProfile { uid } => match crate::profiles::Profiles::load() {
            Ok(mut profiles) => match profiles.delete(&uid) {
                Ok(()) => {
                    flags.restart.store(true, Ordering::SeqCst);
                    flags.wake();
                    crate::logger::info("ipc", &format!("profile deleted: {uid}"));
                    Response::Profiles(Box::new(profiles))
                }
                Err(e) => Response::Err { error: format!("{e:#}") },
            },
            Err(e) => Response::Err { error: format!("{e:#}") },
        },
        Request::UpdateProfile { uid } => match crate::profiles::Profiles::load() {
            Ok(mut profiles) => {
                let config = match crate::core::load_daemon_config() {
                    Ok(cfg) => cfg,
                    Err(e) => return Response::Err { error: format!("{e:#}") },
                };
                match profiles.update_validated(&uid, &config).await {
                    Ok(()) => {
                        flags.restart.store(true, Ordering::SeqCst);
                        flags.wake();
                        crate::logger::info("ipc", &format!("profile updated: {uid}"));
                        Response::Profiles(Box::new(profiles))
                    }
                    Err(e) => Response::Err { error: format!("{e:#}") },
                }
            }
            Err(e) => Response::Err { error: format!("{e:#}") },
        },
        Request::ImportProfile { source, name, overrides } => match crate::profiles::Profiles::load() {
            Ok(mut profiles) => {
                let config = match crate::core::load_daemon_config() {
                    Ok(cfg) => cfg,
                    Err(e) => return Response::Err { error: format!("{e:#}") },
                };
                let remote = source.starts_with("http://") || source.starts_with("https://");
                let result = if remote {
                    profiles.import_remote(&source, name.as_deref(), *overrides, &config).await
                } else {
                    profiles.import_local(std::path::Path::new(&source), name.as_deref(), &config).await
                };
                match result {
                    Ok(uid) => {
                        flags.restart.store(true, Ordering::SeqCst);
                        flags.wake();
                        crate::logger::info("ipc", &format!("profile imported: {uid}"));
                        Response::Imported {
                            profiles: Box::new(profiles),
                            uid,
                        }
                    }
                    Err(e) => Response::Err { error: format!("{e:#}") },
                }
            }
            Err(e) => Response::Err { error: format!("{e:#}") },
        },
        Request::SaveProfiles(candidate) => match candidate.save() {
            Ok(()) => {
                flags.restart.store(true, Ordering::SeqCst);
                flags.wake();
                crate::logger::info("ipc", "profiles saved");
                Response::Ok
            }
            Err(e) => Response::Err { error: format!("{e:#}") },
        },
        Request::AdoptCore { path } => {
            let dest = crate::config::Config::data_dir().join("bin/mihomo");
            match crate::core::adopt_core_binary(&path, &dest) {
                Ok(adopted) => {
                    if let Err(error) = crate::config::Config::set_mihomo_override(None) {
                        crate::logger::warn("ipc", &format!("failed to clear mihomo override: {error:#}"));
                    }
                    flags.process_restart.store(true, Ordering::SeqCst);
                    flags.wake();
                    crate::logger::info(
                        "ipc",
                        &format!("core adopted {} -> {}", path.display(), adopted.display()),
                    );
                    Response::Ok
                }
                Err(error) => {
                    crate::logger::error("ipc", &format!("failed to adopt core: {error:#}"));
                    Response::Err {
                        error: format!("{error:#}"),
                    }
                }
            }
        },
        Request::AdoptGeo { name, path } => {
            match crate::geo::adopt_geo_file(&name, &path) {
                Ok(adopted) => {
                    crate::logger::info(
                        "ipc",
                        &format!("geo adopted {} -> {}", path.display(), adopted.display()),
                    );
                    Response::Ok
                }
                Err(error) => {
                    crate::logger::error("ipc", &format!("failed to adopt geo {name}: {error:#}"));
                    Response::Err {
                        error: format!("{error:#}"),
                    }
                }
            }
        },
    }
}

#[cfg(unix)]
fn verb(enabled: bool) -> &'static str {
    if enabled { "enable" } else { "disable" }
}

/// Ask the daemon for its current supervisor state.
pub async fn state() -> Result<SupervisorState> {
    match call(&Request::State).await? {
        Response::State(state) => Ok(*state),
        Response::Err { error } => bail!(error),
        _ => bail!("unexpected reply to state request"),
    }
}

/// Ask the daemon to reload or restart the core.
pub async fn restart() -> Result<()> {
    expect_ok(call(&Request::Restart).await?)
}

/// Ask the daemon to stop the core process and start it again (picks up
/// a replaced core binary; a plain reload would keep the old process).
pub async fn restart_process() -> Result<()> {
    expect_ok(call(&Request::RestartProcess).await?)
}

/// Enable or disable the core (supervisor stops/starts mihomo).
pub async fn set_enabled(enabled: bool) -> Result<()> {
    expect_ok(call(&Request::SetEnabled { enabled }).await?)
}

/// Ask the supervisor daemon to stop the core and exit cleanly.
pub async fn shutdown() -> Result<()> {
    expect_ok(call(&Request::Shutdown).await?)
}

/// Query dynamic configuration from the supervisor daemon.
pub async fn dynamic_config() -> Result<crate::config::DynamicConfig> {
    match call(&Request::GetConfig).await? {
        Response::Config(cfg) => Ok(*cfg),
        Response::Err { error } => bail!("{error}"),
        other => bail!("unexpected response: {other:?}"),
    }
}

/// Ask the supervisor daemon to save dynamic config and apply it.
pub async fn save_config(patch: crate::config::DynamicConfig) -> Result<()> {
    expect_ok(call(&Request::SaveConfig(Box::new(patch))).await?)
}

/// Record proxy selection in profiles through the supervisor daemon.
pub async fn select_proxy(group: &str, proxy: &str) -> Result<()> {
    expect_ok(
        call(&Request::SelectProxy {
            group: group.to_owned(),
            proxy: proxy.to_owned(),
        })
        .await?,
    )
}

/// Clear proxy selection/pin in profiles through the supervisor daemon.
pub async fn unselect_proxy(group: &str) -> Result<()> {
    expect_ok(
        call(&Request::UnselectProxy {
            group: group.to_owned(),
        })
        .await?,
    )
}

/// Ask the supervisor daemon for current profiles.
#[allow(dead_code)]
pub async fn profiles() -> Result<crate::profiles::Profiles> {
    match call(&Request::GetProfiles).await? {
        Response::Profiles(p) => Ok(*p),
        Response::Err { error } => bail!(error),
        _ => bail!("unexpected reply to get_profiles"),
    }
}

/// Activate a profile and reload the core through the supervisor daemon.
pub async fn activate_profile(uid: &str) -> Result<crate::profiles::Profiles> {
    match call(&Request::ActivateProfile {
        uid: uid.to_owned(),
    })
    .await?
    {
        Response::Profiles(p) => Ok(*p),
        Response::Err { error } => bail!(error),
        _ => bail!("unexpected reply to activate_profile"),
    }
}

/// Delete a profile through the supervisor daemon.
pub async fn delete_profile(uid: &str) -> Result<crate::profiles::Profiles> {
    match call(&Request::DeleteProfile {
        uid: uid.to_owned(),
    })
    .await?
    {
        Response::Profiles(p) => Ok(*p),
        Response::Err { error } => bail!(error),
        _ => bail!("unexpected reply to delete_profile"),
    }
}

/// Update a profile through the supervisor daemon.
pub async fn update_profile(uid: &str) -> Result<crate::profiles::Profiles> {
    match call(&Request::UpdateProfile {
        uid: uid.to_owned(),
    })
    .await?
    {
        Response::Profiles(p) => Ok(*p),
        Response::Err { error } => bail!(error),
        _ => bail!("unexpected reply to update_profile"),
    }
}

/// Import a profile through the supervisor daemon.
pub async fn import_profile(
    source: &str,
    name: Option<&str>,
    overrides: crate::profiles::ImportOverrides,
) -> Result<(crate::profiles::Profiles, String)> {
    match call(&Request::ImportProfile {
        source: source.to_owned(),
        name: name.map(str::to_owned),
        overrides: Box::new(overrides),
    })
    .await?
    {
        Response::Imported { profiles, uid } => Ok((*profiles, uid)),
        Response::Err { error } => bail!(error),
        _ => bail!("unexpected reply to import_profile"),
    }
}

/// Save profiles index through the supervisor daemon.
pub async fn save_profiles(candidate: crate::profiles::Profiles) -> Result<()> {
    expect_ok(call(&Request::SaveProfiles(Box::new(candidate))).await?)
}

/// Adopt a core binary into the managed bin directory through the supervisor daemon.
pub async fn adopt_core(path: &std::path::Path) -> Result<()> {
    expect_ok(call(&Request::AdoptCore {
        path: path.to_path_buf(),
    }).await?)
}

/// Adopt a geo file into the managed data directory through the supervisor daemon.
pub async fn adopt_geo(name: &str, path: &std::path::Path) -> Result<()> {
    expect_ok(call(&Request::AdoptGeo {
        name: name.to_string(),
        path: path.to_path_buf(),
    }).await?)
}

fn expect_ok(response: Response) -> Result<()> {
    match response {
        Response::Ok => Ok(()),
        Response::Err { error } => bail!(error),
        _ => bail!("unexpected reply"),
    }
}

async fn call(request: &Request) -> Result<Response> {
    #[cfg(unix)]
    {
        call_at(&socket_path(), request).await
    }
    #[cfg(not(unix))]
    {
        let _ = request;
        bail!("IPC is only supported on Unix platforms")
    }
}

#[cfg(unix)]
async fn call_at(path: &std::path::Path, request: &Request) -> Result<Response> {
    let future = async {
        let mut stream = match UnixStream::connect(path).await {
            Ok(s) => s,
            Err(e)
                if e.kind() == std::io::ErrorKind::PermissionDenied
                    && crate::config::is_system_mode() =>
            {
                bail!(
                    "permission denied connecting to {} (add user to 'clashlime' group: sudo usermod -aG clashlime $USER)",
                    path.display()
                );
            }
            Err(e) => {
                return Err(e).with_context(|| format!("failed to connect to {}", path.display()));
            }
        };
        let mut line = serde_json::to_string(request)?;
        line.push('\n');
        stream.write_all(line.as_bytes()).await?;
        stream.flush().await?;
        let mut reader = BufReader::new(stream);
        let mut reply = String::new();
        reader.read_line(&mut reply).await?;
        if reply.trim().is_empty() {
            bail!("daemon closed the connection without a reply");
        }
        serde_json::from_str::<Response>(&reply)
            .with_context(|| format!("malformed daemon reply: {}", reply.trim()))
    };
    match tokio::time::timeout(CALL_TIMEOUT, future).await {
        Ok(result) => result,
        Err(_) => bail!("daemon IPC timed out after {:?}", CALL_TIMEOUT),
    }
}

#[cfg(all(unix, test))]
mod tests {
    use super::*;

    #[test]
    fn socket_path_override_wins_over_platform_default() {
        let custom = PathBuf::from("/run/user/1000/custom-clashlime.sock");
        assert_eq!(resolve_socket_path(Some(&custom)), custom);
        // Without an override the platform default still applies; computed
        // directly because the process-wide override is a shared static
        // that other tests in this binary may already have installed.
        assert!(resolve_socket_path(None).ends_with(SOCKET_NAME));
    }

    #[test]
    fn socket_path_override_is_installed_once() {
        // First writer wins; later calls cannot move an in-flight process.
        set_socket_override(Some(PathBuf::from("/run/user/1000/first.sock")));
        assert_eq!(socket_path(), PathBuf::from("/run/user/1000/first.sock"));
        set_socket_override(Some(PathBuf::from("/run/user/1000/second.sock")));
        assert_eq!(socket_path(), PathBuf::from("/run/user/1000/first.sock"));
    }

    #[tokio::test]
    async fn round_trip_state_restart_and_toggle() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test.sock");
        let listener = bind_at(path.clone()).await.expect("bind");
        let state: SharedState = Arc::new(Mutex::new(SupervisorState {
            running: true,
            pid: Some(42),
            restarts: 1,
            reloads: 2,
            enabled: true,
            error: None,
            ..Default::default()
        }));
        let flags = Arc::new(Flags::default());
        tokio::spawn(crate::ipc::serve(listener, state.clone(), flags.clone()));

        match call_at(&path, &Request::State).await.expect("state") {
            Response::State(snapshot) => {
                assert!(snapshot.running);
                assert_eq!(snapshot.pid, Some(42));
                assert_eq!(snapshot.restarts, 1);
                assert!(snapshot.enabled);
            }
            other => panic!("unexpected response: {other:?}"),
        }

        expect_ok(call_at(&path, &Request::Restart).await.expect("restart")).expect("ok");
        assert!(flags.take_restart());
        assert!(!flags.take_restart(), "restart flag must be consumed once");

        expect_ok(
            call_at(&path, &Request::SetEnabled { enabled: false })
                .await
                .expect("set_enabled"),
        )
        .expect("ok");
        assert!(!flags.desired_enabled());
    }

    #[tokio::test]
    async fn shutdown_flag_round_trip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("shutdown.sock");
        let listener = bind_at(path.clone()).await.expect("bind");
        let state: SharedState = Arc::new(Mutex::new(SupervisorState::default()));
        let flags = Arc::new(Flags::default());
        tokio::spawn(crate::ipc::serve(listener, state, flags.clone()));

        expect_ok(call_at(&path, &Request::Shutdown).await.expect("shutdown")).expect("ok");
        assert!(flags.take_shutdown());
        assert!(
            !flags.take_shutdown(),
            "shutdown flag must be consumed once"
        );
    }

    #[tokio::test]
    async fn stale_socket_is_recovered() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("stale.sock");
        // First listener leaves its socket file behind on drop
        drop(bind_at(path.clone()).await.expect("first bind"));
        assert!(path.exists());
        // Nothing answers anymore, so the second bind must recover
        let listener = bind_at(path.clone()).await.expect("rebind");
        drop(listener);
    }

    #[test]
    fn ipc_error_response_formats_cause_chain() {
        let root = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
        let err = anyhow::Error::new(root)
            .context("failed to write file")
            .context("action failed");
        let resp = Response::Err {
            error: format!("{err:#}"),
        };
        match resp {
            Response::Err { error } => {
                assert!(error.contains("action failed"));
                assert!(error.contains("failed to write file"));
                assert!(error.contains("permission denied"));
            }
            _ => panic!("expected err"),
        }
    }

    #[tokio::test]
    async fn probe_disconnect_is_silent_and_server_survives() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("probe.sock");
        let listener = bind_at(path.clone()).await.expect("bind");
        let state: SharedState = Arc::new(Mutex::new(SupervisorState::default()));
        tokio::spawn(crate::ipc::serve(
            listener,
            state,
            Arc::new(Flags::default()),
        ));

        // Liveness probe: connect and go away without a word.
        drop(UnixStream::connect(&path).await.expect("connect"));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        // A dropped caller mid-reply must not take the server down either.
        let mut stream = UnixStream::connect(&path).await.expect("connect");
        use tokio::io::AsyncWriteExt as _;
        stream
            .write_all(b"{\"cmd\":\"state\"}\n")
            .await
            .expect("write");
        drop(stream);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        // The server still answers the next real client.
        match call_at(&path, &Request::State).await.expect("state") {
            Response::State(_) => {}
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn malformed_request_gets_err_response() {
        use tokio::io::AsyncWriteExt as _;
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("bad.sock");
        let listener = bind_at(path.clone()).await.expect("bind");
        let state: SharedState = Arc::new(Mutex::new(SupervisorState::default()));
        tokio::spawn(crate::ipc::serve(
            listener,
            state,
            Arc::new(Flags::default()),
        ));

        let mut stream = UnixStream::connect(&path).await.expect("connect");
        stream.write_all(b"not json\n").await.expect("write");
        stream.flush().await.expect("flush");
        let mut reader = BufReader::new(stream);
        let mut reply = String::new();
        reader.read_line(&mut reply).await.expect("read");
        assert!(matches!(
            serde_json::from_str::<Response>(reply.trim()).expect("parsed"),
            Response::Err { .. }
        ));
    }

    #[tokio::test]
    async fn round_trip_select_and_unselect_proxy() {
        let _guard = crate::config::tests::test_env_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("proxy.sock");
        let listener = bind_at(path.clone()).await.expect("bind");
        let state: SharedState = Arc::new(Mutex::new(SupervisorState::default()));
        let flags = Arc::new(Flags::default());
        tokio::spawn(crate::ipc::serve(listener, state, flags));

        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let orig_data = std::env::var_os("XDG_DATA_HOME");
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &data_dir);
        }

        let profiles = crate::profiles::Profiles {
            current: Some("test-uid".into()),
            items: vec![crate::profiles::Profile {
                uid: "test-uid".into(),
                name: "Test".into(),
                file: "test.yaml".into(),
                kind: crate::profiles::ProfileKind::Local,
                updated: 0,
                update_interval: None,
                auto_update: None,
                fixed_interval: None,
                update_timeout: None,
                use_proxy: None,
                auth_token: None,
                user_agent: None,
                url: None,
                home: None,
                merge: None,
                rules: None,
                proxies: None,
                groups: None,
                subscription: None,
                selected: vec![],
            }],
        };
        profiles.save().expect("save profiles");

        expect_ok(
            call_at(
                &path,
                &Request::SelectProxy {
                    group: "GLOBAL".into(),
                    proxy: "Node-1".into(),
                },
            )
            .await
            .expect("select_proxy"),
        )
        .expect("ok");

        let reloaded = crate::profiles::Profiles::load().expect("load profiles");
        assert_eq!(reloaded.current_selections().len(), 1);
        assert_eq!(reloaded.current_selections()[0].name, "GLOBAL");
        assert_eq!(reloaded.current_selections()[0].now, "Node-1");

        expect_ok(
            call_at(
                &path,
                &Request::UnselectProxy {
                    group: "GLOBAL".into(),
                },
            )
            .await
            .expect("unselect_proxy"),
        )
        .expect("ok");

        let reloaded2 = crate::profiles::Profiles::load().expect("load profiles");
        assert!(reloaded2.current_selections().is_empty());

        unsafe {
            match orig_data {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }

    #[tokio::test]
    async fn round_trip_profile_lifecycle() {
        let _guard = crate::config::tests::test_env_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("profiles.sock");
        let listener = bind_at(path.clone()).await.expect("bind");
        let state: SharedState = Arc::new(Mutex::new(SupervisorState::default()));
        let flags = Arc::new(Flags::default());
        tokio::spawn(crate::ipc::serve(listener, state, flags));

        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let orig_data = std::env::var_os("XDG_DATA_HOME");
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &data_dir);
        }

        let profiles = crate::profiles::Profiles {
            current: None,
            items: vec![crate::profiles::Profile {
                uid: "p1".into(),
                name: "Profile 1".into(),
                file: "p1.yaml".into(),
                kind: crate::profiles::ProfileKind::Local,
                updated: 0,
                update_interval: None,
                auto_update: None,
                fixed_interval: None,
                update_timeout: None,
                use_proxy: None,
                auth_token: None,
                user_agent: None,
                url: None,
                home: None,
                merge: None,
                rules: None,
                proxies: None,
                groups: None,
                subscription: None,
                selected: vec![],
            }],
        };

        // 1. SaveProfiles
        let res = call_at(&path, &Request::SaveProfiles(Box::new(profiles)))
            .await
            .expect("save_profiles");
        assert!(matches!(res, Response::Ok));

        // 2. GetProfiles
        let res = call_at(&path, &Request::GetProfiles)
            .await
            .expect("get_profiles");
        match res {
            Response::Profiles(p) => {
                assert_eq!(p.items.len(), 1);
            }
            other => panic!("unexpected response: {other:?}"),
        }

        // 3. DeleteProfile
        let res = call_at(&path, &Request::DeleteProfile { uid: "p1".into() })
            .await
            .expect("delete_profile");
        match res {
            Response::Profiles(p) => {
                assert_eq!(p.items.len(), 0);
            }
            other => panic!("unexpected response: {other:?}"),
        }

        unsafe {
            match orig_data {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }

    #[tokio::test]
    async fn round_trip_get_config() {
        let _guard = crate::config::tests::test_env_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("get_config.sock");
        let listener = bind_at(path.clone()).await.expect("bind");
        let state: SharedState = Arc::new(Mutex::new(SupervisorState::default()));
        let flags = Arc::new(Flags::default());
        tokio::spawn(crate::ipc::serve(listener, state, flags.clone()));

        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let orig_data = std::env::var_os("XDG_DATA_HOME");
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &data_dir);
        }

        let res = call_at(&path, &Request::GetConfig)
            .await
            .expect("call get_config");
        match res {
            Response::Config(cfg) => {
                assert!(cfg.controller.is_none());
                assert!(cfg.refresh_ms.is_none());
            }
            other => panic!("expected Config response, got {other:?}"),
        }

        unsafe {
            match orig_data {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }

    #[tokio::test]
    async fn round_trip_adopt_core() {
        let _guard = crate::config::tests::test_env_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("adopt.sock");
        let listener = bind_at(path.clone()).await.expect("bind");
        let state: SharedState = Arc::new(Mutex::new(SupervisorState::default()));
        let flags = Arc::new(Flags::default());
        tokio::spawn(crate::ipc::serve(listener, state, flags.clone()));

        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let orig_data = std::env::var_os("XDG_DATA_HOME");
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &data_dir);
        }

        let fake_src = tmp.path().join("fake-mihomo");
        std::fs::write(&fake_src, "#!/bin/sh\necho \"Mihomo Meta v1.18.0 test\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&fake_src, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let res = call_at(&path, &Request::AdoptCore { path: fake_src.clone() })
            .await
            .expect("call adopt");
        expect_ok(res).expect("ok");
        assert!(flags.take_process_restart());

        let adopted = data_dir.join("clashlime/bin/mihomo");
        assert!(adopted.is_file());

        unsafe {
            match orig_data {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }

    #[tokio::test]
    async fn round_trip_adopt_geo() {
        let _guard = crate::config::tests::test_env_guard();
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("adopt_geo.sock");
        let listener = bind_at(path.clone()).await.expect("bind");
        let state: SharedState = Arc::new(Mutex::new(SupervisorState::default()));
        let flags = Arc::new(Flags::default());
        tokio::spawn(crate::ipc::serve(listener, state, flags.clone()));

        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let orig_data = std::env::var_os("XDG_DATA_HOME");
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &data_dir);
        }

        let fake_geo = tmp.path().join("fake-geosite.dat");
        std::fs::write(&fake_geo, b"geosite content").unwrap();

        let res = call_at(&path, &Request::AdoptGeo {
            name: "geosite.dat".to_string(),
            path: fake_geo.clone(),
        })
        .await
        .expect("call adopt geo");
        expect_ok(res).expect("ok");

        let adopted = data_dir.join("clashlime/geosite.dat");
        assert!(adopted.is_file());
        assert_eq!(std::fs::read(&adopted).unwrap(), b"geosite content");

        unsafe {
            match orig_data {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }

    #[tokio::test]
    async fn system_mode_socket_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let _guard = crate::config::tests::test_env_guard();
        let tmp = tempfile::tempdir().expect("tempdir");

        crate::config::set_system_mode(false);
        let user_sock = tmp.path().join("user.sock");
        let listener = bind_at(user_sock.clone()).await.expect("bind user");
        let meta = std::fs::metadata(&user_sock).expect("metadata");
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        drop(listener);

        crate::config::set_system_mode(true);
        let sys_sock = tmp.path().join("sys.sock");
        let listener = bind_at(sys_sock.clone()).await.expect("bind sys");
        let meta = std::fs::metadata(&sys_sock).expect("metadata");
        assert_eq!(meta.permissions().mode() & 0o777, 0o660);
        drop(listener);

        crate::config::set_system_mode(false);
    }
}
