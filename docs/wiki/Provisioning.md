# Provisioning

`swarmdeck-cli provision` deploys the agent to real robots over SSH.

## How It Works

For each configured robot (with `address` and `simulated = false`):

1. SCP the agent binary to the robot
2. Write `/etc/swarm-agent/agent.toml`
3. Install and start the `swarmdeck-agent.service` systemd unit

## Prerequisites

- SSH access to target robots (key-based auth recommended)
- Agent binary cross-compiled for the target architecture
- `openssh` (provided by pixi on Linux/macOS)

## Usage

```sh
# Provision all robots in a swarm
SWARMDECK_AGENT_BIN=target/aarch64-unknown-linux-musl/release/swarmdeck-agent \
  swarmdeck-cli provision --swarm configs/lab --user pi

# Specific robots only
SWARMDECK_CONTROLLER_ENDPOINT=100.64.0.1:50051 \
  swarmdeck-cli provision --robots tb-01,tb-02
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `SWARMDECK_AGENT_BIN` | Path to agent binary (default: `target/aarch64-unknown-linux-musl/release/swarmdeck-agent`) |
| `SWARMDECK_CONTROLLER_ENDPOINT` | Override gRPC endpoint for the controller |

## What Gets Installed

### Agent Binary

Installed to `/opt/swarm-agent/swarmdeck-agent`

### Agent Config

Written to `/etc/swarm-agent/agent.toml`:

```toml
robot_id = "tb-01"

[controller]
endpoint = "100.64.0.1:50051"
id_code  = "lab1-swarm-secret"
tls      = false
```

### Systemd Unit

`/etc/systemd/system/swarmdeck-agent.service`:

```ini
[Unit]
Description=SwarmDeck Agent
After=network.target

[Service]
ExecStart=/opt/swarm-agent/swarmdeck-agent --config /etc/swarm-agent/agent.toml
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

## Skipped Robots

Robots are skipped if:
- No `address` configured
- `simulated = true`

## Using with Pixi

```sh
pixi run provision -- --user pi --robots tb-01
```
