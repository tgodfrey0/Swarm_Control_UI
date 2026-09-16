#!/usr/bin/env bash
# install-agent.sh — Install the Swarmlink agent + systemd service on this machine.
#
# Usage:
#   ./deploy/install-agent.sh [OPTIONS]
#
# Options:
#   --bin <path>         Path to the swarmlink-agent binary (default: auto-detect from target/)
#   --config <path>      Path to agent.toml (default: /etc/swarm-agent/agent.toml)
#   --robot-id <id>      Set the robot identity
#   --name <name>        Set the robot name (display name)
#   --type <type>        Set the robot type (e.g. "sim", "uav")
#   --address <addr>     Set the SSH endpoint for provisioning
#   --simulated <bool>   Mark this robot as simulated (true|false)
#   --var KEY=VALUE      Add an entry to [vars]; repeatable
#   --env KEY=VALUE      Add an entry to [env]; repeatable
#   --endpoint <addr>    Controller endpoint for [controller]
#   --id-code <secret>   Shared secret for [controller]
#   --tls <bool>         Connect to the controller over TLS (true|false)
#   --ca <path>          PEM CA to trust for the controller's certificate
#   --server-name <name> Override the TLS server name
#   --force              Overwrite the config even if it already exists
#   --help               Show this help message
#
# Any of the robot/controller fields above override the config file given by
# --config (or the default path): existing values are merged, then overrides
# are applied on top.
#
# This script:
#   1. Copies the agent binary to /opt/swarm-agent/
#   2. Writes /etc/swarm-agent/agent.toml (placeholder, or the merged result
#      of existing config + the field overrides passed above)
#   3. Installs and enables the systemd service
#
# Logs are written to stdout (visible via journalctl) and to logs/<config-name>-<timestamp>.log
# in the working directory of the service (default: /opt/swarm-agent/).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

BIN_PATH=""
CONFIG_PATH=""
FORCE=0

# CLI overrides requested on the command line (applied after the existing
# config is read, so they win).
ARG_robot_id=""
ARG_name=""
ARG_type=""
ARG_address=""
ARG_simulated=""
ARG_endpoint=""
ARG_id_code=""
ARG_tls=""
ARG_ca=""
ARG_server_name=""
declare -A ARG_vars=()
declare -A ARG_env=()

# Config values after merging the existing file with the CLI overrides.
CFG_robot_id=""
CFG_name=""
CFG_type=""
CFG_address=""
CFG_simulated=""
CFG_endpoint=""
CFG_id_code=""
CFG_tls=""
CFG_ca=""
CFG_server_name=""
declare -A CFG_vars=()
declare -A CFG_env=()

usage() {
    sed -n '2,/^set /{ s/^# \?//; p }' "$0"
    exit 0
}

# Normalise an on/off-ish value to "true"/"false" (empty -> empty).
normalise_bool() {
    case "$1" in
        true|1|on|yes)  echo "true" ;;
        false|0|off|no) echo "false" ;;
        "") echo "" ;;
        *) echo "error: expected a boolean, got '$1'" >&2; exit 1 ;;
    esac
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --bin)        BIN_PATH="$2"; shift 2 ;;
        --config)     CONFIG_PATH="$2"; shift 2 ;;
        --force)      FORCE=1; shift ;;
        --robot-id)   ARG_robot_id="$2"; shift 2 ;;
        --name)       ARG_name="$2"; shift 2 ;;
        --type)       ARG_type="$2"; shift 2 ;;
        --address)    ARG_address="$2"; shift 2 ;;
        --simulated)  ARG_simulated="$(normalise_bool "$2")"; shift 2 ;;
        --var)        ARG_vars["${2%%=*}"]="${2#*=}"; shift 2 ;;
        --env)        ARG_env["${2%%=*}"]="${2#*=}"; shift 2 ;;
        --endpoint)   ARG_endpoint="$2"; shift 2 ;;
        --id-code)    ARG_id_code="$2"; shift 2 ;;
        --tls)        ARG_tls="$(normalise_bool "$2")"; shift 2 ;;
        --ca)         ARG_ca="$2"; shift 2 ;;
        --server-name) ARG_server_name="$2"; shift 2 ;;
        --help)       usage ;;
        *)            echo "Unknown option: $1"; usage ;;
    esac
done

