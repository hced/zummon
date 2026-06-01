// src/launch.rs - Version resolution + command building + launching
use crate::adapters::niri::NiriAdapter;
use crate::cli::Cli;
use crate::find_latest;
use crate::traits::{Adapter, WindowState};
use crate::zummon_debug;
use anyhow::{Context, Result, anyhow};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

// ============================================================================
// Terminal / TUI Command Building (OS-aware)
// ============================================================================

#[cfg(target_os = "linux")]
async fn detect_terminal(cli: &Cli) -> Result<String> {
    if let Some(term) = &cli.terminal {
        return Ok(term.clone());
    }

    if let Ok(term) = std::env::var("TERMINAL") {
        return Ok(term);
    }

    let priority_list = ["ghostty", "kitty", "foot", "alacritty", "wezterm"];

    for term in priority_list {
        if which::which(term).is_ok() {
            return Ok(term.to_string());
        }
    }

    if which::which("x-terminal-emulator").is_ok() {
        return Ok("x-terminal-emulator".to_string());
    }
    if which::which("gnome-terminal").is_ok() {
        return Ok("gnome-terminal".to_string());
    }
    if which::which("konsole").is_ok() {
        return Ok("konsole".to_string());
    }
    if which::which("xterm").is_ok() {
        return Ok("xterm".to_string());
    }

    Err(anyhow!(
        "No terminal emulator found. Install one or set $TERMINAL."
    ))
}

#[cfg(target_os = "macos")]
async fn detect_terminal(cli: &Cli) -> Result<String> {
    if let Some(term) = &cli.terminal {
        return Ok(term.clone());
    }

    let priority_list = ["iTerm", "Terminal", "alacritty", "kitty", "wezterm"];

    for term in priority_list {
        let app_path = format!("/Applications/{}.app", term);
        if Path::new(&app_path).exists() {
            return Ok(term.to_string());
        }
    }

    Ok("Terminal".to_string())
}

