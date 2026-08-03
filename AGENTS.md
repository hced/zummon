# AGENTS.md

Rust CLI (binary `zummon`) for focus-or-launch of apps across Linux/macOS/Windows.
Rust Edition 2024. Only Niri (Linux/Wayland) is verified; all other platform adapters are untested.

## Commands (via `just`, not bare cargo)

Use `just --list` for the full set. The justfile is the source of truth.

- `just build` / `just run <args>` / `just tests` — **release by default**; debug variants are `build-debug` / `run-debug` / `tests-debug`.
- `just check` — fast compile check; use for quick verification.
- `just fmt` / `just fmt-check` — formatting.
- `just lint` — `cargo clippy --release -- -D warnings`. **Currently fails with ~20 pre-existing errors** in main.rs, launch.rs, find_latest.rs, and the adapters; don't fix them wholesale, but don't add new ones.
- `just bump-patch|minor|major` / `just release` — version bump & git tag/release flow (uses `cargo-bump`).

There are **no tests** in the crate; `just tests` only compiles an empty suite.

## CI & Release

- `.github/workflows/cross-platform_ci_and_cd.yml` triggers **only on `v*` tag pushes** (never on main pushes or PRs): builds + `cargo test --release` on ubuntu/windows/macos, then publishes a GitHub Release with per-OS tarballs/zip and `checksums.txt`.
- Release flow: `just release` → interactive version bump → commit → tag `vX.Y.Z` → push → CI builds & publishes.

## Workflow

Once the code compiles, build the release variant, then commit with git using a conventional commit prefix (`chore:` / `fix:` / `feat:` / `docs:` / `ci:`). Use `just` recipes instead of bare `cargo` commands.

## Architecture

- `src/main.rs` — entrypoint + orchestration; restores stripped session env vars (DBUS_SESSION_BUS_ADDRESS, XDG_RUNTIME_DIR, XDG_DATA_DIRS, XDG_CONFIG_DIRS) at startup, then dispatches by detected `Platform`/`LinuxWindowSystem`.
- `src/adapters/` — one module per platform/compositor (niri, hyprland, sway, kwin, mutter, macos, windows) implementing the `Adapter` trait (`src/traits.rs`). Add/extend platform behavior here, not in `main.rs`.
- `src/launch.rs` — launching, `--if-focused` commands, arg delivery to running instances, `$TERMINAL` detection for `--tui` (see `.opencode/plans/terminal_env_*.md` for known pitfalls).
- `src/find_latest.rs` — 4-phase cascading search for latest version in a versioned directory tree (wax globs → Jaro-Winkler → multi-tier fuzzy → exhaustive fallback).
- `src/focus.rs` / `src/cli.rs` — process/window matching heuristics and clap CLI definition.

Window heuristics run only for Hyprland/Sway/Niri/macOS/Windows when no exact window match is found (Mutter is launch-only).

## Notes

- VCS is git only (`origin/main`). No `.jj` repo.
- `.bkp/` holds stale source backups — gitignored; never edit or rely on it.
- `.opencode/agents/` defines the project team (James builds & commits; Sarah maintains SPECS.md/README.md/AGENTS.md); `.opencode/plans/` holds design docs.
- Debug/logging: `--debug` (stdout), `--log` (file), combined. Default log at `~/.local/state/zummon/zummon.log` (Linux).
