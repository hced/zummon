![Zummon Logo](assets/zummon_logo-512.png#gh-light-mode-only)![Zummon Logo](assets/zummon_logo-alt-512.png#gh-dark-mode-only)

Summon any application to the foreground – or launch it if it isn't running.

## Overview

Zummon is a cross-platform CLI tool that intelligently manages application windows. When invoked, it first checks if an instance of the application is already running. If found, it focuses the existing window. If not, it launches a new instance.

For applications where the window's class/ID differs from the binary name (common with AppImages, browsers, and terminal emulators), Zummon uses heuristic matching to find the correct window automatically.

It can also intelligently determine the latest version if you have a root directory containing multiple versioned subdirectories for a particular program.

## Features

### Core Functionality

- **Focus or Launch:** Provides true single-instance behavior for any app.
- **Cross-Platform:** Runs on Linux (X11/Wayland), macOS, and Windows.
- **Process Detection:** Finds running processes even when the window class mismatches.
- **Heuristic Matching:** Uses the Jaro-Winkler algorithm for fuzzy window matching.
- **Version Resolution:** Launches the latest version from a versioned directory tree.
- **TUI Support:** Launches terminal applications with proper custom window classes.
- **Environment Variables:** Injects custom environment variables into launched apps.
- **Debug Logging:** Supports console output and file logging with automatic rotation.

### Window Management

- **State Flags:** Set windows to fullscreen, maximized, or floating (where supported).
- **Override Mode:** Apply state flags to existing windows as well as new ones.

## Platform Support Matrix

**Note:** The program has currently only been tested on Niri. All other platforms and window systems should be regarded as untested and may not work properly. Feel free to file issue reports so they can potentially be fixed in the future.

### Operating Systems

| Feature          | Linux                 | macOS | Windows |
|------------------|-----------------------|-------|---------|
| Window focusing  | Yes                   | Yes   | Yes     |
| App launching    | Yes                   | Yes   | Yes     |
| Fullscreen/Max   | Yes                   | Yes   | Planned |
| Floating Windows | Yes (compositor-only) | No    | No      |
| XWayland         | Yes                   | N/A   | N/A     |

### Linux Window Systems

| Feature        | Niri/Hyprland | Sway/KWin | Mutter (GNOME) |
|----------------|---------------|-----------|----------------|
| Focus/Launch   | Yes           | Yes       | Launch only    |
| Heuristics     | Yes           | No        | No             |
| Fullscreen/Max | Yes           | Yes       | No             |
| Floating       | Yes           | Yes       | No             |

## Installation

### From Source
```bash
git clone https://github.com/hced/zummon.git
cd zummon
cargo build --release
cp target/release/zummon /usr/local/bin/
```

### Requirements

- Rust 1.70+
- Linux: `pgrep` (usually pre-installed)
- macOS: `pgrep` (usually pre-installed) or `ps`
- Windows: PowerShell 5.0+

## Usage

### Basic
```bash
# Focus Firefox if running, otherwise launch it
zummon firefox

# Always launch a new instance
zummon --new-instance nvim

# Use explicit app-id for matching
zummon --app-id org.kde.dolphin dolphin
```

### Terminal Applications (TUI)
```bash
# Launch yazi in a terminal, focus existing window on subsequent runs
zummon --tui yazi

# Custom terminal
zummon --tui --terminal alacritty btop
```

### Version Resolution
```bash
# Launch latest Blender from versioned directory
zummon --latest ~/Applications/blender blender

# Implicit latest when APP is a directory
zummon ~/Applications/blender blender
```

### Window States
```bash
# Launch maximized and floating (Linux/Wayland)
zummon --maximized-to-edges --floating myapp

# Apply states when focusing existing window
zummon --override --fullscreen myapp
```

### Environment Variables

Set environment variables (each requires its own `-e` flag):
```bash
zummon -e FOO=bar -e BAZ=qux myapp
```

Qt-specific vars example:
```bash
zummon -e QT_SCALE_FACTOR=2 -e QT_QPA_PLATFORM=xcb myapp
```

Force XWayland for legacy apps (Linux only):
```bash
zummon --use-xwayland my-legacy-app
```

### Debugging

You may log debug info to console, a default or custom file, or both.

- Console (stdout): `zummon --debug myapp`
- Default logfile: `zummon --log myapp`
- Custom file: `zummon --log=/tmp/custom.log myapp`
- Combined: `zummon --debug --log myapp`

### Default Log Locations

- Linux: `~/.local/state/zummon/zummon.log`
- macOS: `~/Library/Logs/zummon/zummon.log`
- Windows: `%LOCALAPPDATA%\zummon\logs\zummon.log`

## Exit Status

- **0** - Success (launched, focused, or if-focused command executed)
- **1** - Error (invalid options, unsupported platform)

## License

MIT

## Author

H. Cederblad
