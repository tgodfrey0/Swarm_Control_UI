use crate::config::RobotConfig;
use crate::error::TemplateError;
use crate::spec::RunSpec;

/// Resolve `{{placeholders}}` in an action command using robot-specific data.
///
/// Supported placeholders:
/// - `{{robot_id}}`, `{{robot_name}}`, `{{robot_type}}`, `{{address}}`
/// - `{{vars.<key>}}` for keys in `RobotConfig::vars`
///
/// Double braces avoid collisions with shell `${VAR}` expansion and brace
/// ranges like `{1..3}`.
pub fn resolve_command(
    template: &str,
    robot: &RobotConfig,
    timeout_sec: Option<u64>,
    extra_env: &std::collections::BTreeMap<String, String>,
    cwd: Option<String>,
) -> std::result::Result<RunSpec, TemplateError> {
    let mut out = String::with_capacity(template.len());
    let rest = template.as_bytes();
    let mut i = 0;

    while i < rest.len() {
        if rest[i] != b'{' || i + 1 >= rest.len() || rest[i + 1] != b'{' {
            out.push(rest[i] as char);
            i += 1;
            continue;
        }
        // found "{{"
        let close = template[i + 2..]
            .find("}}")
            .ok_or(TemplateError::Unclosed)?;
        let expr = &template[i + 2..i + 2 + close];
        let value = lookup(expr, robot)?;
        out.push_str(value);
        i += 2 + close + 2;
    }

    let mut env = robot.env.clone();
    for (k, v) in extra_env {
        env.insert(k.clone(), v.clone());
    }

    Ok(RunSpec {
        command: out,
        env,
        cwd,
        timeout_sec,
    })
}

fn lookup<'a>(expr: &str, robot: &'a RobotConfig) -> std::result::Result<&'a str, TemplateError> {
    match expr.trim() {
        "robot_id" => Ok(&robot.id),
        "robot_name" => Ok(robot.display_name()),
        "robot_type" => Ok(&robot.kind),
        "address" => Ok(robot.address.as_deref().unwrap_or("")),
        e if e.starts_with("vars.") => {
            let key = &e["vars.".len()..];
            robot.vars.get(key).map(String::as_str).ok_or_else(|| {
                TemplateError::UnknownPlaceholder {
                    name: key.to_string(),
                }
            })
        }
        other => Err(TemplateError::UnknownPlaceholder {
            name: other.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn robot() -> RobotConfig {
        RobotConfig {
            id: "tb-01".into(),
            name: Some("turtlebot-1".into()),
            kind: "turtlebot3".into(),
            address: Some("10.0.0.21".into()),
            simulated: false,
            vars: BTreeMap::from([("ns".into(), "tb01".into())]),
            env: BTreeMap::new(),
            adopted: false,
        }
    }

    #[test]
    fn resolves_placeholders() {
        let spec = resolve_command(
            "ros2 launch x ns:={{vars.ns}} id:={{robot_id}}",
            &robot(),
            None,
            &BTreeMap::new(),
            None,
        )
        .unwrap();
        assert_eq!(spec.command, "ros2 launch x ns:=tb01 id:=tb-01");
    }

    #[test]
    fn leaves_shell_braces_alone() {
        let spec = resolve_command(
            "echo ${HOME} {1..3}",
            &robot(),
            None,
            &BTreeMap::new(),
            None,
        )
        .unwrap();
        assert_eq!(spec.command, "echo ${HOME} {1..3}");
    }

    #[test]
    fn unknown_var_is_error() {
        let err =
            resolve_command("x {{vars.nope}}", &robot(), None, &BTreeMap::new(), None).unwrap_err();
        assert!(matches!(err, TemplateError::UnknownPlaceholder { .. }));
    }
}