toml_quote() {
    local s="$1"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    printf '"%s"' "$s"
}

# Read the known robot/controller fields out of an existing agent.toml.
# Since `set -e` applies, guard failures: a file we cannot read (or parse)
# is treated as "no existing values".
read_config() {
    local path="$1" section="top" line key raw val
    while IFS= read -r line; do
        line="${line%%#*}"                    # strip trailing comment
        line="${line#"${line%%[![:space:]]*}"}"  # trim leading whitespace
        [[ -z "$line" ]] && continue
        if [[ "$line" =~ ^\[([^]]+)\] ]]; then
            section="${BASH_REMATCH[1]}"
            continue
        fi
        [[ "$line" =~ ^[[:space:]]*([A-Za-z0-9_.-]+)[[:space:]]*=[[:space:]]*(.*)$ ]] || continue
        key="${BASH_REMATCH[1]}"
        raw="${BASH_REMATCH[2]}"
        if [[ "$raw" =~ ^\"(.*)\"$ ]]; then
            val="${BASH_REMATCH[1]}"
        elif [[ "$raw" =~ ^'(.*)'$ ]]; then
            val="${BASH_REMATCH[1]}"
        else
            val="${raw%%#*}"
            val="${val#"${val%%[![:space:]]*}"}"
            val="${val%"${val##*[![:space:]]}"}"
        fi
        case "$section" in
            vars) CFG_vars["$key"]="$val" ;;
            env)  CFG_env["$key"]="$val" ;;
            controller)
                case "$key" in
                    endpoint)    CFG_endpoint="$val" ;;
                    id_code)     CFG_id_code="$val" ;;
                    tls)         CFG_tls="$val" ;;
                    ca)          CFG_ca="$val" ;;
                    server_name) CFG_server_name="$val" ;;
                esac ;;
            *)
                case "$key" in
                    robot_id)    CFG_robot_id="$val" ;;
                    name)        CFG_name="$val" ;;
                    type)        CFG_type="$val" ;;
                    address)     CFG_address="$val" ;;
                    simulated)   CFG_simulated="$val" ;;
                esac ;;
        esac
    done < <(cat "$path" 2>/dev/null || true)
}

