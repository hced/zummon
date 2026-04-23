// src/cli.rs - CLI parsing only
use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, ValueEnum)]
pub enum WindowStateFlag {
    Fullscreen,
    Maximized,
    MaximizedToEdges,
    Floating,
}

impl WindowStateFlag {
    pub fn to_abstract_state(&self) -> &'static str {
        match self {
            WindowStateFlag::Fullscreen => "fullscreen",
            WindowStateFlag::Maximized => "maximize-edges",
            WindowStateFlag::MaximizedToEdges => "maximize-edges",
            WindowStateFlag::Floating => "floating",
        }
    }
}

fn get_after_help() -> String {
    format!("\
NAME
     zummon — summon an application to the foreground or launch it if not running

SYNOPSIS
     zummon [OPTIONS] <APP> [--] [EXTRA_ARGS]...

DESCRIPTION
     Zummon intelligently manages application windows. When invoked, it first
     checks if an instance of the application is already running. If found, it
     focuses the existing window. If not, it launches a new instance.

     For applications where the window's class/ID differs from the binary name
     (common with AppImages and some browsers), zummon uses heuristic matching
     to find the correct window automatically.

UNIVERSAL OPTIONS
   Help & Information:
     --help                Show this help message and exit
     --version             Print version information and exit
     --debug               Enable detailed debug output to stdout
     --log[=<FILE>]        Write debug output to a file. Use without value for
                           default location, or with = to specify custom path.
     -V, --verbose         Enable informational output

   Window Management:
     --new-instance        Launch a new instance, don't try to focus existing
     --if-focused <CMD>    Execute command if the application is already focused

   Application Matching:
     --app-id <ID>         Explicitly set the window class/ID for matching
     --class <NAME>        Set custom window class for both launch and matching

   Terminal (TUI) Applications:
     --tui                 Launch application inside a terminal emulator
     -t, --terminal <TERM> Specify terminal emulator to use

   Launch Environment:
     -e, --env <KEY=VAL>   Set environment variable (may be used multiple times)
     --use-xwayland        Force XWayland (Linux only: sets QT_QPA_PLATFORM=xcb)
     --bypass-adapter      Launch directly without window system integration

   Version Resolution:
     --latest <PATH>       Resolve and launch the latest version from a directory.
                           If omitted and the APP argument is a directory,
                           zummon will automatically resolve the latest version.
     -m, --mod             Use modification time as version tiebreaker

PLATFORM-SPECIFIC OPTIONS
   The following options depend on platform and window system support.

   Linux (Niri, Hyprland, Sway, KWin):
     --fullscreen          Launch window in fullscreen mode
     --maximized           Maximize window (shorthand for --maximized-to-edges)
     --maximized-to-edges  Stretch window to screen edges
     --floating            Launch window as floating rather than tiled
     --override            Apply window state flags when focusing existing windows

   macOS, Windows:
     --fullscreen          Launch window in fullscreen mode
     --maximized           Maximize window (shorthand for --maximized-to-edges)
     --maximized-to-edges  Stretch window to screen edges

EXAMPLES
   Universal usage:
     zummon firefox              Focus Firefox if running, otherwise launch it
     zummon --new-instance nvim  Always launch a new instance
     zummon -e EDITOR=nvim myapp
     zummon --app-id myapp myapp-bin
     zummon --tui --class finance.terminal yazi

   Version-aware (--latest optional if APP is a directory):
     zummon --latest ~/Applications/blender blender
     zummon ~/Applications/blender blender   # Same as above

   Debugging:
     zummon --debug myapp                        # Log to stdout
     zummon --log myapp                          # Log to default file
     zummon --log=/path/to/custom.log myapp      # Log to custom file
     zummon --debug --log myapp                  # Log to both stdout and file

   Platform-specific:
     # Linux (Niri, Hyprland, Sway)
     zummon --maximized-to-edges --floating --override myapp
     zummon --tui --floating htop
     zummon --use-xwayland -e QT_SCALE_FACTOR=2 /path/to/legacy.AppImage

PLATFORM SUPPORT
     Linux:   Niri (full), Hyprland, Sway, KWin (KDE), Mutter (GNOME, launch only)
     macOS:   Quartz (core features)
     Windows: DWM (core features)

DEFAULT LOG LOCATIONS
     Linux:   ~/.local/state/zummon/zummon.log
     macOS:   ~/Library/Logs/zummon/zummon.log
     Windows: %LOCALAPPDATA%\\zummon\\logs\\zummon.log

EXIT STATUS
     0    Success (launched, focused, or if-focused command executed)
     1    Error (invalid options, unsupported platform)

AUTHOR
     H. Cederblad\n\nZummon {}",
     env!("CARGO_PKG_VERSION"))
}

#[derive(Debug, Parser)]
#[command(
    name = "zummon",
    version,
    about = "Summons an app to the foreground or launches it if not running",
    long_about = None,
    after_help = get_after_help(),
    disable_help_flag = true,
    disable_version_flag = true
)]
pub struct Cli {
    #[arg(long, action = clap::ArgAction::Help)]
    help: Option<bool>,

    #[arg(long, action = clap::ArgAction::Version)]
    version: Option<bool>,

    #[arg(long)]
    pub version_history: bool,

    #[arg(long)]
    pub debug: bool,

    /// Write debug output to a file. Use without value for default location,
    /// or with = to specify path: --log=/path/to/file.log
    #[arg(long = "log", require_equals = true, num_args = 0..=1)]
    pub log: Option<Option<PathBuf>>,

    #[arg(short = 'V', long)]
    pub verbose: bool,

    #[arg(long)]
    pub new_instance: bool,

    #[arg(long)]
    pub tui: bool,

    #[arg(short = 't', long)]
    pub terminal: Option<String>,

    #[arg(long)]
    pub app_id: Option<String>,

    #[arg(long)]
    pub class: Option<String>,

    #[arg(long)]
    pub latest: Option<PathBuf>,

    #[arg(short = 'm', long)]
    pub use_mod: bool,

    #[arg(long)]
    pub if_focused: Option<String>,

    #[arg(long = "override")]
    pub override_state: bool,

    #[arg(long)]
    pub use_xwayland: bool,

    #[arg(long)]
    pub bypass_adapter: bool,

    #[arg(short = 'e', long = "env", value_parser = parse_env_var)]
    pub env: Vec<(String, String)>,

    #[arg(long)]
    pub fullscreen: bool,

    #[arg(long)]
    pub maximized: bool,

    #[arg(long, alias = "maximized-to-edges", alias = "maximize-to-edges")]
    pub maximized_to_edges: bool,

    #[arg(long)]
    pub floating: bool,

    pub app: String,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub extra_args: Vec<String>,
}

impl Cli {
    pub fn window_states(&self) -> Vec<WindowStateFlag> {
        let mut states = Vec::new();
        if self.fullscreen {
            states.push(WindowStateFlag::Fullscreen);
        }
        if self.maximized {
            states.push(WindowStateFlag::Maximized);
        }
        if self.maximized_to_edges {
            states.push(WindowStateFlag::MaximizedToEdges);
        }
        if self.floating {
            states.push(WindowStateFlag::Floating);
        }
        states
    }
}

fn parse_env_var(s: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = s.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid environment variable: {}", s));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}
