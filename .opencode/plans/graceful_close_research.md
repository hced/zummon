# Graceful Window Close — Platform Research (for `--toggle`)

Author: Donnie (Technical Documentation Researcher). All claims verified against primary sources (no remaining [UNVERIFIED] items).

## Per-platform graceful close mechanism

### Linux / Wayland

**Niri** — `niri msg action close-window --id <id>`
- `Action::CloseWindow { id: Option<u64> }` — "Id of the window to close. If None, uses the focused window."
- Source: docs.rs `niri_ipc` 26.4.0 (published 2026-07-10), enum `Action`: https://docs.rs/niri_ipc/latest/niri_ipc/enum.Action.html
- zummon already has `id: u64` per NiriWindow. IPC is the compositor asking the client to close (Wayland close request) — graceful.

**Hyprland** — two syntaxes, depending on version (verified against both wiki generations):
- **Hyprland ≥ 0.55 (Lua dispatchers, current)**: `hyprctl dispatch 'hl.dsp.window.close({ window = "address:0x…" })'`
  - Current wiki, Dispatchers → Window: `hl.dsp.window.close({ window? })` — "Send a graceful request to close the window." (https://wiki.hypr.land/Configuring/Basics/Dispatchers/). `hl.dsp.window.kill({ window? })` is the SIGKILL variant ("Kill the process owning the window with a SIGKILL") and `hl.dsp.window.signal({ signal, window? })` sends an arbitrary POSIX signal — not graceful.
  - `hyprctl dispatch` is now "a shorthand for `eval 'hl.dispatch(...)'`" and takes the Lua expression as its argument (Using-hyprctl.md: `hyprctl dispatch 'hl.dsp.focus({ workspace = "3" })'`). Verified from the wiki source repo (hyprwm/hyprland-wiki, main branch).
  - Window selector: `address:0x…` (exact hex address — what zummon's HyprlandWindow.address holds), `pid:…`, `stableid:…`, `class:…`, `title:…`, `activewindow`, etc. "If no window is provided, the active window is used."
- **Hyprland ≤ 0.54 (hyprlang era, still common)**: `hyprctl dispatch closewindow address:<addr>`
  - Legacy wiki (https://wiki.hypr.land/0.54.0/Configuring/Dispatchers/): `closewindow` — "closes a specified window", param `window` (address/pid/class:/title:…). The dispatcher name + `address:<addr>` string form as previously found in discussion https://github.com/hyprwm/Hyprland/discussions/2036.
- `killactive` (legacy): "closes (not kills) the active window", param `none` — **focused-window-only, no address parameter** (confirmed on 0.54 wiki). Not usable for targeting a specific window by address; that is exactly why `closewindow`/`hl.dsp.window.close` is required.
- Caveat: the current Lua dispatcher API is marked "not guaranteed to be stable at all"; implementation should detect Hyprland version (or just use the Lua form, as it's the only documented one since 0.55). Old URL https://wiki.hyprland.org/Configuring/Dispatchers/ 404s; use the `.land` domain.

**Sway** — `swaymsg '[con_id=<id>] kill'`
- Verified against sway(5) man page (https://man.archlinux.org/man/sway.5.en.html, package 1:1.12-4, 2026-06-23):
  - `kill` — "Kills (closes) the currently focused container and all of its children." Targets the focused container by default; criteria select a specific container.
  - CRITERIA section: `con_id` — "Compare against the internal container ID, which you can find via IPC. If value is `__focused__`…". Valid criterion. The man page itself demonstrates criteria+kill: `[title="Emacs"] kill`.
  - The IPC `get_tree` JSON nodes' `id` field IS the con_id — matches the `id: u64` the zummon sway adapter already stores. `pid` is also a valid criterion as an alternative.
- Caveat: `kill` "closes" — sway sends the client a close request (xdg_shell close / X11 WM_DELETE), not a signal; a container with children kills the whole subtree. sway IPC protocol docs at https://swaywm.org/ipc/ are 404; the man page is the authoritative reference.

**KWin** — QDBus script: `var c = workspace.clientList().find(w => w.caption.includes("…")); if (c) c.closeWindow();`
- KWin 6 scripting API page (https://develop.kde.org/docs/plasma/kwin/api/) shows `KWin::Window` with `closeWindow()=0` in its Functions, alongside `slotWindowClose()`. The zummon adapter already uses `workspace.clientList()` via `loadScript`, so this is a drop-in addition.
- Caveat: exact binding name confirmed (`closeWindow()`); the docs list it as a pure-virtual on the Window class, which is what script clients call. Verified.

**Mutter (GNOME)** — no public graceful mechanism.
- `Meta.Window` API does expose `can_close` and `delete` (https://mutter.gnome.org/meta/class.Window.html, API v51), but only within GNOME Shell extensions; the author of https://discourse.gnome.org/t/graceful-meta-window-close/28474 reports `delete()` is forceful in practice. Keep the adapter launch-only; `--toggle` close on mutter should error with a clear "unsupported" message.

### macOS

`osascript -e 'quit app "Slack"'` (app name; or `quit id "com.app"`)
- Top-voted source: https://apple.stackexchange.com/questions/354954 — AppleScript `quit` triggers save/cleanup tasks; `pkill` (SIGTERM) "could be that the application will not shut down cleanly".
- The zummon macOS adapter already shells out to osascript with System Events and holds both window id and pid. AppleScript quit is the graceful path; SIGTERM via `kill <pid>` is an acceptable fallback (unlike pkill-by-name, direct SIGTERM is standard macOS practice).

### Windows

Graceful: `PostMessage(hwnd, WM_CLOSE, 0, 0)` — WM_CLOSE prompts the app to run its close/cleanup path.
- The zummon Windows adapter already enumerates hwnds via PowerShell P/Invoke (`EnumWindows`, `GetWindowText`), so WM_CLOSE is a small addition with the same machinery. Note: must be the **window's own thread** message pump target — `PostMessage` (async, safe) not `SendMessage` (risk of deadlock from PowerShell's STA host).
- Sources: https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-close; community confirmation of semantics: https://stackoverflow.com/questions/32346219
- `taskkill` **without** `/f` is NOT graceful — it operates at TerminateProcess level (same SO answer). MS docs: `/f` = "forcefully ended": https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/taskkill

## Process-kill fallback (escalation path)

Verified directly against sysinfo source (clone of `GuillaumeGomez/sysinfo` master):

- `Process::kill()` = `kill_with(Signal::Kill)` (`src/common/system.rs:1579-1581`) → on Unix this is `libc::kill(pid, SIGKILL)` — **always last resort**.
- Linux: `kill_with(Signal::Term)` → `libc::kill(pid, SIGTERM)` (`src/unix/linux/process.rs:174-177`; `Signal::Term => libc::SIGTERM` at `src/unix/linux/system.rs:92`). SIGTERM is the correct escalation signal on Linux. **macOS**: same `libc::kill` (`src/unix/apple/macos/process.rs:123-126`).
- **Windows: sysinfo has no graceful kill at all.** `declare_signals` supports only `Signal::Kill` (`src/windows/system.rs`), and `kill_with` unconditionally runs `taskkill.exe /PID <pid> /F` (`src/windows/process.rs:369-378`) — a forced TerminateProcess. `kill_with(Signal::Term)` returns `None` (unsupported signal). On Windows the only graceful route is WM_CLOSE; the sysinfo fallback is inherently forceful.

Recommended escalation: window-close request → poll (e.g. 5 × 250 ms) for window/process disappearance → if still present, `kill_with(Signal::Term)` (Unix) / `taskkill /f` (Windows) → report result. Never use plain `kill()` (SIGKILL) except as documented user-visible last resort.

## Adapter trait design recommendation

- Add `async fn close_window(&self, window: &WindowRef) -> Result<()>` to the existing `Adapter` trait with a **default implementation that returns an "unsupported on this platform" error** (Mutter). Keeping one trait (rather than a separate `WindowCloser` trait) matches the existing single-trait architecture in `src/traits.rs` and keeps `main.rs` dispatch trivial.
- Orchestration (main.rs or launch.rs) owns the escalation: try adapter close → poll existence → SIGTERM via sysinfo (`refresh_process(pid)` + `kill_with(Signal::Term)`) → error if still alive.
- To enable the fallback, `focus.rs` should expose the matched process's PID (currently `is_process_running` returns bool only); the pid is available from `Process::pid()` on the already-iterated processes.
- Per-adapter notes: niri/sway/kwin/hyprland adapters need only new IPC/script strings; the Windows adapter needs the `PostMessage`/`WM_CLOSE` P/Invoke; macOS needs the `quit` osascript; mutter errors.

## Source list

- https://docs.rs/niri_ipc/latest/niri_ipc/enum.Action.html (CloseWindow variant, verified 2026)
- https://wiki.hypr.land/Configuring/Basics/Dispatchers/ (current, Lua: `hl.dsp.window.close({ window = "address:0x…" })`, verified via hyprwm/hyprland-wiki source)
- https://wiki.hypr.land/0.54.0/Configuring/Dispatchers/ (legacy: `closewindow address:…`, `killactive` = focused-only, verified)
- https://wiki.hypr.land/Configuring/Advanced-and-Cool/Using-hyprctl/ (dispatch = Lua eval shorthand, verified)
- https://man.archlinux.org/man/sway.5.en.html (sway `kill` + `con_id` criterion + `[title="Emacs"] kill` example, verified; sway 1.12-4)
- https://develop.kde.org/docs/plasma/kwin/api/ (KWin::Window::closeWindow, verified)
- https://mutter.gnome.org/meta/class.Window.html (Meta.Window delete/can_close, verified; no external mechanism)
- https://apple.stackexchange.com/questions/354954 (osascript quit vs pkill, verified)
- https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-close (WM_CLOSE semantics)
- https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/taskkill (/f = forceful; verified 2026-02-16 rev)
- https://stackoverflow.com/questions/32346219 (taskkill without /f is still TerminateProcess-level, verified)
- sysinfo master source clone (kill/kill_with/convert_signal paths above, verified)
