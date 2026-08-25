# CLI Reference

The CLI (`swarmdeck-cli`) communicates with the control host over HTTP/WS.

## Usage

```sh
./bin/swarmdeck-cli --host http://<host>:<port> <subcommand> [flags]
```

## Subcommands

### `status`

Show swarm status (table or JSON).

```sh
swarmdeck-cli status
swarmdeck-cli status --json
```

### `run`

Dispatch an action to targets.

```sh
# Single robot
swarmdeck-cli run turtlebot3.rotate --robots tb-01

# Multiple robots
swarmdeck-cli run turtlebot3.controller --robots tb-01,tb-02

# By type
swarmdeck-cli run uav.takeoff --types uav

# By name pattern
swarmdeck-cli run uav.arm --name mav-1

# All robots
swarmdeck-cli run smoke.echo --all

# Dangerous action with confirmation
swarmdeck-cli run uav.arm --name mav-1 --yes

# Swarm-level action
swarmdeck-cli run start_trial --all --yes

# JSON output
swarmdeck-cli run sim.echo --all --json
```

### `stop`

Stop all running actions on targets.

```sh
swarmdeck-cli stop --all --yes
swarmdeck-cli stop --robots tb-01,tb-02 --yes
```

### `ps`

Show running actions and recent runs.

```sh
swarmdeck-cli ps
swarmdeck-cli ps --running
```

### `logs`

View robot logs (one-shot or follow).

```sh
swarmdeck-cli logs tb-01 --tail 200
swarmdeck-cli logs tb-01 --follow
```

### `config`

Validate a swarm's configuration.

```sh
swarmdeck-cli config --config configs/lab/swarm.toml
```

### `provision`

SSH-provision robots with the agent binary.

```sh
SWARMDECK_AGENT_BIN=target/aarch64-unknown-linux-musl/release/swarmdeck-agent \
  swarmdeck-cli provision --config configs/lab/swarm.toml --user pi

# Specific robots
SWARMDECK_CONTROLLER_ENDPOINT=100.64.0.1:50051 \
  swarmdeck-cli provision --robots tb-01,tb-02
```

### `workflow`

Run a named workflow (multi-step action sequence).

```sh
# Run a workflow
swarmdeck-cli workflow deploy_fleet

# Skip confirmation for dangerous steps
swarmdeck-cli workflow deploy_fleet --yes

# JSON output
swarmdeck-cli workflow quick_test --json
```

## Target Flags

Targets are mutually exclusive -- exactly one of:
- `--all` -- all robots
- `--robots <id[,id...]>` -- specific robot IDs
- `--types <type[,type...]>` -- all robots of given types
- `--name <pattern>` -- substring match on ID/name

## Dangerous Actions

Actions marked `dangerous` targeting more than one robot require `--yes` (CLI) or `confirm: true` (API). The host enforces this -- a `RunRequest` without confirmation is rejected.
