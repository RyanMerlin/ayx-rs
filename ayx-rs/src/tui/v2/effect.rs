//! Effects: side-effect requests emitted by `update`, executed by the worker.
use crate::tui::v2::resource::Kind;

#[derive(Debug, Clone)]
pub enum Effect {
    FetchList { kind: Kind },
}
