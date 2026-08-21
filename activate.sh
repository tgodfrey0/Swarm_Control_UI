#!/usr/bin/env bash
# Rust toolchain bootstrap for SwarmDeck development.
#
# Installs the stable toolchain plus std libraries for non-host targets
# (aarch64 / armv7 / x86_64 musl), which conda-forge/distro rust does not
# ship. This script is idempotent and fails open — it just ensures the
# toolchain + targets exist.
set -u

export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"

# Put rustup/cargo on PATH if not already there.
if [ -f "$CARGO_HOME/env" ]; then
  # shellcheck disable=SC1090
  source "$CARGO_HOME/env"
fi

if command -v rustup >/dev/null 2>&1; then
  rustup toolchain install stable --profile minimal >/dev/null 2>&1 || true
  rustup default stable >/dev/null 2>&1 || true
  # Standard libraries for cross targets (used by cargo zigbuild).
  rustup target add \
    aarch64-unknown-linux-musl \
    armv7-unknown-linux-musleabihf \
    x86_64-unknown-linux-musl >/dev/null 2>&1 || true
fi
