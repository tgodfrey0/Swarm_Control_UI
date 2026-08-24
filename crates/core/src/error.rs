use thiserror::Error;

pub type Result<T> = std::result::Result<T, ConfigError>;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to parse TOML: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("robot '{robot}' references unknown robot type '{kind}'")]
    UnknownRobotType { robot: String, kind: String },

    #[error("duplicate robot id '{id}' (also used by '{first}')")]
    DuplicateRobotId { id: String, first: String },

    #[error("robot '{robot}' is not defined in the swarm and has no type; adopt it first")]
    UnadoptedRobot { robot: String },

    #[error("unknown robot id '{id}'")]
    UnknownRobot { id: String },

    #[error("action reference must be '<type>.<action>' or a swarm action name")]
    BadActionRef,

    #[error("swarm action name must not contain '.' (got '{action}')")]
    BadSwarmActionName { action: String },

    #[error("unknown action '{action}' for robot type '{kind}'")]
    UnknownAction { kind: String, action: String },

    #[error("unknown swarm action '{action}'")]
    UnknownSwarmAction { action: String },

    #[error("action '{action}' targets {count} robots and is flagged dangerous; confirm with confirm=true")]
    ConfirmRequired { action: String, count: usize },

    #[error("agent config `extends` cycle at '{path}'")]
    ConfigCycle { path: String },

    #[error(
        "agent config is missing `robot_id` (set it in the per-agent TOML or pass --robot-id)"
    )]
    MissingRobotId,
}

#[derive(Debug, Error)]
pub enum TemplateError {
    #[error("unknown placeholder '{name}' in command template")]
    UnknownPlaceholder { name: String },

    #[error("unclosed placeholder in template")]
    Unclosed,
}
