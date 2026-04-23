_default:
    @just --list

build:
    cargo build --release

build-debug:
    cargo build

run *args:
    cargo run -- {{args}}

debug *args:
    RUST_LOG=debug cargo run -- {{args}}

test:
    cargo test

test-verbose:
    cargo test -- --nocapture

test-filter filter:
    cargo test {{filter}} -- --nocapture

fmt-check:
    cargo fmt --all -- --check

fmt:
    cargo fmt --all

lint:
    cargo clippy -- -D warnings

lint-fix:
    cargo clippy --fix --allow-dirty --allow-staged

check: fmt-check lint test
    @echo "✅ All checks passed!"

clean:
    cargo clean

install:
    cargo install --path . --force

install-release:
    cargo install --path . --force --profile release

uninstall:
    cargo uninstall zummon

watch:
    cargo watch -x build

watch-test:
    cargo watch -x test

update:
    cargo update

outdated:
    cargo outdated || echo "Install cargo-outdated: cargo install cargo-outdated"

debug-log *args:
    RUST_LOG=debug cargo run -- {{args}} 2>&1 | tee zummon-debug.log
    @echo "Debug log saved to: zummon-debug.log"

bench-build:
    @time cargo build --release

profile *args:
    cargo flamegraph -- {{args}}

audit:
    cargo audit || echo "Install cargo-audit: cargo install cargo-audit"

docs:
    cargo doc --open

version:
    @cargo run -- --version 2>/dev/null || echo "Build first: just build"

help:
    @cargo run -- --help 2>/dev/null || echo "Build first: just build"

dev: fmt-check lint test build
    @echo "✅ Ready to commit!"
