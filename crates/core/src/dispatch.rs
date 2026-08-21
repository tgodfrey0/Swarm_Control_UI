//! Batch target resolution shared by the host's dispatch engine.
//! Both the WebUI and the CLI funnel through here, so batch semantics are
//! identical everywhere.

use crate::api::{parse_action_ref, ApiTargets};
use crate::config::{ActionConfig, RobotConfig, SwarmConfig};
use crate::error::{ConfigError, Result};

/// A resolved batch: the action to run plus the ordered list of robot configs.
#[derive(Debug)]
pub struct ResolvedRun {
    pub action_name: String,
    pub action: ActionConfig,
    pub robots: Vec<RobotConfig>,
    /// Flagged `dangerous` in config, for confirmation enforcement.
    pub dangerous: bool,
}

/// Resolve an action ref and target selector against the swarm.
///
/// Two action namespaces:
/// * `<type>.<action>` — an action defined on a robot type (`turtlebot3.bringup`).
/// * `<action>` (bare) — a swarm-level action defined in `[actions]`, dispatched
///   to any robot regardless of type (e.g. `start_experiment`).
pub fn resolve(
    cfg: &SwarmConfig,
    action: &str,
    targets: &ApiTargets,
    known_robot_ids: &[String],
) -> Result<ResolvedRun> {
    let robots = select_robots(cfg, targets, known_robot_ids)?;

    if let Some((ty, action_name)) = parse_action_ref(action) {
        let r#type = cfg
            .robot_types
            .get(ty)
            .ok_or_else(|| ConfigError::UnknownRobotType {
                robot: "targets".into(),
                kind: ty.to_string(),
            })?;
        let action_cfg =
            r#type
                .actions
                .get(action_name)
                .ok_or_else(|| ConfigError::UnknownAction {
                    kind: ty.to_string(),
                    action: action_name.to_string(),
                })?;
        return Ok(ResolvedRun {
            action_name: format!("{ty}.{action_name}"),
            action: action_cfg.clone(),
            dangerous: action_cfg.dangerous,
            robots,
        });
    }

    let action_cfg = cfg
        .actions
        .get(action)
        .ok_or_else(|| ConfigError::UnknownSwarmAction {
            action: action.to_string(),
        })?;
    Ok(ResolvedRun {
        action_name: action.to_string(),
        action: action_cfg.clone(),
        dangerous: action_cfg.dangerous,
        robots,
    })
}

