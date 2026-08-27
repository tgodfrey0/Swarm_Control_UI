//! HTTP + WebSocket API for the WebUI and `swarmlink-cli`.
//!
//! JSON routes:
//!   GET  /api/robots            -> Vec<RobotView>
//!   GET  /api/types             -> Vec<String>
//!   GET  /api/actions           -> ActionsView
//!   GET  /api/config            -> ConfigView
//!   GET  /api/health            -> {"status":"ok"}
//!   GET  /api/runs              -> Vec<RunView>
//!   GET  /api/runs/{id}         -> RunView
//!   POST /api/run               -> RunResponse
//!   POST /api/stop              -> stopped robot ids
//!   POST /api/clear             -> clears all logs and runs
//!   POST /api/adopt/{robot}     -> {}
//!   POST /api/release/{robot}   -> {}
//!   GET  /api/robots/{id}/logs  -> Vec<LogLine>
//!   GET  /api/export/logs       -> zip of every robot's logs (attachment)
//!   GET  /api/ws                -> live Event stream (text JSON)
//!
//! Static / WebUI:
//!   GET /            -> index.html
//!   GET /static/*    -> ui/static assets

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    Path, Query, State,
};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use swarmlink_core::{
    ActionsView, AdoptRequest, Event, RunRequest, RunResponse, StopRequest, WorkflowRunRequest,
};

use crate::dispatch::Dispatcher;
use crate::registry::Registry;

pub(crate) fn ui_dir() -> PathBuf {
    let cwd = std::path::Path::new("ui");
    if cwd.exists() {
        return cwd.to_path_buf();
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("../ui")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| cwd.to_path_buf())
}

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<Registry>,
    pub dispatcher: Arc<Dispatcher>,
}

pub type ApiResult<T> = Result<T, (StatusCode, String)>;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/robots", get(list_robots))
        .route("/api/types", get(list_types))
        .route("/api/actions", get(list_actions))
        .route("/api/config", get(get_config))
        .route("/api/health", get(health))
        .route("/api/runs", get(list_runs))
        .route("/api/runs/{run_id}", get(get_run))
        .route("/api/run", post(run_action))
        .route("/api/workflow", post(run_workflow))
        .route("/api/stop", post(stop_action))
        .route("/api/clear", post(clear_all))
        .route("/api/adopt/{robot}", post(adopt_robot))
        .route("/api/release/{robot}", post(release_robot))
        .route("/api/robots/{robot}/logs", get(robot_logs))
        .route("/api/export/logs", get(export_logs))
        .route("/api/ws", get(ws_upgrade))
        .route("/static/{*path}", get(static_asset))
        .with_state(state)
}

fn err<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}

async fn index() -> Result<impl IntoResponse, StatusCode> {
    let file = ui_dir().join("index.html");
    let bytes = std::fs::read(file).map_err(|_| StatusCode::NOT_FOUND)?;
    let mut response = (StatusCode::OK, bytes).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html"));
    Ok(response)
}

/// Serve `ui/static/*` with a no-store cache header so browsers always pick
/// up updated UI files.
async fn static_asset(Path(path): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    let base = ui_dir().join("static");
    let file = base.join(&path);
    if !file.starts_with(base) {
        return Err(StatusCode::FORBIDDEN);
    }
    let bytes = std::fs::read(&file).map_err(|_| StatusCode::NOT_FOUND)?;
    let mime = match file.extension().and_then(|e| e.to_str()) {
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("html") => "text/html",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    };
    let mut response = (StatusCode::OK, bytes).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    Ok(response)
}

async fn list_robots(State(state): State<AppState>) -> Json<Vec<swarmlink_core::RobotView>> {
    Json(state.registry.all_views().await)
}

async fn list_types(State(state): State<AppState>) -> Json<Vec<String>> {
    let cfg = state.registry.config.read().await;
    Json(cfg.robot_types.keys().cloned().collect())
}

async fn list_actions(State(state): State<AppState>) -> Json<ActionsView> {
    let cfg = state.registry.config.read().await;
    let active_types: std::collections::HashSet<&str> = cfg
        .robots
        .iter()
        .map(|r| r.kind.as_str())
        .filter(|k| !k.is_empty())
        .collect();
    let mut robot_type: Vec<String> = cfg
        .robot_types
        .iter()
        .filter(|(ty, _)| active_types.contains(ty.as_str()))
        .flat_map(|(ty, t)| t.actions.keys().map(move |a| format!("{ty}.{a}")))
        .collect();
    robot_type.sort();
    let mut swarm: Vec<String> = cfg.actions.keys().cloned().collect();
    swarm.sort();
    let mut workflows: Vec<String> = cfg.workflows.keys().cloned().collect();
    workflows.sort();
    let mut type_workflows: Vec<String> = Vec::new();
    for (ty, ty_cfg) in &cfg.robot_types {
        for wf_name in ty_cfg.workflows.keys() {
            type_workflows.push(format!("{ty}.{wf_name}"));
        }
    }
    type_workflows.sort();
    Json(ActionsView {
        robot_type,
        swarm,
        workflows,
        type_workflows,
    })
}

async fn list_runs(State(state): State<AppState>) -> Json<Vec<swarmlink_core::RunView>> {
    Json(state.registry.run_store.recent(50).await)
}

async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> ApiResult<Json<swarmlink_core::RunView>> {
    state
        .registry
        .run_store
        .get(&run_id)
        .await
        .map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("no such run: {run_id}")))
}

