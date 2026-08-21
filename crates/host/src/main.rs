mod dispatch;
mod events;
mod grpc;
mod http;
mod registry;

use std::io;
use std::path::{Path, PathBuf};

use clap::Parser;
use swarmdeck_core::SwarmConfig;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt::writer::Tee, EnvFilter};

use dispatch::Dispatcher;
use http::AppState;
use registry::Registry;

#[derive(Debug, Parser)]
#[command(name = "swarmdeck", about = "SwarmDeck control host")]
struct Args {
    /// Swarm config directory (contains {swarm}/swarm.toml).
    #[arg(long)]
    swarm: PathBuf,
    /// Override: swarm configuration (TOML) instead of {swarm}/swarm.toml.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Directory of shared robot-type TOML files.
    #[arg(long, default_value = "robots")]
    robot_types: PathBuf,
}

fn setup_logging(name: &str) -> WorkerGuard {
    let filter = EnvFilter::from_default_env();
    std::fs::create_dir_all("logs").ok();

    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let log_path = Path::new("logs").join(format!("{name}-{ts}.log"));

    let file_appender = tracing_appender::rolling::never("logs", log_path.file_name().unwrap());
    let (file_non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let (stdout_non_blocking, _) = tracing_appender::non_blocking(io::stdout());

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(Tee::new(file_non_blocking, stdout_non_blocking))
        .with_ansi(false)
        .init();
    guard
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let swarm_file = args.config.unwrap_or_else(|| args.swarm.join("swarm.toml"));
    let types_dir = Some(args.robot_types);
    let cfg = SwarmConfig::from_files(&swarm_file, types_dir.as_deref())?;

    let _guard = setup_logging(&cfg.controller.name);

    tracing::info!(
        controller = %cfg.controller.name,
        robot_types = ?cfg.robot_types.keys().collect::<Vec<_>>(),
        robots = cfg.robots.len(),
        "swarm config loaded"
    );

    let registry = Registry::new(cfg.clone(), swarm_file, types_dir);
    let dispatcher = Dispatcher::new(registry.clone());

    // gRPC: robots phone home here.
    let grpc_addr = cfg.controller.grpc_listen;
    let mut grpc_builder = tonic::transport::Server::builder();
    if let Some(tls) = &cfg.controller.tls {
        let identity = tonic::transport::Identity::from_pem(
            std::fs::read(&tls.cert)?,
            std::fs::read(&tls.key)?,
        );
        let mut tls_cfg = tonic::transport::ServerTlsConfig::new().identity(identity);
        if let Some(ca) = &tls.ca {
            let ca = tonic::transport::Certificate::from_pem(std::fs::read(ca)?);
            tls_cfg = tls_cfg.client_ca_root(ca);
        }
        grpc_builder = grpc_builder.tls_config(tls_cfg)?;
        tracing::info!(tls = true, client_ca = tls.ca.is_some(), "gRPC TLS enabled");
    }
    let grpc_server = grpc_builder
        .add_service(grpc::server(registry.clone()))
        .serve(grpc_addr);
    tracing::info!(%grpc_addr, "gRPC endpoint listening");

    // HTTP + WS + WebUI.
    let ui_addr = cfg.controller.ui_bind;
    let http_server = axum::serve(
        tokio::net::TcpListener::bind(ui_addr).await?,
        http::router(AppState {
            registry: registry.clone(),
            dispatcher,
        }),
    );
    let ui_host = if ui_addr.ip().is_unspecified() {
        "localhost".to_string()
    } else {
        ui_addr.ip().to_string()
    };
    println!("WebUI: http://{ui_host}:{}", ui_addr.port());
    tracing::info!(%ui_addr, "WebUI/API listening");

    let reload_registry = registry.clone();
    let mut signals = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let sighup = async move {
        loop {
            signals.recv().await;
            if let Err(e) = reload_registry.reload_config().await {
                tracing::error!(error = %e, "config reload failed");
            }
        }
    };

    tokio::select! {
        r = grpc_server => r?,
        r = http_server => r?,
        _ = sighup => {}
        _ = tokio::signal::ctrl_c() => {}
    }

    tracing::info!("host shutting down");
    Ok(())
}
