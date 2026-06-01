# Show available recipes
_default:
    @just --list

# Combine project source files into clipboard (excludes target, assets, lockfiles)
clump:
    clump . --exclude-dir target,assets --exclude *.lock

# Build optimized release binary
build:
    cargo build --release

# Build debug binary (faster, with symbols)
build-debug:
    cargo build

# Run the app with arguments
run *args:
    cargo run -- {{ args }}

# Run with debug logging enabled
debug *args:
    RUST_LOG=debug cargo run -- {{ args }}

# Run all tests
test:
    cargo test

# Run tests with full output
test-verbose:
    cargo test -- --nocapture

# Run a specific test
test-filter filter:
    cargo test {{ filter }} -- --nocapture

# Check code formatting (no changes)
fmt-check:
    cargo fmt --all -- --check

# Auto-format all code
fmt:
    cargo fmt --all

# Run clippy with strict warnings
lint:
    cargo clippy -- -D warnings

# Auto-fix clippy suggestions
lint-fix:
    cargo clippy --fix --allow-dirty --allow-staged

# Run all checks: format, lint, test
check: fmt-check lint test
    @echo "✅ All checks passed!"

# Remove build artifacts
clean:
    cargo clean

# Install debug build to ~/.cargo/bin
install:
    cargo install --path . --force

# Install optimized release build to ~/.cargo/bin
install-release:
    cargo install --path . --force --profile release

# Remove installed binary
uninstall:
    cargo uninstall zummon

# Auto-rebuild on file changes (requires cargo-watch)
watch:
    cargo watch -x build

# Auto-test on file changes (requires cargo-watch)
watch-test:
    cargo watch -x test

# Update dependency versions
update:
    cargo update

# Check for outdated dependencies (requires cargo-outdated)
outdated:
    cargo outdated || echo "Install cargo-outdated: cargo install cargo-outdated"

# Run app and save debug output to log file
debug-log *args:
    RUST_LOG=debug cargo run -- {{ args }} 2>&1 | tee zummon-debug.log
    @echo "Debug log saved to: zummon-debug.log"

# Benchmark release build time
bench-build:
    @time cargo build --release

# Profile with flamegraph (requires cargo-flamegraph)
profile *args:
    cargo flamegraph -- {{ args }}

# Audit dependencies for vulnerabilities (requires cargo-audit)
audit:
    cargo audit || echo "Install cargo-audit: cargo install cargo-audit"

# Open API documentation in browser
docs:
    cargo doc --open

# Show app version
version:
    @cargo run -- --version 2>/dev/null || echo "Build first: just build"

# Show app help
help:
    @cargo run -- --help 2>/dev/null || echo "Build first: just build"

# Full development cycle: format, lint, test, build
dev: fmt-check lint test build
    @echo "✅ Ready to commit!"
