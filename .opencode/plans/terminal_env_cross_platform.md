# Plan: Honor `$TERMINAL` (with flags) consistently across platforms

## Context
The user asked whether we should support users who set an executable-with-flags as
their `TERMINAL` env var, and chose a **cross-platform review**. Investigation of
`src/launch.rs` shows:

- **Linux** already supports this fully: `detect_terminal` reads `$TERMINAL`
  (launch.rs:22) and `build_tui_command` splits it via `split_whitespace()` and
  injects `terminal_args` before its own `--class`/`--`/`app` args (launch.rs:123-150).
  So `TERMINAL="kitty --single-instance"` -> `kitty --single-instance --class
  "kitty.yazi.zummoned" -- yazi`. Only ghostty's `+new-window` is deliberately
  stripped (launch.rs:119-120).
- **macOS** `detect_terminal` (launch.rs:52-68) does **NOT** read `$TERMINAL` at all
  - it only probes `/Applications/*.app`. A macOS user's `TERMINAL` is silently ignored.
- **Windows** `detect_terminal` (launch.rs:70-81) also ignores `$TERMINAL`
  (non-standard there, but inconsistent).

The Unix/Unix-like `build_tui_command` path is already flag-capable, so the only real
gap is **detection not reading `$TERMINAL` on macOS/Windows**.

## Changes

### A. macOS `detect_terminal` (launch.rs, after line 56)
Add `$TERMINAL` read before the priority list, mirroring Linux:
```rust
if let Ok(term) = std::env::var("TERMINAL") {
    return Ok(term);
}
```
This routes `TERMINAL="kitty --single-instance"` (or any flags) into
`build_tui_command`, which already handles flags via the shared Unix path. Note: the
macOS-only osascript branch (launch.rs:106) only triggers for `iTerm`/`Terminal`;
kitty/alacritty/wezterm/`$TERMINAL` fall through to the flag-aware Unix path, which
is the correct behavior.

### B. Windows `detect_terminal` (launch.rs, after line 74) — optional / low-value
Add `$TERMINAL` read for strict parity:
```rust
if let Ok(term) = std::env::var("TERMINAL") {
    return Ok(term);
}
```
Caveat: `$TERMINAL` is a Unix convention; on Windows it is rarely set. The Windows
`build_tui_command` branch (launch.rs:92) is hardcoded to `wt`/`powershell` with no
flag injection, so a flags-bearing `$TERMINAL` on Windows would NOT be parsed the same
way. Recommendation: include the read for consistency, but document that Windows flag
handling is limited. Alternative: skip Windows entirely (see Open decision).

### C. No change to `build_tui_command`
Flag injection already works on all Unix-like paths. Keep the ghostty `+new-window`
strip as-is (Linux/Unix only, intentional).

## Verification
- `just check` and `just build`.
- Note: `just lint` currently fails repo-wide due to ~20 **pre-existing** clippy errors
  in untouched files (find_latest.rs, main.rs, niri.rs, etc.). These are out of scope;
  confirm the new edits introduce **no new** lint errors.
- Cross-platform runtime testing is not possible in this Linux/Niri environment;
  macOS/Windows paths are verified by code review plus the shared flag-handling path
  already exercised on Linux.

## Open decision
Should the Windows `$TERMINAL` read (step B) be included, given `$TERMINAL` is
non-standard on Windows and Windows flag parsing isn't implemented?
- Default if no objection: **Include A (macOS) + B (Windows read for parity)**.
- Alternative: **macOS only**, leaving Windows untouched.

Separate task (not part of this plan): the repo-wide `just lint` failures. Fix on
request.
