//! Typed client for the SwarmDeck control host HTTP/WebSocket API.
//!
//! Every UI (the WebUI, `swarmdeck-cli`, future TUIs) is just a thin shell over
//! the backend; this crate is that shared shell for Rust clients. The wire
//! contract is documented in `docs/api.md`.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::Stream;
use serde::de::DeserializeOwned;
use serde::Serialize;

use swarmdeck_core::{
    ActionsView, AdoptRequest, ApiTargets, ConfigView, Event, LogLine, RobotView, RunRequest,
    RunResponse, RunView, StopRequest, WorkflowRunRequest,
};

/// A ready-to-use client for the SwarmDeck host API.
#[derive(Debug, Clone)]
pub struct Client {
    base: String,
    http: reqwest::Client,
}

/// Errors from talking to the host.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("request to {url} failed: {source}")]
    Transport { url: String, source: reqwest::Error },
    #[error("{url} → HTTP {status}: {body}")]
    Http {
        url: String,
        status: u16,
        body: String,
    },
    #[error("bad JSON from {url}: {message}")]
    Json { url: String, message: String },
    /// The host rejected the dispatch because the action is flagged dangerous
    /// and targets more than one robot. The operator must confirm and resubmit
    /// with `confirm = true`. `message` is the host's exact rejection text.
    #[error("{message}")]
    ConfirmRequired { action: String, message: String },
    #[error("websocket: {0}")]
    Ws(String),
}

impl Client {
    /// Base URL like `http://127.0.0.1:8080` (trailing slashes trimmed).
    pub fn new(base_url: impl Into<String>) -> Self {
        let base = base_url.into().trim_end_matches('/').to_string();
        Self {
            base,
            http: reqwest::Client::new(),
        }
    }

    pub async fn robots(&self) -> Result<Vec<RobotView>, ClientError> {
        self.get_json("/api/robots").await
    }

    pub async fn actions(&self) -> Result<ActionsView, ClientError> {
        self.get_json("/api/actions").await
    }

    pub async fn config(&self) -> Result<ConfigView, ClientError> {
        self.get_json("/api/config").await
    }

    pub async fn runs(&self) -> Result<Vec<RunView>, ClientError> {
        self.get_json("/api/runs").await
    }

    pub async fn run(&self, run_id: &str) -> Result<RunView, ClientError> {
        self.get_json(&format!("/api/runs/{run_id}")).await
    }

    pub async fn logs(&self, robot: &str, tail: usize) -> Result<Vec<LogLine>, ClientError> {
        self.get_json(&format!("/api/robots/{robot}/logs?tail={tail}"))
            .await
    }

