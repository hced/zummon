# Zummon Specification

Status: maintained by Sarah (Compliance Auditor). Source of truth for project behavior; README.md mirrors the user-facing parts.

## Project Overview

Zummon is a cross-platform Rust CLI that summons an application to the foreground, or launches it if it is not running. It performs heuristic window matching when the window class/ID differs from the binary name, and can resolve the latest version from a versioned directory tree.

## Core Behavior

- **Summon (default):** If the target app is running, focus its window. Otherwise, launch it.
- **New instance:** `--new-instance` always launches, skipping window lookup.
- **If-focused command:** `--if-focused <CMD>` runs `<CMD>` when the target window is already focused (used for "open new window", app toggles, and toggle chains).
- **Window states:** `--fullscreen`, `--maximized`, `--maximized-to-edges`, `--floating` apply at launch; `--override` applies them to an existing window on focus.
- **TUI:** `--tui` launches the app inside a terminal emulator (`$TERMINAL` or built-in detection).
- **Version resolution:** `--latest[=<PATH>]` resolves the latest version from a versioned directory tree via a 4-phase cascading search; implied when APP is a directory.

## Toggle Mode (`--toggle`)

**Flag:** `--toggle` (long-only; no short form — `-t` is reserved for `--terminal`).

**Semantics:** Flips the target application's running state.

- App is **running** (window or process found) → quit it (graceful window close where the platform supports it; SIGTERM escalation if the process survives).
- App is **not running** → launch it (and focus, per the standard launch path).

**Exit status:** 0 on success, including the no-op case where the app is not running (nothing to quit). 1 on error (unsupported platform, conflicting flags, failed quit/launch).

**Flag conflicts (error out):**
- `--toggle` + `--new-instance` — contradictory (always-launch vs quit-if-running).
- `--toggle` + `--if-focused` — ambiguous (the quit path does not consult focus state).

**Platform support:**

| Platform/Compositor | Quit mechanism | Supported |
|---|---|---|
| Niri | `niri msg action close-window --id <id>` | Yes |
| Hyprland | `hyprctl dispatch closewindow address:<addr>` (legacy) / Lua `hl.dsp.window.close` (≥ 0.55) | Yes (untested) |
| Sway | `swaymsg '[con_id=<id>] kill'` | Yes (untested) |
| KWin | KWin scripting `client.closeWindow()` | Yes (untested) |
| Mutter (GNOME) | none (launch-only) | No — clear error |
| macOS | osascript `quit app "<name>"` | Yes (untested) |
| Windows | `PostMessage(hwnd, WM_CLOSE)` | Yes (untested) |

**Process fallback (all platforms):** if no window is found but the process is running, or the process survives the window close (e.g. tray apps), terminate it: SIGTERM on Unix; forced termination on Windows (the only available mechanism — sysinfo `kill_with` maps to `taskkill /F` there).

**Implementation notes (architecture):**
- New `Adapter` trait method `close_window(window_id)` with a default "unsupported" implementation (Mutter keeps launch-only).
- Orchestration (main.rs) owns the escalation: window close → poll for process exit → SIGTERM fallback.
- `focus.rs` exposes process PID lookup and a `terminate_process` helper (reuses the existing sysinfo process-matching heuristics).

## Debugging & Logging

- `--debug` (stdout), `--log[=<FILE>]` (file, rotated at 10 MB up to 5 files), combined.
- Default log locations: Linux `~/.local/state/zummon/zummon.log`; macOS `~/Library/Logs/zummon/zummon.log`; Windows `%LOCALAPPDATA%\zummon\logs\zummon.log`.
