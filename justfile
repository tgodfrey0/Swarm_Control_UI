# SwarmDeck — build/test tasks. Run `just` (or `just --list`) to see them.
# Binaries are run directly from bin/ (or target/<triple>/release for cross builds).

default:
    @just --list

# --- build --------------------------------------------------------------------

# Build all workspace binaries in release mode and move them into bin/.
build:
    cargo build --release --workspace
    @just _post-build

# Copy release binaries from target/release/ into bin/.
_post-build:
    @mkdir -p bin
    @cp --remove-destination -f target/release/swarmdeck bin/
    @cp --remove-destination -f target/release/swarmdeck-agent bin/
    @cp --remove-destination -f target/release/swarmdeck-cli bin/

# Format all Rust code.
fmt:
    cargo fmt --all

# Lint the whole workspace, deny all warnings.
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Type-check the workspace without building binaries.
check:
    cargo check --workspace

# --- test ---------------------------------------------------------------------

# Run Rust workspace tests only.
test-rust:
    cargo test --workspace

# Run the WebUI contract test (real app.js against docs/api.md fixtures).
test-webui:
    node ui/test/contract.test.js

# Run all tests: Rust workspace tests + WebUI contract test.
test: test-rust test-webui

# --- cross-compile the agent (static musl, no glibc deps) ---------------------

# Cross-compile a static musl agent for aarch64 Linux (RPi / Jetson, 64-bit OS).
cross-compile-arm:
    cargo zigbuild -p swarmdeck-agent --release --target aarch64-unknown-linux-musl
    @mkdir -p bin
    @cp --remove-destination -f target/aarch64-unknown-linux-musl/release/swarmdeck-agent bin/swarmdeck-agent-aarch64

# Cross-compile a static musl agent for armv7 Linux (32-bit Raspberry Pi OS).
cross-compile-armv7:
    cargo zigbuild -p swarmdeck-agent --release --target armv7-unknown-linux-musleabihf
    @mkdir -p bin
    @cp --remove-destination -f target/armv7-unknown-linux-musleabihf/release/swarmdeck-agent bin/swarmdeck-agent-armv7

# Cross-compile a static musl agent for x86_64 Linux (SBCs / laptops).
cross-compile-x86_64:
    cargo zigbuild -p swarmdeck-agent --release --target x86_64-unknown-linux-musl
    @mkdir -p bin
    @cp --remove-destination -f target/x86_64-unknown-linux-musl/release/swarmdeck-agent bin/swarmdeck-agent-x86_64

# Cross-compile for all three targets (aarch64 + armv7 + x86_64).
cross-compile-all: cross-compile-arm cross-compile-armv7 cross-compile-x86_64
