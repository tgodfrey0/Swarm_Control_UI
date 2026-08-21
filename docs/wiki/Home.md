# SwarmDeck

**Config-driven swarm robotics control deck: WebUI + HTTP/WS API + CLI**

SwarmDeck is a control host that maintains gRPC sessions with every robot agent, executes shell actions across the swarm, and exposes a live WebUI, HTTP/WebSocket API, and CLI.

## Key Features

- **Generic agents** -- just run shell commands, no ROS/MAVSDK dependency
- **Config-driven** -- swarms, robot types, and actions declared in TOML
- **Batch dispatch** -- target by type, robot ID, name pattern, or all
- **Static musl binaries** -- cross-compiled for aarch64/armv7/x86_64, no Docker
- **Live updates** -- WebSocket pushes robot state and logs in real time

## Getting Started

Prerequisites: [just](https://github.com/casey/just), Rust 1.85+ (via [rustup](https://rustup.rs)), `protoc`, and `cargo-zigbuild` for cross-compiling.

```sh
# Build the binaries (into bin/)
just build

# Start sim host (WebUI at http://localhost:18082)
./bin/swarmdeck --swarm configs/sim

# Start two simulated robots
./bin/swarmdeck-agent --config configs/sim/agent-1.toml
./bin/swarmdeck-agent --config configs/sim/agent-2.toml
```

## Wiki Pages

- [[Architecture]] -- System overview and design
- [[Configuration]] -- TOML config reference
- [[CLI-Reference]] -- All CLI subcommands
- [[HTTP-API]] -- HTTP and WebSocket API docs
- [[Agent-API]] -- gRPC protocol between agents and the controller
- [[Cross-Compilation]] -- Building static musl agents
- [[Provisioning]] -- SSH deployment to robots
- [[Security]] -- TLS, auth, and security notes
- [[Development]] -- Dev setup, testing, contributing
