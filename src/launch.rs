// src/launch.rs - Version resolution + command building + launching
use anyhow::{Result, anyhow, Context};
use glob::glob;
use semver::Version;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::fs as async_fs;
use tokio::process::Command;
use crate::cli::Cli;
use crate::traits::{Adapter, WindowState};
use crate::adapters::niri::NiriAdapter;
use crate::zummon_debug;

// ============================================================================
// Version Resolution (--latest)
// ============================================================================

pub async fn resolve_latest(
    versioned_path: &Path,
    app_pattern: &str,
    use_mod_time: bool,
) -> Result<String> {
    zummon_debug!("Version resolution — path: {}  filename: {}", versioned_path.display(), app_pattern);

    let (search_dir, dir_pattern) = if versioned_path.exists() {
        (versioned_path.to_path_buf(), None)
    } else {
        let mut parent = versioned_path.to_path_buf();
        let mut pattern = None;

        while let Some(p) = parent.parent() {
            if p.exists() {
                let relative = versioned_path.strip_prefix(p).unwrap_or(versioned_path);
                pattern = relative.components().next().map(|c| c.as_os_str().to_string_lossy().to_string());
                parent = p.to_path_buf();
                break;
            }
            parent = p.to_path_buf();
        }

        if !parent.exists() {
            return Err(anyhow!("Could not find valid parent directory in path: {}", versioned_path.display()));
        }

        (parent, pattern)
    };

    zummon_debug!("Search directory: {}", search_dir.display());

    if let Some(exe) = find_executable_in_dir(&search_dir, app_pattern).await? {
        zummon_debug!("Found executable directly in search directory");
        return Ok(exe.to_string_lossy().to_string());
    }

    let pattern = dir_pattern.as_deref().unwrap_or(app_pattern);
    zummon_debug!("No direct executable found — searching versioned subdirectories matching: *{}*", pattern);

    let glob_pattern = format!("{}/*{}*", search_dir.display(), pattern);
    let mut subdirs = Vec::new();

    for entry in glob(&glob_pattern)? {
        let path = entry?;
        if path.is_dir() {
            subdirs.push(path);
        }
    }

    if subdirs.is_empty() {
        return Err(anyhow!(
            "No executable or directory matching '*{}*' found in {}",
            pattern,
            search_dir.display()
        ));
    }

    let latest = find_latest_version(&subdirs, use_mod_time).await?;
    zummon_debug!("Selected: {}", latest.file_name().unwrap_or_default().to_string_lossy());

    if let Some(exe) = find_executable_in_dir(&latest, app_pattern).await? {
        zummon_debug!("Resolved binary: {}", exe.display());
        Ok(exe.to_string_lossy().to_string())
    } else {
        Err(anyhow!(
            "No executable named '{}' in {}",
            app_pattern,
            latest.file_name().unwrap_or_default().to_string_lossy()
        ))
    }
}

