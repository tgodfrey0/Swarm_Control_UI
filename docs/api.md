# SwarmDeck backend API

The control host (`swarmdeck-host`) is the backend. Every UI — the WebUI,
`swarmdeck-cli`, and any future TUI — is a thin client over this HTTP/WebSocket
API. All JSON is snake_case. Responses are never cached (the host sets
`Cache-Control: no-store` on `/` and `/static/*`).

- Base URL: `http://<host>:<ui-bind>` (see `ControllerConfig.ui_bind`).
- Errors: non-2xx with a plain-text body describing the problem.
- Live updates: `WS /api/ws` pushes `Event` objects (below).

## Endpoints

| Method | Path                    | Request          | Response            |
|--------|-------------------------|------------------|---------------------|
| GET    | `/api/robots`           | —                | `RobotView[]`       |
| GET    | `/api/types`            | —                | `string[]`          |
| GET    | `/api/actions`          | —                | `ActionsView`       |
| GET    | `/api/config`           | —                | `ConfigView`        |
| GET    | `/api/health`           | —                | `{"status":"ok"}`   |
| GET    | `/api/runs`             | —                | `RunView[]` (newest 50) |
| GET    | `/api/runs/{id}`        | —                | `RunView` (404 if unknown) |
| POST   | `/api/run`              | `RunRequest`     | `RunResponse`       |
| POST   | `/api/workflow`         | `WorkflowRunRequest` | `RunResponse`   |
| POST   | `/api/stop`             | `StopRequest`    | `string[]` (stopped ids) |
| POST   | `/api/adopt/{robot}`    | `AdoptRequest`   | `{}`                |
| POST   | `/api/release/{robot}`  | —                | `{}`                |
| GET    | `/api/robots/{id}/logs` | `?tail=N` (200)  | `LogLine[]`         |
| WS     | `/api/ws`               | —                | `Event` stream      |
| GET    | `/`                     | —                | WebUI `index.html`  |
| GET    | `/static/*`             | —                | UI assets (js/css)  |

## Types

```jsonc
// RobotView — a robot as seen by clients.
{
  "id": "tb-01",
  "name": "tb-01",
  "kind": "turtlebot3",
  "address": "10.0.0.5",     // null unless configured
  "simulated": false,
  "adopted": false,
  "connected": true,
  "agent_version": "0.1.0",
  "hostname": "tb-01",        // null before first register
  "last_seen_ms": 0,          // 0 when offline
  "active": null              // null, or ActiveView
}

// ActiveView — the action currently running on a robot.
{
  "action_id": "9f7c...:tb-01",
  "action_name": "turtlebot3.bringup",
  "command": "ros2 launch ...",
  "started_ms": 1720000000000
}

// ActionsView — dispatchable actions. `robot_type` refs are "<type>.<action>".
{
  "robot_type": ["sim.echo", "turtlebot3.bringup"],
  "swarm": ["trial", "trial_danger"],
  "workflows": ["deploy_fleet", "quick_test"]
}

// ConfigView — loaded swarm summary.
{
  "controller": "lab",
  "robot_types": ["sim", "turtlebot3", "uav"],
  "robot_count": 3,
  "grpc_listen": "0.0.0.0:50051",
  "ui_bind": "0.0.0.0:8080"
}

// ApiTargets — externally tagged, snake_case. The all-target is the bare
// string "all" (the WebUI also sends {"all": null}, which the host accepts).
// "all" | {"robots": [...]} | {"types": [...]} | {"name": "..."}
{
  "robots": ["tb-01", "tb-02"]
}

// RunRequest — dispatch a batch.
{
  "action": "sim.echo",
  "targets": { "all": {} },
  "timeout_sec": 0,           // optional; 0 = config default
  "confirm": false            // must be true for dangerous batch actions
}

// RunResponse.
{
  "run_id": "9f7c...",
  "action": "sim.echo",
  "targeted": ["sim-01", "sim-02"],
  "busy": [],                 // skipped: already running an action
  "offline": []               // skipped: not connected
}

// StopRequest.
{ "targets": { "robots": ["tb-01"] }, "confirm": false }

// WorkflowRunRequest — start a named workflow.
{ "workflow": "deploy_fleet", "confirm": false }

// AdoptRequest — claim an unknown robot that phoned home.
{ "kind": "turtlebot3", "name": "front-lidar" }   // name optional

// RunView — one batch run (or one step within a workflow).
{
  "run_id": "9f7c...",
  "action": "sim.echo",
  "created_ms": 1720000000000,
  "robots": [["sim-01", { "status": "running", "action_id": "...", "started_ms": 1720000000001 }]],
  // Present when this run is part of a workflow:
  "workflow": {
    "workflow_name": "deploy_fleet",
    "current_step": 2,
    "total_steps": 4,
    "step_action": "sim-uav.takeoff",
    "step_run_id": "a3b1..."
  }
}
// RunRobotStatus (tagged by `status`):
//   {"status":"queued"}
//   {"status":"running","action_id":"...","started_ms":...}
//   {"status":"done","exit_code":0,"killed":false,"finished_ms":...}
//   {"status":"failed","error":"..."}

// LogLine.
{ "ts_ms": 1720000001000, "stderr": false, "text": "hello" }
```

## Confirmation policy

`POST /api/run` rejects with status 400 and body containing
`confirm with confirm=true` when the action is marked `dangerous` and targets
more than one robot. Clients must prompt the operator and resubmit with
`confirm: true`. Non-dangerous or single-robot dispatches never require it.

## WebSocket events

On connect the server sends two snapshots first: `{"type":"robots",...}` then
`{"type":"runs",...}`. Afterwards every event is one `Event`:

```jsonc
{"type":"robots","robots":[RobotView,...]}   // full snapshot
{"type":"robot","robot":RobotView}           // single robot changed
{"type":"runs","runs":[RunView,...]}         // full snapshot
{"type":"run","run":RunView}                 // a run changed
{"type":"logs","robot":"tb-01","lines":[LogLine,...]}
```

Clients should treat `robots`/`runs` as authoritative full state and `robot`/
`run` as deltas, applied to their local copy.

## Workflows

Workflows are multi-step action sequences defined in `swarm.toml` under
`[workflows]`. Each step references an existing standalone action and targets
a set of robots. Steps run sequentially; the engine waits for all targeted
robots to finish each step before proceeding to the next.

`POST /api/workflow` accepts a `WorkflowRunRequest` and returns a `RunResponse`
immediately (the workflow runs asynchronously). The workflow's progress is
tracked via the same `RunView` system — each workflow run has a `workflow` field
with `current_step`, `total_steps`, `step_action`, and `step_run_id`.

### Failure semantics

- `continue_on_error: false` (default) — `&&` semantics: abort the workflow if the step fails.
- `continue_on_error: true` — `;` semantics: proceed to the next step regardless.
- A workflow-level `on_failure` setting provides the default; per-step overrides it.
