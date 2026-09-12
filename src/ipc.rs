//! Unix-socket IPC between the omash TUI and the supervisor daemon.
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

pub const SOCKET_NAME: &str = "omash.sock";

/// Legacy files superseded by the socket; removed on daemon startup.
pub const LEGACY_FILES: [&str; 4] = [
    "supervisor.pid",
    "restart-request",
    "core-disabled",
    "supervisor-state.json",
];

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    State,
    Restart,
    SetEnabled { enabled: bool },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "res", rename_all = "snake_case")]
pub enum Response {
    State(Box<SupervisorState>),
    Ok,
    Err { error: String },
}

/// Mutable flags the IPC server flips on behalf of TUI clients and the
/// supervisor loop consumes each tick.
#[derive(Debug, Default)]
pub struct Flags {
    pub restart: AtomicBool,
    pub enabled: AtomicBool,
}

impl Flags {
    pub fn take_restart(&self) -> bool {
        self.restart.swap(false, Ordering::SeqCst)
    }

    pub fn set_enabled(&self, value: bool) {
        self.enabled.store(value, Ordering::SeqCst);
    }

    pub fn desired_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
}

pub type SharedState = Arc<Mutex<SupervisorState>>;

pub fn socket_path() -> PathBuf {
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
    }
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            if daemon_alive_at(&path).await {
                bail!("another omash daemon owns {}", path.display());
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
fn harden(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(SOCKET_FILE_MODE))
        .with_context(|| format!("failed to chmod {}", path.display()))
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
            error: error.to_string(),
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
                    error: error.to_string(),
                },
            }
        }
        Request::Restart => {
            flags.restart.store(true, Ordering::SeqCst);
            crate::logger::info("ipc", "restart requested");
            Response::Ok
        }
        Request::SetEnabled { enabled } => {
            flags.set_enabled(enabled);
            crate::logger::info("ipc", &format!("core {} requested", verb(enabled)));
            Response::Ok
        }
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
        Response::Ok => bail!("unexpected reply to state request"),
    }
}

/// Ask the daemon to reload or restart the core.
pub async fn restart() -> Result<()> {
    expect_ok(call(&Request::Restart).await?)
}

/// Enable or disable the core (supervisor stops/starts mihomo).
pub async fn set_enabled(enabled: bool) -> Result<()> {
    expect_ok(call(&Request::SetEnabled { enabled }).await?)
}

fn expect_ok(response: Response) -> Result<()> {
    match response {
        Response::Ok => Ok(()),
        Response::Err { error } => bail!(error),
        Response::State(_) => bail!("unexpected reply"),
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
        let mut stream = UnixStream::connect(path)
            .await
            .with_context(|| format!("failed to connect to {}", path.display()))?;
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
}
