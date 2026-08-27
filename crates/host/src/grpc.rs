//! gRPC server: robots phone home here and keep one bidirectional
//! `Session` stream open. The first report must be a `Register` carrying the
//! controller's `id_code`; anything else is rejected with PermissionDenied.

use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status, Streaming};

use swarmlink_proto::v1::{
    agent_server::{Agent, AgentServer},
    report::Report as ReportMsg,
    Command, Report,
};

use crate::registry::Registry;

pub fn server(registry: Arc<Registry>) -> AgentServer<AgentService> {
    AgentServer::new(AgentService { registry })
}

#[derive(Clone)]
pub struct AgentService {
    pub registry: Arc<Registry>,
}

#[tonic::async_trait]
impl Agent for AgentService {
    type SessionStream = Pin<Box<dyn tokio_stream::Stream<Item = Result<Command, Status>> + Send>>;

    async fn session(
        &self,
        request: Request<Streaming<Report>>,
    ) -> Result<Response<Self::SessionStream>, Status> {
        let mut incoming = request.into_inner();

        let first = incoming
            .message()
            .await
            .map_err(|e| Status::unavailable(format!("stream error: {e}")))?
            .ok_or_else(|| Status::unavailable("agent disconnected before registering"))?;

        let reg = match &first.report {
            Some(ReportMsg::Register(r)) => r,
            _ => return Err(Status::invalid_argument("first report must be a register")),
        };
        let robot_id = reg.robot_id.clone();
        let agent_version = reg.agent_version.clone();
        let hostname = reg.hostname.clone();

        let expected = self.registry.config.read().await.controller.id_code.clone();
        if reg.id_code != expected {
            tracing::warn!(robot = %robot_id, "registration rejected (id_code mismatch)");
            return Err(Status::permission_denied(
                "registration rejected: id_code mismatch",
            ));
        }

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Command>();
        let seq = self.registry.set_cmd_tx(&robot_id, cmd_tx.clone()).await;

        // Register was consumed for validation above; feed it into the
        // registry so the entry is marked connected before the stream opens.
        self.registry.handle_report(&robot_id, &first).await;

        let registry = self.registry.clone();
        let reader_robot = robot_id.clone();
        tokio::spawn(async move {
            while let Ok(Some(report)) = incoming.message().await {
                registry.handle_report(&reader_robot, &report).await;
            }
            registry.disconnect(&reader_robot, seq).await;
            tracing::info!(robot = %reader_robot, "agent disconnected");
        });

        tracing::info!(robot = %robot_id, hostname = %hostname, agent_version = %agent_version, "agent registered");
        let stream = UnboundedReceiverStream::new(cmd_rx).map(Ok);
        Ok(Response::new(Box::pin(stream)))
    }
}
