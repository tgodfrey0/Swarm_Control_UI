# Cross-Compilation

Swarmlink cross-compiles static musl agent binaries for ARM and x86_64 Linux robots using `cargo-zigbuild`. No Docker required.

## Prerequisites

- Rust via [rustup](https://rustup.rs) (run `activate.sh` to install the toolchain + musl targets)
- [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) and `zig`

## Build Commands

```sh
# Primary target: 64-bit Raspberry Pi / Jetson
just compile-arm
# Output: bin/swarmlink-agent-aarch64

# 32-bit Raspberry Pi OS
just compile-armv7
# Output: bin/swarmlink-agent-armv7

# x86_64 SBCs / laptops
just compile-x86_64
# Output: bin/swarmlink-agent-x86_64

# All three at once
just compile-all
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

Ensure `protoc` is installed and on your `PATH`:

```sh
protoc --version
```
