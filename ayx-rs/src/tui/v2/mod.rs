//! Phase-0 spine of the rearchitected TUI (the "v2" surface).
//!
//! Unidirectional: Event -> Action -> update(state) -> [Effect] -> worker ->
//! Action. The render loop never blocks on I/O. Gated behind AYX_TUI_V2 so the
//! legacy `tui/app.rs` path stays live until later phases port it.
#![allow(dead_code, unused_imports)] // trait surface lands ahead of all callers during Phase 0

pub mod action;
pub mod context;
pub mod effect;
pub mod nav;
pub mod resource;
pub mod state;
pub mod view;
pub mod worker;

pub use entry::run;

mod entry;
