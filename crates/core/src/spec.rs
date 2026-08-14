use std::collections::BTreeMap;

/// A fully-resolved action invocation ready to send to one robot's agent.
/// Produced by `crate::template::resolve_command` / the dispatch engine.
#[derive(Debug, Clone)]
pub struct RunSpec {
    pub command: String,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub timeout_sec: Option<u64>,
}