async fn get_config(State(state): State<AppState>) -> Json<swarmlink_core::ConfigView> {
    let cfg = state.registry.config.read().await;
    let mut robot_types: Vec<String> = cfg.robot_types.keys().cloned().collect();
    robot_types.sort();
    Json(swarmlink_core::ConfigView {
        controller: cfg.controller.name.clone(),
        robot_types,
        robot_count: cfg.robots.len(),
        grpc_listen: cfg.controller.grpc_listen.to_string(),
        ui_bind: cfg.controller.ui_bind.to_string(),
    })
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

#[derive(Deserialize)]
struct LogsQuery {
    #[serde(default = "default_tail")]
    tail: usize,
}

fn default_tail() -> usize {
    0
}

async fn robot_logs(
    State(state): State<AppState>,
    Path(robot): Path<String>,
    Query(q): Query<LogsQuery>,
) -> ApiResult<Json<Vec<swarmlink_core::LogLine>>> {
    let lines = state.registry.logs(&robot, q.tail).await;
    Ok(Json(lines))
}

/// Build and return a zip of every robot's in-memory logs, one `<robot>.log`
/// file per robot. Streamed to the client for direct download.
async fn export_logs(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    use std::io::Cursor;
    use zip::write::SimpleFileOptions;

    let logs = state.registry.export_logs().await;
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        if logs.is_empty() {
            zw.add_directory("logs/", opts).map_err(err)?;
        }
        for (robot_id, lines) in &logs {
            let name = format!("{robot_id}.log");
            let mut text = String::new();
            for l in lines {
                text.push_str(&format!(
                    "{}[{}] {}\n",
                    if l.stderr { "ERR " } else { "" },
                    format_log_ts(l.ts_ms),
                    l.text
                ));
            }
            zw.start_file(name, opts).map_err(err)?;
            zw.write_all(text.as_bytes()).map_err(err)?;
        }
        zw.finish().map_err(err)?;
    }

    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let filename = format!("swarmlink-logs-{ts}.zip");
    let mut response = (StatusCode::OK, buf.into_inner()).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")).map_err(err)?,
    );
    Ok(response)
}

/// Format a unix-millisecond timestamp as `HH:MM:SS` (local time).
fn format_log_ts(ts_ms: u64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ts_ms as i64)
        .single()
        .map(|t| t.format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

async fn run_action(
    State(state): State<AppState>,
    Json(req): Json<RunRequest>,
) -> ApiResult<Json<RunResponse>> {
    match state.dispatcher.run(req).await {
        Ok(resp) => Ok(Json(resp)),
        Err(e) => Err(err(e)),
    }
}

async fn run_workflow(
    State(state): State<AppState>,
    Json(req): Json<WorkflowRunRequest>,
) -> ApiResult<Json<RunResponse>> {
    match state
        .dispatcher
        .run_workflow(&req.workflow, req.confirm)
        .await
    {
        Ok(resp) => Ok(Json(resp)),
        Err(e) => Err(err(e)),
    }
}

async fn stop_action(
    State(state): State<AppState>,
    Json(req): Json<StopRequest>,
) -> ApiResult<Json<Vec<String>>> {
    match state.dispatcher.stop(req).await {
        Ok(stopped) => Ok(Json(stopped)),
        Err(e) => Err(err(e)),
    }
}

async fn clear_all(State(state): State<AppState>) -> StatusCode {
    state.registry.clear_logs().await;
    state.registry.run_store.clear().await;
    state
        .registry
        .events
        .publish(Event::Runs { runs: Vec::new() });
    StatusCode::OK
}

async fn adopt_robot(
    State(state): State<AppState>,
    Path(robot): Path<String>,
    Json(req): Json<AdoptRequest>,
) -> ApiResult<impl IntoResponse> {
    match state
        .dispatcher
        .adopt(&robot, &req.kind, req.name.as_deref())
        .await
    {
        Ok(()) => Ok(StatusCode::OK),
        Err(e) => Err(err(e)),
    }
}

async fn release_robot(
    State(state): State<AppState>,
    Path(robot): Path<String>,
) -> ApiResult<impl IntoResponse> {
    match state.dispatcher.release(&robot).await {
        Ok(()) => Ok(StatusCode::OK),
        Err(e) => Err(err(e)),
    }
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------

async fn ws_upgrade(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: AppState) {
    // Send a full snapshot first so late joiners get current state.
    let snapshot = Event::Robots {
        robots: state.registry.all_views().await,
    };
    if socket
        .send(Message::Text(
            serde_json::to_string(&snapshot).unwrap().into(),
        ))
        .await
        .is_err()
    {
        return;
    }
    let runs = Event::Runs {
        runs: state.registry.run_store.recent(50).await,
    };
    if socket
        .send(Message::Text(serde_json::to_string(&runs).unwrap().into()))
        .await
        .is_err()
    {
        return;
    }

    let mut events = state.registry.events.subscribe();
    loop {
        tokio::select! {
            // Drain client messages so we notice disconnect.
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(p))) => {
                        if socket.send(Message::Pong(p)).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            event = events.recv() => {
                if let Ok(ev) = event {
                    if let Ok(text) = serde_json::to_string(&ev) {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                } // Lagged broadcast just misses an update; next snapshot catches up.
            }
        }
    }
}
