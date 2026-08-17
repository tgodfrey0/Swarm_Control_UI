# Cross-Compilation

SwarmDeck cross-compiles static musl agent binaries for ARM and x86_64 Linux robots using `cargo-zigbuild`. No Docker required.

## Prerequisites

The pixi environment provides all tools:

```sh
pixi install
```

This installs: `rustup` (via `activate.sh`), `zig`, `cargo-zigbuild`, musl targets.

## Build Commands

```sh
# Primary target: 64-bit Raspberry Pi / Jetson
pixi run agent-aarch64
# Output: target/aarch64-unknown-linux-musl/release/swarmdeck-agent

# 32-bit Raspberry Pi OS
pixi run agent-armv7
# Output: target/armv7-unknown-linux-musleabihf/release/swarmdeck-agent

# x86_64 SBCs / laptops
pixi run agent-x86_64
# Output: target/x86_64-unknown-linux-musl/release/swarmdeck-agent

# All three at once
pixi run agent-all
```

## Binary Details

- **Fully static**: no glibc dependency
- **Size**: ~2.0 MB (aarch64)
- **Targets**:
  - `aarch64-unknown-linux-musl` -- 64-bit ARM (RPi 4, Jetson)
  - `armv7-unknown-linux-musleabihf` -- 32-bit ARM (RPi 2/3/Zero 2)
  - `x86_64-unknown-linux-musl` -- x86_64 (Intel NUC, Intel-based SBCs)

## Why Zig?

`cargo-zigbuild` uses Zig as a cross-linker, which provides:
- Static musl linking without installing musl-gcc
- Seamless target support (no separate toolchain per arch)
- Fast builds via Zig's compilation pipeline

## Troubleshooting

### Missing musl targets

```sh
rustup target add aarch64-unknown-linux-musl armv7-unknown-linux-musleabihf x86_64-unknown-linux-musl
```

### Protobuf compilation errors

Ensure `protoc` is available (pixi installs it):

```sh
pixi run protoc --version
```
