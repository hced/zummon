<p align="center">
  <img src="assets/branding/zummon_logo-512.png#gh-light-mode-only" alt="Zummon Logo" width="512">
  <img src="assets/branding/zummon_logo-alt-512.png#gh-dark-mode-only" alt="Zummon Logo" width="512">
</p>

Summon any application to the foreground – or launch it if it isn't running.

---

**⚠ Status:** Currently this program has been tested on **Linux with Niri**. While it includes adapters for macOS, Windows, Hyprland, Sway, Mutter (GNOME) and KWin (KDE), these are **unverified** and may not work correctly. Testing and contributions are welcome.

---

## Overview

Zummon is a cross-platform CLI tool that intelligently manages application windows. When invoked, it first checks if an instance of the application is already running. If found, it focuses the existing window. If not, it launches a new instance.

For applications where the window's class/ID differs from the binary name (common with AppImages, browsers, and terminal emulators), Zummon uses heuristic matching to find the correct window automatically.

It can also intelligently determine the latest program version if you have a root directory containing multiple versioned subdirectories:

```
zummon ~/.local/opt blender

.local/opt/blender 
├── blender-5.0.1-linux-x64
│   ├── 5.0
│   ├── blender
│   ├── ...
└── blender-5.1.0-linux-x64
    ├── 5.1
    ├── blender <-- will match this executable
    ├── ...
```

## Features

### Core Functionality

- **Focus or Launch:** Provides true single-instance behavior for any app.
- **Cross-Platform:** Runs on Linux (X11/Wayland), macOS, and Windows.
- **Process Detection:** Finds running processes even when the window class mismatches.
- **Heuristic Matching:** Uses the Jaro-Winkler algorithm for fuzzy window matching.
- **Version Resolution:** Can launch the latest application from a versioned directory tree.
- **TUI Support:** Can launch TUI apps in separate terminal windows with custom window classes.
- **Environment Variables:** Can inject custom environment variables into launched apps.
- **Debug Logging:** Supports console output and file logging with automatic rotation.

### Smart Focus Actions

* **Alternative action if focused:** Using the `--if-focused` flag you can run an alternative command when the target app is already focused. For example, tell Ghostty to open a new window instead of doing nothing: `zummon --if-focused "ghostty +new-window" ghostty`.
* **App Toggling:** Using the same flag you can create toggles between two apps: `zummon --if-focused "zummon app2" app1` + `zummon --if-focused "zummon app1" app2`.
* **Toggle Chains:** Similarly, you can even set up multi-app toggle chains by cycling through a list of apps, launching any that aren't running yet: `zummon --if-focused "zummon app2" app1` + `zummon --if-focused "zummon app3" app2` + `zummon --if-focused "zummon app1" app3`.

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

### From Source (Linux, macOS)
```bash
git clone https://github.com/hced/zummon.git
cd zummon
cargo build --release
cp target/release/zummon /usr/local/bin/
```

## Usage (Linux)

These examples are Linux-specific but should be pretty similar on other platforms.

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

The source code is licensed under the MIT License (see LICENSE).

The Zummon logo and branding assets are Copyright © 2026 H. Cederblad.
All rights reserved. They may not be used, modified, or distributed
without explicit permission.

## Author

H. Cederblad
