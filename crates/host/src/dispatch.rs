//! Dispatch engine: turns a `RunRequest`/`StopRequest` into per-robot
//! gRPC commands, tracks batch lifecycle in the `RunStore`, and enforces
//! dangerous-action confirmation and per-robot concurrency limits.

use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use swarmdeck_core::{
    resolve_command, ConfigError, Event, Result as CoreResult, RunRequest, RunResponse,
    RunRobotStatus, RunView,
};
use swarmdeck_proto::v1::{command::Command as CommandMsg, Command, RunAction, StopAction};

use crate::registry::{now_ms, Registry};

/// In-memory store of batch runs (a batch = one action fanned out to N robots).
#[derive(Clone, Default)]
pub struct RunStore {
    inner: Arc<RwLock<BTreeMap<String, RunGroup>>>,
}

pub struct RunGroup {
    pub run_id: String,
    pub action: String,
    pub created_ms: u64,
    pub robots: BTreeMap<String, RunRobotStatus>,
}

impl RunGroup {
    pub fn view(&self) -> RunView {
        RunView {
            run_id: self.run_id.clone(),
            action: self.action.clone(),
            created_ms: self.created_ms,
            robots: self
                .robots
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }
}

impl RunStore {
    pub async fn insert(&self, group: RunGroup) {
        self.inner.write().await.insert(group.run_id.clone(), group);
    }

    /// Most recent runs, newest first.
    pub async fn recent(&self, n: usize) -> Vec<RunView> {
        let mut all: Vec<RunView> = self
            .inner
            .read()
            .await
            .values()
            .map(RunGroup::view)
            .collect();
        all.sort_by_key(|r| std::cmp::Reverse(r.created_ms));
        all.truncate(n);
        all
    }

    /// Single run by id.
    pub async fn get(&self, run_id: &str) -> Option<RunView> {
        self.inner.read().await.get(run_id).map(RunGroup::view)
    }

    /// Update one robot's status within a run; returns the refreshed run view.
    pub async fn update_robot(
        &self,
        run_id: &str,
        robot: &str,
        status: RunRobotStatus,
    ) -> Option<RunView> {
        let mut inner = self.inner.write().await;
        let group = inner.get_mut(run_id)?;
        group.robots.insert(robot.to_string(), status);
        Some(group.view())
    }
}

/// High-level runner over the registry's robot command channels.
pub struct Dispatcher {
    pub registry: Arc<Registry>,
}

impl Dispatcher {
    pub fn new(registry: Arc<Registry>) -> Arc<Self> {
        Arc::new(Self { registry })
    }

