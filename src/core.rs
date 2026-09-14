use crate::{api::MihomoClient, config::Config, profiles::Profiles};
use anyhow::{Context, Result, bail};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::{Duration, Instant, SystemTime},
};
use tokio::process::{Child, Command};

const VALIDATION_TIMEOUT: Duration = Duration::from_secs(30);
const CORE_READINESS_ATTEMPTS: usize = 30;
const CORE_READINESS_INTERVAL: Duration = Duration::from_millis(100);
const CORE_READINESS_PROBE_TIMEOUT: Duration = Duration::from_millis(400);
const START_RETRY_BACKOFF: Duration = Duration::from_secs(5);

pub enum ConfigApply {
    Reloaded,
    Restarted,
}

pub struct CoreManager {
    child: Option<Child>,
}

impl CoreManager {
    pub const fn new() -> Self {
        Self { child: None }
    }

    pub async fn validate(&self, config: &Config, profiles: &Profiles) -> Result<()> {
        crate::logger::info("core", "validate runtime requested");
        let res = self.validate_runtime(config, profiles, true).await;
        if let Err(e) = &res {
            crate::logger::warn("core", &format!("validate failed: {e}"));
        }
        res
    }

    pub async fn validate_only(&self, config: &Config, profiles: &Profiles) -> Result<()> {
        self.validate_runtime(config, profiles, false).await
    }

    async fn validate_runtime(
        &self,
        config: &Config,
        profiles: &Profiles,
        commit: bool,
    ) -> Result<()> {
        ensure_core_resources()?;
        if !Config::mihomo_path().is_file() {
            bail!(
                "mihomo core not installed; pick Download or a binary path in the TUI core dialog"
            );
        }
        let runtime = Config::runtime_path();
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let staged = runtime.with_file_name(format!(
            "runtime.pending-{}-{nonce}.yaml",
            std::process::id()
        ));
        profiles.build_runtime_at(config, &staged)?;
        // Profiles with GEOIP/GEOSITE rules need the databases next to the
        // data dir, otherwise `mihomo -t` blocks ~90s on its own download and
        // then fails. Fetch what's missing up front with a clear error.
        let staged_text = fs::read_to_string(&staged).unwrap_or_default();
        if let Err(error) = crate::geo::ensure_for_content(&staged_text, &config.geo).await {
            let _ = fs::remove_file(&staged);
            return Err(error);
        }
        let mut command = Command::new(Config::mihomo_path());
        command
            .args(["-t", "-d"])
            .arg(Config::data_dir())
            .arg("-f")
            .arg(&staged)
            .kill_on_drop(true);
        let output = match tokio::time::timeout(VALIDATION_TIMEOUT, command.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                let _ = fs::remove_file(&staged);
                return Err(error).context("failed to execute Mihomo validator");
            }
            Err(_) => {
                let _ = fs::remove_file(&staged);
                bail!("Mihomo configuration validation timed out after 30 seconds");
            }
        };
        if !output.status.success() {
            let _ = fs::remove_file(&staged);
            bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        if commit {
            fs::rename(&staged, &runtime).with_context(|| {
                format!("failed to commit validated runtime {}", runtime.display())
            })?;
        } else {
            fs::remove_file(&staged)?;
        }
        Ok(())
    }

