//! Per-top-level-command dispatch modules.
//!
//! Each file in this directory owns the body of one `Command::X` match arm
//! from `main.rs`. The `Cli` struct + the top-level clap tree still live in
//! `main.rs`, but the dispatch lives here — this lets the parent stay a
//! shallow router and gives each command family its own file to grow in.
//!
//! Convention: every cmd module exposes one `execute(...)` entry point that
//! returns `anyhow::Result<Envelope>`. They take whatever Cli state they
//! need (apply flag, environment override, etc.) as parameters rather than
//! reaching back into a shared `cli` struct, so the boundary is explicit.

pub mod confirm;
pub mod mongo;
pub mod registry;
pub mod server;
pub mod sqlserver;
pub mod tools;
pub mod workflow;