write_config() {
    local path="$1"
    {
        if [[ -n "$CFG_robot_id" ]]; then echo "robot_id = $(toml_quote "$CFG_robot_id")"; fi
        if [[ -n "$CFG_name" ]]; then echo "name = $(toml_quote "$CFG_name")"; fi
        if [[ -n "$CFG_type" ]]; then echo "type = $(toml_quote "$CFG_type")"; fi
        if [[ -n "$CFG_address" ]]; then echo "address = $(toml_quote "$CFG_address")"; fi
        if [[ -n "$CFG_simulated" ]]; then echo "simulated = $CFG_simulated"; fi

        if [[ ${#CFG_vars[@]} -gt 0 ]]; then
            echo ""
            echo "[vars]"
            {
                for k in "${!CFG_vars[@]}"; do
                    printf '%s = %s\n' "$k" "$(toml_quote "${CFG_vars[$k]}")"
                done
            } | sort
        fi

        if [[ ${#CFG_env[@]} -gt 0 ]]; then
            echo ""
            echo "[env]"
            {
                for k in "${!CFG_env[@]}"; do
                    printf '%s = %s\n' "$k" "$(toml_quote "${CFG_env[$k]}")"
                done
            } | sort
        fi

        if [[ -n "$CFG_endpoint" || -n "$CFG_id_code" ]]; then
            echo ""
            echo "[controller]"
            if [[ -n "$CFG_endpoint" ]]; then echo "endpoint = $(toml_quote "$CFG_endpoint")"; fi
            if [[ -n "$CFG_id_code" ]]; then echo "id_code = $(toml_quote "$CFG_id_code")"; fi
            if [[ -n "$CFG_tls" ]]; then
                echo "tls = $CFG_tls"
            else
                echo "tls = false"
            fi
            if [[ -n "$CFG_ca" ]]; then echo "ca = $(toml_quote "$CFG_ca")"; fi
            if [[ -n "$CFG_server_name" ]]; then echo "server_name = $(toml_quote "$CFG_server_name")"; fi
        fi
    } > "$path"
}

have_overrides() {
    [[ -n "$ARG_robot_id" || -n "$ARG_name" || -n "$ARG_type" || -n "$ARG_address" \
        || -n "$ARG_simulated" || -n "$ARG_endpoint" || -n "$ARG_id_code" \
        || -n "$ARG_ca" || -n "$ARG_server_name" \
        || ${#ARG_vars[@]} -gt 0 || ${#ARG_env[@]} -gt 0 ]]
}

# Copy the CLI overrides onto the merged config (they win over the file).
apply_overrides() {
    [[ -z "$ARG_robot_id" ]]   || CFG_robot_id="$ARG_robot_id"
    [[ -z "$ARG_name" ]]       || CFG_name="$ARG_name"
    [[ -z "$ARG_type" ]]       || CFG_type="$ARG_type"
    [[ -z "$ARG_address" ]]    || CFG_address="$ARG_address"
    [[ -z "$ARG_simulated" ]]  || CFG_simulated="$ARG_simulated"
    [[ -z "$ARG_endpoint" ]]   || CFG_endpoint="$ARG_endpoint"
    [[ -z "$ARG_id_code" ]]    || CFG_id_code="$ARG_id_code"
    [[ -z "$ARG_tls" ]]        || CFG_tls="$ARG_tls"
    [[ -z "$ARG_ca" ]]         || CFG_ca="$ARG_ca"
    [[ -z "$ARG_server_name" ]] || CFG_server_name="$ARG_server_name"
    for k in "${!ARG_vars[@]}"; do CFG_vars["$k"]="${ARG_vars[$k]}"; done
    for k in "${!ARG_env[@]}"; do CFG_env["$k"]="${ARG_env[$k]}"; done
}

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

# --- Resolve the config ----------------------------------------------------
if [[ -z "$CONFIG_PATH" ]]; then
    CONFIG_PATH="/etc/swarm-agent/agent.toml"
fi

echo "Using config: $CONFIG_PATH"

# --- Install binary ---------------------------------------------------------
echo "Installing binary to /opt/swarm-agent/swarmlink-agent ..."
sudo mkdir -p /opt/swarm-agent
sudo install -m 0755 "$BIN_PATH" /opt/swarm-agent/swarmlink-agent

# --- Write the config -------------------------------------------------------
TMP_CONFIG="$(mktemp)"

if [[ -f "$CONFIG_PATH" ]]; then
    read_config "$CONFIG_PATH"
    apply_overrides

    if ! have_overrides && [[ "$FORCE" -eq 0 ]]; then
        echo "Config already exists at $CONFIG_PATH — skipping (pass --force to overwrite)."
    else
        echo "Config exists at $CONFIG_PATH — merging $(if [[ "$FORCE" -eq 0 ]]; then echo "overrides"; else echo "(--force: overwriting)"; fi)."
        write_config "$TMP_CONFIG"
    fi
else
    if ! have_overrides; then
        echo "Config not found at $CONFIG_PATH — creating placeholder."
        cat > "$TMP_CONFIG" <<'EOF'
# Generate this file via the provisioner, or fill in manually.
# See configs/ and README.md for an example agent.toml.
EOF
    else
        echo "Config not found at $CONFIG_PATH — creating with the values passed above."
        apply_overrides
        write_config "$TMP_CONFIG"
    fi
fi

if [[ -s "$TMP_CONFIG" ]]; then
    sudo mkdir -p "$(dirname "$CONFIG_PATH")"
    sudo install -m 0600 "$TMP_CONFIG" "$CONFIG_PATH"
    echo "Wrote config to $CONFIG_PATH"
fi
rm -f "$TMP_CONFIG"

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
echo "Done. The service is installed and enabled."
if [[ -n "$CFG_robot_id" || -n "$CFG_endpoint" || -n "$CFG_id_code" ]]; then
    echo "Explicit robot_id / endpoint / id_code were applied to the config."
else
    echo "NOTE: The config may still need a robot_id, controller endpoint, and id_code."
    echo "  Re-run with --robot-id, --endpoint, --id-code to set them, or edit $CONFIG_PATH."
fi
echo ""
echo "  Start now:      sudo systemctl start swarmlink-agent"
echo "  Check status:   sudo systemctl status swarmlink-agent"
echo "  View logs:      journalctl -u swarmlink-agent -f"
echo "  Logs on disk:   ls /opt/swarm-agent/logs/"