# HTTP/WebSocket API

The control host exposes a JSON API. All responses are `Cache-Control: no-store`.

Base URL: `http://<host>:<ui-bind>` (see `ControllerConfig.ui_bind`)

## Endpoints

| Method | Path | Request | Response |
|--------|------|---------|----------|
| GET | `/api/robots` | -- | `RobotView[]` |
| GET | `/api/types` | -- | `string[]` |
| GET | `/api/actions` | -- | `ActionsView` |
| GET | `/api/config` | -- | `ConfigView` |
| GET | `/api/health` | -- | `{"status":"ok"}` |
| GET | `/api/runs` | -- | `RunView[]` (newest 50) |
| GET | `/api/runs/{id}` | -- | `RunView` (404 if unknown) |
| POST | `/api/run` | `RunRequest` | `RunResponse` |
| POST | `/api/stop` | `StopRequest` | `string[]` (stopped ids) |
| POST | `/api/adopt/{robot}` | `AdoptRequest` | `{}` |
| POST | `/api/release/{robot}` | -- | `{}` |
| GET | `/api/robots/{id}/logs` | `?tail=N` (200) | `LogLine[]` |
| WS | `/api/ws` | -- | `Event` stream |
| GET | `/` | -- | WebUI `index.html` |
| GET | `/static/*` | -- | UI assets |

## Types

### RobotView

```json
{
  "id": "tb-01",
  "name": "tb-01",
  "kind": "turtlebot3",
  "address": "10.0.0.5",
  "simulated": false,
  "adopted": false,
  "connected": true,
  "agent_version": "0.1.0",
  "hostname": "tb-01",
  "last_seen_ms": 0,
  "active": null
}
```

### RunRequest

```json
{
  "action": "sim.echo",
  "targets": { "all": {} },
  "timeout_sec": 0,
  "confirm": false
}
```

### RunResponse

```json
{
  "run_id": "9f7c...",
  "action": "sim.echo",
  "targeted": ["sim-01", "sim-02"],
  "busy": [],
  "offline": []
}
```

### ActionsView

```json
{
  "robot_type": ["sim.echo", "turtlebot3.bringup"],
  "swarm": ["trial", "trial_danger"]
}
```

### LogLine

```json
{ "ts_ms": 1720000001000, "stderr": false, "text": "hello" }
```

## Confirmation Policy

`POST /api/run` returns 400 with `confirm with confirm=true` when:
- Action is marked `dangerous`
- Targets more than one robot

Clients must prompt the operator and resubmit with `confirm: true`.

## WebSocket Events

On connect, the server sends full snapshots:
```json
{"type":"robots","robots":[RobotView,...]}
{"type":"runs","runs":[RunView,...]}
```

Afterwards, deltas:
```json
{"type":"robot","robot":RobotView}
{"type":"run","run":RunView}
{"type":"logs","robot":"tb-01","lines":[LogLine,...]}
```

Clients should treat `robots`/`runs` as authoritative full state and `robot`/`run` as deltas.
