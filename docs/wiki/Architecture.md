# Architecture

## Overview

Swarmlink follows a hub-and-spoke architecture: a single **control host** manages many **robot agents** over gRPC.

```
                    +----------------------------------------------+
                    |              control host (host crate)        |
                    |                                              |
  WebUI (browser) --+ HTTP+WS   /api/*, /api/ws                    |
  swarmlink-cli ----+          |                                   |
                    |          +-- dispatch engine --+              |
                    |          |                     | sends gRPC   |
                    |   gRPC server (tonic) <--------+ Commands     |
                    +----------+---------------------+              |
                     Session stream (Register/Status/Log/Result)    |
                  +----------+-----------+                          |
                  v                       v                          |
          robot agent (RPis)      simulated agents (this host)     |
   /opt/swarm-agent/swarmlink-agent   same binary, local agent.toml|
   runs shell actions, streams logs   simulated = true in swarm    |
```

## Components

### Control Host (`crates/host`)

The binary `swarmlink` runs:
- **gRPC server** (tonic): bidirectional `Session` stream with each agent
- **Dispatch engine**: `RunStore` + `Dispatcher` for run/stop/adopt/release
- **Registry**: central robot state, log ring, staleness sweeper
- **Event bus**: tokio broadcast channel for live WebSocket updates
- **HTTP server** (Axum): serves WebUI, JSON API, WebSocket upgrades

### Robot Agent (`crates/agent`)

The binary `swarmlink-agent` runs on each robot:
- **Session**: gRPC client, register, heartbeat, reconnect with exponential backoff
- **Runner**: process group management (spawn, timeout, kill)
- **Procfs**: lightweight `/proc` metrics (CPU, memory, uptime, battery)

### CLI (`crates/cli`)

The binary `swarmlink-cli` provides:
- `status`, `run`, `stop`, `ps`, `logs`, `config`, `provision`
- SSH provisioning: scp agent + config, install systemd unit

### Shared Core (`crates/core`)

- **Config schema**: `SwarmConfig`, `RobotConfig`, `ActionConfig`
- **Template engine**: `{{placeholder}}` resolution per robot
- **Target selection**: `select_robots`, `resolve action refs`
- **API types**: `RobotView`, `RunRequest`, `Event`, `LogLine`

### Proto (`proto/`)

gRPC definitions in `swarm.proto`:
- Service `Agent` with bidirectional `Session` stream
- Messages: `Register`, `Status`, `Log`, `Result`, `Command`

## Data Flow

1. Host loads `swarm.toml` + `robots/*.toml`
2. Agents connect via gRPC, send `Register` with robot ID
3. Host sends `Command` messages (action + args)
4. Agents run shell commands, stream `Log` and `Status` updates
5. Host pushes updates to WebUI via WebSocket

## Design Principles

- **Agent is generic**: just runs shell commands, no ROS/MAVSDK dependency
- **Config is TOML**: all swarms, types, and actions declared in files
- **All UIs are thin**: WebUI, CLI, future TUI all use the same HTTP/WS API
- **Static binaries**: musl cross-compilation, no Docker, no glibc
- **Config reload**: SIGHUP reloads `swarm.toml` without restart
