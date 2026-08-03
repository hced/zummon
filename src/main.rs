// src/main.rs - Entry point and orchestration
use tracing_subscriber::prelude::*;
mod adapters;
mod cli;
mod find_latest;
mod focus;
mod launch;
mod traits;

#[macro_export]
macro_rules! zummon_debug {
    ($($arg:tt)*) => {
        ::tracing::debug!("[zummon {}] {}", env!("CARGO_PKG_VERSION"), format!($($arg)*))
    };
}

use anyhow::{Result, anyhow};
use clap::Parser;
use cli::Cli;
use std::path::{Path, PathBuf};
use traits::{Adapter, LinuxWindowSystem, Platform};

/// CRITICAL FIX: Restore essential session environment variables that Niri strips.
/// By doing this at the very start of main, ALL child processes (launch, deliver_args, etc.)
/// will automatically inherit the correct environment without needing individual patches.
fn ensure_session_environment() {
    // 1. Fix DBUS_SESSION_BUS_ADDRESS
    if std::env::var("DBUS_SESSION_BUS_ADDRESS").is_err() {
        if let Ok(output) = std::process::Command::new("id").arg("-u").output() {
            if let Ok(uid_str) = String::from_utf8(output.stdout) {
                let uid = uid_str.trim();
                let dbus_path = format!("/run/user/{}/bus", uid);
                if std::path::Path::new(&dbus_path).exists() {
                    // SAFETY: Called at the very start of main, before any threads are spawned.
                    unsafe {
                        std::env::set_var(
                            "DBUS_SESSION_BUS_ADDRESS",
                            format!("unix:path={}", dbus_path),
                        );
                    }
                }
            }
        }
    }

    // 2. Fix XDG_RUNTIME_DIR
    if std::env::var("XDG_RUNTIME_DIR").is_err() {
        if let Ok(output) = std::process::Command::new("id").arg("-u").output() {
            if let Ok(uid_str) = String::from_utf8(output.stdout) {
                let runtime_dir = format!("/run/user/{}", uid_str.trim());
                if std::path::Path::new(&runtime_dir).exists() {
                    // SAFETY: Called at the very start of main, before any threads are spawned.
                    unsafe {
                        std::env::set_var("XDG_RUNTIME_DIR", runtime_dir);
                    }
                }
            }
        }
    }

    // 3. Fix XDG_DATA_DIRS (required for MIME type resolution)
    if std::env::var("XDG_DATA_DIRS").is_err() {
        // SAFETY: Called at the very start of main, before any threads are spawned.
        unsafe {
            std::env::set_var("XDG_DATA_DIRS", "/usr/local/share:/usr/share");
        }
    }

    // 4. Fix XDG_CONFIG_DIRS
    if std::env::var("XDG_CONFIG_DIRS").is_err() {
        // SAFETY: Called at the very start of main, before any threads are spawned.
        unsafe {
            std::env::set_var("XDG_CONFIG_DIRS", "/etc/xdg");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Restore environment before anything else happens
    ensure_session_environment();

    let cli = Cli::parse();
    if cli.toggle && (cli.new_instance || cli.if_focused.is_some()) {
        return Err(anyhow!(
            "--toggle cannot be combined with --new-instance or --if-focused"
        ));
    }
    if let Some(maybe_path) = &cli.log {
        let log_path = match maybe_path {
            Some(path) => path.clone(),
            None => default_log_path(),
        };
        let log_file = setup_log_file(&log_path).await?;
        if cli.debug {
            let console_layer = tracing_subscriber::fmt::layer()
                .with_target(false)
                .without_time()
                .with_writer(std::io::stdout)
                .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG);
            let file_layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_target(false)
                .without_time()
                .with_writer(log_file)
                .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG);
            tracing_subscriber::registry()
                .with(console_layer)
                .with(file_layer)
                .init();
        } else {
            let level_filter = if cli.verbose {
                tracing_subscriber::filter::LevelFilter::INFO
            } else {
                tracing_subscriber::filter::LevelFilter::DEBUG
            };
            let file_layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_target(false)
                .without_time()
                .with_writer(log_file)
                .with_filter(level_filter);
            tracing_subscriber::registry().with(file_layer).init();
        }
    } else if cli.debug {
        let filter = if cli.verbose { "info" } else { "debug" };
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .without_time()
            .with_writer(std::io::stdout)
            .init();
    }

    let platform = Platform::detect();
    if cli.debug || cli.verbose {
        eprintln!("[zummon {}] Starting...", env!("CARGO_PKG_VERSION"));
        match &platform {
            Platform::Linux(window_system) => {
                eprintln!(
                    "[zummon {}] Operating System: Linux",
                    env!("CARGO_PKG_VERSION")
                );
                eprintln!(
                    "[zummon {}] Window system: {}",
                    env!("CARGO_PKG_VERSION"),
                    window_system.name()
                );
            }
            Platform::MacOS => {
                eprintln!(
                    "[zummon {}] Operating System: macOS",
                    env!("CARGO_PKG_VERSION")
                );
            }
            Platform::Windows => {
                eprintln!(
                    "[zummon {}] Operating System: Windows",
                    env!("CARGO_PKG_VERSION")
                );
            }
            Platform::Unknown => {
                eprintln!(
                    "[zummon {}] Operating System: Unknown",
                    env!("CARGO_PKG_VERSION")
                );
            }
        }
    }

    zummon_debug!("Detected platform: {:?}", platform);
    zummon_debug!("CLI: app={}, new_instance={}", cli.app, cli.new_instance);

    let match_app = launch::build_match_app(&cli).await?;
    zummon_debug!("Match app ID: {}", match_app);

    let mut adapter: Box<dyn Adapter> = match &platform {
        Platform::Linux(window_system) => match window_system {
            LinuxWindowSystem::Niri => Box::new(adapters::niri::NiriAdapter::new().await?),
            LinuxWindowSystem::Hyprland => {
                Box::new(adapters::hyprland::HyprlandAdapter::new().await?)
            }
            LinuxWindowSystem::Sway => Box::new(adapters::sway::SwayAdapter::new().await?),
            LinuxWindowSystem::Kwin => Box::new(adapters::kwin::KwinAdapter::new().await?),
            LinuxWindowSystem::Mutter => Box::new(adapters::mutter::MutterAdapter::new().await?),
            _ => {
                return Err(anyhow!(
                    "Unsupported Linux window system: {}\nCurrently Niri, Hyprland, Sway, KWin, and Mutter are supported.",
                    window_system.name()
                ));
            }
        },
        Platform::MacOS => Box::new(adapters::macos::MacOSAdapter::new().await?),
        Platform::Windows => Box::new(adapters::windows::WindowsAdapter::new().await?),
        Platform::Unknown => {
            return Err(anyhow!("Could not detect operating system."));
        }
    };

    let validated_states = adapter.validate_states(cli.window_states()).await?;
    zummon_debug!("Validated window states: {:?}", validated_states);

    let found_window = if !cli.new_instance {
        let mut found_window = None;
        if let Some(window_id) = adapter.find_window(&match_app).await? {
            found_window = Some(window_id);
        }

        if found_window.is_none() && cli.app_id.is_none() && cli.class.is_none() && !cli.tui {
            let is_running = focus::is_process_running(&cli.app)?;
            if is_running {
                zummon_debug!("Process is running but no window found. Applying heuristics...");
                if let Some(hypr_adapter) = adapter
                    .as_any_mut()
                    .downcast_mut::<adapters::hyprland::HyprlandAdapter>()
                {
                    if let Some(window_id) =
                        hypr_adapter.find_window_with_heuristics(&cli.app).await?
                    {
                        zummon_debug!("Heuristics (Hyprland) found window!");
                        found_window = Some(window_id);
                    }
                } else if let Some(sway_adapter) = adapter
                    .as_any_mut()
                    .downcast_mut::<adapters::sway::SwayAdapter>()
                {
                    if let Some(window_id) =
                        sway_adapter.find_window_with_heuristics(&cli.app).await?
                    {
                        zummon_debug!("Heuristics (Sway) found window!");
                        found_window = Some(window_id);
                    }
                } else if let Some(niri_adapter) = adapter
                    .as_any_mut()
                    .downcast_mut::<adapters::niri::NiriAdapter>()
                {
                    if let Some(window_id) =
                        focus::find_window_with_heuristics(niri_adapter, &cli.app).await?
                    {
                        zummon_debug!("Heuristics (Niri) found window!");
                        found_window = Some(window_id);
                    }
                } else if let Some(macos_adapter) = adapter
                    .as_any_mut()
                    .downcast_mut::<adapters::macos::MacOSAdapter>()
                {
                    if let Some(window_id) =
                        macos_adapter.find_window_with_heuristics(&cli.app).await?
                    {
                        zummon_debug!("Heuristics (macOS) found window!");
                        found_window = Some(window_id);
                    }
                } else if let Some(windows_adapter) = adapter
                    .as_any_mut()
                    .downcast_mut::<adapters::windows::WindowsAdapter>(
                ) {
                    if let Some(window_id) = windows_adapter
                        .find_window_with_heuristics(&cli.app)
                        .await?
                    {
                        zummon_debug!("Heuristics (Windows) found window!");
                        found_window = Some(window_id);
                    }
                }
            }
        }

        found_window
    } else {
        zummon_debug!("--new-instance set, skipping window lookup");
        None
    };

    if cli.toggle {
        match found_window {
            Some(window_id) => {
                zummon_debug!("Toggle: closing window {}", window_id);
                adapter.close_window(&window_id).await?;
                let mut attempts = 0;
                while attempts < 8 && focus::is_process_running(&cli.app)? {
                    tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
                    attempts += 1;
                }
                if focus::is_process_running(&cli.app)? {
                    zummon_debug!("Process still running after window close, terminating it");
                    focus::terminate_process(&cli.app)?;
                }
            }
            None => {
                if focus::is_process_running(&cli.app)? {
                    zummon_debug!("Toggle: no window found but process running, terminating it");
                    focus::terminate_process(&cli.app)?;
                } else {
                    zummon_debug!("Toggle: app not running, launching");
                    launch::launch_app(&cli, &match_app, &validated_states, &mut *adapter).await?;
                }
            }
        }
        return Ok(());
    }

    let should_launch = match found_window {
        Some(window_id) => {
            zummon_debug!("Found window: {}", window_id);
            let focused_id = adapter.get_focused_window().await?;
            if focused_id == Some(window_id.clone()) {
                if let Some(cmd) = &cli.if_focused {
                    zummon_debug!("Window already focused, executing: {}", cmd);
                    launch::execute_if_focused_command(cmd).await?;
                    if !cli.extra_args.is_empty() {
                        zummon_debug!("Delivering extra_args after if-focused command");
                        launch::deliver_args_to_running(&cli).await?;
                    }
                    return Ok(());
                }
                if !cli.extra_args.is_empty() {
                    zummon_debug!(
                        "Window already focused, delivering extra_args ({} item(s))",
                        cli.extra_args.len()
                    );
                    launch::deliver_args_to_running(&cli).await?;
                    return Ok(());
                }
                zummon_debug!("Window already focused, doing nothing");
                return Ok(());
            }
            zummon_debug!("Focusing window: {}", window_id);
            adapter.focus_window(&window_id).await?;
            if cli.override_state && !validated_states.is_empty() {
                adapter
                    .apply_states_to_window(&window_id, &validated_states)
                    .await?;
            }
            if !cli.extra_args.is_empty() {
                zummon_debug!(
                    "extra_args present ({} item(s)), delivering to running instance",
                    cli.extra_args.len()
                );
                launch::deliver_args_to_running(&cli).await?;
            }
            false
        }
        None => {
            zummon_debug!("No existing window found");
            true
        }
    };

    if should_launch {
        launch::launch_app(&cli, &match_app, &validated_states, &mut *adapter).await?;
    }

    Ok(())
}

