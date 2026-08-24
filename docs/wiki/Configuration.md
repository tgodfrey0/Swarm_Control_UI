# Configuration

SwarmDeck uses TOML configuration files organised in directories.

## Directory Structure

```
configs/
  lab/                      # one directory per swarm
    swarm.toml              # controller + [[robots]]
  sim/                      # fake swarm for local testing
    swarm.toml
    agent-1.toml
    agent-2.toml

robots/                     # shared robot kinds + actions
  turtlebot3.toml
  uav.toml
  sim.toml
```

## `swarm.toml`

```toml
[controller]
name        = "lab-1"
id_code     = "lab1-swarm-secret"   # shared secret agents present on connect
grpc_listen = "0.0.0.0:50051"       # where agents phone home
ui_bind     = "0.0.0.0:8080"        # WebUI + HTTP/WS API
# tls = { cert = "certs/host.crt", key = "certs/host.key", ca = "certs/ca.crt" }

[vars]                          # swarm-wide defaults, inherited by every robot
site        = "lab-1"
ros_distro  = "humble"

[[robots]]
id       = "tb-01"
name     = "turtlebot-1"
type     = "turtlebot3"       # must exist in robot_types
address  = "10.0.0.21"        # SSH endpoint for provisioning
simulated = false             # true for agents on the host
env      = { ROS_DOMAIN_ID = "42" }
vars     = { ns = "tb01", model = "burger" }   # overrides [vars] per key
```

## Robot Types (`robots/*.toml`)

```toml
[robot_types.turtlebot3]
display_name = "TurtleBot3"

[robot_types.turtlebot3.actions.bringup]
command     = "ros2 launch turtlebot3_bringup turtlebot3_core.launch.py"
timeout_sec = 120
dangerous   = false

[robot_types.turtlebot3.actions.controller]
command = "ros2 run swarm_utils follower --namespace {{vars.ns}} --model {{vars.model}}"
timeout_sec = 0

[robot_types.turtlebot3.actions.shutdown]
command   = "sudo shutdown -h now"
dangerous = true
```

### Action Options

| Key | Meaning |
|-----|---------|
| `command` | Shell command; `{{placeholders}}` resolved per robot |
| `timeout_sec` | Kill after N seconds (`0` = no timeout) |
| `env` | Extra environment for every invocation |
| `cwd` | Working directory (default: agent's home) |
| `dangerous` | Require confirmation for batch dispatch |
| `concurrency` | Max concurrent invocations per robot (default `1`) |

## Swarm Actions

Swarm-level actions live in `swarm.toml` and dispatch by bare name:

```toml
[actions.start_trial]
command     = "python3 /srv/experiments/run.py --robot {{robot_id}}"
timeout_sec = 300
```

```sh
swarmdeck-cli run start_trial --all --yes
```

## Template Placeholders

| Placeholder | Value |
|-------------|-------|
| `{{robot_id}}` | `tb-01` |
| `{{robot_name}}` | `turtlebot-1` (or the id if unnamed) |
| `{{robot_type}}` | `turtlebot3` |
| `{{address}}` | the robot's `address` (may be empty) |
| `{{vars.<key>}}` | per-robot values from `swarm.toml`, falling back to swarm-level `[vars]` |

### Swarm-Level Variables

A `[vars]` table in `swarm.toml` defines variables shared by the whole
swarm. Every robot inherits them, so an action can reference
`{{vars.site}}` without each robot having to repeat the value. A key set
in a robot's own `vars` takes precedence over the swarm default on a
per-key basis, and robots that phone home unannounced and are adopted
receive the swarm values as well.

## `agent.toml` (on the robot)

```toml
robot_id = "tb-01"

[controller]
endpoint = "100.64.0.1:50051"
id_code  = "lab1-swarm-secret"
tls      = false
```

### Generic agent config (`extends`)

Per-agent files can inherit from a generic config and override only what
differs (e.g. the robot id). `extends` takes a path relative to the file
itself; tables are merged key-by-key, scalars replaced — the child wins.

```toml
# configs/sim/agent-base.toml — shared by every sim agent
[controller]
endpoint = "127.0.0.1"
id_code  = "sim-swarm-secret"
```

```toml
# configs/sim/agent-1.toml — per-agent override
extends = "agent-base.toml"
robot_id = "sim-01"
```

## Host Defaults

- `--config configs/lab/swarm.toml` (swarm config file)
- `--robot-types robots` (shared robot type definitions)
- Override with `--config` / `--robot-types` flags
