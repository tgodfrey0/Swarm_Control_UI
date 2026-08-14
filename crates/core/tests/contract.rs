//! Pins the exact JSON wire shape of the host API (see docs/api.md).
//! Client code in `ui/static/app.js`, `crates/client`, and the CLI all depend
//! on these field names — a rename here without updating clients is a bug.

use serde_json::{json, Value};

use swarmdeck_core::{
    ActionsView, AdoptRequest, ApiTargets, ConfigView, Event, RunRequest, RunResponse,
    RunRobotStatus, RunView, StopRequest,
};

fn to_value<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap()
}

#[test]
fn actions_view_is_snake_case() {
    let v = to_value(&ActionsView {
        robot_type: vec!["sim.echo".into()],
        swarm: vec!["trial".into()],
    });
    assert_eq!(
        v["robot_type"],
        json!(["sim.echo"]),
        "wire key must be robot_type"
    );
    assert_eq!(v["swarm"], json!(["trial"]));
}

#[test]
fn config_view_is_snake_case() {
    let v = to_value(&ConfigView {
        controller: "lab".into(),
        robot_types: vec!["sim".into()],
        robot_count: 2,
        grpc_listen: "0.0.0.0:50051".into(),
        ui_bind: "0.0.0.0:8080".into(),
    });
    assert_eq!(v["robot_types"], json!(["sim"]));
    assert_eq!(v["robot_count"], json!(2));
    assert_eq!(v["grpc_listen"], json!("0.0.0.0:50051"));
    assert_eq!(v["ui_bind"], json!("0.0.0.0:8080"));
    assert!(v.get("controller").is_some());
}

#[test]
fn api_targets_are_tagged_snake_case() {
    // Canonical wire form: unit variant serializes as a bare string.
    assert_eq!(to_value(&ApiTargets::All), json!("all"));
    assert_eq!(
        to_value(&ApiTargets::Robots(vec!["a".into()])),
        json!({ "robots": ["a"] })
    );
    assert_eq!(
        to_value(&ApiTargets::Types(vec!["sim".into()])),
        json!({ "types": ["sim"] })
    );
    assert_eq!(
        to_value(&ApiTargets::Name("tb".into())),
        json!({ "name": "tb" })
    );

    // The webui sends {"all": null} for the all-target; the host accepts it.
    let a: ApiTargets = serde_json::from_value(json!({ "all": null })).unwrap();
    assert!(matches!(a, ApiTargets::All));
    let a: ApiTargets = serde_json::from_value(json!("all")).unwrap();
    assert!(matches!(a, ApiTargets::All));
}

#[test]
fn run_request_uses_camel_free_snake_keys() {
    let v = to_value(&RunRequest {
        action: "sim.echo".into(),
        targets: ApiTargets::All,
        timeout_sec: Some(10),
        confirm: false,
    });
    assert_eq!(v["action"], json!("sim.echo"));
    assert_eq!(v["timeout_sec"], json!(10));
    assert_eq!(v["confirm"], json!(false));
}

#[test]
fn run_status_is_tagged() {
    let v = to_value(&RunRobotStatus::Running {
        action_id: "a".into(),
        started_ms: 1,
    });
    assert_eq!(v["status"], json!("running"));
    assert_eq!(v["action_id"], json!("a"));

    let v = to_value(&RunRobotStatus::Done {
        exit_code: 0,
        killed: false,
        finished_ms: 2,
    });
    assert_eq!(v["status"], json!("done"));
}

#[test]
fn events_are_tagged() {
    assert_eq!(
        to_value(&Event::Robot {
            robot: default_robot()
        })["type"],
        json!("robot")
    );
    assert_eq!(
        to_value(&Event::Robots {
            robots: vec![default_robot()]
        })["type"],
        json!("robots")
    );
    assert_eq!(
        to_value(&Event::Runs { runs: vec![] })["type"],
        json!("runs")
    );
    assert_eq!(
        to_value(&Event::Run { run: default_run() })["type"],
        json!("run")
    );
    assert_eq!(
        to_value(&Event::Logs {
            robot: "a".into(),
            lines: vec![]
        })["type"],
        json!("logs")
    );
}

#[test]
fn response_round_trips() {
    let resp = RunResponse {
        run_id: "r1".into(),
        action: "sim.echo".into(),
        targeted: vec!["sim-01".into()],
        busy: vec![],
        offline: vec!["sim-02".into()],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: RunResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.run_id, "r1");

    // StopRequest / AdoptRequest / RunView deserialize too (client needs it).
    let s: StopRequest = serde_json::from_value(json!({ "targets": "all" })).unwrap();
    assert!(matches!(s.targets, ApiTargets::All));
    let a: AdoptRequest = serde_json::from_value(json!({ "kind": "sim" })).unwrap();
    assert_eq!(a.kind, "sim");
    let r: RunView = serde_json::from_value(json!({
        "run_id": "r1",
        "action": "sim.echo",
        "created_ms": 0,
        "robots": [["sim-01", { "status": "queued" }]]
    }))
    .unwrap();
    assert_eq!(r.robots.len(), 1);
}

fn default_robot() -> swarmdeck_core::RobotView {
    swarmdeck_core::RobotView {
        id: "sim-01".into(),
        name: "sim-01".into(),
        kind: "sim".into(),
        address: None,
        simulated: true,
        adopted: false,
        connected: true,
        agent_version: "0.1.0".into(),
        hostname: None,
        last_seen_ms: 0,
        active: None,
    }
}

fn default_run() -> RunView {
    RunView {
        run_id: "r1".into(),
        action: "sim.echo".into(),
        created_ms: 0,
        robots: vec![],
    }
}
