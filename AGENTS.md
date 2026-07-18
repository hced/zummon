# AGENTS.md

Rust CLI (binary `zummon`) for focus-or-launch of apps across Linux/macOS/Windows.
Rust Edition 2024. Only Niri (Linux/Wayland) is verified; all other platform adapters are untested.

## Commands (via `just`, not bare cargo)

Use `just --list` for the full set.

- `just build` / `just run <args>` / `just tests` — defaults to debug for development.
- `just build-release` / `just run-release <args>` — verify final artifacts/performance.
- `just fmt` / `just fmt-check` — formatting (CI uses `fmt --check`)
- `just lint` — `cargo clippy -- -D warnings` (warnings are errors; don't add code that trips clippy)
- `just check` — fast compile check
- `just bump-patch|minor|major` / `just release` — version bumps & git tag/release flow (uses `cargo-bump`)

Run a single test: `cargo test <test_name>`.

## Workflow
Debug builds are the default for development. Once confirmed that there are no errors, always build the release variant and afterwards commit the changes using git (not jj).

## Architecture

- `src/main.rs` — entrypoint + orchestration; restores stripped session env vars (DBUS/XDG) at startup, then dispatches by detected `Platform`/`LinuxWindowSystem`.
- `src/adapters/` — one module per platform/compositor (niri, hyprland, sway, kwin, mutter, macos, windows) implementing the `Adapter` trait (`src/traits.rs`). Add/extend platform behavior here, not in `main.rs`.
- `src/launch.rs` — launching, `--if-focused` commands, arg delivery to running instances.
- `src/find_latest.rs` — 4-phase cascading search for latest version in a versioned directory tree (wax globs → Jaro-Winkler → multi-tier fuzzy → exhaustive fallback).
- `src/focus.rs` / `src/cli.rs` — process/window matching heuristics and clap CLI definition.

Window heuristics run only for Hyprland/Sway/Niri/macOS/Windows when no exact window match is found.

## Notes

- No CI config or other instruction files exist in this repo. Follow the `just` recipes above as the source of truth for build/test/lint order.
- VCS is git (`origin/main`). A `.jj` repo also exists alongside it; treat git as authoritative.
- Debug/logging: `--debug` (stdout), `--log` (file), combined. Default log at `~/.local/state/zummon/zummon.log` (Linux).
