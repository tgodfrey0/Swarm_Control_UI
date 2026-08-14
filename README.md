# SwarmDeck

A config-driven swarm robotics control deck. One **control host** keeps a
gRPC session open to every **robot agent**, executes shell actions across the
swarm, and exposes a live WebUI + HTTP/WS API + CLI.

- **Generic agents**: the robot side is deliberately dumb — it just runs shell
  commands the host sends it and streams back logs/status. No ROS/MAVSDK
  dependency, so the same agent runs on TurtleBots, PX4 UAVs, RPis, or a plain
  laptop.
- **Config-driven**: swarms, robot types, and actions are declared in TOML.
  Action commands are arbitrary shell strings with `{{vars.*}}` template
  substitution resolved host-side per robot.
- **Batch dispatch**: `--all`, `--robots tb-01,tb-02`, `--types uav`,
  `--name mav-1`, with `dangerous`-action confirmation and per-robot
  concurrency enforcement.
- **Static musl agents**: cross-compiled with `cargo zigbuild` (no Docker,
  no glibc) for `aarch64` / `armv7` / `x86_64` Linux robots.

```
                    ┌──────────────────────────────────────────────┐
                    │              control host (host crate)        │
                    │                                              │
  WebUI (browser) ──┤ HTTP+WS   /api/*, /api/ws                    │
  swarmdeck-cli ────┤          │                                   │
                    │          ├─ dispatch engine ──┐               │
                    │          │                    │ sends gRPC    │
                    │   gRPC server (tonic) ◄───────┤ Commands      │
                    └──────────┬────────────────────┘               │
                     Session stream (Register/Status/Log/Result)    │
                  ┌────────────┴──────────────┐                     │
                  ▼                           ▼                     │
          robot agent (RPis)          simulated agents (this host)  │
   /opt/swarm-agent/swarmdeck-agent   same binary, local agent.toml │
   runs shell actions, streams logs   simulated = true in swarm     │
```

## Components

| Crate             | What it is                                                        |
|-------------------|-------------------------------------------------------------------|
| `crates/host`     | `swarmdeck` — control host: gRPC server, dispatch engine, WebUI, HTTP/WS API |
| `crates/agent`    | `swarmdeck-agent` — robot agent: executes commands, streams logs/status |
| `crates/cli`      | `swarmdeck-cli` — batch status/run/stop/ps/logs + SSH provisioning |
| `crates/core`     | Shared config schema, template resolution, target selection       |
| `proto`           | gRPC definitions (`swarm.proto`)                                  |
| `configs/lab`      | Example swarm (TurtleBot3 + UAV)                      |

## Quick start (simulated agents on one machine)