    pub async fn start(&mut self, config: &Config, profiles: &Profiles) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }
        self.validate(config, profiles).await?;
        self.start_validated(config, profiles).await
    }

    async fn start_validated(&mut self, config: &Config, profiles: &Profiles) -> Result<()> {
        crate::logger::info(
            "core",
            &format!("starting mihomo via {}", Config::mihomo_path().display()),
        );
        let log_path =
            Config::logs_dir().join(format!("mihomo-{}.log", Local::now().format("%Y-%m-%d")));
        let stdout = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let stderr = stdout.try_clone()?;
        let mut child = Command::new(Config::mihomo_path())
            .arg("-d")
            .arg(Config::data_dir())
            .arg("-f")
            .arg(Config::runtime_path())
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .kill_on_drop(true)
            .spawn()
            .context("failed to start Mihomo")?;

        let api = MihomoClient::new(&config.controller, config.secret.clone())?;
        let mut last_error = "Mihomo API did not answer".to_owned();
        let mut ready = false;
        for attempt in 0..CORE_READINESS_ATTEMPTS {
            if let Some(status) = child.try_wait()? {
                bail!(
                    "Mihomo exited during startup with {status}; see {}",
                    log_path.display()
                );
            }
            match tokio::time::timeout(CORE_READINESS_PROBE_TIMEOUT, api.version()).await {
                Ok(Ok(_)) => {
                    ready = true;
                    break;
                }
                Ok(Err(error)) => last_error = error.to_string(),
                Err(_) => last_error = "readiness probe timed out".into(),
            }
            if attempt + 1 < CORE_READINESS_ATTEMPTS {
                tokio::time::sleep(CORE_READINESS_INTERVAL).await;
            }
        }
        if !ready {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
            bail!(
                "Mihomo API did not become ready: {last_error}; see {}",
                log_path.display()
            );
        }
        self.child = Some(child);
        let _ = restore_selected_nodes(config, profiles).await;
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            child.start_kill()?;
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await;
        }
        Ok(())
    }

    /// Reload the running core, or stop + start it when `force_process`.
    /// The process path skips the hot PUT /configs reload: it picks up a
    /// replaced core binary and settings a reload cannot apply. Validation
    /// runs first so a broken config/binary never kills a healthy core.
    pub async fn restart(
        &mut self,
        config: &Config,
        profiles: &Profiles,
        force_process: bool,
    ) -> Result<ConfigApply> {
        self.validate(config, profiles).await?;
        if !force_process {
            let api = MihomoClient::new(&config.controller, config.secret.clone())?;
            if api.reload_config(&Config::runtime_path()).await.is_ok() {
                let _ = restore_selected_nodes(config, profiles).await;
                return Ok(ConfigApply::Reloaded);
            }
        }
        self.stop().await?;
        self.start_validated(config, profiles).await?;
        Ok(ConfigApply::Restarted)
    }

    pub fn is_running(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }

    pub fn recent_logs(limit: usize) -> Result<Vec<String>> {
        let Some(path) = fs::read_dir(Config::logs_dir())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("mihomo-") && name.ends_with(".log"))
            })
            .max_by_key(|path| fs::metadata(path).and_then(|m| m.modified()).ok())
        else {
            return Ok(vec![]);
        };
        let text = fs::read_to_string(path)?;
        let mut lines: Vec<_> = text.lines().rev().take(limit).map(str::to_owned).collect();
        lines.reverse();
        Ok(lines)
    }
}

pub fn ensure_system_core() -> Result<()> {
    let path = Config::mihomo_path();
    if path.is_file() {
        return Ok(());
    }
    let candidates = Config::mihomo_candidates();
    let tried = candidates
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    bail!(
        "Mihomo not found (tried: {tried}). Pick one in the TUI core dialog \
         (download a release or point at an existing binary), or set \
         $CLASHLIME_MIHOMO to its path"
    );
}

