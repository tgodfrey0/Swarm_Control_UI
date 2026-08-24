use std::path::PathBuf;

use anyhow::bail;
use clap::{Parser, Subcommand};

use swarmdeck_client::{Client, ClientError};
use swarmdeck_core::{ApiTargets, RunRobotStatus};

#[derive(Debug, Parser)]
#[command(name = "swarmdeck-cli", about = "SwarmDeck command-line client")]
struct Cli {
    /// Control host HTTP endpoint (the host's WebUI/API address).
    #[arg(long, global = true, default_value = "http://127.0.0.1:8080")]
    host: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List robots and their status.
    Status,
    /// Dispatch an action to robots. Targets: --all, --robots, --types, --name.
    Run {
        /// `<type>.<action>` (robot-type action, e.g. `turtlebot3.bringup`)
        /// or a bare swarm action name (e.g. `start_experiment`).
        action: String,
        #[arg(long)]
        all: bool,
        #[arg(long, value_delimiter = ',')]
        robots: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        types: Vec<String>,
        #[arg(long)]
        name: Option<String>,
        /// Skip the interactive confirmation for dangerous batch actions.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
        /// Run as a background action (doesn't block other dispatches on the robot).
        #[arg(long)]
        background: bool,
    },
    /// Stop every running action on the targeted robots.
    Stop {
        #[arg(long)]
        all: bool,
        #[arg(long, value_delimiter = ',')]
        robots: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        types: Vec<String>,
        #[arg(long)]
        name: Option<String>,
        /// Skip the interactive confirmation.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// List runs (or only running ones).
    Ps {
        /// Only show runs that are still running.
        #[arg(long)]
        running: bool,
    },
    /// Show the log tail for a robot.
    Logs {
        robot: String,
        #[arg(long, default_value_t = 100)]
        tail: usize,
        /// Stream new log lines live over the host's WebSocket until Ctrl-C.
        #[arg(long)]
        follow: bool,
    },
    /// Validate config files without starting anything.
    Config {
        /// Swarm config file (TOML).
        #[arg(long)]
        config: PathBuf,
        /// Directory of shared robot-type TOML files.
        #[arg(long, default_value = "robots")]
        robot_types: PathBuf,
    },
    /// SSH-provision the agent onto robots and install the systemd unit.
    Provision {
        /// Swarm config file (TOML).
        #[arg(long)]
        config: PathBuf,
        /// Directory of shared robot-type TOML files.
        #[arg(long, default_value = "robots")]
        robot_types: PathBuf,
        #[arg(long, value_delimiter = ',')]
        robots: Vec<String>,
        /// SSH user (defaults to current user).
        #[arg(long)]
        user: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let client = Client::new(&cli.host);

    match cli.command {
        Command::Status => cmd_status(&client).await?,
        Command::Run {
            action,
            all,
            robots,
            types,
            name,
            yes,
            json,
            background,
        } => {
            cmd_run(
                &client,
                action,
                target(all, robots, types, name)?,
                yes,
                json,
                background,
            )
            .await?
        }
        Command::Stop {
            all,
            robots,
            types,
            name,
            yes,
            json,
        } => cmd_stop(&client, target(all, robots, types, name)?, yes, json).await?,
        Command::Ps { running } => cmd_ps(&client, running).await?,
        Command::Logs {
            robot,
            tail,
            follow,
        } => cmd_logs(&client, &robot, tail, follow).await?,
        Command::Config {
            config,
            robot_types,
        } => {
            let types_dir = Some(robot_types);
            let cfg = swarmdeck_core::SwarmConfig::from_files(&config, types_dir.as_deref())?;
            println!(
                "config OK: {} controller, {} robot types, {} robots",
                cfg.controller.name,
                cfg.robot_types.len(),
                cfg.robots.len()
            );
        }
        Command::Provision {
            config,
            robot_types,
            robots,
            user,
        } => provision::provision(&config, Some(&robot_types), &robots, user.as_deref())?,
    }
    Ok(())
}

fn target(
    all: bool,
    robots: Vec<String>,
    types: Vec<String>,
    name: Option<String>,
) -> anyhow::Result<ApiTargets> {
    match (all, !robots.is_empty(), !types.is_empty(), name) {
        (true, false, false, None) => Ok(ApiTargets::All),
        (false, true, false, None) => Ok(ApiTargets::Robots(robots)),
        (false, false, true, None) => Ok(ApiTargets::Types(types)),
        (false, false, false, Some(n)) => Ok(ApiTargets::Name(n)),
        (false, false, false, None) => {
            bail!("no targets given; pass one of --all, --robots, --types, --name")
        }
        _ => bail!(
            "targets are mutually exclusive; pass exactly one of --all, --robots, --types, --name"
        ),
    }
}

async fn confirm(yes: bool, msg: &str) -> anyhow::Result<()> {
    if yes {
        return Ok(());
    }
    print!("{msg} [y/N] ");
    use std::io::Write;
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    if !line.trim().eq_ignore_ascii_case("y") {
        bail!("aborted");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Commands — thin shells over the backend API
// ---------------------------------------------------------------------------

async fn cmd_status(client: &Client) -> anyhow::Result<()> {
    let robots = client.robots().await?;
    if robots.is_empty() {
        println!("no robots");
        return Ok(());
    }
    println!(
        "{:<10} {:<16} {:<12} {:<9} {:<7}",
        "ID", "NAME", "TYPE", "STATE", "ACTIVE"
    );
    for r in &robots {
        let state = if r.connected {
            if r.simulated {
                "sim-online"
            } else {
                "online"
            }
        } else {
            "offline"
        };
        let active = r
            .active
            .as_ref()
            .map(|a| a.action_name.as_str())
            .unwrap_or("-");
        println!(
            "{:<10} {:<16} {:<12} {:<9} {}",
            r.id, r.name, r.kind, state, active
        );
    }
    Ok(())
}

async fn cmd_run(
    client: &Client,
    action: String,
    targets: ApiTargets,
    yes: bool,
    json: bool,
    background: bool,
) -> anyhow::Result<()> {
    let req = swarmdeck_core::RunRequest {
        action: action.clone(),
        targets,
        timeout_sec: None,
        confirm: false,
        background,
    };
    let resp = match client.dispatch(&req).await {
        Ok(resp) => resp,
        // The host only asks for confirmation when the action is dangerous and
        // hits more than one robot, so we only prompt then.
        Err(ClientError::ConfirmRequired { message, .. }) => {
            if !yes {
                println!("{message}");
                confirm(false, "run anyway?").await?;
            }
            let retry = swarmdeck_core::RunRequest {
                confirm: true,
                ..req
            };
            client.dispatch(&retry).await?
        }
        Err(e) => return Err(e.into()),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }
    println!("run {}", resp.run_id);
    for r in &resp.targeted {
        println!("  → {r}");
    }
    for r in &resp.busy {
        println!("  busy: {r}");
    }
    for r in &resp.offline {
        println!("  offline: {r}");
    }
    if resp.targeted.is_empty() && (resp.busy.is_empty() && resp.offline.is_empty()) {
        println!("  (no robots matched)");
    }
    Ok(())
}

async fn cmd_stop(
    client: &Client,
    targets: ApiTargets,
    yes: bool,
    json: bool,
) -> anyhow::Result<()> {
    if !yes && !matches!(targets, ApiTargets::Robots(ref v) if v.len() == 1) {
        let label = match &targets {
            ApiTargets::All => "all robots".to_string(),
            ApiTargets::Robots(v) => format!("{} robots", v.len()),
            ApiTargets::Types(v) => format!("all {} robots", v.join(",")),
            ApiTargets::Name(n) => format!("robots matching \"{n}\""),
        };
        confirm(false, &format!("stop every running action on {label}?")).await?;
    }
    let stopped = client.stop(&targets).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&stopped)?);
        return Ok(());
    }
    if stopped.is_empty() {
        println!("nothing running on the targeted robots");
    } else {
        println!("stopped: {}", stopped.join(", "));
    }
    Ok(())
}

async fn cmd_ps(client: &Client, running: bool) -> anyhow::Result<()> {
    let runs = client.runs().await?;
    let runs = if running {
        runs.into_iter()
            .filter(|r| {
                r.robots.iter().any(|(_, s)| {
                    matches!(s, RunRobotStatus::Queued | RunRobotStatus::Running { .. })
                })
            })
            .collect::<Vec<_>>()
    } else {
        runs
    };
    if runs.is_empty() {
        println!("no runs");
        return Ok(());
    }
    for r in runs {
        let summary: Vec<String> = r
            .robots
            .iter()
            .map(|(id, st)| {
                let label = match st {
                    RunRobotStatus::Queued => "queued",
                    RunRobotStatus::Running { .. } => "running",
                    RunRobotStatus::Done { killed, .. } => {
                        if *killed {
                            "stopped"
                        } else {
                            "done"
                        }
                    }
                    RunRobotStatus::Failed { .. } => "failed",
                };
                format!("{id}:{label}")
            })
            .collect();
        println!(
            "{:<8} {:<30} {}",
            &r.run_id[..8.min(r.run_id.len())],
            r.action,
            summary.join(" ")
        );
    }
    Ok(())
}

async fn cmd_logs(client: &Client, robot: &str, tail: usize, follow: bool) -> anyhow::Result<()> {
    let mut last_ts = 0u64;
    let lines = client.logs(robot, tail).await?;
    for l in &lines {
        last_ts = last_ts.max(l.ts_ms);
        print_line(l.stderr, &l.text);
    }
    if !follow {
        return Ok(());
    }
    loop {
        let mut stream = match client.subscribe().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("log stream disconnected ({e}); retrying in 2s…");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
        };
        use futures_util::StreamExt;
        while let Some(ev) = stream.next().await {
            match ev {
                Ok(swarmdeck_core::Event::Logs { robot: r, lines }) if r == robot => {
                    for l in lines {
                        if l.ts_ms > last_ts {
                            last_ts = l.ts_ms;
                            print_line(l.stderr, &l.text);
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("log stream error ({e}); reconnecting…");
                    break;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

fn print_line(stderr: bool, text: &str) {
    let mark = if stderr { "E" } else { " " };
    println!("{mark} {text}");
}

mod provision {
    //! SSH provisioning: push the agent binary + config to each robot and
    //! install/start the systemd unit. Implemented in cli/src/provision.rs.

    include!("provision.rs");
}