The fastest way to try it: run the host, then run the **same agent binary
locally** with a small `agent.toml`. Robots that run on the host are marked
`simulated = true` in the swarm config (see [Simulated agents](#simulated-agents)).

### Pre-made sim swarm (2 fake robots)

A ready-to-run config lives in `configs/sim/` — a controller plus two
simulated robots (`sim-01`, `sim-02`) that use plain shell actions, on
separate ports so it can run alongside the lab swarm:

```sh
# 1. Set up the pixi toolchain (rustup + zig + cargo-zigbuild + protobuf …)
pixi install

# 2. Start the sim host (WebUI at http://localhost:18082)
pixi run host -- --swarm configs/sim

# 3. In two other terminals, start the fake robots
pixi run agent-sim -- --config configs/sim/agent-1.toml
pixi run agent-sim -- --config configs/sim/agent-2.toml

# 4. In another terminal, talk to it
pixi run cli -- --host http://127.0.0.1:18082 status
```

### Running robot-specific tasks

An action on a robot type is referenced as `<type>.<action>` and targeted
with exactly one of `--robots <id[,id…]>`, `--types <type[,type…]>`,
`--name <pattern>`, or `--all`:

```sh
# single robot
pixi run cli -- run sim.whoami --robots sim-01

# several by id
pixi run cli -- run sim.whoami --robots sim-01,sim-02

# every robot of a type
pixi run cli -- run sim.echo --types sim

# substring match on id/name
pixi run cli -- run sim.echo --name sim-

# swarm-level action fanned out to everyone
pixi run cli -- run trial --all
```

All template placeholders resolve per robot (`{{robot_id}}`, `{{robot_name}}`,
`{{vars.<key>}}`), so `sim.whoami` prints `robot=sim-01 var=alpha` on one
robot and `robot=sim-02 var=beta` on the other.

> The pre-made configs above are just examples: run the host against any
> swarm with `pixi run host -- --swarm <dir>`, and point each simulated
> agent at it with `--config <agent.toml>`.

Open http://localhost:18082 — the robots show **online**, and you can
dispatch `sim.echo`, watch live logs, and see run results.

## Simulated agents

Local simulated agents are normal agents, with two conventions:

1. **In the swarm config** (`swarm.toml`) the robot is marked
   `simulated = true`. The WebUI shows it with a distinct indicator, the
   provisioner skips it (nothing to SSH to), and `swarmdeck-cli status`
   labels it `sim-online`.
2. **On disk** the agent needs a local `agent.toml` pointing at the host's
   gRPC endpoint and the shared `id_code`:

```toml
# /etc/swarm-agent/agent.toml  (or anywhere, pass with --config)
robot_id = "tb-01"

[controller]
endpoint = "127.0.0.1:50051"
id_code  = "lab1-swarm-secret"
```

```sh
pixi run agent-sim      # = cargo run -p swarmdeck-agent -- (defaults to /etc/swarm-agent/agent.toml)
```

> For real robots, run `swarmdeck-cli provision` instead — it SSHes in and
> installs the agent + systemd unit automatically (see below).

## Configuration

Swarm configs live under a `configs/` directory. Each swarm is its own
directory with a `swarm.toml` describing the controller + robots; robot types
are **shared across swarms** in `robots/*.toml` (and merged by
default). This is kept separate from machine/system config (`agent.toml`,
systemd units, install paths).

```
configs/
├── lab/                      # one directory per swarm
│   └── swarm.toml            # controller + [[robots]]
├── sim/                      # fake swarm for local testing
│   ├── swarm.toml
│   ├── agent-1.toml
│   └── agent-2.toml
robots/                       # shared robot kinds + actions
├── turtlebot3.toml           # [robot_types.turtlebot3] + actions
├── uav.toml
└── sim.toml                  # shell-action robot type for the sim swarm
```

The host default is `--swarm configs/lab` with `--robot-types robots`
(override either with `--config` / `--robot-types`). The CLI `config` and
`provision` subcommands take the same flags.

### `swarm.toml`

```toml
[controller]
name        = "lab-1"
id_code     = "lab1-swarm-secret"   # shared secret agents present on connect
grpc_listen = "0.0.0.0:50051"       # where agents phone home
ui_bind     = "0.0.0.0:8080"        # WebUI + HTTP/WS API
# tls = { cert = "certs/host.crt", key = "certs/host.key", ca = "certs/ca.crt" }

[[robots]]
id       = "tb-01"
name     = "turtlebot-1"
type     = "turtlebot3"       # must exist in robot_types
address  = "10.0.0.21"        # SSH endpoint for the provisioner (optional)
simulated = false             # true for agents that run on the host
env      = { ROS_DOMAIN_ID = "42" }           # env for every action on this robot
vars     = { ns = "tb01", model = "burger" }  # {{vars.*}} in action commands
```

### Swarm actions (`[actions]` in `swarm.toml`)

Swarm-level actions live in the swarm file and are dispatched **by bare name**
to any robot regardless of type — useful for experiments/trials that fan out
across a mixed swarm (the generic bringup/teardown commands stay in robot
types). All template placeholders work, including `{{robot_id}}`:

```toml
[actions.start_trial]
command     = "python3 /srv/experiments/run.py --robot {{robot_id}}"
timeout_sec = 300
```

```sh
swarmdeck-cli run start_trial --all --yes
```

Action names must not contain `.` (that prefix is reserved for robot-type
actions). The same action options as robot types apply (`dangerous`,
`timeout_sec`, `env`, `cwd`, `concurrency`).

### `robot_types/*.toml`

```toml
[robot_types.turtlebot3]
display_name = "TurtleBot3"

[robot_types.turtlebot3.actions.bringup]
command     = "ros2 launch turtlebot3_bringup turtlebot3_core.launch.py"
timeout_sec = 120
dangerous   = false

[robot_types.turtlebot3.actions.controller]
command = "ros2 run swarm_utils follower --namespace {{vars.ns}} --model {{vars.model}}"
timeout_sec = 0                # 0 = no timeout

[robot_types.turtlebot3.actions.shutdown]
command   = "sudo shutdown -h now"
dangerous = true               # requires explicit confirmation for batches
```

Action options:

| Key            | Meaning                                                              |
|----------------|----------------------------------------------------------------------|
| `command`      | Shell command; `{{placeholders}}` are resolved host-side per robot   |
| `timeout_sec`  | Kill the process after N seconds (`0` = no timeout)                  |
| `env`          | Extra environment applied to every invocation                        |
| `cwd`          | Working directory for the process (default: agent's home)            |
| `dangerous`    | Require explicit confirmation when dispatched to more than one robot |
| `concurrency`  | Max concurrent invocations per robot (default `1`)                   |

### Template placeholders

Double braces avoid clashing with shell `${VAR}` and brace ranges `{1..3}`:

| Placeholder       | Value                                        |
|-------------------|----------------------------------------------|
| `{{robot_id}}`    | `tb-01`                                      |
| `{{robot_name}}`  | `turtlebot-1` (or the id if unnamed)         |
| `{{robot_type}}`  | `turtlebot3`                                 |
| `{{address}}`     | the robot's `address` (may be empty)         |
| `{{vars.<key>}}`  | per-robot values from `swarm.toml`           |

Per-robot `env` entries are merged into every action's environment, so
`ROS_DOMAIN_ID` etc. never need to appear in the action itself.

### `agent.toml` (on the robot)

```toml
robot_id = "tb-01"

[controller]
endpoint = "100.64.0.1:50051"   # control host gRPC address
id_code  = "lab1-swarm-secret"
tls      = false                # true if the host serves TLS
```

The agent phones home, registers, and keeps one bidirectional stream open,
reconnecting with exponential backoff (1 s → 30 s). A wrong `id_code` is
rejected with PermissionDenied; the agent retries and keeps the host log
clean of another controller's robots.

## CLI

```sh
pixi run cli -- --help

# Swarm status (table or --json)
swarmdeck-cli status

# Dispatch an action to a batch
swarmdeck-cli run turtlebot3.rotate --all
swarmdeck-cli run turtlebot3.controller --robots tb-01,tb-02
swarmdeck-cli run uav.takeoff --types uav
swarmdeck-cli run uav.arm --name mav-1 --yes      # dangerous action: confirm
swarmdeck-cli run smoke.echo --all --json
swarmdeck-cli run start_trial --all --yes         # swarm-level [actions]

# Stop everything running on the targets
swarmdeck-cli stop --all --yes

# Running actions + recent runs
swarmdeck-cli ps
swarmdeck-cli ps --running

# Logs (one-shot or follow)
swarmdeck-cli logs tb-01 --tail 200
swarmdeck-cli logs tb-01 --follow

# Validate a swarm's config
swarmdeck-cli config --swarm configs/lab
```

Targets are mutually exclusive: exactly one of `--all`, `--robots`,
`--types`, `--name`. `dangerous` actions targeting more than one robot prompt
for confirmation unless `--yes` is given (the host enforces this too — a
`RunRequest` without `confirm=true` is rejected).

## HTTP / WebSocket API

JSON routes (all relative to `ui_bind`):

| Method | Path                        | Body / Params                         | Returns                       |
|--------|-----------------------------|---------------------------------------|-------------------------------|
| GET    | `/`                         | —                                     | WebUI HTML                    |
| GET    | `/static/*`                 | —                                     | `ui/static` assets            |
| GET    | `/api/robots`               | —                                     | `[RobotView]`                 |
| GET    | `/api/types`               | —                                     | `[string]` robot type ids     |
| GET    | `/api/actions`             | —                                     | `{robot_type: [..], swarm: [..]}` |
| GET    | `/api/runs`                 | —                                     | `[RunView]` (50 latest)       |
| GET    | `/api/robots/{id}/logs`     | `?tail=200`                           | `[LogLine]`                   |
| POST   | `/api/run`                  | `RunRequest`                          | `RunResponse`                 |
| POST   | `/api/stop`                 | `StopRequest`                         | `[string]` stopped robot ids  |
| POST   | `/api/adopt/{robot}`        | `AdoptRequest {kind, name?}`          | `200`                         |
| GET    | `/api/ws`                   | WebSocket upgrade                     | stream of `Event` JSON        |

```jsonc
// POST /api/run
{ "action": "uav.takeoff",
  "targets": { "types": ["uav"] },
  "timeout_sec": null,
  "confirm": true }
```

Events on `/api/ws` are `{ "type": "robots" | "robot" | "runs" | "run" | "logs", … }`.
The socket first sends a full snapshot so late joiners get current state.

## Cross-compiling the agent (Raspberry Pi / Jetson)

The pixi env provides `zig` + `cargo-zigbuild` (rustup provides Rust + the
musl targets via `activate.sh`). Static binaries, no Docker:

```sh
pixi run agent-aarch64   # target/aarch64-unknown-linux-musl/release/swarmdeck-agent
pixi run agent-armv7     # 32-bit Raspberry Pi OS
pixi run agent-x86_64    # x86_64 SBCs
pixi run agent-all       # all three
```

Each binary is fully static (verified 2.0 MB aarch64, no glibc deps).

## Provisioning real robots

`swarmdeck-cli provision` SSHes into each robot (by `address`), copies the
agent, writes `/etc/swarm-agent/agent.toml`, and installs+starts the
`swarmdeck-agent.service` systemd unit:

```sh
# The agent to push (default: aarch64 musl release build)
SWARMDECK_AGENT_BIN=target/aarch64-unknown-linux-musl/release/swarmdeck-agent \
  swarmdeck-cli provision --swarm configs/lab --user pi

# Only some robots, or a specific controller endpoint override
SWARMDECK_CONTROLLER_ENDPOINT=100.64.0.1:50051 \
  swarmdeck-cli provision --robots tb-01,tb-02
```

Robots without an `address`, or with `simulated = true`, are skipped. The
systemd unit restarts the agent automatically (`Restart=always`).

## TLS

Set `[controller.tls]` in `swarm.toml` to serve gRPC over TLS (and optionally
mTLS with a client CA):

```toml
[controller.tls]
cert = "certs/host.crt"
key  = "certs/host.key"
ca   = "certs/ca.crt"   # optional: require client certificates
```

Agents connect with `tls = true` in `agent.toml`.

## Security notes

- `id_code` is a shared secret that isolates controllers on the same LAN —
  robots only accept commands from a host that knows it. It is **not**
  cryptography; use TLS (`[controller.tls]`) or a VPN/tailnet when hostile
  traffic is a concern.
- Actions are arbitrary shell strings run **as the agent's user**. Only add
  actions you trust, and be careful granting the agent user `sudo` (the
  example `turtlebot3.shutdown` needs it).
- The WebUI/API binds `0.0.0.0:8080` by default — restrict it with
  `ui_bind = "127.0.0.1:8080"` or a firewall if the network is untrusted.
- Adopted robots (phoned home but not in the config) are runtime-only until
  you add them to `swarm.toml`.

## Development

```sh
pixi run host       # run the control host (configs/lab); WebUI at localhost:8080
pixi run agent      # run the agent against /etc/swarm-agent/agent.toml
pixi run cli        # run the CLI
pixi run check      # cargo check --workspace
pixi run lint       # cargo clippy --workspace --all-targets -- -D warnings
pixi run test       # cargo test --workspace
pixi run fmt        # cargo fmt --all
```

The host reloads `swarm.toml` on `SIGHUP` (no restart needed). The whole
workspace passes `cargo clippy -- -D warnings` and `cargo test`.

> Build note: `crates/host/templates` is a symlink to `ui/templates` so the
> compiled Askama templates resolve; keep them in sync (edit the files under
> `ui/templates/`).

## Repo layout

```
crates/host/src/     host: grpc.rs, dispatch.rs, registry.rs, http.rs, ui.rs
crates/agent/src/    agent: session.rs (gRPC client), runner.rs (process groups), procfs.rs
crates/cli/src/      cli: main.rs (commands), provision.rs (ssh/scp/systemd)
crates/core/src/     core: config.rs, dispatch.rs, template.rs, spec.rs, api.rs
proto/               swarm.proto (service Agent / Session bidi stream)
configs/lab/         example swarm; configs/sim/ fake swarm; robots/ shared robot types
configs/sim/         fake swarm (2 local simulated robots) + agent configs
ui/                  webui: templates/index.html, static/{styles.css,app.js}
deploy/              (reserved) install packaging / recipes
```
