//! v2 worker: a single background thread that runs Effects off the UI thread
//! and returns Actions. Staleness is handled by the reducer via tokens carried
//! on each Action, so the worker no longer tracks request ids.
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::{self, JoinHandle};

use ayx_core::profile::Config;
use serde_json::Value;

use crate::tui::v2::action::Action;
use crate::tui::v2::effect::{Effect, ListScope};
use crate::tui::v2::resource::{Kind, Row, kind_impl, str_field};

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
            Effect::FetchList { kind, token, scope } => {
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
                list_payload_to_action(kind, token, scope.as_ref(), payload)
            }
            Effect::FetchDetail { kind, id, token } => match kind_impl(kind).detail_endpoint() {
                Some(endpoint) => {
                    let payload = crate::one_api_live_request(
                        &job.config,
                        endpoint.surface,
                        endpoint.operation,
                        "GET",
                        endpoint.path,
                        false,
                        &[("id", id.as_str())],
                    )
                    .map(|env| env.data)
                    .map_err(|e| e.to_string());
                    detail_payload_to_action(token, payload)
                }
                None => Action::DetailFailed {
                    token,
                    error: "no detail endpoint for this kind".into(),
                },
            },
        };
        let _ = tx.send(Outcome { action });
    }
}

/// Pure mapping from a raw list payload (or error) to an Action. When a `scope`
/// is present, items are filtered to the parent's children before row-mapping
/// (the display Row does not carry the parent id, so the filter must run on the
/// raw item JSON). Unit-tested.
pub fn list_payload_to_action(
    kind: Kind,
    token: u64,
    scope: Option<&ListScope>,
    payload: Result<Value, String>,
) -> Action {
    match payload {
        Ok(value) => {
            let imp = kind_impl(kind);
            let rows: Vec<Row> = imp
                .extract_items(&value)
                .iter()
                .filter(|item| scope.is_none_or(|s| item_in_scope(item, s)))
                .map(|i| imp.row(i))
                .collect();
            Action::ListLoaded { token, rows }
        }
        Err(error) => Action::ListFailed { token, error },
    }
}

/// Does `item` belong to `scope`'s parent? Only `Kind::Flow` parents filter
/// (flow -> runs: keep jobs whose flow id matches). Other parent kinds have no
/// scoped relation yet, so they pass everything through.
fn item_in_scope(item: &Value, scope: &ListScope) -> bool {
    match scope.parent_kind {
        Kind::Flow => str_field(item, &["flowId", "flow_id"]) == Some(scope.parent_id.as_str()),
        _ => true,
    }
}

/// Pure mapping from a raw detail payload (or error) to an Action.
pub fn detail_payload_to_action(token: u64, payload: Result<Value, String>) -> Action {
    match payload {
        Ok(json) => Action::DetailLoaded { token, json },
        Err(error) => Action::DetailFailed { token, error },
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
        match list_payload_to_action(Kind::Flow, 7, None, payload) {
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
        match list_payload_to_action(Kind::Flow, 7, None, Err("401 unauthorized".into())) {
            Action::ListFailed { token, error } => {
                assert_eq!(token, 7);
                assert!(error.contains("401"));
            }
            other => panic!("expected ListFailed, got {other:?}"),
        }
    }

    #[test]
    fn scope_filters_jobs_by_flow_id() {
        use crate::tui::v2::effect::ListScope;

        let payload = Ok(json!({ "data": [
            { "id": "jg_1", "flowId": "fl_a", "status": "Succeeded" },
            { "id": "jg_2", "flowId": "fl_b", "status": "Failed" },
            { "id": "jg_3", "flow_id": "fl_a", "status": "Running" }
        ]}));
        let scope = ListScope {
            parent_kind: Kind::Flow,
            parent_id: "fl_a".into(),
        };
        match list_payload_to_action(Kind::Job, 1, Some(&scope), payload) {
            Action::ListLoaded { rows, .. } => {
                assert_eq!(rows.len(), 2, "only fl_a's jobs survive");
                assert!(rows.iter().all(|r| r.id == "jg_1" || r.id == "jg_3"));
            }
            other => panic!("expected ListLoaded, got {other:?}"),
        }
    }

    #[test]
    fn no_scope_keeps_all_items() {
        let payload = Ok(json!({ "data": [
            { "id": "jg_1", "flowId": "fl_a" }, { "id": "jg_2", "flowId": "fl_b" }
        ]}));
        match list_payload_to_action(Kind::Job, 1, None, payload) {
            Action::ListLoaded { rows, .. } => assert_eq!(rows.len(), 2),
            other => panic!("expected ListLoaded, got {other:?}"),
        }
    }

    #[test]
    fn detail_ok_maps_to_detail_loaded() {
        match detail_payload_to_action(3, Ok(json!({ "id": "x" }))) {
            Action::DetailLoaded { token, json } => {
                assert_eq!(token, 3);
                assert_eq!(json["id"], "x");
            }
            other => panic!("expected DetailLoaded, got {other:?}"),
        }
    }

    #[test]
    fn detail_err_maps_to_detail_failed() {
        match detail_payload_to_action(3, Err("404".into())) {
            Action::DetailFailed { token, error } => {
                assert_eq!(token, 3);
                assert!(error.contains("404"));
            }
            other => panic!("expected DetailFailed, got {other:?}"),
        }
    }
}