    pub async fn run(&self, req: RunRequest) -> CoreResult<RunResponse> {
        let (cfg, known) = {
            let cfg = self.registry.config.read().await;
            let known = self
                .registry
                .robots
                .read()
                .await
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            (cfg.clone(), known)
        };

        let resolved = swarmdeck_core::resolve(&cfg, &req.action, &req.targets, &known)?;

        if resolved.dangerous && resolved.robots.len() > 1 && !req.confirm {
            return Err(ConfigError::ConfirmRequired {
                action: resolved.action_name.clone(),
                count: resolved.robots.len(),
            });
        }

        let run_id = uuid::Uuid::new_v4().simple().to_string();
        let timeout_sec = req.timeout_sec.or(resolved.action.timeout_sec);
        let mut targeted = Vec::new();
        let mut busy = Vec::new();
        let mut offline = Vec::new();
        let mut group = RunGroup {
            run_id: run_id.clone(),
            action: resolved.action_name.clone(),
            created_ms: now_ms(),
            robots: BTreeMap::new(),
        };

        let background = req.background || resolved.action.background;

        for robot in &resolved.robots {
            let view = self.registry.view(&robot.id).await;
            if !view.connected {
                offline.push(robot.id.clone());
                continue;
            }
            if !background && view.active.is_some() {
                busy.push(robot.id.clone());
                continue;
            }

            let spec = resolve_command(
                &resolved.action.command,
                robot,
                timeout_sec,
                &resolved.action.env,
                resolved.action.cwd.clone(),
            )
            .map_err(|e| ConfigError::UnknownAction {
                kind: robot.kind.clone(),
                action: e.to_string(),
            })?;

            let action_id = format!("{run_id}:{}", robot.id);
            let run_cmd = Command {
                command: Some(CommandMsg::Run(RunAction {
                    action_id: action_id.clone(),
                    action_name: resolved.action_name.clone(),
                    command: spec.command,
                    env: spec.env.into_iter().collect(),
                    cwd: spec.cwd.unwrap_or_default(),
                    timeout_sec: spec.timeout_sec.unwrap_or(0) as u32,
                    kill_on_disconnect: true,
                    background,
                })),
            };

            match self.registry.cmd_tx(&robot.id).await {
                Some(tx) if tx.send(run_cmd).is_ok() => {
                    // Background actions don't count as "active" — another
                    // action can run concurrently on the same robot.
                    if background {
                        self.registry
                            .mark_background_action(
                                &robot.id,
                                action_id.clone(),
                                resolved.action_name.clone(),
                            )
                            .await;
                    } else {
                        self.registry
                            .mark_action_started(
                                &robot.id,
                                action_id.clone(),
                                resolved.action_name.clone(),
                            )
                            .await;
                    }
                    group.robots.insert(
                        robot.id.clone(),
                        RunRobotStatus::Running {
                            action_id: action_id.clone(),
                            started_ms: now_ms(),
                        },
                    );
                    targeted.push(robot.id.clone());
                }
                _ => offline.push(robot.id.clone()),
            }
        }

        self.registry.run_store.insert(group).await;
        self.registry.events.publish(Event::Runs {
            runs: self.registry.run_store.recent(20).await,
        });

        Ok(RunResponse {
            run_id,
            action: resolved.action_name,
            targeted,
            busy,
            offline,
        })
    }

    /// Send a stop command to every targeted robot that is running something.
    pub async fn stop(&self, req: swarmdeck_core::StopRequest) -> CoreResult<Vec<String>> {
        let (cfg, known) = {
            let cfg = self.registry.config.read().await;
            let known = self
                .registry
                .robots
                .read()
                .await
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            (cfg.clone(), known)
        };
        let robots = swarmdeck_core::select_robots(&cfg, &req.targets, &known)?;

        let mut stopped = Vec::new();
        for robot in &robots {
            let view = self.registry.view(&robot.id).await;
            if let Some(active) = view.active {
                let cmd = Command {
                    command: Some(CommandMsg::Stop(StopAction {
                        action_id: active.action_id,
                    })),
                };
                if self
                    .registry
                    .cmd_tx(&robot.id)
                    .await
                    .map(|t| t.send(cmd).is_ok())
                    .unwrap_or(false)
                {
                    stopped.push(robot.id.clone());
                }
            } else {
                // Check for background actions on this robot.
                let bg_ids: Vec<String> = {
                    let robots = self.registry.robots.read().await;
                    robots
                        .get(&robot.id)
                        .map(|e| e.background_actions.keys().cloned().collect())
                        .unwrap_or_default()
                };
                for action_id in bg_ids {
                    let cmd = Command {
                        command: Some(CommandMsg::Stop(StopAction {
                            action_id: action_id.clone(),
                        })),
                    };
                    if self
                        .registry
                        .cmd_tx(&robot.id)
                        .await
                        .map(|t| t.send(cmd).is_ok())
                        .unwrap_or(false)
                    {
                        stopped.push(robot.id.clone());
                    }
                }
            }
        }
        Ok(stopped)
    }

    pub async fn adopt(&self, id: &str, kind: &str, name: Option<&str>) -> CoreResult<()> {
        self.registry.adopt(id, kind, name).await
    }

    /// Release an adopted robot (see `Registry::release`).
    pub async fn release(&self, id: &str) -> CoreResult<()> {
        self.registry.release(id).await
    }
}
