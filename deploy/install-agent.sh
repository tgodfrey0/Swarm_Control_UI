#!/usr/bin/env bash
# install-agent.sh — Install the Swarmlink agent + systemd service on this machine.
#
# Usage:
#   ./deploy/install-agent.sh [OPTIONS]
#
# Options:
#   --bin <path>        Path to the swarmlink-agent binary (default: auto-detect from target/)
#   --config <path>     Path to agent.toml (default: /etc/swarm-agent/agent.toml)
#   --help              Show this help message
#
# This script:
#   1. Copies the agent binary to /opt/swarm-agent/
#   2. Copies the config to /etc/swarm-agent/agent.toml (if not already there)
#   3. Installs and enables the systemd service
#
# Logs are written to stdout (visible via journalctl) and to logs/<config-name>-<timestamp>.log
# in the working directory of the service (default: /opt/swarm-agent/).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

BIN_PATH=""
CONFIG_PATH=""

usage() {
    sed -n '2,/^set /{ s/^# \?//; p }' "$0"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --bin)   BIN_PATH="$2"; shift 2 ;;
        --config) CONFIG_PATH="$2"; shift 2 ;;
        --help)  usage ;;
        *)       echo "Unknown option: $1"; usage ;;
    esac
done

# --- Resolve the binary ----------------------------------------------------
if [[ -z "$BIN_PATH" ]]; then
    for candidate in \
        "$REPO_ROOT/bin/swarmlink-agent" \
        "$REPO_ROOT/target/release/swarmlink-agent" \
        "$REPO_ROOT/target/debug/swarmlink-agent"; do
        if [[ -x "$candidate" ]]; then
            BIN_PATH="$candidate"
            break
        fi
    done
    if [[ -z "$BIN_PATH" ]]; then
        echo "error: no swarmlink-agent binary found. Build first (just build) or pass --bin." >&2
        exit 1
    fi
fi

if [[ ! -x "$BIN_PATH" ]]; then
    echo "error: binary not executable: $BIN_PATH" >&2
    exit 1
fi

echo "Using binary: $BIN_PATH"

# --- Resolve the config -----------------------------------------------------
if [[ -z "$CONFIG_PATH" ]]; then
    CONFIG_PATH="/etc/swarm-agent/agent.toml"
fi

echo "Using config: $CONFIG_PATH"

# --- Install binary ---------------------------------------------------------
echo "Installing binary to /opt/swarm-agent/swarmlink-agent ..."
sudo mkdir -p /opt/swarm-agent
sudo install -m 0755 "$BIN_PATH" /opt/swarm-agent/swarmlink-agent

# --- Install config ---------------------------------------------------------
if [[ -f "$CONFIG_PATH" ]]; then
    echo "Config already exists at $CONFIG_PATH — skipping."
else
    echo "Config not found at $CONFIG_PATH — creating placeholder."
    sudo mkdir -p "$(dirname "$CONFIG_PATH")"
    sudo tee "$CONFIG_PATH" >/dev/null <<'EOF'
# Generate this file via the provisioner, or fill in manually:
#   robot_id   = "my-robot"
#   log_file   = "/tmp/swarm-agent.log"
#
#   [controller]
#   endpoint = "10.0.0.1:50051"
#   id_code  = "shared-secret"
EOF
    sudo chmod 0600 "$CONFIG_PATH"
    echo "NOTE: Edit $CONFIG_PATH with the correct robot_id, controller endpoint, and id_code."
fi

# --- Install systemd service ------------------------------------------------
echo "Installing systemd service ..."
sudo tee /etc/systemd/system/swarmlink-agent.service >/dev/null <<'EOF'
[Unit]
Description=Swarmlink robot agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/opt/swarm-agent/swarmlink-agent --config /etc/swarm-agent/agent.toml
WorkingDirectory=/opt/swarm-agent
Restart=always
RestartSec=3
StandardOutput=journal
StandardError=journal
SyslogIdentifier=swarmlink-agent

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable swarmlink-agent.service

echo ""
echo "Done. The service is installed but not started yet."
echo ""
echo "  Start now:      sudo systemctl start swarmlink-agent"
echo "  Check status:   sudo systemctl status swarmlink-agent"
echo "  View logs:      journalctl -u swarmlink-agent -f"
echo "  Logs on disk:   ls /opt/swarm-agent/logs/"
