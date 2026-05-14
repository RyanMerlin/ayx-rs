//! Bridge between async axum handlers and the blocking telemetry functions.

use std::path::Path;

use anyhow::Result;
use axum::extract::Query;
use ayx_core::envelope::Envelope;
use serde::Deserialize;

use crate::cmd::telemetry::TelemetryArgs;

/// Query parameters accepted by every panel route. Mirrors the CLI flags so
/// the URL and the CLI stay in lock-step.
#[derive(Debug, Default, Deserialize)]
pub struct PanelQuery {
    pub source: Option<String>,
    pub since: Option<String>,
    pub top: Option<usize>,
    pub sort: Option<String>,
    pub status: Option<String>,
    pub owner: Option<String>,
    pub health: Option<String>,
    pub all: Option<bool>,
    pub max_pages: Option<u32>,
}

pub type PanelQ = Query<PanelQuery>;

pub fn build_args(q: &PanelQuery, default_source: &str, profile: &Path) -> TelemetryArgs {
    TelemetryArgs {
        profile: profile.to_path_buf(),
        source: q
            .source
            .clone()
            .unwrap_or_else(|| default_source.to_owned()),
        since: q.since.clone().unwrap_or_else(|| "7d".to_owned()),
        top: q.top.unwrap_or(10),
        all: q.all.unwrap_or(false),
        max_pages: q.max_pages,
    }
}

/// Run a blocking telemetry call on a worker thread and surface the result.
pub async fn run_blocking<F, T>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f).await?
}

/// Convenience for handlers that just need the envelope.
pub async fn run_envelope<F>(f: F) -> Result<Envelope>
where
    F: FnOnce() -> Result<Envelope> + Send + 'static,
{
    run_blocking(f).await
}
