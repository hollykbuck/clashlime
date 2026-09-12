use crate::config::Config;
use chrono::Local;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
/// True while running as the supervisor daemon; selects the daemon log file.
static DAEMON_ROLE: AtomicBool = AtomicBool::new(false);
/// WARN/ERROR echo to stderr (for journald under `--daemon`). The TUI turns
/// this off: stderr would corrupt the alternate screen.
static STDERR_ECHO: AtomicBool = AtomicBool::new(true);

/// Enable/disable the stderr echo of WARN/ERROR logs.
pub fn set_stderr_echo(enabled: bool) {
    STDERR_ECHO.store(enabled, Ordering::Relaxed);
}

pub fn init() {
    init_role(false);
}

/// Logger for the supervisor daemon process (separate `omash-daemon-*.log`
/// so TUI and daemon output stay attributable).
pub fn init_daemon() {
    init_role(true);
}

fn init_role(daemon: bool) {
    DAEMON_ROLE.store(daemon, Ordering::Relaxed);
    let path = role_log_path(daemon);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut guard) = LOG_PATH.lock() {
        *guard = Some(path);
    }
    info("omash", "logger initialized");
}

fn role_log_path(daemon: bool) -> PathBuf {
    if daemon {
        Config::omash_daemon_log_path()
    } else {
        Config::omash_log_path()
    }
}

fn log_path() -> PathBuf {
    if let Ok(guard) = LOG_PATH.lock() {
        if let Some(path) = guard.as_ref() {
            return path.clone();
        }
    }
    role_log_path(DAEMON_ROLE.load(Ordering::Relaxed))
}

pub fn log(level: Level, target: &str, message: &str) {
    let now = Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{}] {:5} [{}] {}\n", now, level.as_str(), target, message);
    // stderr for journal/systemd; suppressed while the TUI owns the screen
    if matches!(level, Level::Error | Level::Warn)
        && STDERR_ECHO.load(Ordering::Relaxed)
    {
        eprint!("{line}");
    }
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn debug(target: &str, msg: &str) {
    log(Level::Debug, target, msg);
}
pub fn info(target: &str, msg: &str) {
    log(Level::Info, target, msg);
}
pub fn warn(target: &str, msg: &str) {
    log(Level::Warn, target, msg);
}
pub fn error(target: &str, msg: &str) {
    log(Level::Error, target, msg);
}

pub fn recent_logs(limit: usize) -> Vec<String> {
    recent_logs_for("omash-tui-", limit)
}

/// Tail of the newest `<prefix>*.log` (e.g. `omash-daemon-`). Exact-prefix
/// match so legacy `omash-<date>.log` files are never picked up.
pub fn recent_logs_for(prefix: &str, limit: usize) -> Vec<String> {
    use std::fs;
    let dir = Config::logs_dir();
    let Some(path) = fs::read_dir(&dir).ok().and_then(|entries| {
        entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(prefix) && n.ends_with(".log"))
            })
            .max_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok())
    }) else {
        return vec![];
    };
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<_> = text.lines().rev().take(limit).map(str::to_owned).collect();
    lines.reverse();
    lines
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::logger::info(module_path!(), &format!($($arg)*))
    };
}
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::logger::warn(module_path!(), &format!($($arg)*))
    };
}
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::logger::error(module_path!(), &format!($($arg)*))
    };
}
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        $crate::logger::debug(module_path!(), &format!($($arg)*))
    };
}
