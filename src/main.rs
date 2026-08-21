mod api;
mod app;
mod backup;
mod config;
mod core;
mod enhance;
mod logger;
mod omarchy;
mod profiles;
mod statusbar;
mod theme;
mod ui;

use anyhow::Result;
use app::App;
use clap::Parser;
use config::{Cli, Command, Config};
use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{self, stdout};

#[tokio::main]
async fn main() -> Result<()> {
    logger::init();
    let cli = Cli::parse();
    let config = Config::load(&cli)?;
    log_info!("omash started, controller={}, mixed_port={}", config.controller, config.mixed_port);
    if let Some(Command::Bar(args)) = &cli.command {
        return statusbar::run(&config, &args.command).await;
    }
    if let Err(error) = core::ensure_system_core() {
        // Non-privileged mode: allow TUI to run without core, supervisor will retry.
        // Only bail for the daemon itself if core is required to supervise.
        if cli.daemon {
            return Err(error);
        }
        eprintln!("warning: {error}");
    }
    if cli.daemon {
        return core::run_supervisor(config).await;
    }
    // Supervisor setup is best-effort for non-systemd environments
    if let Err(error) = core::ensure_supervisor(config.auto_start).await {
        eprintln!("warning: supervisor setup failed: {error} (continuing without systemd)");
    }
    let mut app = App::new(config)?;
    let mut terminal = setup_terminal()?;
    let result = app.run(&mut terminal).await;
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    execute!(
        stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    Ok(Terminal::new(CrosstermBackend::new(stdout()))?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}
