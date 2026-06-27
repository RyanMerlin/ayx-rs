//! Effects: side-effect requests emitted by `update`, executed by the worker.
//! Each fetch carries a monotonic `token`; the reducer drops results whose
//! token no longer matches the target view (stale-result protection).
use crate::tui::v2::resource::Kind;

/// A list-fetch scope: restrict results to children of a parent resource.
/// Only `Kind::Flow` parents filter today (flow -> runs); other kinds pass
/// through (see `worker::item_in_scope`).
#[derive(Debug, Clone)]
pub struct ListScope {
    pub parent_kind: Kind,
    pub parent_id: String,
}

#[derive(Debug, Clone)]
pub enum Effect {
    FetchList {
        kind: Kind,
        token: u64,
        scope: Option<ListScope>,
    },
    FetchDetail {
        kind: Kind,
        id: String,
        token: u64,
    },
}
