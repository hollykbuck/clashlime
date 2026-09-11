mod api;
mod app;
mod backup;
mod config;
mod core;
mod enhance;
mod geo;
mod ipc;
mod logger;
mod omarchy;
mod profiles;
mod statusbar;
mod theme;
mod ui;
mod update;

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
    log_info!(
        "omash started, controller={}, mixed_port={}",
        config.controller,
        config.mixed_port
    );
    if let Some(Command::Bar(args)) = &cli.command {
        return statusbar::run(&config, &args.command).await;
    }
    if let Some(Command::Update(args)) = &cli.command {
        return handle_update_command(args, &config).await;
    }
    if !cli.daemon {
        // The TUI owns feedback from here on (interactive core-missing dialog);
        // WARN/ERROR stay in the log file instead of the future alternate screen.
        logger::set_stderr_echo(false);
    }
    if let Err(error) = core::ensure_system_core() {
        if cli.daemon {
            return Err(error);
        }
        // Non-privileged mode: enter the TUI anyway; its dialog offers download
        // or a manual path, and the supervisor retries in the background.
        log_warn!("core missing, starting TUI anyway: {error}");
    }
    if cli.daemon {
        return core::run_supervisor(config).await;
    }
    // Supervisor setup is best-effort for non-systemd environments
    if let Err(error) = core::ensure_supervisor(config.auto_start).await {
        log_warn!("supervisor setup failed: {error} (continuing without systemd)");
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

async fn handle_update_command(args: &config::UpdateArgs, _config: &Config) -> Result<()> {
    use config::UpdateCommand;
    match &args.command {
        UpdateCommand::Check { force, json } => {
            let current = update::current_version_from_binary().or_else(|_| {
                // fallback to version file or snapshot stub
                anyhow::bail!("cannot determine local mihomo version")
            })?;
            match update::check_update(&current, *force).await {
                Ok((release, available)) => {
                    if *json {
                        let out = serde_json::json!({
                            "current": current,
                            "latest": release.tag_name,
                            "available": available,
                            "url": release.html_url,
                            "prerelease": release.prerelease,
                            "published_at": release.published_at,
                        });
                        println!("{}", serde_json::to_string_pretty(&out)?);
                    } else if available {
                        println!(
                            "Update available: {current} → {} \n{}",
                            release.tag_name, release.html_url
                        );
                    } else {
                        println!("Up to date: {current} (latest {})", release.tag_name);
                    }
                }
                Err(e) => {
                    if *json {
                        let out = serde_json::json!({
                            "current": current,
                            "error": e.to_string(),
                        });
                        println!("{}", serde_json::to_string_pretty(&out)?);
                    } else {
                        eprintln!("Update check failed (current {current}): {e}");
                    }
                    std::process::exit(1);
                }
            }
            Ok(())
        }
    }
}
