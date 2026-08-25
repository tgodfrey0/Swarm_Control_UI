use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use crate::api::ApiTargets;
use crate::error::{ConfigError, Result};

/// Top-level swarm configuration read by the control host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmConfig {
    pub controller: ControllerConfig,
    #[serde(default)]
    pub robot_types: BTreeMap<String, RobotTypeConfig>,
    /// Swarm-level actions: dispatched by bare name (no `<type>.` prefix) to
    /// any robot, with `{{robot_id}}` / `{{vars.*}}` substituted per robot.
    #[serde(default)]
    pub actions: BTreeMap<String, ActionConfig>,
    /// Swarm-wide default variables, inherited by every robot. A robot's own
    /// `vars` entry wins per key.
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    /// Named workflows: multi-step sequences composed from existing actions.
    #[serde(default)]
    pub workflows: BTreeMap<String, WorkflowConfig>,
    #[serde(default)]
    pub robots: Vec<RobotConfig>,
}

impl SwarmConfig {
    pub fn from_toml_str(s: &str) -> Result<Self> {
        let cfg: Self = toml::from_str(s).map_err(ConfigError::Toml)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Load a swarm's config from a directory containing `{dir}/swarm.toml`.
    /// Robot types are shared across swarms and passed separately via
    /// [`Self::from_files`]. Kept separate from machine/system config
    /// (agent.toml, systemd units, install paths).
    pub fn from_swarm_dir(dir: &Path) -> Result<Self> {
        Self::from_files(&dir.join("swarm.toml"), None)
    }

    /// Load a swarm file, then merge any `robot_types` from `*.toml` files in
    /// `types_dir` (if given). Robot types are shared across swarms, so they
    /// live in their own directory; entries in the swarm file win on conflict.
    pub fn from_files(swarm: &Path, types_dir: Option<&Path>) -> Result<Self> {
        let text = std::fs::read_to_string(swarm)?;
        let mut cfg: Self = toml::from_str(&text).map_err(ConfigError::Toml)?;
        if let Some(dir) = types_dir {
            if !dir.exists() {
                return Err(ConfigError::MissingTypesDir {
                    path: dir.display().to_string(),
                });
            }
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                    continue;
                }
                let types: RobotTypesFile =
                    toml::from_str(&std::fs::read_to_string(&path)?).map_err(ConfigError::Toml)?;
                for (name, ty) in types.robot_types {
                    cfg.robot_types.entry(name).or_insert(ty);
                }
            }
        }
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        let mut seen = BTreeMap::<&str, &str>::new();
        for robot in &self.robots {
            if !self.robot_types.contains_key(&robot.kind) {
                return Err(ConfigError::UnknownRobotType {
                    robot: robot.id.clone(),
                    kind: robot.kind.clone(),
                });
            }
            if let Some(prev) = seen.insert(&robot.id, robot.display_name()) {
                return Err(ConfigError::DuplicateRobotId {
                    id: robot.id.clone(),
                    first: prev.to_string(),
                });
            }
        }
        for name in self.actions.keys() {
            if name.contains('.') || name.is_empty() {
                return Err(ConfigError::BadSwarmActionName {
                    action: name.clone(),
                });
            }
        }
        // Validate that every workflow step references an existing action.
        for (wf_name, wf) in &self.workflows {
            for (i, step) in wf.steps.iter().enumerate() {
                if !self.action_exists(&step.action) {
                    return Err(ConfigError::UnknownWorkflowAction {
                        workflow: wf_name.clone(),
                        step: i + 1,
                        action: step.action.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Check whether an action reference resolves to an existing action.
    fn action_exists(&self, action: &str) -> bool {
        if let Some((ty, name)) = action.split_once('.') {
            self.robot_types
                .get(ty)
                .is_some_and(|t| t.actions.contains_key(name))
        } else {
            self.actions.contains_key(action)
        }
    }

    /// Merge an unclaimed robot (phoned home with a valid id_code but not yet
    /// in the swarm config) into a runtime view. Returns the merged config.
    pub fn with_unclaimed(&self, id: &str, hostname: Option<&str>) -> Self {
        let mut clone = self.clone();
        if clone.robots.iter().any(|r| r.id == id) {
            return clone;
        }
        let robot = RobotConfig {
            id: id.to_string(),
            name: Some(
                hostname
                    .map(|h| h.to_string())
                    .unwrap_or_else(|| id.to_string()),
            ),
            kind: String::new(), // unknown until adopted
            address: None,
            simulated: false,
            vars: BTreeMap::new(),
            env: BTreeMap::new(),
            adopted: false,
        };
        clone.robots.push(robot);
        clone
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerConfig {
    pub name: String,
    /// Shared secret presented by agents at registration. Keeps controllers
    /// on the same LAN from stealing each other's robots.
    pub id_code: String,
    pub grpc_listen: SocketAddr,
    #[serde(default = "default_ui_bind")]
    pub ui_bind: SocketAddr,
    #[serde(default)]
    pub tls: Option<TlsConfig>,
}

fn default_ui_bind() -> SocketAddr {
    "0.0.0.0:8080".parse().unwrap()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    pub cert: PathBuf,
    pub key: PathBuf,
    /// Optional CA bundle for client certificates (mTLS).
    pub ca: Option<PathBuf>,
}

/// A `robot_types/*.toml` file: contains only a `robot_types` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RobotTypesFile {
    pub robot_types: BTreeMap<String, RobotTypeConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RobotTypeConfig {
    pub display_name: Option<String>,
    #[serde(default)]
    pub actions: BTreeMap<String, ActionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionConfig {
    /// Shell command; may contain `{{...}}` template placeholders resolved by
    /// the host per robot (see `crate::template`).
    pub command: String,
    #[serde(default)]
    pub timeout_sec: Option<u64>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Actions that need explicit confirmation when dispatched to a batch.
    #[serde(default)]
    pub dangerous: bool,
    /// Maximum number of concurrent invocations of this action per robot.
    /// The default (1) prevents launching competing controllers on one robot.
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    /// Background actions don't count as "active" — another action can be
    /// dispatched while this one runs (e.g. a bringup that must stay up).
    #[serde(default)]
    pub background: bool,
}

fn default_concurrency() -> usize {
    1
}

// ---------------------------------------------------------------------------
// Workflows
// ---------------------------------------------------------------------------

/// A named sequence of actions dispatched across the swarm.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowConfig {
    pub description: Option<String>,
    pub steps: Vec<WorkflowStep>,
    /// Workflow-level default for failure handling (overridden per-step).
    #[serde(default)]
    pub on_failure: WorkflowOnFailure,
}

/// One step inside a [`WorkflowConfig`]. The `action` must reference an
/// existing standalone action (either `<type>.<action>` or a swarm action name).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStep {
    pub action: String,
    pub targets: ApiTargets,
    /// `true` = `;` semantics: run the next step regardless of this step's outcome.
    /// `false` / absent = `&&` semantics: abort the workflow on failure.
    #[serde(default)]
    pub continue_on_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowOnFailure {
    #[default]
    Abort,
    Continue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RobotConfig {
    /// Registration identity. Unique across the swarm.
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Reference to a key in `robot_types` (TOML key: `type`).
    #[serde(rename = "type")]
    pub kind: String,
    /// SSH endpoint used by the provisioner. Omitted robots still appear once
    /// they phone home with a valid id_code.
    #[serde(default)]
    pub address: Option<String>,
    /// True for agents that run on this host (e.g. Gazebo/ROS2 sim nodes).
    /// The provisioner skips them; `swarmdeck-cli sim` spawns them locally.
    #[serde(default)]
    pub simulated: bool,
    /// Per-robot values available as `{{vars.<key>}}` in action commands.
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    /// Extra environment applied to every action process on this robot.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// True when the robot was not defined in the config file but phoned home
    /// and was adopted. Not serialized back to disk.
    #[serde(default, skip_serializing)]
    pub adopted: bool,
}

impl RobotConfig {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// Runtime configuration written to `/etc/swarm-agent/agent.toml` by the
/// provisioner and read by `swarmdeck-agent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    /// Robot identity. Set in the per-agent TOML (a small file that `extends`
    /// a shared base config); the agent rejects an empty id at startup.
    #[serde(default)]
    pub robot_id: String,
    /// Optional human-readable name supplied by the agent. Sent to the host
    /// at registration; the swarm TOML's per-robot `name` takes precedence
    /// for pre-defined robots.
    #[serde(default)]
    pub name: Option<String>,
    pub controller: AgentControllerConfig,
    /// Robot-local environment inherited by every spawned action process.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

const DEFAULT_GRPC_PORT: &str = "50051";

impl AgentConfig {
    pub fn from_toml_path(path: &Path) -> Result<Self> {
        let value = Self::load_merged(path, &mut Vec::new())?;
        let mut cfg: AgentConfig = value.try_into().map_err(ConfigError::Toml)?;
        if !cfg.controller.endpoint.contains(':') {
            cfg.controller
                .endpoint
                .push_str(&format!(":{DEFAULT_GRPC_PORT}"));
        }
        Ok(cfg)
    }

    /// Reject an empty id (a base config may omit `robot_id`; the per-agent
    /// file or the `--robot-id` flag must supply one before the agent runs).
    pub fn validate(&self) -> Result<()> {
        if self.robot_id.trim().is_empty() {
            return Err(ConfigError::MissingRobotId);
        }
        Ok(())
    }

    /// Load a config file, following an optional `extends = "<file>"` chain
    /// (path relative to the referencing file). Tables are merged key-by-key,
    /// so a per-agent file can override single fields (e.g. just `robot_id`)
    /// while inheriting everything else from the generic config; scalars and
    /// arrays are replaced wholesale.
    fn load_merged(path: &Path, seen: &mut Vec<PathBuf>) -> Result<toml::Value> {
        let canon = path.canonicalize()?;
        if seen.contains(&canon) {
            return Err(ConfigError::ConfigCycle {
                path: canon.display().to_string(),
            });
        }
        seen.push(canon);
        let text = std::fs::read_to_string(path)?;
        let mut value: toml::Value = toml::from_str(&text).map_err(ConfigError::Toml)?;
        let extends = value
            .as_table()
            .and_then(|t| t.get("extends"))
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        if let Some(base) = extends {
            value
                .as_table_mut()
                .expect("extends read from a table above")
                .remove("extends");
            let base_path = path.parent().unwrap_or(Path::new(".")).join(base);
            let mut merged = Self::load_merged(&base_path, seen)?;
            merge_toml(&mut merged, &value);
            return Ok(merged);
        }
        Ok(value)
    }
}

/// Deep-merge `over` into `base`: tables recursively, everything else replaced.
fn merge_toml(base: &mut toml::Value, over: &toml::Value) {
    match (base, over) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(bv) => merge_toml(bv, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, o) => *b = o.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write files under a unique temp dir and load the entry point.
    fn load(files: &[(&str, &str)], entry: &str) -> Result<AgentConfig> {
        let dir = std::env::temp_dir().join(format!(
            "swarmdeck-agent-cfg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir)?;
        for (name, contents) in files {
            std::fs::write(dir.join(name), contents)?;
        }
        AgentConfig::from_toml_path(&dir.join(entry))
    }

    #[test]
    fn extends_overrides_only_given_fields() {
        let cfg = load(
            &[
                (
                    "base.toml",
                    "[controller]\nendpoint = \"10.0.0.1\"\nid_code = \"s3cret\"\n",
                ),
                (
                    "child.toml",
                    "extends = \"base.toml\"\nrobot_id = \"r-1\"\n",
                ),
            ],
            "child.toml",
        )
        .unwrap();
        assert_eq!(cfg.robot_id, "r-1");
        assert_eq!(cfg.controller.endpoint, "10.0.0.1:50051");
        assert_eq!(cfg.controller.id_code, "s3cret");
    }

    #[test]
    fn child_wins_over_base() {
        let cfg = load(
            &[
                (
                    "base.toml",
                    "robot_id = \"generic\"\n[controller]\nendpoint = \"10.0.0.1\"\nid_code = \"s3cret\"\n",
                ),
                (
                    "child.toml",
                    "extends = \"base.toml\"\nrobot_id = \"r-2\"\n[controller]\nendpoint = \"127.0.0.1\"\n",
                ),
            ],
            "child.toml",
        )
        .unwrap();
        assert_eq!(cfg.robot_id, "r-2");
        assert_eq!(cfg.controller.endpoint, "127.0.0.1:50051");
        assert_eq!(cfg.controller.id_code, "s3cret");
    }

    #[test]
    fn missing_required_field_still_errors() {
        // Loading succeeds (the CLI may still supply --robot-id), but the
        // agent must refuse to run with an empty id.
        let cfg = load(
            &[
                (
                    "base.toml",
                    "[controller]\nendpoint = \"10.0.0.1\"\nid_code = \"s3cret\"\n",
                ),
                ("child.toml", "extends = \"base.toml\"\n"),
            ],
            "child.toml",
        )
        .unwrap();
        let err = cfg.validate().unwrap_err();
        assert!(err.to_string().contains("robot_id"), "{err}");
    }

    #[test]
    fn whitespace_only_robot_id_is_rejected() {
        let cfg = load(
            &[(
                "solo.toml",
                "robot_id = \"   \"\n[controller]\nendpoint = \"10.0.0.1\"\nid_code = \"s3cret\"\n",
            )],
            "solo.toml",
        )
        .unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn extends_cycle_is_rejected() {
        let err = load(
            &[
                ("a.toml", "extends = \"b.toml\"\nrobot_id = \"r\"\n"),
                ("b.toml", "extends = \"a.toml\"\n"),
            ],
            "a.toml",
        )
        .unwrap_err();
        assert!(err.to_string().contains("cycle"), "{err}");
    }

    #[test]
    fn no_extends_works_as_before() {
        let cfg = load(
            &[(
                "solo.toml",
                "robot_id = \"r-3\"\n[controller]\nendpoint = \"10.0.0.1\"\nid_code = \"s3cret\"\n",
            )],
            "solo.toml",
        )
        .unwrap();
        assert_eq!(cfg.robot_id, "r-3");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentControllerConfig {
    /// e.g. "100.64.0.1:50051". The agent phones home here and reconnects.
    pub endpoint: String,
    pub id_code: String,
    /// Connect over TLS (host must be configured with `[controller.tls]`).
    #[serde(default)]
    pub tls: bool,
    /// Optional PEM CA to trust for the controller's certificate. When unset,
    /// the system/webpki root store is used.
    #[serde(default)]
    pub ca: Option<PathBuf>,
    /// Override the TLS server name (defaults to the endpoint host).
    #[serde(default)]
    pub server_name: Option<String>,
}
