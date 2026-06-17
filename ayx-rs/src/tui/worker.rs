//! Background worker for the TUI.
//!
//! All network and disk-bound calls that previously ran inline inside the
//! keyboard event handler now run on a single worker thread. The UI side
//! enqueues a `BackgroundTask`, the worker produces a `TaskResult`, and the
//! main loop drains results during `tick()`. This stops the UI freezing
//! while the One API answers a slow request.
//!
//! Channels are mpsc; one worker thread is sufficient — tasks serialize
//! naturally and there's no benefit to parallelism for a single user. If
//! anything, parallel One calls would race for the same access token.
//!
//! Stale-result discipline: every result carries a `request_id` (a
//! monotonically increasing u64 stamped by `next_request_id`). When the UI
//! enqueues a request it remembers the latest id for that *kind* of result;
//! a result arriving with a stale id is dropped on the floor. This avoids
//! "the last network call before you navigated away wins" bugs.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;

use anyhow::Result;
use ayx_core::profile::Config;
use serde_json::Value;

use super::app::OneBrowserResource;

/// Identifier for a single in-flight request. Cheap, monotonic, and only ever
/// compared for equality with the latest id we expect for a given lane.
pub type RequestId = u64;

fn next_request_id() -> RequestId {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A unit of work for the background thread. Add new variants here as the
/// TUI grows new network-bound interactions.
pub enum BackgroundTask {
    Connectivity {
        id: RequestId,
        target_path: PathBuf,
        target_environment: Option<String>,
        config: Config,
    },
    OneBrowser {
        id: RequestId,
        config: Config,
        resource: OneBrowserResource,
        resource_id: Option<String>,
    },
}

/// A completed unit of work, ready for the UI to apply during `tick()`.
pub enum TaskResult {
    Connectivity {
        id: RequestId,
        panels: Vec<crate::tui::app::PanelState>,
    },
    OneBrowser {
        id: RequestId,
        resource: OneBrowserResource,
        /// The resource id the request was issued against. Currently unused by
        /// the receiver (we only key on the request id) but kept on the wire
        /// for future per-id routing (e.g. invalidating cached panels for a
        /// specific workspace id).
        #[allow(dead_code)]
        resource_id: Option<String>,
        result: std::result::Result<Value, String>,
    },
}

pub struct BackgroundWorker {
    tx: Sender<BackgroundTask>,
    rx: Receiver<TaskResult>,
    // Holding the JoinHandle keeps the type around for debug; we never join,
    // the thread exits when the channel is dropped (App is dropped on quit).
    _handle: thread::JoinHandle<()>,
}

impl BackgroundWorker {
    pub fn spawn() -> Self {
        let (task_tx, task_rx) = channel::<BackgroundTask>();
        let (result_tx, result_rx) = channel::<TaskResult>();
        let handle = thread::Builder::new()
            .name("ayx-tui-worker".to_string())
            .spawn(move || worker_loop(task_rx, result_tx))
            .expect("tui worker thread should spawn");
        Self {
            tx: task_tx,
            rx: result_rx,
            _handle: handle,
        }
    }

    pub fn submit(&self, task: BackgroundTask) -> Result<()> {
        self.tx
            .send(task)
            .map_err(|err| anyhow::anyhow!("tui worker channel closed: {err}"))?;
        Ok(())
    }

    pub fn try_recv(&self) -> std::result::Result<TaskResult, TryRecvError> {
        self.rx.try_recv()
    }

    pub fn new_request_id() -> RequestId {
        next_request_id()
    }
}

fn worker_loop(rx: Receiver<BackgroundTask>, tx: Sender<TaskResult>) {
    while let Ok(task) = rx.recv() {
        match task {
            BackgroundTask::Connectivity {
                id,
                target_path,
                target_environment,
                config,
            } => {
                let panels =
                    build_connectivity_panels(&target_path, target_environment.as_deref(), &config);
                let _ = tx.send(TaskResult::Connectivity { id, panels });
            }
            BackgroundTask::OneBrowser {
                id,
                config,
                resource,
                resource_id,
            } => {
                let result = super::one_browser::request_for_one_browser_blocking(
                    &config,
                    resource,
                    resource_id.as_deref(),
                )
                .map_err(|err| err.to_string());
                let _ = tx.send(TaskResult::OneBrowser {
                    id,
                    resource,
                    resource_id,
                    result,
                });
            }
        }
    }
}

fn build_connectivity_panels(
    target_path: &std::path::Path,
    target_environment: Option<&str>,
    config: &Config,
) -> Vec<crate::tui::app::PanelState> {
    use crate::tui::render_helpers::render_envelope_panel;
    vec![
        render_envelope_panel(
            "Doctor Config",
            crate::doctor_config_envelope_from_path(target_path, false).map(|env| env.data),
        ),
        render_envelope_panel(
            "Doctor Auth",
            crate::doctor_auth_envelope_from_path(target_path, target_environment)
                .map(|env| env.data),
        ),
        render_envelope_panel(
            "One Auth Status",
            crate::one_platform_auth_status_envelope(config).map(|env| env.data),
        ),
        render_envelope_panel(
            "One Auth Diagnose",
            crate::one_platform_auth_diagnose_envelope(config).map(|env| env.data),
        ),
    ]
}
