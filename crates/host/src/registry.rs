//! Central registry of robot state. The gRPC service writes into it; the
//! dispatch engine, HTTP API and WebSocket read from it. All mutations push
//! events onto the bus so the UI/CLI stay live.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::{mpsc::UnboundedSender, Mutex, RwLock};
use tokio::time::{interval, MissedTickBehavior};

use swarmdeck_core::{
    ActiveView, ConfigError, Event, LogLine, Result, RobotConfig, RobotView, RunRobotStatus,
    SwarmConfig,
};

use crate::dispatch::RunStore;
use crate::events::EventBus;

/// Robots are considered offline after this much silence.
pub const STALE_AFTER_MS: u64 = 15_000;
const LOG_RING_MAX: usize = 2000;
const LOG_FLUSH_EVERY_MS: u64 = 250;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub struct Registry {
    pub config: RwLock<SwarmConfig>,
    pub robots: RwLock<BTreeMap<String, RobotEntry>>,
    pub actions_meta: RwLock<HashMap<String, ActionMeta>>,
    pub run_store: RunStore,
    pub events: EventBus,
    /// Path of the swarm file passed at startup (`--config`, else
    /// `{swarm}/swarm.toml`). Reloaded on SIGHUP.
    pub swarm_file: PathBuf,
    pub types_dir: Option<PathBuf>,
    pending_logs: Mutex<HashMap<String, Vec<LogLine>>>,
    session_seq: AtomicU64,
}

#[derive(Debug, Clone)]
pub struct ActionMeta {
    pub action_name: String,
    pub command: String,
}

#[derive(Clone)]
pub struct RobotEntry {
    pub robot_id: String,
    pub name: String,
    pub kind: String,
    pub simulated: bool,
    pub adopted: bool,
    pub address: Option<String>,
    pub connected: bool,
    pub connected_since_ms: u64,
    pub last_seen_ms: u64,
    pub agent_version: String,
    pub hostname: Option<String>,
    pub active_action_id: Option<String>,
    /// Background actions: action_id -> action_name. These don't count as
    /// "active" for concurrency but must be stoppable.
    pub background_actions: BTreeMap<String, String>,
    pub cmd_tx: Option<UnboundedSender<swarmdeck_proto::v1::Command>>,
    cmd_tx_seq: u64,
    pub logs: LogRing,
}

impl RobotEntry {
    fn new(id: String) -> Self {
        Self {
            robot_id: id.clone(),
            name: id,
            kind: String::new(),
            simulated: false,
            adopted: false,
            address: None,
            connected: false,
            connected_since_ms: 0,
            last_seen_ms: 0,
            agent_version: String::new(),
            hostname: None,
            active_action_id: None,
            background_actions: BTreeMap::new(),
            cmd_tx: None,
            cmd_tx_seq: 0,
            logs: LogRing::new(LOG_RING_MAX),
        }
    }
}

#[derive(Clone)]
pub struct LogRing {
    lines: VecDeque<LogLine>,
    max: usize,
}

impl LogRing {
    pub fn new(max: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            max,
        }
    }
    pub fn push(&mut self, line: LogLine) {
        if self.lines.len() >= self.max {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }
    pub fn tail(&self, n: usize) -> Vec<LogLine> {
        self.lines.iter().rev().take(n).rev().cloned().collect()
    }
}

impl Registry {
    pub fn new(cfg: SwarmConfig, swarm_file: PathBuf, types_dir: Option<PathBuf>) -> Arc<Self> {
        let reg = Arc::new(Self {
            config: RwLock::new(cfg),
            robots: RwLock::new(BTreeMap::new()),
            actions_meta: RwLock::new(HashMap::new()),
            run_store: RunStore::default(),
            events: EventBus::new(),
            swarm_file,
            types_dir,
            pending_logs: Mutex::new(HashMap::new()),
            session_seq: AtomicU64::new(1),
        });
        reg.spawn_log_flusher();
        reg.spawn_staleness_sweeper();
        reg
    }

