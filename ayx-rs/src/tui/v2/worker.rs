//! v2 worker: a single background thread that runs Effects off the UI thread
//! and returns Actions. Staleness is handled by the reducer via tokens carried
//! on each Action, so the worker no longer tracks request ids.
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::{self, JoinHandle};

use ayx_core::profile::Config;
use serde_json::Value;

use crate::tui::v2::action::Action;
use crate::tui::v2::effect::Effect;
use crate::tui::v2::resource::{Kind, Row, kind_impl};

struct Job {
    effect: Effect,
    config: Config,
}

pub struct Outcome {
    pub action: Action,
}

pub struct Worker {
    tx: Sender<Job>,
    rx: Receiver<Outcome>,
    _handle: JoinHandle<()>,
}

impl Worker {
    pub fn spawn() -> Self {
        let (job_tx, job_rx) = channel::<Job>();
        let (out_tx, out_rx) = channel::<Outcome>();
        let handle = thread::Builder::new()
            .name("ayx-tui-v2-worker".into())
            .spawn(move || worker_loop(job_rx, out_tx))
            .expect("v2 worker thread should spawn");
        Self {
            tx: job_tx,
            rx: out_rx,
            _handle: handle,
        }
    }

    pub fn submit(&self, effect: Effect, config: Config) {
        let _ = self.tx.send(Job { effect, config });
    }

    pub fn try_recv(&self) -> Result<Outcome, TryRecvError> {
        self.rx.try_recv()
    }
}

fn worker_loop(rx: Receiver<Job>, tx: Sender<Outcome>) {
    while let Ok(job) = rx.recv() {
        let action = match job.effect {
            Effect::FetchList { kind, token } => {
                let endpoint = kind_impl(kind).list_endpoint();
                let payload = crate::one_api_live_request(
                    &job.config,
                    endpoint.surface,
                    endpoint.operation,
                    "GET",
                    endpoint.path,
                    false,
                    &[],
                )
                .map(|env| env.data)
                .map_err(|e| e.to_string());
                list_payload_to_action(kind, token, payload)
            }
        };
        let _ = tx.send(Outcome { action });
    }
}

/// Pure mapping from a raw list payload (or error) to an Action. Unit-tested.
pub fn list_payload_to_action(kind: Kind, token: u64, payload: Result<Value, String>) -> Action {
    match payload {
        Ok(value) => {
            let imp = kind_impl(kind);
            let rows: Vec<Row> = imp
                .extract_items(&value)
                .iter()
                .map(|i| imp.row(i))
                .collect();
            Action::ListLoaded { token, rows }
        }
        Err(error) => Action::ListFailed { token, error },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::Kind;
    use serde_json::json;

    #[test]
    fn ok_payload_maps_to_list_loaded_with_rows() {
        let payload = Ok(json!({
            "data": [ { "id": "fl_1", "name": "ETL" }, { "id": "fl_2", "name": "Roll" } ]
        }));
        match list_payload_to_action(Kind::Flow, 7, payload) {
            Action::ListLoaded { token, rows } => {
                assert_eq!(token, 7);
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].cells[0].text, "ETL");
            }
            other => panic!("expected ListLoaded, got {other:?}"),
        }
    }

    #[test]
    fn err_payload_maps_to_list_failed() {
        match list_payload_to_action(Kind::Flow, 7, Err("401 unauthorized".into())) {
            Action::ListFailed { token, error } => {
                assert_eq!(token, 7);
                assert!(error.contains("401"));
            }
            other => panic!("expected ListFailed, got {other:?}"),
        }
    }
}
