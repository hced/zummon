# Plan: Niri keybind `--tui` launches nothing — stale `$TERMINAL` in Niri's env

## Diagnosis (root cause)
The user reports:
- `zummon --tui --maximized-to-edges yazi` in a terminal → **opens maximized correctly**.
- Niri keybind `spawn "zummon" "--tui" "--maximized-to-edges" "yazi"` → **nothing opens**.
- Dolphin keybind (`--maximized-to-edges dolphin`, no `--tui`) → works.
- Only `kitty` + `konsole` are installed; `ghostty` is NOT installed.

History: the user's `config.fish` previously set `TERMINAL="ghostty +new-window"` and was
later changed to `TERMINAL="kitty"`. Niri is a long-running process started at session
login, so it captured `TERMINAL='ghostty +new-window'` at login — **before** the edit.
New interactive shells pick up `TERMINAL=kitty`; Niri's environment still has the stale
ghostty value.

Flow when the keybind fires:
1. `detect_terminal` reads `$TERMINAL` and returns `'ghostty +new-window'` verbatim
   (launch.rs:22-24) — no existence check.
2. `build_tui_command` strips `+new-window`, builds `ghostty --class "..." -e yazi`
   (launch.rs:159-164).
3. `spawn_command_string` runs `sh -c "ghostty ..."` (niri.rs) → ghostty not installed
   → spawn fails. stderr is discarded by Niri → **nothing opens, silent failure**.

Why the terminal command works: that shell has `TERMINAL=kitty` → kitty is found → works.

So this is an **environment propagation** problem (Niri's stale env), NOT a window-state
race (terminal maximize already works, so the earlier 3s-timeout theory does NOT apply
to this user's case — out of scope here).

## Part 1 — Immediate user action (no code change needed to unblock)
1. **Restart Niri** (or log out / reboot) so its environment picks up `TERMINAL=kitty`.
   Less drastic than reboot: from a terminal, `niri msg action restart` or reload the
   compositor per your setup.
2. **Verify Niri's current env** (read-only, safe) before/after:
   - `grep TERMINAL /proc/$(pidof niri)/environ | tr '\0' '\n'`
   - Expect to see the stale `ghostty +new-window` now; after restart it should be `kitty`.
3. **Temporarily add `--debug` to the keybind** (or run the exact spawn via Niri) to
   confirm the resolved TUI command once it works.

After restart, `TERMINAL=kitty` → keybind resolves kitty → yazi opens maximized, matching
the terminal behavior.

## Part 2 — Code hardening (prevents this class of silent failure)
`detect_terminal` should not blindly trust `$TERMINAL` when the named binary does not
exist. If the `$TERMINAL` value (first whitespace-separated token) cannot be resolved via
`which::which`, **fall through** to the priority list + konsole fallback instead of
returning it and failing later in `sh -c`.

### Change A — Linux `detect_terminal` (launch.rs, ~lines 22-24)
After reading `$TERMINAL`, validate the program token exists:
```rust
if let Ok(term) = std::env::var("TERMINAL") {
    let prog = term.split_whitespace().next().unwrap_or(&term);
    if which::which(prog).is_ok() {
        return Ok(term);
    }
    // Otherwise fall through to the priority list / konsole fallback.
}
```
This keeps valid flag-bearing values (e.g. `kitty --single-instance`, which `which` can
resolve on the first token) working, while discarding a missing binary like `ghostty`.

### Change B — consider the same guard for macOS/Windows `$TERMINAL` reads
Apply the identical `which`-existence guard to the macOS (launch.rs, after the priority
list insertion) and Windows (launch.rs) `$TERMINAL` reads added in the previous task,
for consistency. Low risk; matches the "harden per-platform" intent. (On Windows `which`
of a store `wt` path may behave differently — if uncertain, guard only when the value
is non-empty and let the existing `wt`/`powershell` logic remain the primary path.)

### Non-goals
- Do NOT change `build_tui_command` flag handling (already correct).
- Do NOT touch the window-state race (Problem B) — unproven for this user; terminal
  `--tui --maximized-to-edges` already works for them.

## Verification
- `just check` and `just build`; confirm no NEW clippy errors beyond the pre-existing 20
  in untouched files (adapters, find_latest.rs, main.rs).
- Linux runtime (this env): `TERMINAL="" zummon --tui yazi` and
  `TERMINAL="ghostty +new-window" zummon --tui yazi` should both now fall back to kitty
  (installed) instead of silently failing. `TERMINAL="kitty --single-instance" zummon
  --tui yazi` still uses kitty with the flag.
- User-side: after Niri restart, the Mod+A keybind opens yazi maximized; confirm via the
  `/proc/$(pidof niri)/environ` grep that Niri's `TERMINAL=kitty`.
