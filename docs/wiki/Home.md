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

```sh
# Install toolchain
pixi install

# Start sim host (WebUI at http://localhost:18082)
pixi run host -- --swarm configs/sim

# Start two simulated robots
pixi run agent-sim -- --config configs/sim/agent-1.toml
pixi run agent-sim -- --config configs/sim/agent-2.toml
```

## Wiki Pages

- [[Architecture]] -- System overview and design
- [[Configuration]] -- TOML config reference
- [[CLI-Reference]] -- All CLI subcommands
- [[HTTP-API]] -- HTTP and WebSocket API docs
- [[Cross-Compilation]] -- Building static musl agents
- [[Provisioning]] -- SSH deployment to robots
- [[Security]] -- TLS, auth, and security notes
- [[Development]] -- Dev setup, testing, contributing