fn default_log_path() -> PathBuf {
    match std::env::consts::OS {
        "linux" => {
            let state_dir = dirs::state_dir().unwrap_or_else(|| Path::new("~/.local/state").into());
            state_dir.join("zummon").join("zummon.log")
        }
        "macos" => {
            let home = dirs::home_dir().unwrap_or_else(|| Path::new(".").into());
            home.join("Library")
                .join("Logs")
                .join("zummon")
                .join("zummon.log")
        }
        "windows" => {
            let local_app_data = dirs::data_local_dir().unwrap_or_else(|| Path::new(".").into());
            local_app_data
                .join("zummon")
                .join("logs")
                .join("zummon.log")
        }
        _ => Path::new("zummon.log").to_path_buf(),
    }
}

async fn setup_log_file(path: &Path) -> Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    rotate_log_if_needed(path).await?;
    Ok(std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?)
}

async fn rotate_log_if_needed(path: &Path) -> Result<()> {
    const MAX_SIZE: u64 = 10 * 1024 * 1024;
    const MAX_FILES: usize = 5;
    if let Ok(metadata) = tokio::fs::metadata(path).await
        && metadata.len() > MAX_SIZE
    {
        for i in (1..MAX_FILES).rev() {
            let old = format!("{}.{}", path.display(), i);
            let new = format!("{}.{}", path.display(), i + 1);
            let _ = tokio::fs::rename(&old, &new).await;
        }
        let _ = tokio::fs::rename(path, format!("{}.1", path.display())).await;
    }
    Ok(())
}
