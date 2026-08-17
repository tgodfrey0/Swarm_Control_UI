<p align="center">
  <img src="assets/logo-readme.svg" alt="SwarmDeck" width="320">
</p>

<p align="center">
  <a href="https://jenkins.tgodfrey.com/job/SwarmDeck/"><img src="https://img.shields.io/jenkins/build?jobUrl=https%3A%2F%2Fjenkins.tgodfrey.com%2Fjob%2FSwarmDeck&label=build" alt="Build Status"></a>
  <a href="https://jenkins.tgodfrey.com/job/SwarmDeck/"><img src="https://img.shields.io/jenkins/tests?jobUrl=https%3A%2F%2Fjenkins.tgodfrey.com%2Fjob%2FSwarmDeck&label=tests" alt="Test Status"></a>
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License: MIT">
  <img src="https://img.shields.io/badge/rust-1.85%2B-orange" alt="MSRV 1.85">
</p>

<p align="center">
  <strong>Config-driven swarm robotics control deck</strong><br>
  WebUI + HTTP/WS API + CLI for dispatching actions across robot swarms
</p>

---

A **control host** maintains gRPC sessions with every **robot agent**, executes shell actions across the swarm, and exposes a live WebUI, HTTP/WebSocket API, and CLI.

- **Generic agents** -- just run shell commands, no ROS/MAVSDK dependency
- **Config-driven** -- swarms, robot types, and actions declared in TOML
- **Batch dispatch** -- target by type, robot ID, name pattern, or all
- **Static musl binaries** -- cross-compiled for aarch64/armv7/x86_64, no Docker

## Quick start

```sh
# Install toolchain
pixi install

# Start sim host (WebUI at http://localhost:18082)
pixi run host -- --swarm configs/sim

# Start two simulated robots
pixi run agent-sim -- --config configs/sim/agent-1.toml
pixi run agent-sim -- --config configs/sim/agent-2.toml
```

## Documentation

| Topic | Link |
|-------|------|
| Architecture | [wiki/Architecture](../../wiki/Architecture) |
| Configuration | [wiki/Configuration](../../wiki/Configuration) |
| CLI Reference | [wiki/CLI-Reference](../../wiki/CLI-Reference) |
| HTTP/WS API | [wiki/HTTP-API](../../wiki/HTTP-API) |
| Cross-Compilation | [wiki/Cross-Compilation](../../wiki/Cross-Compilation) |
| Provisioning | [wiki/Provisioning](../../wiki/Provisioning) |
| Security | [wiki/Security](../../wiki/Security) |
| Development | [wiki/Development](../../wiki/Development) |

## Components

| Crate | Description |
|-------|-------------|
| `crates/host` | Control host: gRPC server, dispatch engine, WebUI, HTTP/WS API |
| `crates/agent` | Robot agent: executes commands, streams logs/status |
| `crates/cli` | CLI: batch status/run/stop/ps/logs + SSH provisioning |
| `crates/core` | Shared config schema, template resolution, target selection |
| `proto` | gRPC definitions (`swarm.proto`) |

## Contributing

1. Fork the repo
2. Create a feature branch
3. Run `pixi run lint` and `pixi run test`
4. Submit a pull request

See [Development](../../wiki/Development) for detailed setup instructions.

## License

MIT -- see [LICENSE](LICENSE) for details.
