# Agent API (gRPC)

The agent protocol is the contract between robot agents and the control host.
It is a single gRPC service defined in [`proto/swarm.proto`](../../proto/swarm.proto)
(package `swarmlink.v1`). Implement it if you want to:

- **Write your own controller** -- replace `swarmlink-host` with your own
  dispatch logic while keeping the stock agent on robots.
- **Write your own agent** -- integrate Swarmlink into an existing robot
  stack instead of running `swarmlink-agent`.

Reference implementations: agent side in `crates/agent/src/session.rs`,
host side in `crates/host/src/grpc.rs` + `crates/host/src/registry.rs`.

## Service

```proto
service Agent {
  // One long-lived bidirectional stream per agent.
  rpc Session(stream Report) returns (stream Command);
}
```

The **agent is the gRPC client**: it dials the host and keeps one stream open.
The host never connects to robots, so agents can live behind NAT/firewalls.

## Transport

| Setting | Value |
|---------|-------|
| Transport | gRPC over HTTP/2 (tonic) |
| Endpoint | `controller.endpoint` in `agent.toml`, e.g. `10.0.0.1:50051` |
| Default port | `50051` (appended automatically when the endpoint has no port) |
| TLS | Optional; `[controller] tls = true` + optional `ca` / `server_name` |
| mTLS | Host may require client certs via `[controller.tls] ca` |

## Session lifecycle

```
Agent                                   Host
  |--- Report{Register} ----------------->|  first message, must be Register
  |<-- (stream open) ---------------------|
  |--- Report{Status} ------------------->|  sent immediately after registering
  |<-- Command{RunAction} ----------------|
  |--- Report{Ack} ---------------------->|  spawn ok/fail
  |--- Report{Log}* -------------------->|  stdout/stderr lines as they appear
  |--- Report{Result} ------------------->|  process exited
  |--- Report{Status} ------------------->|  every 5 s (heartbeat + metrics)
  |<-- Command{Ping} --------------------|
  |--- Report{Heartbeat} ---------------->|  reply to Ping
```

Rules every implementation must follow:

1. **The first report must be a `Register`.** Anything else is rejected with
   `INVALID_ARGUMENT`; disconnecting before registering yields `UNAVAILABLE`.
2. **Registration is authenticated by `id_code`.** A mismatch is rejected with
   `PERMISSION_DENIED` ("registration rejected: id_code mismatch"). The
   reference agent logs this and keeps retrying with backoff -- fix the config
   rather than expecting the robot to give up.
3. **Send a `Status` at least every 15 s.** The reference agent sends one every
   5 s; the host marks a robot offline after 15 s of silence (`STALE_AFTER_MS`).
4. **Reconnect forever.** The reference agent retries with exponential backoff:
   1 s, doubling up to a 30 s cap. On disconnect it kills actions flagged
   `kill_on_disconnect`; other actions keep running locally, but their logs and
   results produced during the outage are lost (not replayed).
5. **One session per robot.** If an agent opens a second session with the same
   `robot_id`, the newest wins; a stale session closing must not mark the robot
   disconnected.

## Agent -> Host: `Report`

Exactly one variant of the `oneof report` is set per message.

### Register

Sent once, first, on every (re)connect.

| Field | Type | Notes |
|-------|------|-------|
| `robot_id` | string | Unique identity within the swarm. Unknown ids appear on the host as unclaimed robots available for adoption. |
| `id_code` | string | Shared secret; must match the host's `controller.id_code`. |
| `agent_version` | string | Semantic version, shown in the UI (`RobotView.agent_version`). |
| `hostname` | string | May be empty; displayed when set. |
| `capabilities` | map<string,string> | Reserved -- currently always empty. |

### Heartbeat

Liveness only. The reference agent leaves `timestamp_ms` unset (0); hosts
should stamp arrival time themselves rather than trust the field.

### Status

Periodic health snapshot (every 5 s in the reference agent).

| Field | Type | Notes |
|-------|------|-------|
| `timestamp_ms` | uint64 | Unix ms at sampling time. |
| `active_action_id` | string | Empty when idle. Only foreground actions count: `background` actions are excluded. |
| `cpu_usage` | double | Whole-system CPU percent, 0-100. First sample after connect is 0. |
| `memory_used_kb` | uint64 | RAM used (total - available). |
| `uptime_sec` | uint64 | Machine uptime. |
| `battery_percent` | float | 0 means unknown/unavailable -- do not treat 0 as "empty battery". |