#[cfg(target_os = "windows")]
async fn detect_terminal(cli: &Cli) -> Result<String> {
    if let Some(term) = &cli.terminal {
        return Ok(term.clone());
    }

    // Check for Windows Terminal
    if which::which("wt").is_ok() {
        return Ok("wt".to_string());
    }

    // Fallback to PowerShell
    Ok("powershell".to_string())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
async fn detect_terminal(_cli: &Cli) -> Result<String> {
    Err(anyhow!("Unsupported operating system"))
}

async fn build_tui_command(cli: &Cli, app: &str, extra_args: &[String]) -> Result<String> {
    let terminal_cmd = detect_terminal(cli).await?;

    // OS-specific terminal command building
    if cfg!(target_os = "windows") {
        if terminal_cmd == "wt" {
            let mut parts = vec!["wt".to_string()];
            parts.push(app.to_string());
            parts.extend(extra_args.iter().cloned());
            return Ok(parts.join(" "));
        } else {
            let mut parts = vec!["powershell".to_string(), "-Command".to_string()];
            parts.push(app.to_string());
            parts.extend(extra_args.iter().cloned());
            return Ok(parts.join(" "));
        }
    }

    if cfg!(target_os = "macos") && (terminal_cmd == "iTerm" || terminal_cmd == "Terminal") {
        let script = format!(
            "tell application \"{}\" to activate\ntell application \"{}\" to do script \"{} {}\"",
            terminal_cmd,
            terminal_cmd,
            app,
            extra_args.join(" ")
        );
        return Ok(format!("osascript -e '{}'", script));
    }

    // Linux/Unix terminal building
    let mut terminal_cmd = terminal_cmd.clone();
    if terminal_cmd.contains("ghostty") && terminal_cmd.contains("+new-window") {
        terminal_cmd = terminal_cmd.replace("+new-window", "").trim().to_string();
    }

    let parts: Vec<&str> = terminal_cmd.split_whitespace().collect();
    let (program, terminal_args) = parts
        .split_first()
        .ok_or_else(|| anyhow!("Empty terminal command"))?;

    let terminal_name = Path::new(program)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    let app_display = if app.contains('/') {
        Path::new(app)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    } else {
        app.to_string()
    };

    let window_class = if let Some(class) = &cli.class {
        class.clone()
    } else {
        format!("{}.{}.zummoned", terminal_name, app_display)
    };

    let mut cmd_parts = vec![program.to_string()];
    cmd_parts.extend(terminal_args.iter().map(|s| s.to_string()));

    match terminal_name.as_ref() {
        "kitty" => {
            cmd_parts.push(format!("--class \"{}\"", window_class));
            cmd_parts.push("--".to_string());
            cmd_parts.push(app.to_string());
            cmd_parts.extend(extra_args.iter().cloned());
        }
        "ghostty" => {
            cmd_parts.push(format!("--class=\"{}\"", window_class));
            cmd_parts.push("-e".to_string());
            cmd_parts.push(app.to_string());
            cmd_parts.extend(extra_args.iter().cloned());
        }
        "alacritty" => {
            cmd_parts.push(format!("--class \"{}\"", window_class));
            cmd_parts.push("-e".to_string());
            cmd_parts.push(app.to_string());
            cmd_parts.extend(extra_args.iter().cloned());
        }
        "foot" => {
            cmd_parts.push(format!("--app-id \"{}\"", window_class));
            cmd_parts.push(app.to_string());
            cmd_parts.extend(extra_args.iter().cloned());
        }
        "wezterm" => {
            cmd_parts.push("start".to_string());
            cmd_parts.push(format!("--class \"{}\"", window_class));
            cmd_parts.push("--".to_string());
            cmd_parts.push(app.to_string());
            cmd_parts.extend(extra_args.iter().cloned());
        }
        "gnome-terminal" => {
            cmd_parts.push(format!("--class=\"{}\"", window_class));
            cmd_parts.push("--".to_string());
            cmd_parts.push(app.to_string());
            cmd_parts.extend(extra_args.iter().cloned());
        }
        "xterm" => {
            cmd_parts.push(format!("-class \"{}\"", window_class));
            cmd_parts.push("-e".to_string());
            cmd_parts.push(app.to_string());
            cmd_parts.extend(extra_args.iter().cloned());
        }
        _ => {
            cmd_parts.push("-e".to_string());
            cmd_parts.push(app.to_string());
            cmd_parts.extend(extra_args.iter().cloned());
        }
    }

    Ok(cmd_parts.join(" "))
}

// ============================================================================
// Launch Orchestration (OS-aware)
// ============================================================================

pub async fn launch_app(
    cli: &Cli,
    match_app: &str,
    validated_states: &[WindowState],
    adapter: &mut dyn Adapter,
) -> Result<()> {
    // Check if app path is a directory (handles trailing slashes)
    let app_path = Path::new(&cli.app);
    let is_dir = if app_path.is_dir() {
        true
    } else {
        Path::new(cli.app.trim_end_matches('/')).is_dir()
    };

    // Parse --latest flexibility: bare flag, explicit path, or omitted
    let latest_path = cli.latest.as_ref().filter(|p| p.as_os_str() != ".");
    let used_latest_flag = cli.latest.is_some();

    let (launch_app, extra_args) = if is_dir && !used_latest_flag {
        // Implicit --latest: app is a directory, no flag passed
        zummon_debug!("App is a directory, treating as implicit --latest");
        let pattern = cli
            .extra_args
            .first()
            .map(|s| s.as_str())
            .unwrap_or(&cli.app);
        zummon_debug!("Pattern: {}", pattern);
        let resolved = find_latest::resolve_latest(
            Path::new(cli.app.trim_end_matches('/')),
            pattern,
            cli.use_mod,
        )
        .await?;
        let remaining_args = if cli.extra_args.len() > 1 {
            cli.extra_args[1..].to_vec()
        } else {
            Vec::new()
        };
        (resolved, remaining_args)
    } else if let Some(path) = latest_path {
        // Explicit --latest with a provided path
        zummon_debug!("Resolving latest binary under: {}", path.display());
        (
            find_latest::resolve_latest(path, &cli.app, cli.use_mod).await?,
            cli.extra_args.clone(),
        )
    } else if used_latest_flag {
        // --latest used as a standalone flag
        if is_dir {
            zummon_debug!("--latest flag used, treating APP directory as implicit latest");
            let pattern = cli
                .extra_args
                .first()
                .map(|s| s.as_str())
                .unwrap_or(&cli.app);
            let resolved = find_latest::resolve_latest(
                Path::new(cli.app.trim_end_matches('/')),
                pattern,
                cli.use_mod,
            )
            .await?;
            let remaining_args = if cli.extra_args.len() > 1 {
                cli.extra_args[1..].to_vec()
            } else {
                Vec::new()
            };
            (resolved, remaining_args)
        } else {
            return Err(anyhow!(
                "--latest used but APP '{}' is not a directory. Provide a directory path or use --latest <path>.",
                cli.app
            ));
        }
    } else {
        // Standard launch
        (cli.app.clone(), cli.extra_args.clone())
    };

    zummon_debug!("Launch binary: {}", launch_app);

    let cmd_str = if cli.tui {
        build_tui_command(cli, &launch_app, &extra_args).await?
    } else {
        let mut parts = vec![launch_app.clone()];
        if let Some(class) = &cli.class {
            if cfg!(target_os = "windows") {
                // Windows doesn't use --class
            } else {
                parts.push(format!("--class {}", class));
            }
        }
        for arg in &extra_args {
            parts.push(arg.clone());
        }
        parts.join(" ")
    };

    let mut full_cmd = cmd_str;
    if cli.use_xwayland && cfg!(target_os = "linux") {
        full_cmd = format!(
            "env QT_QPA_PLATFORM=xcb GDK_BACKEND=x11 GDK_SCALE=2 {}",
            full_cmd
        );
    }
    for (key, value) in &cli.env {
        if cfg!(target_os = "windows") {
            full_cmd = format!("set {}={} && {}", key, value, full_cmd);
        } else {
            full_cmd = format!("env {}={} {}", key, value, full_cmd);
        }
    }

    zummon_debug!("Full launch command: {}", full_cmd);

    let pre_spawn_ids = adapter.get_window_ids().await?;
    zummon_debug!("Pre-spawn window IDs: {:?}", pre_spawn_ids);

    if cli.bypass_adapter {
        zummon_debug!("Bypassing window system, launching directly");

        let (shell, shell_arg) = if cfg!(target_os = "windows") {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };

        let child = Command::new(shell)
            .arg(shell_arg)
            .arg(&full_cmd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn application")?;

        zummon_debug!("Process spawned with PID: {}", child.id().unwrap_or(0));
        std::mem::forget(child);
    } else {
        zummon_debug!("Launching through adapter");

        if let Some(niri_adapter) = adapter.as_any_mut().downcast_mut::<NiriAdapter>() {
            let discovered_app_id = niri_adapter.spawn_and_discover_app_id(&full_cmd).await?;

            if let Some(actual_app_id) = discovered_app_id {
                zummon_debug!(
                    "Discovered actual app_id: {} (was looking for: {})",
                    actual_app_id,
                    match_app
                );

                if actual_app_id != match_app {
                    zummon_debug!(
                        "Note: Actual app_id differs. Future runs may need --app-id {}",
                        actual_app_id
                    );
                }
            }
        } else {
            adapter.spawn_command_string(&full_cmd).await?;
        }
    }

    if !validated_states.is_empty() && !cli.bypass_adapter {
        zummon_debug!("Applying window states: {:?}", validated_states);
        adapter
            .apply_window_state(&pre_spawn_ids, validated_states)
            .await?;
    }

    Ok(())
}

pub async fn execute_if_focused_command(cmd_str: &str) -> Result<()> {
    let parts = shell_words::split(cmd_str).context("Failed to parse if-focused command")?;

    let (program, args) = parts
        .split_first()
        .ok_or_else(|| anyhow!("Empty if-focused command"))?;

    let (shell, shell_arg) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };

    let full_cmd = format!("{} {}", program, args.join(" "));

    let child = Command::new(shell)
        .arg(shell_arg)
        .arg(&full_cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to execute if-focused command")?;

    zummon_debug!(
        "If-focused command spawned with PID: {}",
        child.id().unwrap_or(0)
    );
    std::mem::forget(child);

    Ok(())
}

pub async fn build_match_app(cli: &Cli) -> Result<String> {
    if cli.tui && cli.app_id.is_none() && cli.class.is_none() {
        let terminal_cmd = detect_terminal(cli).await?;

        let terminal_name = Path::new(&terminal_cmd)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let terminal_name = terminal_name
            .split_whitespace()
            .next()
            .unwrap_or(&terminal_name)
            .to_string();

        let app_display = if cli.app.contains('/') || cli.app.contains('\\') {
            Path::new(&cli.app)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        } else {
            cli.app.clone()
        };

        Ok(format!("{}.{}.zummoned", terminal_name, app_display))
    } else {
        Ok(cli
            .app_id
            .clone()
            .or_else(|| cli.class.clone())
            .unwrap_or_else(|| {
                let trimmed = cli.app.trim_end_matches('/').trim_end_matches('\\');
                trimmed
                    .rsplit('/')
                    .next()
                    .or_else(|| trimmed.rsplit('\\').next())
                    .unwrap_or(trimmed)
                    .to_string()
            }))
    }
}
