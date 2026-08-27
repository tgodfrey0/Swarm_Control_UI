# Security

## Overview

Swarmlink is designed for trusted lab environments. This page covers security considerations and hardening options.

## Authentication

### `id_code`

A shared secret in `swarm.toml` that isolates controllers on the same LAN:

```toml
[controller]
id_code = "lab1-swarm-secret"
```

- Robots present this on connect
- Wrong `id_code` is rejected with `PermissionDenied`
- Agent retries and keeps host log clean of other controllers

**Note**: `id_code` is a pre-shared key, not cryptographic auth. Use TLS for hostile networks.

## TLS

Enable TLS for gRPC (and optionally mTLS with client CA):

```toml
[controller.tls]
cert = "certs/host.crt"
key  = "certs/host.key"
ca   = "certs/ca.crt"   # optional: require client certificates
```

Agents connect with `tls = true` in `agent.toml`:

```toml
[controller]
tls = true
```

## Network Bindings

By default, the WebUI/API binds `0.0.0.0:8080`. Restrict if the network is untrusted:

```toml
[controller]
ui_bind = "127.0.0.1:8080"
```

## Action Safety

- Actions are arbitrary shell strings run **as the agent's user**
- Only add actions you trust
- Be careful granting the agent user `sudo`
- `dangerous` actions require explicit confirmation for batch dispatch

## Adopted Robots

Robots that phone home but aren't in the config are "adopted" at runtime:

- They appear in the UI and CLI
- You must explicitly add them to `swarm.toml` for persistence
- Adopted robots can receive actions

## Recommendations

| Concern | Mitigation |
|---------|------------|
| Unauthorised agent connections | Use strong `id_code` + TLS |
| Command injection | Audit all action `command` strings |
| Agent privilege escalation | Run agent as unprivileged user |
| Network exposure | Restrict `ui_bind` to localhost or firewall |
| Config tampering | Restrict file permissions on `swarm.toml` |
