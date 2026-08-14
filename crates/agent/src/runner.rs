//! Shell-command runner. Each action runs in its own process group so that
//! stop/timeout can kill the whole tree (SIGTERM then SIGKILL), not just the
//! shell. stdout/stderr are streamed as line chunks with backpressure.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::timeout;

use swarmdeck_proto::v1::RunAction;

#[derive(Debug)]
pub enum RunnerEvent {
    Log {
        action_id: String,
        stderr: bool,
        line: String,
    },
    Done {
        action_id: String,
        exit_code: Option<u32>,
        killed: bool,
        error: Option<String>,
        started_ms: u64,
        finished_ms: u64,
    },
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct RunningEntry {
    pid: u32,
    killed: Arc<AtomicBool>,
    kill_on_disconnect: bool,
    started_ms: u64,
}

#[derive(Clone)]
pub struct Runner {
    inner: Arc<RunnerInner>,
}

struct RunnerInner {
    running: tokio::sync::Mutex<BTreeMap<String, RunningEntry>>,
    events: Mutex<Option<UnboundedSender<RunnerEvent>>>,
}

impl Runner {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RunnerInner {
                running: tokio::sync::Mutex::new(BTreeMap::new()),
                events: Mutex::new(None),
            }),
        }
    }

    /// Called with a fresh channel each time a session connects.
    pub fn attach_events(&self, tx: UnboundedSender<RunnerEvent>) {
        *self.inner.events.lock().unwrap() = Some(tx);
    }

    /// Drops the event sink when the session goes away.
    pub fn detach_events(&self) {
        *self.inner.events.lock().unwrap() = None;
    }

    fn send(&self, ev: RunnerEvent) {
        if let Some(tx) = self.inner.events.lock().unwrap().as_ref() {
            let _ = tx.send(ev);
        }
    }

    pub async fn spawn(&self, run: RunAction) -> std::io::Result<()> {
        use std::process::Stdio;

        let mut cmd = tokio::process::Command::new("/bin/sh");
        cmd.arg("-c").arg(&run.command);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        for (k, v) in &run.env {
            cmd.env(k, v);
        }
        if !run.cwd.is_empty() {
            cmd.current_dir(&run.cwd);
        }
        // New process group: child becomes group leader, so a negative kill
        // reaches the whole tree (shell + children).
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);
        let started_ms = now_ms();
        let killed = Arc::new(AtomicBool::new(false));
        let action_id = run.action_id.clone();
        let timeout_sec = run.timeout_sec as u64;

        self.inner.running.lock().await.insert(
            action_id.clone(),
            RunningEntry {
                pid,
                killed: killed.clone(),
                kill_on_disconnect: run.kill_on_disconnect,
                started_ms,
            },
        );

        println!("running task {}", run.action_name);
        use std::io::Write;
        let _ = std::io::stdout().flush();
        self.send(RunnerEvent::Log {
            action_id: action_id.clone(),
            stderr: false,
            line: format!("running task {}", run.action_name),
        });

        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");

        let runner = self.clone();
        let aid = action_id.clone();
        tokio::spawn(async move {
            let reader = tokio::io::BufReader::new(stdout);
            runner.stream_lines(&aid, false, reader).await;
        });
        let runner = self.clone();
        let aid = action_id.clone();
        tokio::spawn(async move {
            let reader = tokio::io::BufReader::new(stderr);
            runner.stream_lines(&aid, true, reader).await;
        });

        let runner = self.clone();
        tokio::spawn(async move {
            let wait = child.wait();
            let status = if timeout_sec > 0 {
                timeout(Duration::from_secs(timeout_sec), wait).await
            } else {
                Ok(wait.await)
            };

            let (exit_code, error) = match status {
                Err(_elapsed) => {
                    // Timed out: kill the process group, then report.
                    runner.kill_group(pid);
                    (None, Some("timed out".to_string()))
                }
                Ok(Ok(st)) => {
                    let code = st.code().and_then(|c| u32::try_from(c).ok());
                    (code, None)
                }
                Ok(Err(e)) => (None, Some(e.to_string())),
            };

            runner.inner.running.lock().await.remove(&action_id);
            let killed = killed.load(Ordering::SeqCst);
            runner.send(RunnerEvent::Done {
                action_id: action_id.clone(),
                exit_code,
                killed,
                error,
                started_ms,
                finished_ms: now_ms(),
            });
        });

        Ok(())
    }

    async fn stream_lines<R>(
        &self,
        action_id: &str,
        stderr: bool,
        mut reader: tokio::io::BufReader<R>,
    ) where
        R: tokio::io::AsyncRead + Unpin,
    {
        let mut dropped = 0usize;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let text = line.trim_end_matches(['\r', '\n']).to_string();
                    if text.is_empty() {
                        continue;
                    }
                    let ev = RunnerEvent::Log {
                        action_id: action_id.to_string(),
                        stderr,
                        line: text,
                    };
                    if let Some(tx) = self.inner.events.lock().unwrap().as_ref() {
                        if tx.send(ev).is_err() {
                            dropped += 1;
                        }
                    }
                }
            }
        }
        if dropped > 0 {
            tracing::warn!(action_id, dropped, "log lines dropped due to backpressure");
        }
    }

    /// Send SIGTERM to the process group; the waiter task reaps and reports.
    pub async fn kill(&self, action_id: &str) -> bool {
        let entry = {
            let mut running = self.inner.running.lock().await;
            running.remove(action_id)
        };
        if let Some(e) = entry {
            e.killed.store(true, Ordering::SeqCst);
            self.kill_group(e.pid);
            true
        } else {
            false
        }
    }

    fn kill_group(&self, pid: u32) {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
    }

    /// Terminate all actions flagged `kill_on_disconnect` (used on drop).
    pub async fn kill_on_disconnect_all(&self) {
        let entries: Vec<(String, u32)> = {
            let running = self.inner.running.lock().await;
            running
                .iter()
                .filter(|(_, e)| e.kill_on_disconnect)
                .map(|(id, e)| (id.clone(), e.pid))
                .collect()
        };
        for (id, pid) in entries {
            if let Some(e) = self.inner.running.lock().await.get_mut(&id) {
                e.killed.store(true, Ordering::SeqCst);
            }
            self.kill_group(pid);
        }
    }

    pub async fn active_action(&self) -> Option<(String, u64)> {
        let running = self.inner.running.lock().await;
        running
            .iter()
            .next()
            .map(|(id, e)| (id.clone(), e.started_ms))
    }
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}