### ActionLog

One line of process output. `data` is UTF-8 without the trailing newline;
`stderr = true` marks the stderr stream. `seq` is reserved (always 0 today).
Logs are lossy under backpressure -- senders may drop chunks rather than block,
so receivers must tolerate gaps.

### ActionResult

Terminal event for an action. Sent exactly once per accepted `RunAction`.

| Field | Type | Notes |
|-------|------|-------|
| `action_id` | string | Echoes the id from `RunAction`. |
| `exit_code` | uint32 | Process exit code; `1` when unknown (spawn error, timeout). |
| `killed` | bool | True when stopped via `StopAction` or killed on disconnect. Not set on timeout (check `error`). |
| `error` | string | Human-readable failure, e.g. `"timed out"`. Empty on success. |
| `started_ms` / `finished_ms` | uint64 | Unix ms wall-clock bounds. |

Hosts typically classify: `killed` -> done(killed), else `exit_code == 0 &&
error empty` -> done, else failed.

### ActionAck

Immediate response to a `RunAction`: `accepted = true` once the process spawned,
or `accepted = false` with a `reason` (e.g. spawn failure). Exactly one ack per
run command. `StopAction` and `Ping` are not acked.

## Host -> Agent: `Command`

Exactly one variant of the `oneof command` is set per message.

### RunAction

Execute a shell command.

| Field | Type | Notes |
|-------|------|-------|
| `action_id` | string | Unique id minted by the host; convention `{run_id}:{robot_id}` so results route back to batch runs. Must be echoed in all logs/ack/result. |
| `action_name` | string | Config name (e.g. `turtlebot3.bringup`) for display/log headers. |
| `command` | string | Shell command, already template-resolved by the host. Executed via `/bin/sh -c`. |
| `env` | map<string,string> | Extra environment for the process. |
| `cwd` | string | Working directory; empty = inherit. |
| `timeout_sec` | uint32 | Kill the process group after this many seconds; `0` = no timeout. On timeout the result carries `error = "timed out"`. |
| `kill_on_disconnect` | bool | Terminate the action when the session drops (e.g. bringup processes). |
| `background` | bool | Doesn't count as the robot's active action, so other actions can run concurrently (e.g. long-lived launches). |

Implementations should run each action in its own **process group** and stop it
by signalling the group (`SIGTERM`), so shell children die too.

### StopAction

Kill the running action with the given `action_id` (SIGTERM to its process
group). No ack; completion is reported through the normal `ActionResult` with
`killed = true`. Ignored if the id is unknown/already finished.

### Ping

Liveness probe from the host. Agents must answer promptly with a `Heartbeat`
report.

## Building a custom controller

1. Copy `proto/swarm.proto` and generate stubs for your language
   (`protoc`/`grpcio`/`tonic`/...).
2. Implement `Agent.Session`: read the first `Report`, require
   `Register` with a matching `id_code`, otherwise fail with
   `PERMISSION_DENIED`.
3. Track per-robot state: `last_seen` updated on every report, offline after
   15 s of silence; store the response stream to send `Command`s.
4. To dispatch: send `RunAction` with a unique `action_id`, expect an
   `ActionAck`, consume `ActionLog`s, and wait for `ActionResult`.
   To cancel: send `StopAction`.
5. Send `Ping` if you want an explicit round-trip liveness check.

See `crates/host/src/grpc.rs` (stream handling) and
`crates/host/src/dispatch.rs` (run/stop bookkeeping) for the canonical flow.

## Building a custom agent

Minimum viable behaviour:

1. Dial `controller.endpoint` (TLS per config) and open `Session`.
2. Send `Register` (correct `robot_id` + `id_code`) as the very first message.
3. Send `Status` every ~5 s (never longer than 15 s).
4. Handle commands: spawn `RunAction` (ack it, stream logs, report a final
   `ActionResult`), honour `StopAction` and `timeout_sec`, reply to `Ping`
   with `Heartbeat`.
5. Reconnect with backoff on any stream error; kill `kill_on_disconnect`
   actions first.

See `crates/agent/src/session.rs` and `crates/agent/src/runner.rs` for the
reference behaviour, including process-group handling and log backpressure.