    /// Background task that coalesces log chunks into batched `Event::Logs`
    /// broadcasts (prevents flooding the WebSocket).
    fn spawn_log_flusher(self: &Arc<Self>) {
        let reg = self.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(LOG_FLUSH_EVERY_MS));
            loop {
                ticker.tick().await;
                let drained = {
                    let mut pending = reg.pending_logs.lock().await;
                    if pending.is_empty() {
                        continue;
                    }
                    std::mem::take(&mut *pending)
                };
                for (robot, lines) in drained {
                    reg.events.publish(Event::Logs { robot, lines });
                }
            }
        });
    }

    /// Background task that flips robots online/offline when their heartbeats
    /// go stale (agent crashed/network dropped with no clean disconnect) and
    /// pushes the change to the UI over the WebSocket — no refresh needed.
    fn spawn_staleness_sweeper(self: &Arc<Self>) {
        let reg = self.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(5));
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let now = now_ms();
                let ids = {
                    let robots = reg.robots.read().await;
                    robots.keys().cloned().collect::<Vec<_>>()
                };
                for id in ids {
                    let state = {
                        let robots = reg.robots.read().await;
                        match robots.get(&id) {
                            // A session channel means the agent is (was)
                            // connected; connectedness then depends on freshness.
                            Some(e) if e.cmd_tx.is_some() => {
                                now.saturating_sub(e.last_seen_ms) < STALE_AFTER_MS
                            }
                            _ => continue,
                        }
                    };
                    let flip = {
                        let mut robots = reg.robots.write().await;
                        let e = robots.get_mut(&id).unwrap();
                        if e.connected != state {
                            e.connected = state;
                            if !state {
                                e.last_seen_ms = 0;
                            }
                            true
                        } else {
                            false
                        }
                    };
                    if flip {
                        tracing::info!(robot = %id, online = state, "robot connectivity changed");
                        reg.publish_robot(&id).await;
                    }
                }
            }
        });
    }

    /// Handle an incoming Report from a connected agent.
    pub async fn handle_report(&self, robot_id: &str, report: &swarmdeck_proto::v1::Report) {
        use swarmdeck_proto::v1::report::Report as M;
        let Some(msg) = &report.report else { return };
        let now = now_ms();

        match msg {
            M::Register(reg) => {
                {
                    let mut robots = self.robots.write().await;
                    let entry = robots
                        .entry(robot_id.to_string())
                        .or_insert_with(|| RobotEntry::new(robot_id.to_string()));
                    entry.connected = true;
                    entry.connected_since_ms = now;
                    entry.last_seen_ms = now;
                    entry.agent_version = reg.agent_version.clone();
                    entry.hostname = if reg.hostname.is_empty() {
                        None
                    } else {
                        Some(reg.hostname.clone())
                    };
                }
                self.publish_robot(robot_id).await;
            }
            M::Heartbeat(_) => {
                {
                    let mut robots = self.robots.write().await;
                    if let Some(entry) = robots.get_mut(robot_id) {
                        entry.last_seen_ms = now;
                    }
                }
                self.publish_robot(robot_id).await;
            }
            M::Status(s) => {
                {
                    let mut robots = self.robots.write().await;
                    if let Some(entry) = robots.get_mut(robot_id) {
                        entry.last_seen_ms = now;
                        entry.active_action_id = if s.active_action_id.is_empty() {
                            None
                        } else {
                            Some(s.active_action_id.clone())
                        };
                    }
                }
                self.publish_robot(robot_id).await;
            }
            M::Log(log) => {
                let line = LogLine {
                    ts_ms: now,
                    stderr: log.stderr,
                    text: String::from_utf8_lossy(&log.data).to_string(),
                };
                {
                    let mut robots = self.robots.write().await;
                    if let Some(entry) = robots.get_mut(robot_id) {
                        entry.logs.push(line.clone());
                    }
                }
                {
                    let mut pending = self.pending_logs.lock().await;
                    pending.entry(robot_id.to_string()).or_default().push(line);
                }
            }
            M::Result(res) => {
                {
                    let mut robots = self.robots.write().await;
                    if let Some(entry) = robots.get_mut(robot_id) {
                        if entry.active_action_id.as_deref() == Some(res.action_id.as_str()) {
                            entry.active_action_id = None;
                        }
                        entry.background_actions.remove(&res.action_id);
                    }
                }
                self.actions_meta.write().await.remove(&res.action_id);
                if let Some((run_id, robot)) = res.action_id.split_once(':') {
                    let status = if res.killed {
                        RunRobotStatus::Done {
                            exit_code: res.exit_code,
                            killed: true,
                            finished_ms: res.finished_ms,
                        }
                    } else if res.exit_code == 0 && res.error.is_empty() {
                        RunRobotStatus::Done {
                            exit_code: res.exit_code,
                            killed: false,
                            finished_ms: res.finished_ms,
                        }
                    } else {
                        RunRobotStatus::Failed {
                            error: if res.error.is_empty() {
                                format!("exit code {}", res.exit_code)
                            } else {
                                res.error.clone()
                            },
                        }
                    };
                    if let Some(run) = self.run_store.update_robot(run_id, robot, status).await {
                        self.events.publish(Event::Run { run });
                    }
                }
                self.publish_robot(robot_id).await;
            }
            M::Ack(ack) => {
                if !ack.accepted {
                    if let Some((run_id, robot)) = ack.action_id.split_once(':') {
                        if let Some(run) = self
                            .run_store
                            .update_robot(
                                run_id,
                                robot,
                                RunRobotStatus::Failed {
                                    error: ack.reason.clone(),
                                },
                            )
                            .await
                        {
                            self.events.publish(Event::Run { run });
                        }
                    }
                }
            }
        }
    }

    /// Called by the gRPC service when a session ends. Only clears the
    /// command channel if it belongs to this session (guards against a newer
    /// session for the same robot being clobbered by a stale disconnect).
    pub async fn disconnect(&self, robot_id: &str, seq: u64) {
        {
            let mut robots = self.robots.write().await;
            if let Some(entry) = robots.get_mut(robot_id) {
                if entry.cmd_tx_seq != seq {
                    return;
                }
                entry.connected = false;
                entry.last_seen_ms = 0;
                entry.active_action_id = None;
                entry.background_actions.clear();
                entry.cmd_tx = None;
            }
        }
        self.publish_robot(robot_id).await;
    }

    async fn publish_robot(&self, robot_id: &str) {
        self.events.publish(Event::Robot {
            robot: self.view(robot_id).await,
        });
    }

    /// Assign this session's command channel, returning the session token
    /// that `disconnect` must present to clear it.
    pub async fn set_cmd_tx(
        &self,
        robot_id: &str,
        tx: UnboundedSender<swarmdeck_proto::v1::Command>,
    ) -> u64 {
        let seq = self.session_seq.fetch_add(1, Ordering::Relaxed);
        let mut robots = self.robots.write().await;
        let entry = robots
            .entry(robot_id.to_string())
            .or_insert_with(|| RobotEntry::new(robot_id.to_string()));
        entry.cmd_tx = Some(tx);
        entry.cmd_tx_seq = seq;
        seq
    }

    /// Record that an action just started on a robot (dispatch engine calls
    /// this after a successful command send).
    pub async fn mark_action_started(
        &self,
        robot_id: &str,
        action_id: String,
        action_name: String,
    ) {
        {
            let mut robots = self.robots.write().await;
            if let Some(entry) = robots.get_mut(robot_id) {
                entry.active_action_id = Some(action_id.clone());
            }
        }
        self.actions_meta.write().await.insert(
            action_id,
            ActionMeta {
                action_name,
                command: String::new(),
            },
        );
        self.publish_robot(robot_id).await;
    }

    /// Record a background action on a robot (doesn't block other dispatches).
    pub async fn mark_background_action(
        &self,
        robot_id: &str,
        action_id: String,
        action_name: String,
    ) {
        {
            let mut robots = self.robots.write().await;
            if let Some(entry) = robots.get_mut(robot_id) {
                entry
                    .background_actions
                    .insert(action_id.clone(), action_name.clone());
            }
        }
        self.actions_meta.write().await.insert(
            action_id,
            ActionMeta {
                action_name,
                command: String::new(),
            },
        );
        self.publish_robot(robot_id).await;
    }

    pub async fn cmd_tx(
        &self,
        robot_id: &str,
    ) -> Option<UnboundedSender<swarmdeck_proto::v1::Command>> {
        self.robots
            .read()
            .await
            .get(robot_id)
            .and_then(|e| e.cmd_tx.clone())
    }

    /// Live projectable view of one robot.
    pub async fn view(&self, robot_id: &str) -> RobotView {
        let now = now_ms();
        let (entry, cfg_robot) = {
            let robots = self.robots.read().await;
            let cfg = self.config.read().await;
            (
                robots.get(robot_id).cloned(),
                cfg.robots.iter().find(|r| r.id == robot_id).cloned(),
            )
        };
        self.build_view(entry, cfg_robot, now).await
    }

    pub async fn all_views(&self) -> Vec<RobotView> {
        let now = now_ms();
        let (entries, cfg_robots) = {
            let robots = self.robots.read().await;
            let cfg = self.config.read().await;
            (robots.clone(), cfg.robots.clone())
        };
        let mut out = Vec::new();
        for (id, entry) in &entries {
            let cfg_robot = cfg_robots.iter().find(|r| &r.id == id).cloned();
            out.push(self.build_view(Some(entry.clone()), cfg_robot, now).await);
        }
        for r in &cfg_robots {
            if !entries.contains_key(&r.id) {
                out.push(self.build_view(None, Some(r.clone()), now).await);
            }
        }
        out
    }

    async fn build_view(
        &self,
        entry: Option<RobotEntry>,
        cfg_robot: Option<RobotConfig>,
        now: u64,
    ) -> RobotView {
        let entry = entry.unwrap_or_else(|| {
            RobotEntry::new(cfg_robot.as_ref().map(|r| r.id.clone()).unwrap_or_default())
        });
        let kind = if !entry.kind.is_empty() {
            entry.kind.clone()
        } else {
            cfg_robot
                .as_ref()
                .map(|r| r.kind.clone())
                .unwrap_or_default()
        };
        let connected =
            entry.connected && (now.saturating_sub(entry.last_seen_ms) < STALE_AFTER_MS);

        let active = if let Some(aid) = &entry.active_action_id {
            let meta = self.actions_meta.read().await.get(aid).cloned();
            meta.map(|m| ActiveView {
                action_id: aid.clone(),
                action_name: m.action_name,
                command: m.command,
                started_ms: 0,
            })
        } else {
            None
        };

        RobotView {
            id: entry.robot_id.clone(),
            name: if entry.adopted || cfg_robot.is_none() {
                entry.name.clone()
            } else {
                cfg_robot
                    .as_ref()
                    .map(|r| r.display_name().to_string())
                    .unwrap_or_else(|| entry.name.clone())
            },
            kind,
            address: cfg_robot
                .as_ref()
                .and_then(|r| r.address.clone())
                .or(entry.address.clone()),
            simulated: cfg_robot
                .as_ref()
                .map(|r| r.simulated)
                .unwrap_or(entry.simulated),
            adopted: entry.adopted,
            connected,
            agent_version: entry.agent_version.clone(),
            hostname: entry.hostname.clone(),
            last_seen_ms: if connected { entry.last_seen_ms } else { 0 },
            active,
        }
    }

    pub async fn logs(&self, robot_id: &str, tail: usize) -> Vec<LogLine> {
        self.robots
            .read()
            .await
            .get(robot_id)
            .map(|e| e.logs.tail(tail))
            .unwrap_or_default()
    }

    /// Adopt a robot that phoned home but isn't in the config file.
    pub async fn adopt(&self, id: &str, kind: &str, name: Option<&str>) -> Result<()> {
        {
            let cfg = self.config.read().await;
            if !cfg.robot_types.contains_key(kind) {
                return Err(ConfigError::UnknownRobotType {
                    robot: id.to_string(),
                    kind: kind.to_string(),
                });
            }
            if cfg.robots.iter().any(|r| r.id == id) {
                let first = cfg
                    .robots
                    .iter()
                    .find(|r| r.id == id)
                    .unwrap()
                    .display_name();
                return Err(ConfigError::DuplicateRobotId {
                    id: id.to_string(),
                    first: first.to_string(),
                });
            }
        }
        {
            let mut cfg = self.config.write().await;
            cfg.robots.push(RobotConfig {
                id: id.to_string(),
                name: name.map(|n| n.to_string()),
                kind: kind.to_string(),
                address: None,
                simulated: false,
                vars: Default::default(),
                env: Default::default(),
                adopted: true,
            });
        }
        {
            let mut robots = self.robots.write().await;
            if let Some(entry) = robots.get_mut(id) {
                entry.kind = kind.to_string();
                entry.name = name
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| id.to_string());
                entry.adopted = true;
            }
        }
        tracing::info!(robot = id, kind, "robot adopted");
        self.publish_robot(id).await;
        Ok(())
    }

    /// Release a previously adopted robot: remove it from the in-memory config
    /// and clear the adopted flag. Robots defined in the config file are not
    /// touched.
    pub async fn release(&self, id: &str) -> Result<()> {
        let exists = self.robots.read().await.contains_key(id);
        if !exists {
            return Err(ConfigError::UnknownRobot { id: id.to_string() });
        }
        {
            let mut cfg = self.config.write().await;
            cfg.robots.retain(|r| !(r.adopted && r.id == id));
        }
        {
            let mut robots = self.robots.write().await;
            if let Some(entry) = robots.get_mut(id) {
                entry.adopted = false;
                entry.kind.clear();
                entry.name = id.to_string();
            }
        }
        tracing::info!(robot = id, "robot released");
        self.publish_robot(id).await;
        Ok(())
    }

    /// SIGHUP: reload the swarm config from disk, honoring any `--config` /
    /// `--robot-types` overrides given at startup.
    pub async fn reload_config(&self) -> Result<()> {
        let cfg = SwarmConfig::from_files(&self.swarm_file, self.types_dir.as_deref())?;
        *self.config.write().await = cfg;
        self.events.publish(Event::Robots {
            robots: self.all_views().await,
        });
        tracing::info!("swarm config reloaded");
        Ok(())
    }
}
