//! gRPC client: phones home to the controller, registers, then multiplexes
//! runner events / heartbeats out and commands in over one bidirectional
//! stream. Reconnects with exponential backoff.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::time::{interval, sleep, Duration, MissedTickBehavior};
use tokio_stream::wrappers::ReceiverStream;
use tonic::Code;

use swarmdeck_core::AgentConfig;
use swarmdeck_proto::v1::report::Report as ReportMsg;
use swarmdeck_proto::v1::{
    agent_client::AgentClient, ActionAck, Command, Register, Report, Status as StatusProto,
};

use crate::procfs::Probe;
use crate::runner::{now_ms, Runner, RunnerEvent};

const HEARTBEAT_EVERY_MS: u64 = 5_000;
const REPORT_BUFFER: usize = 4096;

pub async fn run_forever(cfg: AgentConfig) -> anyhow::Result<()> {
    let runner = Arc::new(Runner::new());
    let mut backoff = Duration::from_secs(1);

    loop {
        match session_once(&cfg, runner.clone()).await {
            Ok(()) => tracing::info!("session ended; reconnecting"),
            Err(e) => tracing::warn!("session error: {e}"),
        }
        runner.detach_events();
        runner.kill_on_disconnect_all().await;
        sleep(backoff).await;
        if backoff < Duration::from_secs(30) {
            backoff *= 2;
        }
    }
}

async fn session_once(cfg: &AgentConfig, runner: Arc<Runner>) -> anyhow::Result<()> {
    let scheme = if cfg.controller.tls { "https" } else { "http" };
    let endpoint = format!("{scheme}://{}", cfg.controller.endpoint);
    let mut channel = tonic::transport::Channel::from_shared(endpoint)
        .map_err(|e| anyhow::anyhow!("invalid controller endpoint: {e}"))?;
    if cfg.controller.tls {
        let host = cfg
            .controller
            .endpoint
            .split(':')
            .next()
            .unwrap_or("localhost");
        let domain = cfg.controller.server_name.as_deref().unwrap_or(host);
        let mut tls = tonic::transport::ClientTlsConfig::new().domain_name(domain.to_string());
        if let Some(ca) = &cfg.controller.ca {
            let pem = std::fs::read(ca)?;
            tls = tls.ca_certificate(tonic::transport::Certificate::from_pem(pem));
        }
        channel = channel
            .tls_config(tls)
            .map_err(|e| anyhow::anyhow!("invalid TLS config: {e}"))?;
    }
    let channel = channel.connect().await?;
    let mut client = AgentClient::new(channel);

    let (report_tx, report_rx) = mpsc::channel::<Report>(REPORT_BUFFER);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<RunnerEvent>();

    runner.attach_events(event_tx);

    // First message must be the registration.
    report_tx
        .send(Report {
            report: Some(ReportMsg::Register(Register {
                robot_id: cfg.robot_id.clone(),
                id_code: cfg.controller.id_code.clone(),
                agent_version: env!("CARGO_PKG_VERSION").to_string(),
                hostname: hostname(),
                capabilities: Default::default(),
                name: cfg.name.clone().unwrap_or_default(),
            })),
        })
        .await
        .ok();

    let req_stream = ReceiverStream::new(report_rx);
    let response = client.session(req_stream).await?;
    let mut incoming = response.into_inner();

    let mut probe = Probe::default();
    let mut heartbeat = interval(Duration::from_millis(HEARTBEAT_EVERY_MS));
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    report_tx
        .send(status_report(&runner, &mut probe).await)
        .await
        .ok();

    tracing::info!(robot = %cfg.robot_id, "registered with controller");

    loop {
        tokio::select! {
            cmd = incoming.message() => {
                match cmd {
                    Ok(Some(c)) => handle_command(c, &runner, &report_tx).await,
                    Ok(None) => return Ok(()), // server closed the stream
                    Err(e) => {
                        if e.code() == Code::PermissionDenied {
                            anyhow::bail!("registration rejected (wrong id_code?): {e}");
                        }
                        return Err(e.into());
                    }
                }
            }
            ev = event_rx.recv() => {
                if let Some(ev) = ev {
                    forward_event(ev, &report_tx).await;
                }
            }
            _ = heartbeat.tick() => {
                if report_tx.send(status_report(&runner, &mut probe).await).await.is_err() {
                    return Ok(()); // server gone
                }
            }
        }
    }
}

async fn handle_command(cmd: Command, runner: &Runner, report_tx: &mpsc::Sender<Report>) {
    match cmd.command {
        Some(swarmdeck_proto::v1::command::Command::Run(run)) => {
            let action_id = run.action_id.clone();
            match runner.spawn(run).await {
                Ok(()) => {
                    tracing::info!(action_id, "action started");
                    send_ack(report_tx, action_id, true, String::new()).await;
                }
                Err(e) => {
                    tracing::error!(action_id, error = %e, "action spawn failed");
                    send_ack(report_tx, action_id, false, e.to_string()).await;
                }
            }
        }
        Some(swarmdeck_proto::v1::command::Command::Stop(stop)) => {
            tracing::info!(action_id = %stop.action_id, "stop requested");
            runner.kill(&stop.action_id).await;
        }
        Some(swarmdeck_proto::v1::command::Command::Ping(_)) => {
            let report = Report {
                report: Some(ReportMsg::Heartbeat(Default::default())),
            };
            let _ = report_tx.send(report).await;
        }
        None => {}
    }
}

async fn send_ack(tx: &mpsc::Sender<Report>, action_id: String, accepted: bool, reason: String) {
    let report = Report {
        report: Some(ReportMsg::Ack(ActionAck {
            action_id,
            accepted,
            reason,
        })),
    };
    let _ = tx.send(report).await;
}

async fn forward_event(ev: RunnerEvent, tx: &mpsc::Sender<Report>) {
    match ev {
        RunnerEvent::Log {
            action_id,
            stderr,
            line,
        } => {
            let report = Report {
                report: Some(ReportMsg::Log(swarmdeck_proto::v1::ActionLog {
                    action_id,
                    seq: 0,
                    data: line.into_bytes(),
                    stderr,
                })),
            };
            // Logs are the only high-volume event: drop under backpressure
            // rather than stalling the process pipes.
            if tx.try_send(report).is_err() {
                tracing::debug!("log chunk dropped (backpressure)");
            }
        }
        RunnerEvent::Done {
            action_id,
            exit_code,
            killed,
            error,
            started_ms,
            finished_ms,
        } => {
            let report = Report {
                report: Some(ReportMsg::Result(swarmdeck_proto::v1::ActionResult {
                    action_id,
                    exit_code: exit_code.unwrap_or(1),
                    killed,
                    error: error.unwrap_or_default(),
                    started_ms,
                    finished_ms,
                })),
            };
            if tx.send(report).await.is_err() {
                tracing::warn!("failed to send action result");
            }
        }
    }
}

async fn status_report(runner: &Runner, probe: &mut Probe) -> Report {
    let metrics = probe.sample();
    let (active_action_id, _) = runner.active_action().await.unwrap_or_default();
    let status = StatusProto {
        timestamp_ms: now_ms(),
        active_action_id,
        cpu_usage: metrics.cpu_usage,
        memory_used_kb: metrics.memory_used_kb,
        uptime_sec: metrics.uptime_sec,
        battery_percent: metrics.battery_percent,
    };
    Report {
        report: Some(ReportMsg::Status(status)),
    }
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}
