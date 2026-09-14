//! `clashlime server status` / `clashlime server stop`: daemon queries.
//!
//! Deliberately avoids `Config::load()` so these probes create no files.

use anyhow::{Result, bail};
use serde::Serialize;
use std::time::{Duration, Instant};

const EXIT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Serialize)]
struct ServerStatus {
    daemon_alive: bool,
    systemd_active: bool,
    socket: String,
    running: bool,
    enabled: bool,
    pid: Option<u32>,
    restarts: u64,
    reloads: u64,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct StopResult {
    stopped: bool,
    already_stopped: bool,
    daemon_alive: bool,
    systemd_active: bool,
}

pub async fn run_stop(json: bool) -> Result<()> {
    let daemon_alive = crate::ipc::daemon_alive().await;
    let systemd_active = crate::systemd::is_unit_active(crate::systemd::SERVICE).await;
    if !daemon_alive && !systemd_active {
        emit_stop(json, true);
        return Ok(());
    }

    if daemon_alive {
        // Graceful path: the daemon stops the core, undoes the system
        // proxy, removes the socket and exits cleanly (no
        // `Restart=on-failure` trip).
        if let Err(error) = crate::ipc::shutdown().await {
            eprintln!(
                "warning: graceful shutdown request failed ({error}); falling back to systemd stop"
            );
        }
    }
    // Pin the unit down regardless: this is the whole story when the IPC
    // request failed, settles the crash-restart race (!daemon_alive yet
    // active), and keeps a cleanly-exited daemon from being restarted.
    if let Err(error) = crate::systemd::stop().await {
        // Without a user bus there is no unit to settle; the IPC request
        // above is then the whole story — but only if the daemon is
        // actually gone.
        if crate::ipc::daemon_alive().await {
            bail!("cannot stop daemon: {error}");
        }
        eprintln!("warning: systemd stop failed ({error}); daemon is stopped");
    }
    wait_for_exit(EXIT_TIMEOUT).await?;

    let daemon_alive = crate::ipc::daemon_alive().await;
    let systemd_active = crate::systemd::is_unit_active(crate::systemd::SERVICE).await;
    emit_stop_result(
        json,
        StopResult {
            stopped: !daemon_alive,
            already_stopped: false,
            daemon_alive,
            systemd_active,
        },
    );
    if daemon_alive {
        bail!(
            "daemon is still answering on {}",
            crate::ipc::socket_path().display()
        );
    }
    Ok(())
}

fn emit_stop(json: bool, already_stopped: bool) {
    emit_stop_result(
        json,
        StopResult {
            stopped: true,
            already_stopped,
            daemon_alive: false,
            systemd_active: false,
        },
    );
}

fn emit_stop_result(json: bool, result: StopResult) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into())
        );
    } else if result.already_stopped {
        println!("daemon already stopped");
    } else {
        println!(
            "daemon stopped (systemd: {})",
            if result.systemd_active {
                "active"
            } else {
                "inactive"
            }
        );
    }
}

/// Wait until the daemon no longer answers on the IPC socket.
async fn wait_for_exit(timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !crate::ipc::daemon_alive().await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    bail!(
        "daemon did not exit within {timeout:?} (socket: {})",
        crate::ipc::socket_path().display()
    )
}

pub async fn run_status(json: bool) -> Result<()> {
    let socket = crate::ipc::socket_path();
    let daemon_alive = crate::ipc::daemon_alive().await;
    let state = if daemon_alive {
        crate::ipc::state().await.ok()
    } else {
        None
    };
    let systemd_active = crate::systemd::is_unit_active(crate::systemd::SERVICE).await;

    let status = ServerStatus {
        daemon_alive,
        systemd_active,
        socket: socket.display().to_string(),
        running: state.as_ref().is_some_and(|s| s.running),
        enabled: state.as_ref().is_some_and(|s| s.enabled),
        pid: state.as_ref().and_then(|s| s.pid),
        restarts: state.as_ref().map_or(0, |s| s.restarts),
        reloads: state.as_ref().map_or(0, |s| s.reloads),
        error: state.as_ref().and_then(|s| s.error.clone()),
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        print_human(&status);
    }

    // Script-friendly: 0 only when the core is actually running.
    if status.running {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

fn print_human(status: &ServerStatus) {
    let daemon = if status.daemon_alive {
        "running"
    } else {
        "stopped"
    };
    println!("daemon: {daemon} (socket: {})", status.socket);
    println!(
        "systemd: {}",
        if status.systemd_active {
            "active"
        } else {
            "inactive"
        }
    );
    if !status.daemon_alive {
        println!("core: unknown (daemon unreachable)");
        return;
    }
    if status.running {
        match status.pid {
            Some(pid) => println!("core: running (pid {pid})"),
            None => println!("core: running"),
        }
    } else {
        println!("core: stopped");
    }
    println!(
        "enabled: {}  restarts: {}  reloads: {}",
        if status.enabled { "yes" } else { "no" },
        status.restarts,
        status.reloads
    );
    if let Some(error) = &status.error {
        println!("error: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_status_reports_stopped_daemon() {
        let status = ServerStatus {
            daemon_alive: false,
            systemd_active: false,
            socket: "/run/user/1000/clashlime.sock".into(),
            running: false,
            enabled: true,
            pid: None,
            restarts: 0,
            reloads: 0,
            error: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"daemon_alive\":false"));
        assert!(json.contains("clashlime.sock"));
    }

    #[test]
    fn status_serializes_running_core() {
        let status = ServerStatus {
            daemon_alive: true,
            systemd_active: true,
            socket: "/run/user/1000/clashlime.sock".into(),
            running: true,
            enabled: true,
            pid: Some(42),
            restarts: 1,
            reloads: 2,
            error: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"running\":true"));
        assert!(json.contains("\"pid\":42"));
    }

    #[test]
    fn stop_result_serializes_idempotent_stop() {
        let result = StopResult {
            stopped: true,
            already_stopped: true,
            daemon_alive: false,
            systemd_active: false,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"stopped\":true"));
        assert!(json.contains("\"already_stopped\":true"));
    }
}
