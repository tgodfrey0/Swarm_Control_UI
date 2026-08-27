//! Dispatch engine: turns a `RunRequest`/`StopRequest` into per-robot
//! gRPC commands, tracks batch lifecycle in the `RunStore`, and enforces
//! dangerous-action confirmation and per-robot concurrency limits.

use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};

use swarmlink_core::{
    resolve_command, ApiTargets, ConfigError, Event, Result as CoreResult, RunRequest, RunResponse,
    RunRobotStatus, RunView, WorkflowOnFailure, WorkflowRunInfo,
};
use swarmlink_proto::v1::{command::Command as CommandMsg, Command, RunAction, StopAction};

use crate::registry::{now_ms, Registry};

/// In-memory store of batch runs (a batch = one action fanned out to N robots).
#[derive(Clone)]
pub struct RunStore {
    inner: Arc<RwLock<BTreeMap<String, RunGroup>>>,
    step_done: broadcast::Sender<()>,
}

impl Default for RunStore {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self {
            inner: Arc::new(RwLock::new(BTreeMap::new())),
            step_done: tx,
        }
    }
}

pub struct RunGroup {
    pub run_id: String,
    pub action: String,
    pub created_ms: u64,
    pub robots: BTreeMap<String, RunRobotStatus>,
    /// Present when this run is part of a workflow.
    pub workflow_name: Option<String>,
    pub current_step: usize,
    pub total_steps: usize,
    pub step_run_ids: Vec<String>,
    pub current_step_action: String,
}

impl RunGroup {
    pub fn view(&self) -> RunView {
        let workflow = self.workflow_name.as_ref().map(|name| WorkflowRunInfo {
            workflow_name: name.clone(),
            current_step: self.current_step,
            total_steps: self.total_steps,
            step_action: self.current_step_action.clone(),
            step_run_id: self.step_run_ids.last().cloned().unwrap_or_default(),
        });
        RunView {
            run_id: self.run_id.clone(),
            action: self.action.clone(),
            created_ms: self.created_ms,
            robots: self
                .robots
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            workflow,
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
        let is_terminal = status.is_terminal();
        let mut inner = self.inner.write().await;
        let group = inner.get_mut(run_id)?;
        group.robots.insert(robot.to_string(), status);
        let view = group.view();
        drop(inner);
        if is_terminal {
            let _ = self.step_done.send(());
        }
        Some(view)
    }

    /// Wait until all robots in the given run reach a terminal state.
    /// Returns immediately if the run has no robots (nothing to wait for).
    pub async fn wait_for_terminal(&self, run_id: &str) {
        // Fast path: if the run doesn't exist or has no robots, return
        // immediately — there's nothing to wait for.
        {
            let inner = self.inner.read().await;
            if let Some(group) = inner.get(run_id) {
                if group.robots.is_empty() {
                    return;
                }
            } else {
                return;
            }
        }
        let mut recv = self.step_done.subscribe();
        loop {
            {
                let inner = self.inner.read().await;
                if let Some(group) = inner.get(run_id) {
                    let all_done = group.robots.values().all(|s| s.is_terminal());
                    if all_done {
                        return;
                    }
                } else {
                    return;
                }
            }
            let _ = recv.recv().await;
        }
    }

    /// Update the workflow progress fields on a run.
    pub async fn update_workflow_step(
        &self,
        run_id: &str,
        current_step: usize,
        step_action: &str,
        step_run_id: &str,
    ) -> Option<RunView> {
        let mut inner = self.inner.write().await;
        let group = inner.get_mut(run_id)?;
        group.current_step = current_step;
        group.current_step_action = step_action.to_string();
        group.step_run_ids.push(step_run_id.to_string());
        Some(group.view())
    }

    /// Clear all run history.
    pub async fn clear(&self) {
        self.inner.write().await.clear();
    }
}

/// High-level runner over the registry's robot command channels.
#[derive(Clone)]
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