pub fn select_robots(
    cfg: &SwarmConfig,
    targets: &ApiTargets,
    known_robot_ids: &[String],
) -> Result<Vec<RobotConfig>> {
    let all = cfg.robots.to_vec();
    let mut chosen = match targets {
        ApiTargets::All => all,
        ApiTargets::Robots(ids) => {
            let mut out = Vec::new();
            for id in ids {
                if let Some(r) = all.iter().find(|r| &r.id == id) {
                    out.push(r.clone());
                } else if known_robot_ids.iter().any(|k| k == id) {
                    // Adopted robot not (yet) persisted to config.
                    return Err(ConfigError::UnadoptedRobot { robot: id.clone() });
                } else {
                    return Err(ConfigError::UnknownRobot { id: id.clone() });
                }
            }
            out
        }
        ApiTargets::Types(types) => all
            .into_iter()
            .filter(|r| types.contains(&r.kind))
            .collect(),
        ApiTargets::Name(pat) => all
            .into_iter()
            .filter(|r| r.id.contains(pat) || r.display_name().contains(pat))
            .collect(),
    };
    chosen.dedup_by(|a, b| a.id == b.id);
    // Swarm-level [vars] act as defaults for every robot; the robot's own
    // entry wins per key.
    for robot in &mut chosen {
        for (k, v) in &cfg.vars {
            robot.vars.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ActionConfig, ControllerConfig, RobotConfig, RobotTypeConfig};
    use std::collections::BTreeMap;

    fn cfg() -> SwarmConfig {
        SwarmConfig {
            controller: ControllerConfig {
                name: "t".into(),
                id_code: "s".into(),
                grpc_listen: "0.0.0.0:1".parse().unwrap(),
                ui_bind: "0.0.0.0:1".parse().unwrap(),
                tls: None,
            },
            robot_types: BTreeMap::from([
                (
                    "turtlebot3".into(),
                    RobotTypeConfig {
                        display_name: Some("TurtleBot3".into()),
                        actions: BTreeMap::from([(
                            "bringup".into(),
                            ActionConfig {
                                command: "echo {{robot_id}}".into(),
                                timeout_sec: Some(10),
                                env: BTreeMap::new(),
                                cwd: None,
                                dangerous: false,
                                concurrency: 1,
                                background: false,
                            },
                        )]),
                    },
                ),
                (
                    "uav".into(),
                    RobotTypeConfig {
                        display_name: Some("UAV".into()),
                        actions: BTreeMap::new(),
                    },
                ),
            ]),
            actions: BTreeMap::from([(
                "start_trial".into(),
                ActionConfig {
                    command: "trial on {{robot_id}}".into(),
                    timeout_sec: None,
                    env: BTreeMap::new(),
                    cwd: None,
                    dangerous: true,
                    concurrency: 1,
                    background: false,
                },
            )]),
            vars: BTreeMap::from([
                ("site".into(), "lab-1".into()),
                ("ns".into(), "swarm".into()),
            ]),
            robots: vec![
                RobotConfig {
                    id: "tb-01".into(),
                    name: Some("turtlebot-1".into()),
                    kind: "turtlebot3".into(),
                    address: None,
                    simulated: false,
                    env: BTreeMap::new(),
                    vars: BTreeMap::from([("ns".into(), "tb01".into())]),
                    adopted: false,
                },
                RobotConfig {
                    id: "uav-01".into(),
                    name: None,
                    kind: "uav".into(),
                    address: None,
                    simulated: false,
                    env: BTreeMap::new(),
                    vars: BTreeMap::new(),
                    adopted: false,
                },
            ],
        }
    }

    #[test]
    fn resolves_robot_type_action() {
        let c = cfg();
        let r = resolve(
            &c,
            "turtlebot3.bringup",
            &ApiTargets::Types(vec!["turtlebot3".into()]),
            &[],
        )
        .unwrap();
        assert_eq!(r.action_name, "turtlebot3.bringup");
        assert_eq!(r.robots.len(), 1);
        assert!(!r.dangerous);
    }

    #[test]
    fn resolves_swarm_action_across_types() {
        let c = cfg();
        let r = resolve(&c, "start_trial", &ApiTargets::All, &[]).unwrap();
        assert_eq!(r.action_name, "start_trial");
        assert_eq!(r.robots.len(), 2);
        assert!(r.dangerous);
    }

    #[test]
    fn unknown_swarm_action_fails() {
        let c = cfg();
        let e = resolve(&c, "nope", &ApiTargets::All, &[]).unwrap_err();
        assert!(matches!(e, ConfigError::UnknownSwarmAction { .. }));
    }

    #[test]
    fn dot_in_swarm_action_name_rejected() {
        let c = cfg();
        let mut bad = c.clone();
        bad.actions
            .insert("bad.name".into(), bad.actions["start_trial"].clone());
        assert!(matches!(
            bad.validate(),
            Err(ConfigError::BadSwarmActionName { .. })
        ));
    }

    #[test]
    fn swarm_vars_inherit_to_all_robots() {
        let c = cfg();
        let r = select_robots(&c, &ApiTargets::All, &[]).unwrap();
        for robot in &r {
            assert_eq!(
                robot.vars.get("site").map(String::as_str),
                Some("lab-1"),
                "{} inherits swarm var",
                robot.id
            );
        }
    }

    #[test]
    fn robot_var_wins_over_swarm_default() {
        let c = cfg();
        let r = select_robots(&c, &ApiTargets::All, &[]).unwrap();
        let tb = r.iter().find(|r| r.id == "tb-01").unwrap();
        assert_eq!(tb.vars.get("ns").map(String::as_str), Some("tb01"));
        // …while a robot without the key still picks up the swarm default.
        let uav = r.iter().find(|r| r.id == "uav-01").unwrap();
        assert_eq!(uav.vars.get("ns").map(String::as_str), Some("swarm"));
    }

    #[test]
    fn swarm_vars_parse_from_toml() {
        let parsed = SwarmConfig::from_toml_str(
            r#"
[controller]
name = "t"
id_code = "s"
grpc_listen = "0.0.0.0:50051"

[robot_types.x]

[vars]
site = "lab-1"

[[robots]]
id = "r1"
type = "x"
"#,
        )
        .unwrap();
        assert_eq!(parsed.vars.get("site").map(String::as_str), Some("lab-1"));
    }
}
