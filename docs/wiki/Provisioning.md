# Provisioning

`swarmlink-cli provision` deploys the agent to real robots over SSH.

## How It Works

For each configured robot (with `address` and `simulated = false`):

1. SCP the agent binary to the robot
2. Write `/etc/swarm-agent/agent.toml`
3. Install and start the `swarmlink-agent.service` systemd unit

## Prerequisites

- SSH access to target robots (key-based auth recommended)
- Agent binary cross-compiled for the target architecture
- `openssh` (`ssh`/`scp` on Linux/macOS)

## Usage

```sh
# Provision all robots in a swarm
SWARMLINK_AGENT_BIN=target/aarch64-unknown-linux-musl/release/swarmlink-agent \
  swarmlink-cli provision --config configs/lab/swarm.toml --user pi

# Specific robots only
SWARMLINK_CONTROLLER_ENDPOINT=100.64.0.1:50051 \
  swarmlink-cli provision --robots tb-01,tb-02
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `SWARMLINK_AGENT_BIN` | Path to agent binary (default: `target/aarch64-unknown-linux-musl/release/swarmlink-agent`) |
| `SWARMLINK_CONTROLLER_ENDPOINT` | Override gRPC endpoint for the controller |

## What Gets Installed

### Agent Binary

Installed to `/opt/swarm-agent/swarmlink-agent`

### Agent Config

Written to `/etc/swarm-agent/agent.toml`:

```toml
robot_id = "tb-01"
type     = "turtlebot3"
address  = "10.0.0.21"
simulated = false

[vars]
ns    = "tb01"
model = "burger"

[controller]
endpoint = "100.64.0.1:50051"
id_code  = "lab1-swarm-secret"
tls      = false
```

Type, address and vars are copied from the swarm config's `[[robots]]` entry so
the host can adopt the robot with its identity already populated.

### Systemd Unit

`/etc/systemd/system/swarmlink-agent.service`:

```ini
[Unit]
Description=Swarmlink Agent
After=network.target

[Service]
ExecStart=/opt/swarm-agent/swarmlink-agent --config /etc/swarm-agent/agent.toml
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

## Skipped Robots

Robots are skipped if:
- No `address` configured
- `simulated = true`

## Example

```sh
./bin/swarmlink-cli provision --config configs/lab/swarm.toml --user pi --robots tb-01
```

## Manual Install (`install-agent.sh`)

On a robot without SSH provisioning, `deploy/install-agent.sh` installs the
binary, writes `agent.toml`, and enables the systemd unit. Config fields can be
passed as flags — existing values in the target config are kept and the flags
win:

```sh
sudo ./deploy/install-agent.sh \
  --robot-id uav-01 --name mav-1 --type uav \
  --endpoint 100.64.0.1:50051 --id-code uav_swarm \
  --address cm5-01.tailnet.ts.net \
  --var master=udp:127.0.0.1:14550 --var alt_m=10.0
```

| Flag | Meaning |
|------|---------|
| `--bin <path>` | Agent binary (default: auto-detected from `bin/`/`target/`) |
| `--config <path>` | Config file to write (default: `/etc/swarm-agent/agent.toml`) |
| `--robot-id`, `--name`, `--type`, `--address` | Top-level robot fields |
| `--simulated <bool>` | Mark the agent as running on the host |
| `--var KEY=VALUE` | Add a `[vars]` entry (repeatable) |
| `--env KEY=VALUE` | Add an `[env]` entry (repeatable) |
| `--endpoint`, `--id-code` | Controller connection |
| `--tls <bool>`, `--ca`, `--server-name` | Controller TLS options |
| `--force` | Rewrite the config even when no flags changed |

If the config already exists and no override flags are given, the script
leaves it untouched.