        let resolved = swarmlink_core::resolve(&cfg, &req.action, &req.targets, &known)?;

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
            workflow_name: None,
            current_step: 0,
            total_steps: 0,
            step_run_ids: Vec::new(),
            current_step_action: String::new(),
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
    pub async fn stop(&self, req: swarmlink_core::StopRequest) -> CoreResult<Vec<String>> {
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
        let robots = swarmlink_core::select_robots(&cfg, &req.targets, &known)?;

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

    /// Dispatch a named workflow: execute its steps sequentially, waiting for
    /// all targeted robots to finish each step before proceeding to the next.
    pub async fn run_workflow(&self, name: &str, confirm: bool) -> CoreResult<RunResponse> {
        let cfg = self.registry.config.read().await.clone();

        // Resolve the workflow: either swarm-level or type-level (e.g. "sim-uav.deploy").
        let (workflow, is_type_wf) = if let Some((ty, wf_name)) = name.split_once('.') {
            let ty_cfg = cfg.robot_types.get(ty).ok_or_else(|| {
                ConfigError::UnknownWorkflow {
                    name: name.to_string(),
                }
            })?;
            let wf = ty_cfg.workflows.get(wf_name).ok_or_else(|| {
                ConfigError::UnknownWorkflow {
                    name: name.to_string(),
                }
            })?;
            (wf.clone(), true)
        } else {
            let wf = cfg.workflows.get(name).ok_or_else(|| {
                ConfigError::UnknownWorkflow {
                    name: name.to_string(),
                }
            })?;
            (wf.clone(), false)
        };

        // Pre-flight: if any step targets a dangerous action, the entire
        // workflow requires confirmation.
        if !confirm {
            for (i, step) in workflow.steps.iter().enumerate() {
                let is_dangerous = if let Some((ty, act)) = step.action.split_once('.') {
                    cfg.robot_types
                        .get(ty)
                        .and_then(|t| t.actions.get(act))
                        .is_some_and(|a| a.dangerous)
                } else {
                    cfg.actions
                        .get(step.action.as_str())
                        .is_some_and(|a| a.dangerous)
                };
                if is_dangerous {
                    return Err(ConfigError::ConfirmRequired {
                        action: format!(
                            "workflow:{name} (step {} — {} is dangerous)",
                            i + 1,
                            step.action
                        ),
                        count: 1,
                    });
                }
            }
        }

        let run_id = uuid::Uuid::new_v4().simple().to_string();
        let total_steps = workflow.steps.len();

        let group = RunGroup {
            run_id: run_id.clone(),
            action: name.to_string(),
            created_ms: now_ms(),
            robots: BTreeMap::new(),
            workflow_name: Some(name.to_string()),
            current_step: 0,
            total_steps,
            step_run_ids: Vec::new(),
            current_step_action: String::new(),
        };
        self.registry.run_store.insert(group).await;
        self.registry.events.publish(Event::Runs {
            runs: self.registry.run_store.recent(20).await,
        });

        let wf_name = name.to_string();
        let wf_name_spawn = wf_name.clone();
        let wf_name_type = wf_name.clone();

        // Spawn a background task that drives the workflow steps sequentially.
        let dispatcher = self.clone();
        let registry = self.registry.clone();
        let steps = workflow.steps.clone();
        let on_failure = workflow.on_failure.clone();
        let wf_run_id = run_id.clone();

        tokio::spawn(async move {
            let mut all_steps_done = true;
            'steps: for (i, step) in steps.iter().enumerate() {
                // Resolve targets: explicit on the step, or implicit (all robots of the type).
                let targets = match &step.targets {
                    Some(t) => t.clone(),
                    None if is_type_wf => {
                        let ty = wf_name_type.split_once('.').map(|(t, _)| t).unwrap_or("");
                        ApiTargets::Types(vec![ty.to_string()])
                    }
                    _ => {
                        tracing::warn!(
                            workflow = %wf_name_spawn,
                            step = i + 1,
                            "swarm workflow step missing targets"
                        );
                        continue;
                    }
                };
                let req = RunRequest {
                    action: step.action.clone(),
                    targets,
                    confirm,
                    timeout_sec: None,
                    background: false,
                };

                let resp = match dispatcher.run(req).await {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(
                            workflow = %wf_name_type,
                            step = i + 1,
                            error = %e,
                            "workflow step dispatch failed"
                        );
                        all_steps_done = false;
                        if on_failure == WorkflowOnFailure::Abort {
                            break 'steps;
                        }
                        continue;
                    }
                };

                tracing::info!(
                    workflow = %wf_name_type,
                    step = i + 1,
                    total = total_steps,
                    action = %resp.action,
                    targeted = resp.targeted.len(),
                    busy = resp.busy.len(),
                    offline = resp.offline.len(),
                    "workflow step dispatched"
                );

                if resp.targeted.is_empty() {
                    tracing::warn!(
                        workflow = %wf_name_type,
                        step = i + 1,
                        busy = ?resp.busy,
                        offline = ?resp.offline,
                        "workflow step has no targeted robots — skipping"
                    );
                    all_steps_done = false;
                    if on_failure == WorkflowOnFailure::Abort {
                        break 'steps;
                    }
                    continue;
                }

                // Update workflow progress with the step's action and run_id.
                if let Some(run) = registry
                    .run_store
                    .update_workflow_step(
                        &wf_run_id,
                        i + 1,
                        &resp.action,
                        &resp.run_id,
                    )
                    .await
                {
                    registry.events.publish(Event::Run { run });
                }

                // Wait for all robots targeted by this step to finish.
                // The sub-run's run_id tracks this step's batch.
                registry.run_store.wait_for_terminal(&resp.run_id).await;

                // Check if any robot failed in this step.
                let step_failed = {
                    let inner = registry.run_store.inner.read().await;
                    inner
                        .get(&resp.run_id)
                        .map(|g| {
                            g.robots
                                .values()
                                .any(|s| matches!(s, RunRobotStatus::Failed { .. }))
                        })
                        .unwrap_or(false)
                };

                if step_failed && !step.continue_on_error
                    && on_failure == WorkflowOnFailure::Abort
                {
                    all_steps_done = false;
                    break 'steps;
                }
            }

            // Mark workflow as complete only if every step ran; on abort
            // we leave current_step at the last successfully completed step.
            if all_steps_done {
                if let Some(_run) = registry
                    .run_store
                    .update_workflow_step(&wf_run_id, total_steps, "", "")
                    .await
                {
                    registry.events.publish(Event::Runs {
                        runs: registry.run_store.recent(20).await,
                    });
                }
            }
        });

        Ok(RunResponse {
            run_id,
            action: name.to_string(),
            targeted: Vec::new(),
            busy: Vec::new(),
            offline: Vec::new(),
        })
    }
}
