# CLI Reference

The CLI (`swarmlink-cli`) communicates with the control host over HTTP/WS.

## Usage

```sh
./bin/swarmlink-cli --host http://<host>:<port> <subcommand> [flags]
```

## Subcommands

### `status`

Show swarm status (table or JSON).

```sh
swarmlink-cli status
swarmlink-cli status --json
```

### `run`

Dispatch an action to targets.

```sh
# Single robot
swarmlink-cli run turtlebot3.rotate --robots tb-01

# Multiple robots
swarmlink-cli run turtlebot3.controller --robots tb-01,tb-02

# By type
swarmlink-cli run uav.takeoff --types uav

# By name pattern
swarmlink-cli run uav.arm --name mav-1

# All robots
swarmlink-cli run smoke.echo --all

# Dangerous action with confirmation
swarmlink-cli run uav.arm --name mav-1 --yes

# Swarm-level action
swarmlink-cli run start_trial --all --yes

# JSON output
swarmlink-cli run sim.echo --all --json
```

### `stop`

Stop all running actions on targets.

```sh
swarmlink-cli stop --all --yes
swarmlink-cli stop --robots tb-01,tb-02 --yes
```

### `ps`

Show running actions and recent runs.

```sh
swarmlink-cli ps
swarmlink-cli ps --running
```

### `logs`

View robot logs (one-shot or follow).

```sh
swarmlink-cli logs tb-01 --tail 200
swarmlink-cli logs tb-01 --follow
```

### `config`

Validate a swarm's configuration.

```sh
swarmlink-cli config --config configs/lab/swarm.toml
```

### `provision`

SSH-provision robots with the agent binary.

```sh
SWARMLINK_AGENT_BIN=target/aarch64-unknown-linux-musl/release/swarmlink-agent \
  swarmlink-cli provision --config configs/lab/swarm.toml --user pi

# Specific robots
SWARMLINK_CONTROLLER_ENDPOINT=100.64.0.1:50051 \
  swarmlink-cli provision --robots tb-01,tb-02
```

### `workflow`

Run a named workflow (multi-step action sequence).

```sh
# Run a workflow
swarmlink-cli workflow deploy_fleet

# Skip confirmation for dangerous steps
swarmlink-cli workflow deploy_fleet --yes

# JSON output
swarmlink-cli workflow quick_test --json
```

## Target Flags

Targets are mutually exclusive -- exactly one of:
- `--all` -- all robots
- `--robots <id[,id...]>` -- specific robot IDs
- `--types <type[,type...]>` -- all robots of given types
- `--name <pattern>` -- substring match on ID/name

## Dangerous Actions

Actions marked `dangerous` targeting more than one robot require `--yes` (CLI) or `confirm: true` (API). The host enforces this -- a `RunRequest` without confirmation is rejected.