    /// Dispatch a batch. Returns `Err(ClientError::ConfirmRequired { .. })` when
    /// the host requires operator confirmation (dangerous action, >1 robot).
    pub async fn dispatch(&self, req: &RunRequest) -> Result<RunResponse, ClientError> {
        let url = self.url("/api/run");
        let resp = self
            .http
            .post(url.clone())
            .json(req)
            .send()
            .await
            .map_err(|e| ClientError::Transport {
                url: url.clone(),
                source: e,
            })?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| ClientError::Transport {
            url: url.clone(),
            source: e,
        })?;
        if status == 200 {
            return serde_json::from_str(&text).map_err(|e| ClientError::Json {
                url: url.clone(),
                message: e.to_string(),
            });
        }
        if text.contains("confirm with confirm=true") {
            return Err(ClientError::ConfirmRequired {
                action: req.action.clone(),
                message: text.trim().to_string(),
            });
        }
        Err(ClientError::Http {
            url,
            status,
            body: text,
        })
    }

    pub async fn stop(&self, targets: &ApiTargets) -> Result<Vec<String>, ClientError> {
        self.post_json(
            "/api/stop",
            &StopRequest {
                targets: targets.clone(),
                confirm: true,
            },
        )
        .await
    }

    /// Dispatch a named workflow. Returns `Err(ClientError::ConfirmRequired { .. })`
    /// when the host requires operator confirmation.
    pub async fn dispatch_workflow(
        &self,
        name: &str,
        confirm: bool,
    ) -> Result<RunResponse, ClientError> {
        let url = self.url("/api/workflow");
        let body = WorkflowRunRequest {
            workflow: name.to_string(),
            confirm,
        };
        let resp = self
            .http
            .post(url.clone())
            .json(&body)
            .send()
            .await
            .map_err(|e| ClientError::Transport {
                url: url.clone(),
                source: e,
            })?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| ClientError::Transport {
            url: url.clone(),
            source: e,
        })?;
        if status == 200 {
            return serde_json::from_str(&text).map_err(|e| ClientError::Json {
                url: url.clone(),
                message: e.to_string(),
            });
        }
        if text.contains("confirm with confirm=true") {
            return Err(ClientError::ConfirmRequired {
                action: name.to_string(),
                message: text.trim().to_string(),
            });
        }
        Err(ClientError::Http {
            url,
            status,
            body: text,
        })
    }

    pub async fn adopt(
        &self,
        robot: &str,
        kind: &str,
        name: Option<&str>,
    ) -> Result<(), ClientError> {
        let url = self.url(&format!("/api/adopt/{robot}"));
        let body = AdoptRequest {
            kind: kind.to_string(),
            name: name.map(str::to_string),
        };
        self.post_empty(&url, &body).await
    }

    pub async fn release(&self, robot: &str) -> Result<(), ClientError> {
        let url = self.url(&format!("/api/release/{robot}"));
        self.post_empty(&url, &serde_json::json!({})).await
    }

    /// Connect to `WS /api/ws`. The server sends a `robots` + `runs` snapshot
    /// first, then live deltas. The returned stream ends `Ok` on a clean close
    /// and yields `Err` on a transport failure; callers reconnect as needed.
    pub async fn subscribe(&self) -> Result<EventStream, ClientError> {
        let url = self.ws_url();
        let (ws, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| ClientError::Ws(e.to_string()))?;
        Ok(EventStream { inner: ws })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    fn ws_url(&self) -> String {
        let rest = self
            .base
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let scheme = if self.base.starts_with("https://") {
            "wss://"
        } else {
            "ws://"
        };
        format!("{scheme}{rest}/api/ws")
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        let url = self.url(path);
        let resp = self
            .http
            .get(url.clone())
            .send()
            .await
            .map_err(|e| ClientError::Transport {
                url: url.clone(),
                source: e,
            })?;
        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.map_err(|e| ClientError::Transport {
                url: url.clone(),
                source: e,
            })?;
            return Err(ClientError::Http { url, status, body });
        }
        resp.json().await.map_err(|e| ClientError::Json {
            url: url.clone(),
            message: e.to_string(),
        })
    }

    async fn post_json<T: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R, ClientError> {
        let url = self.url(path);
        let resp = self
            .http
            .post(url.clone())
            .json(body)
            .send()
            .await
            .map_err(|e| ClientError::Transport {
                url: url.clone(),
                source: e,
            })?;
        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.map_err(|e| ClientError::Transport {
                url: url.clone(),
                source: e,
            })?;
            return Err(ClientError::Http { url, status, body });
        }
        resp.json().await.map_err(|e| ClientError::Json {
            url: url.clone(),
            message: e.to_string(),
        })
    }

    async fn post_empty<T: Serialize>(&self, url: &str, body: &T) -> Result<(), ClientError> {
        let resp = self
            .http
            .post(url.to_string())
            .json(body)
            .send()
            .await
            .map_err(|e| ClientError::Transport {
                url: url.to_string(),
                source: e,
            })?;
        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.map_err(|e| ClientError::Transport {
                url: url.to_string(),
                source: e,
            })?;
            return Err(ClientError::Http {
                url: url.to_string(),
                status,
                body,
            });
        }
        Ok(())
    }
}

/// A stream of `Event`s from `WS /api/ws`.
pub struct EventStream {
    inner: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

impl Stream for EventStream {
    type Item = Result<Event, ClientError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(ClientError::Ws(e.to_string()))))
                }
                Poll::Ready(Some(Ok(msg))) => {
                    use tokio_tungstenite::tungstenite::Message;
                    match msg {
                        Message::Text(text) => {
                            return Poll::Ready(match serde_json::from_str::<Event>(&text) {
                                Ok(ev) => Some(Ok(ev)),
                                Err(e) => Some(Err(ClientError::Ws(e.to_string()))),
                            });
                        }
                        Message::Close(_) => return Poll::Ready(None),
                        _ => continue,
                    }
                }
            }
        }
    }
}
