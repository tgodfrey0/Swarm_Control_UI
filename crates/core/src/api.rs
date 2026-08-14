//! JSON types shared between the control host's HTTP/WS API and the CLI.
//! Serialized with serde; the WebUI JS and `swarmdeck-cli` both consume these.

use serde::{Deserialize, Serialize};

/// Batch target selector. Resolved against the swarm by the host.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApiTargets {
    #[default]
    All,
    Robots(Vec<String>),
    Types(Vec<String>),
    /// Substring/regex matched against robot id and name.
    Name(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    /// `<type>.<action>`, e.g. `turtlebot3.bringup`.
    pub action: String,
    pub targets: ApiTargets,
    /// Overrides the action's configured timeout (0 = none).
    #[serde(default)]
    pub timeout_sec: Option<u64>,
    /// Client asserts the operator confirmed the batch (required for
    /// `dangerous` actions targeting more than one robot).
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResponse {
    pub run_id: String,
    pub action: String,
    pub targeted: Vec<String>,
    /// Robots that were skipped because another action is running there.
    pub busy: Vec<String>,
    /// Robots that were skipped because they are offline.
    pub offline: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopRequest {
    pub targets: ApiTargets,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptRequest {
    pub kind: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// Dispatchable actions served to the WebUI/CLI: robot-type actions as
/// `"<type>.<action>"` refs, plus swarm-level `[actions]` by bare name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionsView {
    pub robot_type: Vec<String>,
    pub swarm: Vec<String>,
}

/// Summary of the loaded swarm config, served to UI/CLI clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigView {
    pub controller: String,
    pub robot_types: Vec<String>,
    pub robot_count: usize,
    pub grpc_listen: String,
    pub ui_bind: String,
}

/// Live snapshot of one robot, as served to UI/CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RobotView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub address: Option<String>,
    pub simulated: bool,
    pub adopted: bool,
    pub connected: bool,
    pub agent_version: String,
    pub hostname: Option<String>,
    pub last_seen_ms: u64,
    pub active: Option<ActiveView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveView {
    pub action_id: String,
    pub action_name: String,
    pub command: String,
    pub started_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum RunRobotStatus {
    Queued,
    Running {
        action_id: String,
        started_ms: u64,
    },
    Done {
        exit_code: u32,
        killed: bool,
        finished_ms: u64,
    },
    Failed {
        error: String,
    },
}

impl RunRobotStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            RunRobotStatus::Done { .. } | RunRobotStatus::Failed { .. }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunView {
    pub run_id: String,
    pub action: String,
    pub created_ms: u64,
    pub robots: Vec<(String, RunRobotStatus)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub ts_ms: u64,
    pub stderr: bool,
    pub text: String,
}

/// Events pushed over `WS /api/ws`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Robots { robots: Vec<RobotView> },
    Robot { robot: RobotView },
    Runs { runs: Vec<RunView> },
    Run { run: RunView },
    Logs { robot: String, lines: Vec<LogLine> },
}

/// Parse a `<type>.<action>` reference.
pub fn parse_action_ref(s: &str) -> Option<(&str, &str)> {
    let (ty, action) = s.split_once('.')?;
    if ty.is_empty() || action.is_empty() {
        return None;
    }
    Some((ty, action))
}
