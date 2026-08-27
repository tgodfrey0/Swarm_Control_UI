// SSH provisioning: copies the agent binary + config to each robot and
// installs a systemd unit. Uses the system `ssh`/`scp` binaries.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context};

use swarmlink_core::{RobotConfig, SwarmConfig};

pub fn provision(
    swarm_file: &Path,
    robot_types: Option<&Path>,
    only: &[String],
    ssh_user: Option<&str>,
) -> anyhow::Result<()> {
    let cfg = SwarmConfig::from_files(swarm_file, robot_types)?;

    if cfg.controller.grpc_listen.port() == 0 {
        bail!("controller grpc_listen is not set in the swarm config");
    }

    // Derive the endpoint agents should dial from the CLI --host, or the
    // controller's configured listener (falling back to a local address).
    let controller = std::env::var("SWARMLINK_CONTROLLER_ENDPOINT").ok().unwrap_or_else(|| {
        // The agent config needs a routable host:port. Prefer a Tailscale
        // style address on the listener if present, otherwise the loopback.
        let ip = cfg.controller.grpc_listen.ip();
        let ip = if ip.is_unspecified() {
            std::env::var("SWARMLINK_CONTROLLER_IP")
                .unwrap_or_else(|_| "127.0.0.1".to_string())
        } else {
            ip.to_string()
        };
        format!("{ip}:{}", cfg.controller.grpc_listen.port())
    });

    let binary = std::env::var("SWARMLINK_AGENT_BIN")
        .unwrap_or_else(|_| "bin/swarmlink-agent-aarch64".to_string());

    let mut targets: Vec<&RobotConfig> = cfg
        .robots
        .iter()
        .filter(|r| !r.simulated && r.address.is_some())
        .collect();
    if !only.is_empty() {
        targets.retain(|r| only.contains(&r.id));
        let known: Vec<&str> = targets.iter().map(|r| r.id.as_str()).collect();
        for id in only {
            if !known.contains(&id.as_str()) {
                bail!("robot '{id}' has no address in the swarm config (or doesn't exist)");
            }
        }
    }

    if targets.is_empty() {
        bail!("no provisionable robots (with an address) matched");
    }

    let user = ssh_user.unwrap_or("root");

    for robot in &targets {
        let host = robot.address.as_deref().unwrap();
        let hoststr = format!("{user}@{host}");
        println!("── provisioning {robot_id} ({host})", robot_id = robot.id);

        // 1. Agent binary.
        run_ssh(&hoststr, "mkdir -p /opt/swarm-agent /etc/swarm-agent /tmp/.swarmlink")?;
        run_scp(&binary, &format!("{hoststr}:/tmp/.swarmlink/swarmlink-agent"))?;

        // 2. agent.toml (robot_id + controller endpoint + shared secret).
        let agent_toml = format!(
            "robot_id = {:?}\n\n[controller]\nendpoint = {:?}\nid_code = {:?}\ntls = false\n",
            robot.id, controller, cfg.controller.id_code
        );
        let script = format!(
            "cat > /tmp/.swarmlink/agent.toml <<'EOF'\n{agent_toml}EOF\n\
             install -m 0755 /tmp/.swarmlink/swarmlink-agent /opt/swarm-agent/swarmlink-agent\n\
             install -m 0600 /tmp/.swarmlink/agent.toml /etc/swarm-agent/agent.toml\n"
        );
        run_ssh(&hoststr, &script)?;

        // 3. systemd unit + start.
        let unit = include_str!("swarmlink-agent.service");
        let script = format!(
            "cat > /etc/systemd/system/swarmlink-agent.service <<'EOF'\n{unit}EOF\n\
             systemctl daemon-reload\n\
             systemctl enable --now swarmlink-agent.service\n\
             systemctl --no-pager --lines=5 status swarmlink-agent.service || true\n"
        );
        run_ssh(&hoststr, &script)?;
    }

    println!(
        "provisioning complete: {} robots pointed at {controller}",
        targets.len()
    );
    Ok(())
}

fn run_ssh(host: &str, script: &str) -> anyhow::Result<()> {
    use std::io::Write;

    let mut child = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=accept-new", host])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .context("failed to run ssh")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .context("writing script to ssh stdin")?;
    }
    let status = child.wait().context("waiting for ssh")?;
    if !status.success() {
        bail!("ssh to {host} failed");
    }
    Ok(())
}

fn run_scp(src: &str, dst: &str) -> anyhow::Result<()> {
    let status = Command::new("scp")
        .args(["-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=accept-new", src, dst])
        .status()
        .context("running scp")?;
    if !status.success() {
        bail!("scp failed: {src} → {dst}");
    }
    Ok(())
}
