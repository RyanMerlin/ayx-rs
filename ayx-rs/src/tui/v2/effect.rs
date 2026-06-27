//! Effects: side-effect requests emitted by `update`, executed by the worker.
//! Each fetch carries a monotonic `token`; the reducer drops results whose
//! token no longer matches the target view (stale-result protection).
use crate::tui::v2::resource::Kind;

#[derive(Debug, Clone)]
pub enum Effect {
    FetchList { kind: Kind, token: u64 },
    FetchDetail { kind: Kind, id: String, token: u64 },
}