/// Adopt a user-provided core binary into the self-managed slot (`dest`,
/// normally `$XDG_DATA_HOME/clashlime/bin/mihomo`): copy it there, make it
/// executable, and verify it answers `mihomo -v`. The managed copy becomes
/// the single source of truth — no override pointer is left behind.
/// A bad copy is removed again; the user's original file is never modified.
pub fn adopt_core_binary(src: &Path, dest: &Path) -> Result<PathBuf> {
    if !src.is_file() {
        bail!("No file at {}", src.display());
    }
    let same = src == dest
        || std::fs::canonicalize(src)
            .ok()
            .zip(std::fs::canonicalize(dest).ok())
            .is_some_and(|(a, b)| a == b);
    if !same {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        fs::copy(src, dest)
            .with_context(|| format!("copy {} to {} failed", src.display(), dest.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(dest, fs::Permissions::from_mode(0o755))
                .with_context(|| format!("cannot make {} executable", dest.display()))?;
        }
    }
    if let Err(error) = crate::update::version_from_binary_at(dest) {
        if !same {
            let _ = fs::remove_file(dest);
        }
        bail!("{} rejected: {error}", src.display());
    }
    Ok(dest.to_path_buf())
}

async fn restore_selected_nodes(config: &Config, profiles: &Profiles) -> Result<()> {
    let selected = profiles.current_selections();
    if selected.is_empty() {
        return Ok(());
    }
    let api = MihomoClient::new(&config.controller, config.secret.clone())?;
    let proxies = api.proxies().await?;
    for selection in selected {
        let Some(group) = proxies.proxies.get(&selection.name) else {
            continue;
        };
        if group.all.iter().any(|node| node == &selection.now) && group.now != selection.now {
            api.select_proxy(&selection.name, &selection.now).await?;
        }
    }
    Ok(())
}

fn ensure_core_resources() -> Result<()> {
    let destination = Config::data_dir().join("Country.mmdb");
    if destination.exists() {
        return Ok(());
    }
    // Prefer user-local GeoIP first, then system paths. Non-privileged users can
    // place Country.mmdb at ~/.local/share/clashlime/Country.mmdb or
    // ~/.local/share/clashlime/geo/Country.mmdb without needing /etc.
    let user_candidates = [
        Config::data_dir().join("geo/Country.mmdb"),
        Config::data_dir().join("Country.mmdb"),
    ];
    // Already checked destination; check alternate user path
    for candidate in &user_candidates {
        if candidate.is_file() && candidate != &destination {
            link_or_copy(candidate, &destination).with_context(|| {
                format!(
                    "failed to link user Country.mmdb from {} to {}",
                    candidate.display(),
                    destination.display()
                )
            })?;
            return Ok(());
        }
    }
    let system_source = ["/etc/mihomo/Country.mmdb", "/etc/clash/Country.mmdb"]
        .into_iter()
        .map(Path::new)
        .find(|path| path.is_file());
    if let Some(source) = system_source {
        link_or_copy(source, &destination).with_context(|| {
            format!(
                "failed to link system Country.mmdb from {} to {}",
                source.display(),
                destination.display()
            )
        })?;
        return Ok(());
    }
    // Non-privileged fallback: allow mihomo to run without Country.mmdb.
    // Many profiles work without GEOIP; mihomo will error on validation if
    // GEOIP is actually required, which we surface to the user.
    // Create parent dir so later auto-download (if any) has a place.
    if let Some(parent) = destination.parent() {
        let _ = fs::create_dir_all(parent);
    }
    Ok(())
}

#[cfg(unix)]
fn link_or_copy(source: &Path, destination: &Path) -> Result<()> {
    std::os::unix::fs::symlink(source, destination)
        .or_else(|_| fs::copy(source, destination).map(|_| ()))?;
    Ok(())
}

#[cfg(not(unix))]
fn link_or_copy(source: &Path, destination: &Path) -> Result<()> {
    fs::copy(source, destination)?;
    Ok(())
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SupervisorState {
    pub running: bool,
    pub pid: Option<u32>,
    pub restarts: u64,
    pub reloads: u64,
    pub enabled: bool,
    pub error: Option<String>,
}

/// Poll the IPC socket until the freshly started daemon answers.
async fn wait_for_daemon(timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if crate::ipc::daemon_alive().await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    bail!(
        "daemon did not answer on {} in time",
        crate::ipc::socket_path().display()
    )
}

pub async fn ensure_supervisor(auto_start: bool) -> Result<()> {
    // Single backend: the systemd user manager owns the daemon. Loud
    // failures — there is nothing to fall back to.
    set_supervisor_autostart(auto_start).await?;
    if crate::ipc::daemon_alive().await {
        return Ok(());
    }
    crate::systemd::start()
        .await
        .context("failed to start clashlime daemon via systemd")?;
    wait_for_daemon(Duration::from_secs(10)).await
}

pub async fn set_supervisor_autostart(enabled: bool) -> Result<()> {
    // Autostart = enabled user unit, no linger: the daemon lives with login
    // sessions and the manager starts it on next login.
    crate::systemd::set_autostart(enabled).await
}

pub async fn run_supervisor(mut config: Config) -> Result<()> {
    let listener = crate::ipc::bind().await?;
    let shared_state = Arc::new(Mutex::new(SupervisorState::default()));
    let flags = Arc::new(crate::ipc::Flags {
        restart: AtomicBool::new(false),
        process_restart: AtomicBool::new(false),
        enabled: AtomicBool::new(true),
        shutdown: AtomicBool::new(false),
    });
    tokio::spawn(crate::ipc::serve(
        listener,
        shared_state.clone(),
        flags.clone(),
    ));
    let socket_path = crate::ipc::socket_path();
    let mut manager = CoreManager::new();
    let mut fingerprint = 0;
    let mut state = SupervisorState {
        enabled: true,
        ..SupervisorState::default()
    };
    let mut proxy_applied = false;
    let mut last_start_attempt: Option<Instant> = None;
    loop {
        // `server stop`: break out below into the shared cleanup path
        // (system proxy off, core stopped, socket removed) and exit
        // cleanly so `Restart=on-failure` does not pull us back up.
        if flags.take_shutdown() {
            crate::logger::info("core", "shutdown requested, stopping supervisor");
            break;
        }
        state.enabled = flags.desired_enabled();
        let enabled = state.enabled;
        let profiles = Profiles::load().unwrap_or_default();
        let current_fingerprint = configuration_fingerprint();
        let restart_requested = flags.take_restart();
        let process_restart_requested = flags.take_process_restart();

        if !enabled || profiles.items.is_empty() {
            if proxy_applied {
                let _ = apply_system_proxy(&config, false).await;
                proxy_applied = false;
            }
            if manager.is_running() {
                manager.stop().await?;
            }
            state.running = false;
            state.pid = None;
            state.error = if !enabled {
                None
            } else {
                profiles
                    .items
                    .is_empty()
                    .then(|| "Mihomo not started: no profile imported".into())
            };
        } else if !manager.is_running() {
            let retry_due =
                last_start_attempt.is_none_or(|attempt| attempt.elapsed() >= START_RETRY_BACKOFF);
            if retry_due {
                if proxy_applied {
                    let _ = apply_system_proxy(&config, false).await;
                    proxy_applied = false;
                }
                state.running = false;
                state.pid = None;
                state.error = Some("starting Mihomo".into());
                if let Ok(mut shared) = shared_state.lock() {
                    *shared = state.clone();
                }
                last_start_attempt = Some(Instant::now());
                match manager.start(&config, &profiles).await {
                    Ok(()) => {
                        state.restarts = state.restarts.saturating_add(1);
                        state.error = None;
                        last_start_attempt = None;
                        if config.system_proxy {
                            match apply_system_proxy(&config, true).await {
                                Ok(()) => proxy_applied = true,
                                Err(error) => {
                                    state.error =
                                        Some(format!("core running; system proxy failed: {error}"))
                                }
                            }
                        }
                    }
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
        } else if fingerprint != 0
            && (fingerprint != current_fingerprint
                || restart_requested
                || process_restart_requested)
        {
            if proxy_applied {
                let _ = apply_system_proxy(&config, false).await;
                proxy_applied = false;
            }
            let result = manager
                .restart(&config, &profiles, process_restart_requested)
                .await;
            match result {
                Ok(outcome) => {
                    match outcome {
                        ConfigApply::Reloaded => state.reloads = state.reloads.saturating_add(1),
                        ConfigApply::Restarted => state.restarts = state.restarts.saturating_add(1),
                    }
                    state.error = None;
                    if config.system_proxy {
                        match apply_system_proxy(&config, true).await {
                            Ok(()) => proxy_applied = true,
                            Err(error) => {
                                state.error =
                                    Some(format!("core running; system proxy failed: {error}"))
                            }
                        }
                    } else if proxy_applied {
                        let _ = apply_system_proxy(&config, false).await;
                        proxy_applied = false;
                    }
                }
                Err(error) => state.error = Some(error.to_string()),
            }
        }
        state.running = manager.is_running();
        state.pid = manager.pid();
        if let Ok(mut shared) = shared_state.lock() {
            *shared = state.clone();
        }
        fingerprint = current_fingerprint;
        if wait_or_shutdown(Duration::from_secs(1)).await {
            break;
        }
        if let Ok(latest) = load_daemon_config() {
            config = latest;
        }
    }
    if proxy_applied {
        let _ = apply_system_proxy(&config, false).await;
    }
    manager.stop().await?;
    state.running = false;
    state.pid = None;
    if let Ok(mut shared) = shared_state.lock() {
        *shared = state;
    }
    let _ = fs::remove_file(&socket_path);
    Ok(())
}

/// Query supervisor state from the daemon over IPC.
pub async fn supervisor_state() -> SupervisorState {
    crate::ipc::state().await.unwrap_or_default()
}

/// Whether the core should be running per the last enable/disable request.
/// Defaults to enabled when no daemon is reachable.
pub async fn core_desired_enabled() -> bool {
    crate::ipc::state()
        .await
        .map_or(true, |state| state.enabled)
}

pub async fn request_core_enabled(enabled: bool) -> Result<()> {
    crate::ipc::set_enabled(enabled).await
}

pub async fn request_restart() -> Result<()> {
    crate::ipc::restart().await
}

/// Ask the daemon to stop the core process and start it again (picks up a
/// replaced core binary; a plain reload would keep the old process).
pub async fn request_process_restart() -> Result<()> {
    crate::ipc::restart_process().await
}

fn load_daemon_config() -> Result<Config> {
    Config::load(&crate::config::Cli {
        command: None,
        daemon: true,
        refresh_ms: None,
        config: None,
    })
}

fn configuration_fingerprint() -> u128 {
    let mut paths = vec![Config::default_path(), Config::profiles_path()];
    if let Ok(entries) = fs::read_dir(Config::profiles_dir()) {
        paths.extend(entries.filter_map(Result::ok).map(|entry| entry.path()));
    }
    paths
        .into_iter()
        .map(|path| modified_nanos(&path))
        .fold(0, u128::wrapping_add)
}

fn modified_nanos(path: &Path) -> u128 {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

#[cfg(unix)]
async fn wait_or_shutdown(duration: Duration) -> bool {
    use tokio::signal::unix::{SignalKind, signal};
    let Ok(mut terminate) = signal(SignalKind::terminate()) else {
        tokio::time::sleep(duration).await;
        return false;
    };
    tokio::select! {
        () = tokio::time::sleep(duration) => false,
        _ = terminate.recv() => true,
        result = tokio::signal::ctrl_c() => result.is_ok(),
    }
}

#[cfg(not(unix))]
async fn wait_or_shutdown(duration: Duration) -> bool {
    tokio::select! {
        () = tokio::time::sleep(duration) => false,
        result = tokio::signal::ctrl_c() => result.is_ok(),
    }
}

impl Drop for CoreManager {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
    }
}

pub async fn apply_system_proxy(config: &Config, enabled: bool) -> Result<()> {
    if command_exists("gsettings").await {
        if enabled {
            let port = config.mixed_port.to_string();
            for protocol in ["http", "https", "socks"] {
                let schema = format!("org.gnome.system.proxy.{protocol}");
                run("gsettings", &["set", &schema, "host", "127.0.0.1"]).await?;
                run("gsettings", &["set", &schema, "port", &port]).await?;
            }
            let bypass = gsettings_bypass(&config.proxy_bypass);
            run(
                "gsettings",
                &["set", "org.gnome.system.proxy", "ignore-hosts", &bypass],
            )
            .await?;
            run(
                "gsettings",
                &["set", "org.gnome.system.proxy", "use-same-proxy", "true"],
            )
            .await?;
        }
        run(
            "gsettings",
            &[
                "set",
                "org.gnome.system.proxy.http",
                "enabled",
                if enabled { "true" } else { "false" },
            ],
        )
        .await?;
        let mode = if enabled { "manual" } else { "none" };
        run(
            "gsettings",
            &["set", "org.gnome.system.proxy", "mode", mode],
        )
        .await?;
    }

    // Omarchy launches desktop applications as UWSM/systemd user units. Such
    // applications do not consistently consume GNOME's gsettings proxy, but
    // inherit the user manager environment. Keep both backends in sync.
    crate::systemd::set_proxy_environment(enabled, config.mixed_port, &config.proxy_bypass).await?;

    // UWSM scopes inherit the (possibly stale) environment of the menu or
    // compositor that launched them. Services inherit the current systemd
    // user-manager environment, allowing proxy changes to reach Chrome and
    // other newly launched Omarchy applications without a new login.
    if command_exists("uwsm-app").await {
        let unit_type = if enabled { "service" } else { "scope" };
        crate::systemd::set_environment(vec![format!("UWSM_APP_UNIT_TYPE={unit_type}")]).await?;
        if crate::systemd::is_unit_active("wayland-wm-app-daemon.service").await {
            crate::systemd::restart_unit("wayland-wm-app-daemon.service").await?;
        }
    }

    Ok(())
}

fn gsettings_bypass(value: &str) -> String {
    let items = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| format!("'{}'", item.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{items}]")
}

async fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {name}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|s| s.success())
}

async fn run(program: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(program).args(args).output().await?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_mihomo(dir: &Path, name: &str, version_line: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\necho \"{version_line}\"\n")).unwrap();
        path
    }

    #[test]
    fn adopts_external_binary_into_managed_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let src = fake_mihomo(tmp.path(), "src-mihomo", "Mihomo Meta v9.9.9 test");
        let src_bytes = std::fs::read(&src).unwrap();
        let dest = tmp.path().join("bin/mihomo");
        let adopted = adopt_core_binary(&src, &dest).unwrap();
        assert_eq!(adopted, dest);
        // Content copied, executable, version verified…
        assert_eq!(std::fs::read(&dest).unwrap(), src_bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_ne!(
                std::fs::metadata(&dest).unwrap().permissions().mode() & 0o111,
                0
            );
        }
        assert!(crate::update::version_from_binary_at(&dest).is_ok());
        // …while the user's original is byte-identical and untouched.
        assert_eq!(std::fs::read(&src).unwrap(), src_bytes);
    }

    #[test]
    fn rejects_missing_source() {
        let tmp = tempfile::tempdir().unwrap();
        let err = adopt_core_binary(&tmp.path().join("nope"), &tmp.path().join("bin/mihomo"))
            .unwrap_err();
        assert!(err.to_string().contains("No file"), "{err}");
        assert!(!tmp.path().join("bin/mihomo").exists());
    }

    #[test]
    fn rejects_non_mihomo_and_cleans_up() {
        let tmp = tempfile::tempdir().unwrap();
        let src = fake_mihomo(tmp.path(), "impostor", "hello world");
        let dest = tmp.path().join("bin/mihomo");
        let err = adopt_core_binary(&src, &dest).unwrap_err();
        assert!(err.to_string().contains("rejected"), "{err}");
        assert!(!dest.exists(), "bad copy must not linger");
    }
}
