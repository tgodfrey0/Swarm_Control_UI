# Development

## Prerequisites

- [just](https://github.com/casey/just) task runner
- Rust 1.85+ via [rustup](https://rustup.rs) (run `activate.sh` once to install toolchain + musl targets)
- `protoc` (protobuf compiler)
- `cargo-zigbuild` + `zig` (only needed for cross-compilation)
- Git

## Setup

```sh
git clone <repo-url>
cd swarmdeck
./activate.sh   # idempotent: installs the stable toolchain and musl targets
```

## Building

```sh
just build      # cargo build --release --workspace, binaries land in bin/
```

## Running

Binaries are run directly from `bin/`:

```sh
# Control host (WebUI at localhost:8080)
./bin/swarmdeck --swarm configs/lab

# Simulated swarm (WebUI at localhost:18082)
./bin/swarmdeck --swarm configs/sim

# Robot agent
./bin/swarmdeck-agent --config /etc/swarm-agent/agent.toml

# Simulated agent
./bin/swarmdeck-agent --config configs/sim/agent-1.toml

# CLI
./bin/swarmdeck-cli --host http://127.0.0.1:18082 status
```

## Code Quality

```sh
just check      # cargo check --workspace
just lint       # cargo clippy --workspace --all-targets -- -D warnings
just fmt        # cargo fmt --all
```

## Testing

```sh
just test       # Run all tests (Rust + WebUI)
just test-rust  # Cargo tests only
just test-webui # WebUI contract test only
```

### Rust Tests

Located in `crates/core/tests/` and inline in source files:
- Target resolution tests
- Template engine tests
- JSON wire format contract tests

### WebUI Contract Test

`ui/test/contract.test.js` -- Node.js test that:
- Mocks DOM, fetch, WebSocket
- Loads real `app.js` via `eval()`
- Asserts rendering, actions, targets, WebSocket flow

## Project Structure

```
crates/host/src/     Host: grpc.rs, dispatch.rs, registry.rs, http.rs
crates/agent/src/    Agent: session.rs, runner.rs, procfs.rs
crates/cli/src/      CLI: main.rs, provision.rs
crates/core/src/     Core: config.rs, dispatch.rs, template.rs, spec.rs, api.rs
proto/               swarm.proto (gRPC service definitions)
configs/             Swarm configurations
robots/              Shared robot type definitions
ui/                  WebUI (index.html, static/app.js, static/styles.css)
docs/                Documentation and wiki pages
```

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Make changes
4. Run `just lint` and `just test`
5. Commit with a clear message
6. Push and open a pull request

### Commit Messages

Use conventional commits:
- `feat: add new action type`
- `fix: resolve template rendering issue`
- `docs: update configuration wiki`
- `test: add contract tests for dispatch`

### Code Style

- Follow existing patterns
- Rust: `cargo fmt` + `cargo clippy`
- No new comments unless explaining complex logic
- Keep functions focused and small

## CI/CD

The project uses Jenkins for continuous integration:

- **Lint**: `cargo fmt --check`, `cargo clippy`
- **Test**: `cargo test --workspace`, `node ui/test/contract.test.js`
- **Build**: native + cross-compile targets
- **Archive**: artifacts for deployment

See `Jenkinsfile` for pipeline configuration.