async fn find_executable_in_dir(dir: &Path, pattern: &str) -> Result<Option<PathBuf>> {
    let bin_dir = dir.join("bin");
    if bin_dir.exists() {
        let glob_pattern = format!("{}/{}", bin_dir.display(), pattern);
        for entry in glob(&glob_pattern)? {
            let path = entry?;
            if is_executable(&path).await {
                return Ok(Some(path));
            }
        }
    }

    let glob_pattern = format!("{}/{}", dir.display(), pattern);
    for entry in glob(&glob_pattern)? {
        let path = entry?;
        if is_executable(&path).await {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

async fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        if let Ok(metadata) = async_fs::metadata(path).await {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        } else {
            false
        }
    }
    #[cfg(windows)]
    {
        // On Windows, check for .exe extension
        path.extension()
            .map(|ext| ext == "exe" || ext == "EXE")
            .unwrap_or(false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

async fn find_latest_version(dirs: &[PathBuf], use_mod_time: bool) -> Result<PathBuf> {
    let mut latest_dir = None;
    let mut latest_version = None;
    let mut latest_mod_time = None;

    for dir in dirs {
        let name = dir.file_name().unwrap_or_default().to_string_lossy();

        if let Some(version_str) = extract_version(&name) {
            zummon_debug!("  - {} (version: {})", name, version_str);

            if let Ok(version) = Version::parse(&version_str) {
                let mod_time = if use_mod_time {
                    async_fs::metadata(dir).await?.modified().ok()
                } else {
                    None
                };

                let is_newer = match &latest_version {
                    None => true,
                    Some(latest) => {
                        if version > *latest {
                            true
                        } else if version == *latest && use_mod_time {
                            mod_time > latest_mod_time
                        } else {
                            false
                        }
                    }
                };

                if is_newer {
                    latest_dir = Some(dir.clone());
                    latest_version = Some(version);
                    latest_mod_time = mod_time;
                }
            }
        } else {
            zummon_debug!("  - {} (version: unknown)", name);
        }
    }

    latest_dir.ok_or_else(|| anyhow!("No versioned directories found"))
}

fn extract_version(s: &str) -> Option<String> {
    use regex::Regex;
    let re = Regex::new(r"(\d+\.\d+(?:\.\d+)*(?:-\d+)?)").ok()?;
    re.find(s).map(|m| m.as_str().to_string())
}

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

    Err(anyhow!("No terminal emulator found. Install one or set $TERMINAL."))
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

    if cfg!(target_os = "macos") {
        if terminal_cmd == "iTerm" || terminal_cmd == "Terminal" {
            let script = format!(
                "tell application \"{}\" to activate\ntell application \"{}\" to do script \"{} {}\"",
                terminal_cmd, terminal_cmd, app, extra_args.join(" ")
            );
            return Ok(format!("osascript -e '{}'", script));
        }
    }

    // Linux/Unix terminal building
    let mut terminal_cmd = terminal_cmd.clone();
    if terminal_cmd.contains("ghostty") && terminal_cmd.contains("+new-window") {
        terminal_cmd = terminal_cmd.replace("+new-window", "").trim().to_string();
    }

    let parts: Vec<&str> = terminal_cmd.split_whitespace().collect();
    let (program, terminal_args) = parts.split_first()
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

    let (launch_app, extra_args) = if is_dir && cli.latest.is_none() {
        // Implicit --latest: app is a directory
        zummon_debug!("App is a directory, treating as implicit --latest");
        let pattern = cli.extra_args.first().map(|s| s.as_str()).unwrap_or(&cli.app);
        zummon_debug!("Pattern: {}", pattern);
        let resolved = resolve_latest(Path::new(cli.app.trim_end_matches('/')), pattern, cli.use_mod).await?;
        // Remove the pattern from extra_args so it doesn't get passed to the app
        let remaining_args = if cli.extra_args.len() > 1 {
            cli.extra_args[1..].to_vec()
        } else {
            Vec::new()
        };
        (resolved, remaining_args)
    } else if let Some(path) = &cli.latest {
        zummon_debug!("Resolving latest binary under: {}", path.display());
        (resolve_latest(path, &cli.app, cli.use_mod).await?, cli.extra_args.clone())
    } else {
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
        full_cmd = format!("env QT_QPA_PLATFORM=xcb GDK_BACKEND=x11 GDK_SCALE=2 {}", full_cmd);
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
                zummon_debug!("Discovered actual app_id: {} (was looking for: {})", actual_app_id, match_app);

                if actual_app_id != match_app {
                    zummon_debug!("Note: Actual app_id differs. Future runs may need --app-id {}", actual_app_id);
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
    let parts = shell_words::split(cmd_str)
        .context("Failed to parse if-focused command")?;

    let (program, args) = parts.split_first()
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

    zummon_debug!("If-focused command spawned with PID: {}", child.id().unwrap_or(0));
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
        Ok(cli.app_id
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
